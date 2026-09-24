#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod afc;
mod airlift;
mod airtraffic;
mod app;
mod apple;
mod device;
mod flasher;
mod host_guard;
mod i18n;
mod image_skin;
mod keypad;
mod passthm;
mod scanner;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let env_probe = std::env::var("AIRCARD_PROBE_ONLY").as_deref() == Ok("1");
    if let Some(valid_args) = probe_command(&args) {
        attach_probe_console();
        if !valid_args {
            eprintln!("Usage: aircard.exe --probe");
            std::process::exit(2);
        }
        return run_probe_and_exit();
    }
    if env_probe {
        attach_probe_console();
        return run_probe_and_exit();
    }
    if args.len() >= 4 && args[1] == "--flash" {
        match cli_flash(&args[2], &args[3]) {
            Ok(()) => return Ok(()),
            Err(e) => {
                eprintln!("Error: {e:#}");
                std::process::exit(1);
            }
        }
    }
    if args.len() >= 3 && args[1] == "--passcode" {
        let telephony = args.get(3).map(String::as_str);
        let language = args.get(4).map(String::as_str);
        match cli_passcode(&args[2], telephony, language) {
            Ok(()) => return Ok(()),
            Err(e) => {
                eprintln!("Error: {e:#}");
                std::process::exit(1);
            }
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 680.0])
            .with_min_inner_size([850.0, 560.0])
            .with_title("AirCard v1.3.0"),
        ..Default::default()
    };

    eframe::run_native(
        "AirCard v1.3.0",
        options,
        Box::new(|cc| Ok(Box::new(app::AirCardApp::new(cc)))),
    )
}

#[cfg(windows)]
fn attach_probe_console() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }

    // GUI-subsystem processes have no console by default. Attach only for --probe.
    unsafe { AttachConsole(u32::MAX) };
}

#[cfg(not(windows))]
fn attach_probe_console() {}

fn probe_command(args: &[String]) -> Option<bool> {
    (args.get(1).map(String::as_str) == Some("--probe")).then_some(args.len() == 2)
}

fn run_probe_and_exit() -> eframe::Result<()> {
    match cli_probe() {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("Error: {e:#}");
            std::process::exit(1);
        }
    }
}

fn warn_blocking_apps() {
    let apps = host_guard::blocking_sync_apps();
    if !apps.is_empty() {
        eprintln!(
            "warning   = quit these first (they steal AirTraffic): {}",
            apps.join(", ")
        );
    }
}

fn first_udid() -> anyhow::Result<String> {
    let devices = device::list_connected_devices()?;
    if devices.is_empty() {
        anyhow::bail!("No iPhone detected via usbmux / Apple Mobile Device Support");
    }
    for d in &devices {
        println!("device    = {d}  udid={}", d.udid);
    }
    Ok(devices[0].udid.clone())
}

fn cli_probe() -> anyhow::Result<()> {
    // Only query the local usbmux device list. Do not open AFC or run cleanup_books.
    warn_blocking_apps();
    let mut usb_devices = device::query_usbmux_devices()?
        .into_iter()
        .filter(|d| d.connection_type.eq_ignore_ascii_case("USB"));
    let Some(device) = usb_devices.next() else {
        anyhow::bail!("No USB-connected iPhone found for handshake probe");
    };
    if usb_devices.next().is_some() {
        anyhow::bail!("Multiple USB devices found; probe requires exactly one");
    }
    println!("probe = AirTraffic handshake only; no AFC staging or Wallet asset writes");
    airtraffic::sync_assets_via_airtraffic(&device.udid, &[], |msg| println!("log: {msg}"))?;
    println!("probe DONE — ReadyForSync received");
    Ok(())
}

fn cleanup_books(udid: &str) -> anyhow::Result<()> {
    println!("cleanup   = clearing leftover Books/airlift staging...");
    let session = device::ActiveDeviceSession::open(Some(udid))?;
    let afc = afc::AfcClient::new(&session)?;
    for path in airlift::TRACKED_BOOKS_FILES {
        if afc.exists(path) {
            println!("cleanup   = remove {path}");
            let _ = afc.remove_path(path);
        }
    }
    if let Ok(entries) = afc.list_directory(".") {
        for name in entries {
            if name.starts_with("airlift-src-")
                || name.starts_with("airlift-link-")
                || name.starts_with("airlift-recovered-")
            {
                println!("cleanup   = remove tree {name}");
                let _ = afc.remove_tree(&name);
            }
        }
    }
    Ok(())
}

fn cli_flash(card_hash: &str, image_path: &str) -> anyhow::Result<()> {
    println!("=== AirCard CLI flash ===");
    println!("card_hash = {card_hash}");
    println!("image     = {image_path}");
    warn_blocking_apps();

    let udid = first_udid()?;
    cleanup_books(&udid)?;

    let path = std::path::Path::new(image_path);
    let art = if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
    {
        let pdf = std::fs::read(path)?;
        println!("skin      = raw PDF {} bytes (Suica / transit)", pdf.len());
        flasher::WalletArt::Pdf(pdf)
    } else {
        let skin = image_skin::PreparedSkin::from_path(path)?;
        println!(
            "skin      = {}x{} -> 1536x969 png={} bytes",
            skin.source_width,
            skin.source_height,
            skin.png.len()
        );
        flasher::WalletArt::Png(skin.png)
    };

    println!("Keep iPhone UNLOCKED, screen ON. Open Books once if prompted.");
    println!("Starting flash...");

    flasher::flash_wallet_skin(
        &udid,
        card_hash,
        &art,
        |step, total, msg| println!("progress [{step}/{total}] {msg}"),
        |msg| println!("log: {msg}"),
    )?;

    println!("DONE — force-close Wallet on iPhone and reopen.");
    Ok(())
}

fn cli_passcode(
    theme_path: &str,
    telephony: Option<&str>,
    language: Option<&str>,
) -> anyhow::Result<()> {
    println!("=== AirCard CLI passcode ===");
    println!("theme     = {theme_path}");
    warn_blocking_apps();

    let udid = first_udid()?;
    cleanup_books(&udid)?;

    let forced = match telephony {
        Some("8") | Some("TelephonyUI-8") => Some("TelephonyUI-8"),
        Some("9") | Some("TelephonyUI-9") => Some("TelephonyUI-9"),
        Some("10") | Some("TelephonyUI-10") => Some("TelephonyUI-10"),
        _ => Some("TelephonyUI-10"),
    };
    let lang = language.unwrap_or("English");
    println!("telephony = {}", forced.unwrap_or("TelephonyUI-10"));
    println!("language  = {lang} (Bold Text caches included)");

    let theme = passthm::parse_passthm_file(std::path::Path::new(theme_path), forced, lang)?;
    println!("assets    = {}", theme.items.len());

    flasher::flash_passcode_theme(
        &udid,
        &theme.items,
        |step, total, msg| println!("progress [{step}/{total}] {msg}"),
        |msg| println!("log: {msg}"),
    )?;

    println!("DONE — lock the iPhone to view the keypad (Bold Text ON or OFF).");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::probe_command;

    #[test]
    fn probe_requires_exact_cli_arguments() {
        let args = |values: &[&str]| {
            values
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            probe_command(&args(&["aircard.exe", "--probe"])),
            Some(true)
        );
        assert_eq!(
            probe_command(&args(&["aircard.exe", "--probe", "extra"])),
            Some(false)
        );
        assert_eq!(
            probe_command(&args(&["aircard.exe", "--flash", "hash", "art.png"])),
            None
        );
    }
}
