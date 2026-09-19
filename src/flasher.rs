use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::afc::AfcClient;
use crate::airlift::{
    LINK_PREFIX, RECOVERED_PREFIX, SOURCE_PREFIX, build_books_plist,
    build_streaming_zip_archive, restore_books, snapshot_books,
    stage_streaming_zip,
};
use crate::airtraffic::sync_assets_via_airtraffic;
use crate::device::ActiveDeviceSession;

pub const TARGET_WALLET_ASSETS: &[&str] = &[
    "cardBackgroundCombined@3x.png",
    "cardBackgroundCombined@2x.png",
];

pub const TARGET_WALLET_PDF: &[&str] = &[
    "cardBackgroundCombined.pdf",
    "cardBackgroundCombined@2x.pdf",
    "cardBackgroundCombined@3x.pdf",
];

pub const CACHE_FILES: &[&str] = &[
    "FrontFace",
    "PlaceHolder",
    "Preview",
];

#[link(name = "bcrypt")]
unsafe extern "system" {
    fn BCryptGenRandom(
        hAlgorithm: *mut std::ffi::c_void,
        pbBuffer: *mut u8,
        cbBuffer: u32,
        dwFlags: u32,
    ) -> i32;
}

pub fn generate_token() -> String {
    let mut bytes = [0u8; 10];
    unsafe {
        let _ = BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            2, // BCRYPT_USE_SYSTEM_PREFERRED_RNG
        );
    }
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[derive(Clone)]
pub enum WalletArt {
    Png(Vec<u8>),
    Pdf(Vec<u8>),
}

fn skin_backup_dir(card_hash: &str) -> std::path::PathBuf {
    let local = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| r"C:\Users\Default\AppData\Local".to_string());
    let safe: String = card_hash
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let dir = std::path::PathBuf::from(local)
        .join("AirCard")
        .join("skins")
        .join(safe);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn remember_applied_png(card_hash: &str, png: &[u8]) {
    let dir = skin_backup_dir(card_hash);
    let current = dir.join("current.png");
    if current.exists() {
        let _ = std::fs::copy(&current, dir.join("previous.png"));
    }
    let _ = std::fs::write(current, png);
}

pub fn load_previous_png(card_hash: &str) -> Option<Vec<u8>> {
    std::fs::read(skin_backup_dir(card_hash).join("previous.png")).ok()
}

struct StagedAsset {
    source: String,
    link_dest: String,
    recovered: String,
    leaf: String,
}

/// Files per AirTraffic handshake. Each file is 2 Book assets (link + payload).
const AIRTRAFFIC_BATCH_SIZE: usize = 8;

#[allow(dead_code)]
pub fn write_system_file<L>(
    udid: &str,
    target_dir: &str,
    leaf_name: &str,
    payload: &[u8],
    log: L,
) -> Result<()>
where
    L: FnMut(&str),
{
    write_system_files_batch(
        udid,
        &[(
            target_dir.to_string(),
            leaf_name.to_string(),
            payload.to_vec(),
        )],
        |_, _, _| {},
        log,
    )
}

pub fn write_system_files_batch<F, L>(
    udid: &str,
    items: &[(String, String, Vec<u8>)],
    mut progress: F,
    mut log: L,
) -> Result<()>
where
    F: FnMut(usize, usize, &str),
    L: FnMut(&str),
{
    if items.is_empty() {
        return Ok(());
    }

    let total_chunks = items.len().div_ceil(AIRTRAFFIC_BATCH_SIZE);
    let progress_total = items.len() + total_chunks;
    let mut staged_so_far = 0usize;
    for (chunk_idx, chunk) in items.chunks(AIRTRAFFIC_BATCH_SIZE).enumerate() {
        let chunk_label = if total_chunks > 1 {
            format!(" (group {}/{})", chunk_idx + 1, total_chunks)
        } else {
            String::new()
        };
        write_system_files_batch_once(
            udid,
            chunk,
            &chunk_label,
            staged_so_far,
            progress_total,
            &mut progress,
            &mut log,
        )?;
        staged_so_far += chunk.len();
    }
    Ok(())
}

