use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::afc::AfcClient;
use crate::airlift::{
    LINK_PREFIX, RECOVERED_PREFIX, SOURCE_PREFIX, build_books_plist,
    build_streaming_zip_archive, cleanup_airlift_staging, restore_books, snapshot_books,
    stage_streaming_zip,
};
use crate::airtraffic::sync_assets_via_airtraffic;
use crate::device::ActiveDeviceSession;
use crate::wallet_backup::{self, ORIGINAL_ASSETS};

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
    Png { png_3x: Vec<u8>, png_2x: Vec<u8> },
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
    let n = cleanup_airlift_staging(&afc, &mut *log);
    if n > 0 {
        log(&format!("Cleared {n} leftover Airlift/Books path(s) before staging"));
        sleep(Duration::from_millis(400));
    }
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
            match stage_streaming_zip(&session, &source, &archive_data) {
                Ok(()) => {}
                Err(err) => {
                    let link_obj = format!("{source}/p0/p1/p2/link");
                    let payload_obj = format!("{source}/payload");
                    if afc.exists(&source) && afc.exists(&link_obj) && afc.exists(&payload_obj) {
                        log(&format!(
                            "StreamingZip reported {err:#}; staged files present, continuing"
                        ));
                    } else {
                        return Err(err).with_context(|| {
                            format!("Failed to stage streaming zip for {leaf}")
                        });
                    }
                }
            }
            if idx + 1 < items.len() {
                sleep(Duration::from_millis(250));
            }

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

pub fn airlock_relpath(absolute: &str) -> String {
    let trimmed = absolute.trim_start_matches('/');
    let tail = trimmed
        .strip_prefix("var/mobile/")
        .unwrap_or(trimmed);
    format!("../../../{tail}")
}

fn dummy_zip_for_dir(target_dir: &str) -> Result<Vec<u8>> {
    build_streaming_zip_archive(target_dir, b"aircard-v2").context("Failed to build Airlift zip")
}

/// Relocate protected files into Media (Mac-style unlink), then delete the Media copies.
pub fn remove_system_files<L>(
    udid: &str,
    target_dir: &str,
    leaves: &[&str],
    mut log: L,
) -> Result<()>
where
    L: FnMut(&str),
{
    if leaves.is_empty() {
        return Ok(());
    }
    for leaf in leaves {
        if leaf.is_empty() || *leaf == "." || *leaf == ".." || leaf.contains('/') {
            bail!("cache leaves must be plain file names");
        }
    }

    log(&format!(
        "Relocating {} cache file(s) out of {target_dir} (iOS 27 invalidation)...",
        leaves.len()
    ));
    let session = ActiveDeviceSession::open(Some(udid))
        .context("Failed to open device session for cache removal")?;
    let afc = AfcClient::new(&session).context("Failed to open AFC for cache removal")?;
    let n = cleanup_airlift_staging(&afc, &mut log);
    if n > 0 {
        log(&format!("Cleared {n} leftover Airlift/Books path(s) before cache move"));
        sleep(Duration::from_millis(400));
    }
    let snapshot = snapshot_books(&afc).context("Failed to snapshot Books before cache removal")?;

    let token = generate_token();
    let source = format!("{SOURCE_PREFIX}{token}");
    let link_dest = format!("{LINK_PREFIX}{token}");
    let recovered = format!("{RECOVERED_PREFIX}{token}");
    let link_ident = format!("../../{source}/p0/p1/p2/link");

    let mut identifiers: Vec<String> = vec![link_ident.clone()];
    let mut dests: Vec<String> = vec![link_dest.clone()];
    for (i, leaf) in leaves.iter().enumerate() {
        identifiers.push(format!("../../{link_dest}/{leaf}"));
        dests.push(format!("{source}/removed-{i}"));
    }

    let remove_res = (|| -> Result<()> {
        let archive = dummy_zip_for_dir(target_dir)?;
        stage_streaming_zip(&session, &source, &archive)
            .context("Failed to stage cache-removal zip")?;
        let books_plist = build_books_plist(&identifiers)?;
        afc.make_directory_recursive("Books/Sync")?;
        afc.write_file("Books/Sync/Books.plist", &books_plist)?;
        let assets: Vec<(&str, &str)> = identifiers
            .iter()
            .zip(dests.iter())
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        sync_assets_via_airtraffic(udid, &assets, &mut log)
            .context("AirTraffic cache relocation failed")?;
        Ok(())
    })();

    let _ = afc.remove_path(&link_dest);
    let _ = afc.remove_path(&recovered);
    let _ = afc.remove_tree(&source);
    sleep(Duration::from_millis(400));
    let restore_res = restore_books(&afc, &snapshot);
    remove_res?;
    restore_res.context("Failed to restore Books after cache removal")?;
    log("Cache leaves relocated (Wallet must rebuild faces).");
    Ok(())
}

