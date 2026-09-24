use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::apple::{ATHostConnectionRef, get_apple_libraries};

/// Replayable 84-byte host Grappa blob from yinyajiang/go-tunes.
/// Windows AirTrafficHost cannot mint a live FairPlay Grappa; sending
/// SendSyncRequest without this blob yields ErrorCode 12, and the DLL's
/// uninitialized blob yields ErrorCode 4 (invalid Grappa).
const HOST_GRAPPA: [u8; 84] = [
    0x01, 0x01, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x04, 0x40, 0xbc, 0x27, 0x85, 0xe0, 0xdb, 0xf1, 0x66, 0x36, 0x1e, 0x07, 0x98, 0x0a,
    0x5e, 0xa4, 0x8d, 0xba, 0x95, 0xb3, 0xb8, 0xea, 0x26, 0x5d, 0x62, 0xae, 0xfe, 0xa5, 0x1b, 0xb7,
    0xb1, 0x90, 0xe0, 0xb7, 0x71, 0x26, 0x29, 0x0a, 0xd3, 0x9b, 0xb1, 0x3f, 0xec, 0xc0, 0x8c, 0x25,
    0xa9, 0x56, 0x1c, 0x51, 0x7a, 0xc1, 0x1e, 0x64, 0x90, 0x5d, 0xa0, 0x29, 0xe6, 0x1b, 0xdf, 0xd0,
    0xba, 0x22, 0xc3, 0x13,
];

const LIBRARY_ID: &str = "12.6.0.100";
const HOST_VERSION: &str = "12.6.0.100";

fn plist_to_cf(libs: &crate::apple::AppleLibraries, value: &plist::Value) -> Result<crate::apple::CFTypeGuard> {
    let mut bytes = Vec::new();
    plist::to_writer_binary(&mut bytes, value).context("Failed to encode CF plist")?;
    libs.create_cf_plist_from_bytes(&bytes)
}

pub enum SyncEvent {
    Log(String),
    Done(Result<()>),
}

pub fn sync_assets_via_airtraffic<L>(udid: &str, assets: &[(&str, &str)], mut log: L) -> Result<()>
where
    L: FnMut(&str),
{
    let udid_owned = udid.to_string();
    let assets_owned: Vec<(String, String)> = assets
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();

    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::spawn(move || {
        let refs: Vec<(&str, &str)> = assets_owned
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        let tx_log = tx.clone();
        let res = sync_assets_via_airtraffic_internal(&udid_owned, &refs, move |msg| {
            let _ = tx_log.send(SyncEvent::Log(msg.to_string()));
        });
        let _ = tx.send(SyncEvent::Done(res));
    });

    // macOS airlift uses ~300s; Windows port's 35s was aborting mid-handshake on iOS 26+.
    const SYNC_TIMEOUT_SECS: u64 = 120;
    let start = std::time::Instant::now();
    loop {
        let elapsed = start.elapsed();
        if elapsed >= Duration::from_secs(SYNC_TIMEOUT_SECS) {
            bail!("AirTraffic sync timed out ({}s). Keep iPhone unlocked, open Books once, quit iTunes/Apple Devices on PC.", SYNC_TIMEOUT_SECS);
        }
        let timeout = Duration::from_secs(SYNC_TIMEOUT_SECS) - elapsed;
        match rx.recv_timeout(timeout) {
            Ok(SyncEvent::Log(msg)) => log(&msg),
            Ok(SyncEvent::Done(res)) => return res,
            Err(_) => {
                bail!("AirTraffic sync timed out ({}s). Keep iPhone unlocked, open Books once, quit iTunes/Apple Devices on PC.", SYNC_TIMEOUT_SECS);
            }
        }
    }
}

