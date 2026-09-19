#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod afc;
mod airlift;
mod airtraffic;
mod app;
mod apple;
mod device;
mod flasher;
mod image_skin;
mod passthm;
mod scanner;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 4 && args[1] == "--flash" {
        match cli_flash(&args[2], &args[3]) {
            Ok(()) => return Ok(()),
            Err(e) => {
                eprintln!("Error: {e:#}");
                std::process::exit(1);
            }
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 620.0])
            .with_min_inner_size([850.0, 560.0])
            .with_title("AirCard v1.2.1"),
        ..Default::default()
    };

    eframe::run_native(
        "AirCard v1.2.1",
        options,
        Box::new(|cc| Ok(Box::new(app::AirCardApp::new(cc)))),
    )
}

fn cli_flash(card_hash: &str, image_path: &str) -> anyhow::Result<()> {
    println!("=== AirCard CLI flash ===");
    println!("card_hash = {card_hash}");
    println!("image     = {image_path}");

    let devices = device::list_connected_devices()?;
    if devices.is_empty() {
        anyhow::bail!("No iPhone detected via usbmux / Apple Mobile Device Support");
    }
    for d in &devices {
        println!("device    = {d}  udid={}", d.udid);
    }
    let udid = devices[0].udid.clone();

    // Clear leftover Books sync / airlift staging from prior failed attempts.
    {
        println!("cleanup   = clearing leftover Books/airlift staging...");
        let session = device::ActiveDeviceSession::open(Some(&udid))?;
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
    }

    // Diagnostic: probe whether Book dataclass sync is accepted at all (no exploit payload).
    if std::env::var("AIRCARD_PROBE_ONLY").as_deref() == Ok("1") {
        println!("probe     = AirTraffic Book sync only (no staging / no Books.plist poison)");
        airtraffic::sync_assets_via_airtraffic(
            &udid,
            &[],
            |msg| println!("log: {msg}"),
        )?;
        println!("probe DONE — ReadyForSync path works on this device/PC pair");
        return Ok(());
    }

    let skin = image_skin::PreparedSkin::from_path(std::path::Path::new(image_path))?;
    println!(
        "skin      = {}x{} -> 1536x969 png={} bytes",
        skin.source_width,
        skin.source_height,
        skin.png.len()
    );

    println!("Keep iPhone UNLOCKED, screen ON. Open Books once if prompted.");
    println!("Starting flash...");

    flasher::flash_wallet_skin(
        &udid,
        card_hash,
        &skin.png,
        |step, total, msg| println!("progress [{step}/{total}] {msg}"),
        |msg| println!("log: {msg}"),
    )?;

    println!("DONE — force-close Wallet on iPhone and reopen.");
    Ok(())
}