pub fn invalidate_wallet_cache<L>(udid: &str, card_hash: &str, mut log: L) -> Result<()>
where
    L: FnMut(&str),
{
    let mut all_ok = true;
    for ext in [".cache", ".pkcache"] {
        let cache_dir = format!("/var/mobile/Library/Passes/Cards/{card_hash}{ext}");
        match remove_system_files(udid, &cache_dir, CACHE_FILES, &mut log) {
            Ok(()) => {}
            Err(err) => {
                log(&format!(
                    "Cache {ext} relocation: {err:#} (empty cache is OK)"
                ));
                all_ok = false;
            }
        }
    }
    if all_ok {
        log("Wallet cache faces removed so iOS 27 rebuilds from the pass.");
    }
    Ok(())
}

/// Move a protected file into Media, AFC-read it, write it back. None if missing.
pub fn read_protected_file<L>(
    udid: &str,
    target_dir: &str,
    leaf: &str,
    mut log: L,
) -> Result<Option<Vec<u8>>>
where
    L: FnMut(&str),
{
    if leaf.is_empty() || leaf == "." || leaf == ".." || leaf.contains('/') {
        bail!("leaf must be a plain file name");
    }
    let full = format!("{}/{}", target_dir.trim_end_matches('/'), leaf);
    let file_ident = airlock_relpath(&full);

    log(&format!("Exporting {leaf} via Airlift move (will write it back)..."));
    let session = ActiveDeviceSession::open(Some(udid))
        .context("Failed to open device session for original-art read")?;
    let afc = AfcClient::new(&session).context("Failed to open AFC for original-art read")?;
    let n = cleanup_airlift_staging(&afc, &mut log);
    if n > 0 {
        log(&format!("Cleared {n} leftover Airlift/Books path(s) before export"));
        sleep(Duration::from_millis(400));
    }
    let snapshot = snapshot_books(&afc).context("Failed to snapshot Books before original-art read")?;

    let token = generate_token();
    let source = format!("{SOURCE_PREFIX}{token}");
    let link_dest = format!("{LINK_PREFIX}{token}");
    let recovered = format!("{RECOVERED_PREFIX}{token}");
    let link_ident = format!("../../{source}/p0/p1/p2/link");
    let identifiers = vec![link_ident.clone(), file_ident];
    let dests = vec![link_dest.clone(), recovered.clone()];

    let mut keep_recovered = false;
    let read_res = (|| -> Result<Option<Vec<u8>>> {
        let archive = dummy_zip_for_dir(target_dir)?;
        stage_streaming_zip(&session, &source, &archive)
            .context("Failed to stage original-art export zip")?;
        let books_plist = build_books_plist(&identifiers)?;
        afc.make_directory_recursive("Books/Sync")?;
        afc.write_file("Books/Sync/Books.plist", &books_plist)?;
        let assets: Vec<(&str, &str)> = identifiers
            .iter()
            .zip(dests.iter())
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        sync_assets_via_airtraffic(udid, &assets, &mut log)
            .context("AirTraffic original-art export failed")?;

        if !afc.exists(&recovered) {
            log(&format!("{leaf} not present on device"));
            return Ok(None);
        }
        let size = afc.file_size(&recovered).unwrap_or(0);
        if size == 0 || size > 32 * 1024 * 1024 {
            keep_recovered = true;
            bail!("{leaf} export size {size} is invalid; leaving Media copy");
        }
        let data = afc
            .read_file(&recovered)
            .context("AFC read of exported original art failed")?;
        if data.is_empty() {
            keep_recovered = true;
            bail!("exported {leaf} was empty; leaving Media copy");
        }
        Ok(Some(data))
    })();

    match &read_res {
        Ok(Some(data)) => {
            let _ = afc.remove_path(&link_dest);
            let _ = afc.remove_tree(&source);
            let _ = restore_books(&afc, &snapshot);
            drop(afc);
            write_system_files_batch(
                udid,
                &[(target_dir.to_string(), leaf.to_string(), data.clone())],
                |_, _, _| {},
                &mut log,
            )
            .context("Failed to write original art back after export")?;
            // recovered was the only copy during the gap; write-back restores the pass.
            let session2 = ActiveDeviceSession::open(Some(udid)).ok();
            if let Some(session2) = session2 {
                if let Ok(afc2) = AfcClient::new(&session2) {
                    let _ = afc2.remove_path(&recovered);
                }
            }
        }
        Ok(None) => {
            let _ = afc.remove_path(&link_dest);
            let _ = afc.remove_path(&recovered);
            let _ = afc.remove_tree(&source);
            let _ = restore_books(&afc, &snapshot);
        }
        Err(_) if keep_recovered => {
            log("Leaving exported original in Media for manual recovery; Books restored.");
            let _ = restore_books(&afc, &snapshot);
        }
        Err(_) => {
            let _ = afc.remove_path(&link_dest);
            let _ = afc.remove_tree(&source);
            let _ = restore_books(&afc, &snapshot);
        }
    }

    read_res
}