fn sync_assets_via_airtraffic_internal<L>(udid: &str, assets: &[(&str, &str)], mut log: L) -> Result<()>
where
    L: FnMut(&str),
{
    log("Connecting to iOS AirTraffic service (com.apple.atc)...");
    let libs = get_apple_libraries()?;
    let cf_udid = libs.create_cf_string(udid)?;
    let cf_library = libs.create_cf_string(LIBRARY_ID)?;

    // Windows ATH requires CreateWithLibrary(libraryID, udid, flags).
    // One-arg Create(udid) connects but cannot complete Grappa.
    let mut conn: ATHostConnectionRef =
        unsafe { (libs.at_host_connection_create_with_library)(cf_library.raw, cf_udid.raw, 0) };
    if conn.is_null() {
        log("CreateWithLibrary returned null, falling back to ATHostConnectionCreate...");
        conn = unsafe { (libs.at_host_connection_create)(cf_udid.raw) };
    }
    if conn.is_null() {
        bail!("ATHostConnectionCreateWithLibrary failed for the selected device");
    }

    let mut run_sync = || -> Result<()> {
        let session = unsafe { (libs.at_host_connection_get_current_session_number)(conn) };
        log(&format!("AirTraffic session {session}, sending HostInfo..."));

        let host_info_value = plist::Value::Dictionary({
            let mut d = plist::Dictionary::new();
            d.insert("LibraryID".into(), LIBRARY_ID.into());
            d.insert("SyncHostName".into(), "airlift".into());
            d.insert("Version".into(), HOST_VERSION.into());
            d.insert(
                "SyncedDataclasses".into(),
                plist::Value::Array(vec!["Book".into()]),
            );
            d
        });
        let cf_host_info = plist_to_cf(&libs, &host_info_value)?;
        unsafe {
            (libs.at_host_connection_send_host_info)(conn, cf_host_info.raw);
        }

        log("Waiting for SyncAllowed from iPhone (keep screen unlocked)...");
        let mut sync_allowed = false;
        for _ in 0..20 {
            let msg = unsafe { (libs.at_host_connection_read_message)(conn) };
            if msg.is_null() {
                sleep(Duration::from_millis(150));
                continue;
            }
            let name_ref = unsafe { (libs.at_cf_message_get_name)(msg) };
            let name = libs.to_rust_string(name_ref);
            unsafe { (libs.cf_release)(msg) };
            if name == "SyncAllowed" {
                sync_allowed = true;
                break;
            } else {
                log(&format!("AirTraffic message: {name}"));
            }
        }
        if !sync_allowed {
            bail!("AirTraffic: SyncAllowed message not received. Ensure iPhone screen is unlocked and Books app is opened.");
        }

        log("SyncAllowed received! Sending RequestingSync with replay Grappa...");

        let requesting = plist::Value::Dictionary({
            let mut d = plist::Dictionary::new();
            d.insert(
                "Dataclasses".into(),
                plist::Value::Array(vec!["Book".into()]),
            );
            d.insert("DataclassAnchors".into(), {
                let mut a = plist::Dictionary::new();
                a.insert("Book".into(), "0".into());
                plist::Value::Dictionary(a)
            });
            d.insert("HostInfo".into(), {
                let mut hi = plist::Dictionary::new();
                hi.insert("Grappa".into(), plist::Value::Data(HOST_GRAPPA.to_vec()));
                hi.insert("LibraryID".into(), LIBRARY_ID.into());
                hi.insert("SyncHostName".into(), "airlift".into());
                hi.insert("Version".into(), HOST_VERSION.into());
                hi.insert(
                    "SyncedDataclasses".into(),
                    plist::Value::Array(vec!["Book".into()]),
                );
                plist::Value::Dictionary(hi)
            });
            d
        });
        let cf_params = plist_to_cf(&libs, &requesting)?;
        let cf_cmd = libs.create_cf_string("RequestingSync")?;
        let msg = unsafe { (libs.at_cf_message_create)(session, cf_cmd.raw, cf_params.raw) };
        if msg.is_null() {
            bail!("ATCFMessageCreate(RequestingSync) returned null");
        }
        let send_status = unsafe { (libs.at_host_connection_send_message)(conn, msg) };
        unsafe {
            (libs.cf_release)(msg);
        }
        log(&format!("RequestingSync send status={send_status}"));

        log("Waiting for ReadyForSync from iPhone...");
        let mut ready_for_sync = false;
        let ready_deadline = std::time::Instant::now() + Duration::from_secs(90);
        while std::time::Instant::now() < ready_deadline {
            let msg = unsafe { (libs.at_host_connection_read_message)(conn) };
            if msg.is_null() {
                sleep(Duration::from_millis(150));
                continue;
            }
            let name_ref = unsafe { (libs.at_cf_message_get_name)(msg) };
            let name = libs.to_rust_string(name_ref);
            log(&format!("AirTraffic message (post-RequestingSync): {name}"));
            if name == "ReadyForSync" {
                unsafe { (libs.cf_release)(msg) };
                ready_for_sync = true;
                break;
            }
            if name == "SyncFailed" || name == "SyncFinished" {
                // A single numeric parameter is enough for diagnostics. Never
                // print the full message, plist body, Grappa data or token.
                let error_code = libs.create_cf_string("ErrorCode").ok().and_then(|key| {
                    let param = unsafe { (libs.at_cf_message_get_param)(msg, key.raw) };
                    if param.is_null() {
                        return None;
                    }
                    let bytes = libs.cf_plist_to_bytes(param).ok()?;
                    let value = plist::Value::from_reader(std::io::Cursor::new(bytes)).ok()?;
                    numeric_error_code(&value)
                });
                unsafe { (libs.cf_release)(msg) };
                if let Some(code) = error_code {
                    bail!("AirTraffic returned {name} instead of ReadyForSync. ErrorCode={code}");
                }
                bail!("AirTraffic returned {name} instead of ReadyForSync. ErrorCode unavailable");
            }
            unsafe { (libs.cf_release)(msg) };
        }
        if !ready_for_sync {
            bail!("AirTraffic: ReadyForSync message not received from device");
        }

        if assets.is_empty() {
            log("ReadyForSync received (probe, no assets) — handshake OK.");
            return Ok(());
        }

        log("ReadyForSync received. Finishing Books metadata sync...");
        let cf_true = plist_to_cf(&libs, &plist::Value::Boolean(true))?;
        unsafe {
            (libs.at_host_connection_send_power_assertion)(conn, cf_true.raw);
        }

        let sync_types = plist::Value::Dictionary({
            let mut d = plist::Dictionary::new();
            d.insert("Book".into(), plist::Value::Integer(1.into()));
            d
        });
        let cf_sync_types = plist_to_cf(&libs, &sync_types)?;
        let anchors = plist::Value::Dictionary({
            let mut d = plist::Dictionary::new();
            d.insert("Book".into(), "0".into());
            d
        });
        let cf_anchors = plist_to_cf(&libs, &anchors)?;

        unsafe {
            (libs.at_host_connection_send_metadata_sync_finished)(
                conn,
                cf_sync_types.raw,
                cf_anchors.raw,
            );
        }

        // 6. Read AssetManifest
        let cf_key_manifest = libs.create_cf_string("AssetManifest")?;
        let mut manifest_val: Option<plist::Value> = None;

        for _ in 0..30 {
            let msg = unsafe { (libs.at_host_connection_read_message)(conn) };
            if msg.is_null() {
                sleep(Duration::from_millis(150));
                continue;
            }
            let name_ref = unsafe { (libs.at_cf_message_get_name)(msg) };
            let name = libs.to_rust_string(name_ref);
            if name == "AssetManifest" {
                let param = unsafe { (libs.at_cf_message_get_param)(msg, cf_key_manifest.raw) };
                if !param.is_null() {
                    if let Ok(bytes) = libs.cf_plist_to_bytes(param) {
                        manifest_val = plist::Value::from_reader(std::io::Cursor::new(bytes)).ok();
                    }
                }
                unsafe { (libs.cf_release)(msg) };
                break;
            } else if name == "SyncFailed" || name == "SyncFinished" {
                unsafe { (libs.cf_release)(msg) };
                bail!("AirTraffic returned unexpected terminating message: {}", name);
            }
            unsafe { (libs.cf_release)(msg) };
        }

        let Some(manifest) = manifest_val else {
            bail!("AirTraffic: AssetManifest was not received or failed to parse");
        };

        // Validate Book manifest contains downloads
        let book_entries = manifest
            .as_dictionary()
            .and_then(|d| d.get("Book"))
            .and_then(|v| v.as_array())
            .context("AssetManifest does not contain Book list")?;

        let mut available_downloads = Vec::new();
        for entry in book_entries {
            if let Some(dict) = entry.as_dictionary() {
                let is_dl = dict.get("IsDownload").and_then(|b| b.as_boolean()).unwrap_or(false);
                if is_dl {
                    if let Some(asset_id) = dict.get("AssetID").and_then(|s| s.as_string()) {
                        available_downloads.push(asset_id.to_string());
                    }
                }
            }
        }

        for (ident, _) in assets {
            if !available_downloads.iter().any(|d| d == ident) {
                bail!(
                    "Asset '{}' missing from device download manifest (available: {:?})",
                    ident,
                    available_downloads
                );
            }
        }

        // 7. Dispatch AssetCompleted for each asset
        let cf_dataclass = libs.create_cf_string("Book")?;
        for (idx, (ident, dest)) in assets.iter().enumerate() {
            let cf_ident = libs.create_cf_string(ident)?;
            let cf_dest = libs.create_cf_string(dest)?;

            unsafe {
                (libs.at_host_connection_send_asset_completed)(
                    conn,
                    cf_ident.raw,
                    cf_dataclass.raw,
                    cf_dest.raw,
                );
            }

            if idx + 1 < assets.len() {
                sleep(Duration::from_millis(900));
            }
        }

        sleep(Duration::from_millis(2000));
        Ok(())
    };

    let result = run_sync();
    unsafe {
        (libs.at_host_connection_release)(conn);
    }
    result
}

fn numeric_error_code(value: &plist::Value) -> Option<String> {
    value
        .as_signed_integer()
        .map(|n| n.to_string())
        .or_else(|| value.as_unsigned_integer().map(|n| n.to_string()))
        .or_else(|| {
            value.as_string().and_then(|s| {
                let t = s.trim();
                if !t.is_empty()
                    && t.bytes()
                        .enumerate()
                        .all(|(i, b)| b.is_ascii_digit() || (i == 0 && b == b'-'))
                {
                    Some(t.to_string())
                } else {
                    None
                }
            })
        })
}

#[cfg(test)]
mod tests {
    use super::numeric_error_code;

    #[test]
    fn failure_log_accepts_only_numeric_error_code() {
        assert_eq!(
            numeric_error_code(&plist::Value::Integer(4.into())),
            Some("4".into())
        );
        assert_eq!(
            numeric_error_code(&plist::Value::String("4".into())),
            Some("4".into())
        );
        assert_eq!(
            numeric_error_code(&plist::Value::String("private token".into())),
            None
        );
        assert_eq!(numeric_error_code(&plist::Value::Data(vec![1, 2, 3])), None);
    }
}