fn write_system_files_batch_once<F, L>(
    udid: &str,
    items: &[(String, String, Vec<u8>)],
    chunk_label: &str,
    staged_so_far: usize,
    progress_total: usize,
    progress: &mut F,
    log: &mut L,
) -> Result<()>
where
    F: FnMut(usize, usize, &str),
    L: FnMut(&str),
{
    log(&format!(
        "Connecting AFC for {} file(s){}...",
        items.len(),
        chunk_label
    ));
    let session = ActiveDeviceSession::open(Some(udid))
        .context("Failed to open device session for writing")?;
    let afc = AfcClient::new(&session).context("Failed to open AFC connection")?;

    let snapshot = snapshot_books(&afc).context("Failed to snapshot Books state before staging")?;

    let mut staged: Vec<StagedAsset> = Vec::new();
    let mut books_identifiers: Vec<String> = Vec::new();
    let mut assets_owned: Vec<(String, String)> = Vec::new();

    let write_res = (|| -> Result<()> {
        for (idx, (target_dir, leaf, payload)) in items.iter().enumerate() {
            progress(
                staged_so_far + idx + 1,
                progress_total,
                &format!("Staging {leaf}{chunk_label}..."),
            );
            let token = generate_token();
            let source = format!("{SOURCE_PREFIX}{token}");
            let link_dest = format!("{LINK_PREFIX}{token}");
            let recovered = format!("{RECOVERED_PREFIX}{token}");
            let link_ident = format!("../../{source}/p0/p1/p2/link");
            let payload_ident = format!("../../{source}/payload");
            let target_dest = format!("{link_dest}/{leaf}");

            let archive_data = build_streaming_zip_archive(target_dir, payload)
                .with_context(|| format!("Failed to build streaming zip for {leaf}"))?;
            log(&format!(
                "Staging {leaf} archive ({} bytes) via MobileInstallation...",
                archive_data.len()
            ));
            stage_streaming_zip(&session, &source, &archive_data)
                .with_context(|| format!("Failed to stage streaming zip for {leaf}"))?;

            let link_obj = format!("{source}/p0/p1/p2/link");
            let payload_obj = format!("{source}/payload");
            if !afc.exists(&source) || !afc.exists(&link_obj) || !afc.exists(&payload_obj) {
                bail!("StreamingZip completed but staging link/payload object missing on AFC for {leaf}");
            }

            books_identifiers.push(link_ident.clone());
            books_identifiers.push(payload_ident.clone());
            assets_owned.push((link_ident.clone(), link_dest.clone()));
            assets_owned.push((payload_ident.clone(), target_dest.clone()));
            staged.push(StagedAsset {
                source,
                link_dest,
                recovered,
                leaf: leaf.clone(),
            });
        }

        let books_plist =
            build_books_plist(&books_identifiers).context("Failed to build Books.plist")?;
        afc.make_directory_recursive("Books/Sync")?;
        afc.write_file("Books/Sync/Books.plist", &books_plist)?;
        if !afc.exists("Books/Sync/Books.plist") {
            bail!("Failed to stage Books/Sync/Books.plist");
        }

        progress(
            staged_so_far + items.len() + 1,
            progress_total,
            &format!("Synchronizing {} file(s){chunk_label}...", items.len()),
        );
        log(&format!(
            "Synchronizing {} file(s) with one AirTraffic handshake{}...",
            items.len(),
            chunk_label
        ));
        let assets_to_sync: Vec<(&str, &str)> = assets_owned
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        sync_assets_via_airtraffic(udid, &assets_to_sync, &mut *log)
            .context("AirTraffic sync failed")?;

        Ok(())
    })();

    for asset in &staged {
        let _ = afc.remove_path(&asset.link_dest);
        let _ = afc.remove_path(&asset.recovered);
        let _ = afc.remove_tree(&asset.source);
    }
    sleep(Duration::from_millis(800));

    let restore_res = restore_books(&afc, &snapshot);
    write_res?;
    restore_res.context("Failed to restore Books state during cleanup")?;

    for asset in &staged {
        log(&format!("Successfully written: {}", asset.leaf));
    }
    Ok(())
}

pub fn flash_wallet_skin<F, L>(
    udid: &str,
    card_hash: &str,
    art: &WalletArt,
    mut progress: F,
    mut log: L,
) -> Result<()>
where
    F: FnMut(usize, usize, &str),
    L: FnMut(&str),
{
    let pkpass_dir = format!("/var/mobile/Library/Passes/Cards/{}.pkpass", card_hash);

    log(&format!("Target Card Hash: {}", card_hash));

    let mut items: Vec<(String, String, Vec<u8>)> = Vec::new();
    match art {
        WalletArt::Png(png) => {
            log(&format!("Skin payload size: {} bytes PNG", png.len()));
            for asset in TARGET_WALLET_ASSETS {
                items.push((pkpass_dir.clone(), (*asset).to_string(), png.clone()));
            }
        }
        WalletArt::Pdf(pdf) => {
            log(&format!("Skin payload size: {} bytes PDF (Suica / transit artwork)", pdf.len()));
            for asset in TARGET_WALLET_PDF {
                items.push((pkpass_dir.clone(), (*asset).to_string(), pdf.clone()));
            }
        }
    }
    for ext in [".cache", ".pkcache"] {
        let cache_dir = format!("/var/mobile/Library/Passes/Cards/{}{}", card_hash, ext);
        for leaf in CACHE_FILES {
            items.push((cache_dir.clone(), (*leaf).to_string(), b"corrupted".to_vec()));
        }
    }

    write_system_files_batch(udid, &items, &mut progress, &mut log)
        .context("Failed to write card skin batch")?;

    if let WalletArt::Png(png) = art {
        remember_applied_png(card_hash, png);
    }

    let done = items.len() + 1;
    progress(done, done, "Card skin updated successfully!");
    log("Card skin write finished! Close and reopen Wallet on iPhone to view.");
    Ok(())
}

pub fn flash_passcode_theme<F, L>(
    udid: &str,
    items: &[(String, String, Vec<u8>)],
    mut progress: F,
    mut log: L,
) -> Result<()>
where
    F: FnMut(usize, usize, &str),
    L: FnMut(&str),
{
    log(&format!(
        "Flashing passcode theme ({} button assets, batched AirTraffic)...",
        items.len()
    ));
    write_system_files_batch(udid, items, &mut progress, &mut log)
        .context("Failed to write passcode theme batch")?;
    log("Passcode theme successfully written! Lock iPhone to see new keypad.");
    Ok(())
}