pub fn capture_original_artwork<L>(
    udid: &str,
    card_hash: &str,
    mut log: L,
) -> Result<usize>
where
    L: FnMut(&str),
{
    if wallet_backup::backup_exists(udid, card_hash) {
        log("Original artwork backup already exists; not overwriting.");
        return Ok(0);
    }
    let pkpass = format!("/var/mobile/Library/Passes/Cards/{card_hash}.pkpass");
    let mut saved = 0usize;
    for leaf in ORIGINAL_ASSETS {
        match read_protected_file(udid, &pkpass, leaf, &mut log) {
            Ok(Some(data)) => {
                if wallet_backup::save_original_asset(udid, card_hash, leaf, &data)? {
                    saved += 1;
                    log(&format!("Saved original {leaf} ({} bytes)", data.len()));
                }
            }
            Ok(None) => log(&format!("No {leaf} on this card")),
            Err(err) => log(&format!("Could not export {leaf}: {err:#}")),
        }
    }
    if saved == 0 {
        bail!("Could not export any original artwork. Save before the first skin, keep the phone unlocked.");
    }
    Ok(saved)
}

pub fn restore_original_artwork<F, L>(
    udid: &str,
    card_hash: &str,
    progress: F,
    mut log: L,
) -> Result<()>
where
    F: FnMut(usize, usize, &str),
    L: FnMut(&str),
{
    let assets = wallet_backup::load_original_assets(udid, card_hash)?;
    log(&format!(
        "Restoring {} original asset(s) for card {card_hash}",
        assets.len()
    ));
    let pkpass = format!("/var/mobile/Library/Passes/Cards/{card_hash}.pkpass");
    let items: Vec<(String, String, Vec<u8>)> = assets
        .into_iter()
        .map(|(leaf, data)| (pkpass.clone(), leaf, data))
        .collect();
    write_system_files_batch(udid, &items, progress, &mut log)?;
    invalidate_wallet_cache(udid, card_hash, &mut log)?;
    log("Original artwork restored. Force-close Wallet to view.");
    Ok(())
}

pub fn original_backup_exists(udid: &str, card_hash: &str) -> bool {
    wallet_backup::backup_exists(udid, card_hash)
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
        WalletArt::Png { png_3x, png_2x } => {
            log(&format!(
                "Skin payload @3x={} bytes PNG, @2x={} bytes PNG (1024x646)",
                png_3x.len(),
                png_2x.len()
            ));
            debug_assert_eq!(TARGET_WALLET_ASSETS.len(), 2);
            let payloads: [&[u8]; 2] = [png_3x.as_slice(), png_2x.as_slice()];
            for (leaf, data) in TARGET_WALLET_ASSETS.iter().zip(payloads) {
                items.push((pkpass_dir.clone(), (*leaf).to_string(), data.to_vec()));
            }
        }
        WalletArt::Pdf(pdf) => {
            log(&format!("Skin payload size: {} bytes PDF (Suica / transit artwork)", pdf.len()));
            for asset in TARGET_WALLET_PDF {
                items.push((pkpass_dir.clone(), (*asset).to_string(), pdf.clone()));
            }
        }
    }

    write_system_files_batch(udid, &items, &mut progress, &mut log)
        .context("Failed to write card skin batch")?;

    invalidate_wallet_cache(udid, card_hash, &mut log)?;

    if let WalletArt::Png { png_3x, .. } = art {
        remember_applied_png(card_hash, png_3x);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn airlock_relpath_strips_var_mobile() {
        assert_eq!(
            airlock_relpath(
                "/var/mobile/Library/Passes/Cards/abc.pkpass/cardBackgroundCombined@3x.png"
            ),
            "../../../Library/Passes/Cards/abc.pkpass/cardBackgroundCombined@3x.png"
        );
        assert_eq!(
            airlock_relpath("var/mobile/Media/Books/foo"),
            "../../../Media/Books/foo"
        );
    }

    #[test]
    fn wallet_png_assets_are_distinct_3x_and_2x() {
        assert_eq!(
            TARGET_WALLET_ASSETS,
            &[
                "cardBackgroundCombined@3x.png",
                "cardBackgroundCombined@2x.png"
            ]
        );
        assert_eq!(CACHE_FILES, &["FrontFace", "PlaceHolder", "Preview"]);
    }
}
