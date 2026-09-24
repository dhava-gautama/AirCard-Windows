use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::thread;

use eframe::egui;

use crate::airtraffic;
use crate::apple;
use crate::device::{DeviceInfo, list_connected_devices};
use crate::flasher::{WalletArt, flash_passcode_theme, flash_wallet_skin, load_previous_png};
use crate::host_guard;
use crate::i18n::{self, Lang, save_settings};
use crate::image_skin::PreparedSkin;
use crate::keypad::{load_individual_keys, slice_poster, sliced_to_passthm_items};
use crate::passthm::{PasscodeTheme, parse_passthm_file};
use crate::scanner::{
    SavedCard, load_saved_cards, rename_card, scan_syslog_for_cards, set_card_last_image,
};

#[derive(PartialEq, Eq)]
enum AppTab {
    Wallet,
    Passcode,
    Help,
}

enum BackgroundTaskMessage {
    Progress { step: usize, total: usize, message: String },
    Log(String),
    CardFound { hash: String, name: String },
    Done(Result<String, String>),
}

#[cfg(windows)]
fn current_timestamp() -> String {
    #[repr(C)]
    struct SystemTime {
        w_year: u16,
        w_month: u16,
        w_day_of_week: u16,
        w_day: u16,
        w_hour: u16,
        w_minute: u16,
        w_second: u16,
        w_milliseconds: u16,
    }
    unsafe extern "system" {
        fn GetLocalTime(lpSystemTime: *mut SystemTime);
    }
    let mut st = std::mem::MaybeUninit::<SystemTime>::uninit();
    unsafe {
        GetLocalTime(st.as_mut_ptr());
        let st = st.assume_init();
        format!(
            "{:02}:{:02}:{:02}.{:03}",
            st.w_hour, st.w_minute, st.w_second, st.w_milliseconds
        )
    }
}

#[cfg(not(windows))]
fn current_timestamp() -> String {
    "00:00:00.000".to_string()
}

pub struct AirCardApp {
    current_tab: AppTab,
    apple_status: String,
    apple_ready: bool,

    // Device management
    devices: Vec<DeviceInfo>,
    selected_udid: Option<String>,

    // Wallet tab
    card_hash: String,
    saved_cards: Vec<SavedCard>,
    source_path: Option<PathBuf>,
    skin: Option<PreparedSkin>,
    skin_texture: Option<egui::TextureHandle>,
    scanning_syslog: bool,
    scan_stop_flag: Option<Arc<AtomicBool>>,

    // Passcode tab
    theme_path: Option<PathBuf>,
    loaded_theme: Option<PasscodeTheme>,
    forced_telephony_ver: String,
    keypad_language: String,
    keypad_textures: Vec<(String, egui::TextureHandle)>,
    poster_zoom: f32,
    poster_pan_x: f32,
    poster_pan_y: f32,
    sliced_keys: Option<std::collections::HashMap<String, Vec<u8>>>,

    lang: Lang,
    card_name_edit: String,
    crop_zoom: f32,
    crop_pan_x: f32,
    crop_pan_y: f32,
    pdf_bytes: Option<Vec<u8>>,
    blocking_apps: Vec<String>,
    last_guard_check: std::time::Instant,

    // Worker thread & progress
    is_busy: bool,
    progress_step: usize,
    progress_total: usize,
    progress_msg: String,
    status_msg: String,
    task_rx: Option<Receiver<BackgroundTaskMessage>>,
    logs: Vec<String>,
    show_logs_window: bool,
}

impl AirCardApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_custom_fonts(&cc.egui_ctx);
        setup_custom_theme(&cc.egui_ctx);

        let (apple_ready, apple_status) = match apple::verify_support() {
            Ok(msg) => (true, msg),
            Err(err) => (false, err.to_string()),
        };

        let settings = i18n::load_settings();
        let mut app = Self {
            current_tab: AppTab::Wallet,
            apple_status,
            apple_ready,

            devices: Vec::new(),
            selected_udid: None,

            card_hash: String::new(),
            saved_cards: load_saved_cards(),
            source_path: None,
            skin: None,
            skin_texture: None,
            scanning_syslog: false,
            scan_stop_flag: None,

            theme_path: None,
            loaded_theme: None,
            forced_telephony_ver: "Auto (TelephonyUI-10)".to_string(),
            keypad_language: "Russian (Русский)".to_string(),
            keypad_textures: Vec::new(),
            poster_zoom: 1.0,
            poster_pan_x: 0.0,
            poster_pan_y: 0.0,
            sliced_keys: None,

            lang: settings.lang,
            card_name_edit: String::new(),
            crop_zoom: 1.0,
            crop_pan_x: 0.0,
            crop_pan_y: 0.0,
            pdf_bytes: None,
            blocking_apps: host_guard::blocking_sync_apps(),
            last_guard_check: std::time::Instant::now(),

            is_busy: false,
            progress_step: 0,
            progress_total: 0,
            progress_msg: String::new(),
            status_msg: "Ready. Connect iPhone via USB and unlock it.".to_string(),
            task_rx: None,
            logs: Vec::new(),
            show_logs_window: false,
        };

        app.add_log("AirCard Windows v1.3.1 initialized");
        app.add_log(format!("Apple Support Runtime: {}", if app.apple_ready { "Loaded and operational" } else { "Not found (iTunes required)" }));
        app.add_log(format!("Loaded {} saved card(s) from database", app.saved_cards.len()));

        if app.apple_ready {
            app.refresh_devices();
        }

        app
    }

    fn add_log(&mut self, text: impl AsRef<str>) {
        let ts = current_timestamp();
        self.logs.push(format!("[{}] {}", ts, text.as_ref()));
        if self.logs.len() > 1000 {
            self.logs.remove(0);
        }
    }

    fn refresh_devices(&mut self) {
        self.add_log("Scanning for connected iOS devices via usbmuxd...");
        match list_connected_devices() {
            Ok(devs) => {
                self.devices = devs;
                if self.selected_udid.is_none() && !self.devices.is_empty() {
                    self.selected_udid = Some(self.devices[0].udid.clone());
                }
                if self.devices.is_empty() {
                    self.add_log("No devices detected. Please plug in your iPhone and tap 'Trust this Computer'.");
                    self.status_msg = "No devices connected via USB.".to_string();
                } else {
                    let dev_logs: Vec<String> = self.devices.iter().enumerate().map(|(i, d)| {
                        format!("Device #{}: {} - UDID: {}", i + 1, d, d.udid)
                    }).collect();
                    for line in dev_logs {
                        self.add_log(line);
                    }
                    self.status_msg = format!("Found {} connected device(s)", self.devices.len());
                }
            }
            Err(err) => {
                self.add_log(format!("Device scan error: {}", err));
                self.status_msg = format!("Could not enumerate devices: {}", err);
            }
        }
    }

    fn refresh_guard(&mut self) {
        self.blocking_apps = host_guard::blocking_sync_apps();
        self.last_guard_check = std::time::Instant::now();
        if !self.blocking_apps.is_empty() {
            self.add_log(format!(
                "Quit {} before flashing — they steal the AirTraffic session",
                self.blocking_apps.join(", ")
            ));
        }
    }

    fn cycle_lang(&mut self) {
        self.lang = self.lang.cycle();
        save_settings(&i18n::Settings { lang: self.lang });
    }

    fn apply_skin_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext == "pdf" {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    self.add_log(format!(
                        "Loaded PDF artwork: {} ({:.1} KB) — written as cardBackgroundCombined.pdf",
                        path.display(),
                        bytes.len() as f32 / 1024.0
                    ));
                    self.pdf_bytes = Some(bytes);
                    self.skin = None;
                    self.skin_texture = None;
                    self.source_path = Some(path);
                    self.status_msg = "PDF artwork ready (Suica / transit).".to_string();
                }
                Err(err) => {
                    self.status_msg = format!("Could not read PDF: {err}");
                }
            }
            return;
        }

        self.pdf_bytes = None;
        self.add_log(format!("Opening skin image: {}", path.display()));
        match PreparedSkin::from_path_framed(&path, self.crop_zoom, self.crop_pan_x, self.crop_pan_y)
        {
            Ok(skin) => {
                self.add_log(format!(
                    "Skin processed: source {}x{} resampled to 1536x969 PNG ({:.1} KB)",
                    skin.source_width,
                    skin.source_height,
                    skin.png.len() as f32 / 1024.0,
                ));
                self.skin_texture = Some(ctx.load_texture(
                    "card-skin-preview",
                    skin.preview.clone(),
                    egui::TextureOptions::LINEAR,
                ));
                self.status_msg = format!(
                    "Prepared {} ({}x{} -> 1536x969 PNG, {:.1} KB)",
                    path.file_name().and_then(|n| n.to_str()).unwrap_or("image"),
                    skin.source_width,
                    skin.source_height,
                    skin.png.len() as f32 / 1024.0,
                );
                self.source_path = Some(path);
                self.skin = Some(skin);
            }
            Err(error) => {
                self.add_log(format!("Image preparation failed: {error:#}"));
                self.status_msg = format!("Could not prepare image: {error:#}");
            }
        }
    }

    fn reframe_skin(&mut self, ctx: &egui::Context) {
        if self.pdf_bytes.is_some() {
            return;
        }
        let Some(path) = self.source_path.clone() else {
            return;
        };
        if let Ok(skin) =
            PreparedSkin::from_path_framed(&path, self.crop_zoom, self.crop_pan_x, self.crop_pan_y)
        {
            self.skin_texture = Some(ctx.load_texture(
                "card-skin-preview",
                skin.preview.clone(),
                egui::TextureOptions::LINEAR,
            ));
            self.skin = Some(skin);
        }
    }

    fn select_skin(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Artwork", &["png", "jpg", "jpeg", "webp", "pdf"])
            .pick_file()
        else {
            return;
        };
        self.crop_zoom = 1.0;
        self.crop_pan_x = 0.0;
        self.crop_pan_y = 0.0;
        self.apply_skin_path(ctx, path);
    }

    fn reapply_last_image(&mut self, ctx: &egui::Context) {
        let path = self
            .saved_cards
            .iter()
            .find(|c| c.hash == self.card_hash)
            .and_then(|c| c.last_image.clone());
        let Some(path) = path else {
            self.status_msg = "No last image saved for this card.".to_string();
            return;
        };
        self.apply_skin_path(ctx, PathBuf::from(path));
    }

    fn persist_card_name(&mut self) {
        let hash = self.card_hash.trim().to_string();
        if hash.is_empty() || self.card_name_edit.trim().is_empty() {
            return;
        }
        rename_card(&hash, &self.card_name_edit);
        self.saved_cards = load_saved_cards();
    }

    fn save_prepared_png(&mut self) {
        let Some(skin) = &self.skin else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("aircard-skin.png")
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, &skin.png) {
            Ok(()) => {
                self.add_log(format!("Exported prepared card skin PNG: {}", path.display()));
                self.status_msg = format!("Saved prepared PNG: {}", path.display());
            }
            Err(err) => {
                self.add_log(format!("Failed to save PNG: {err}"));
                self.status_msg = format!("Could not save PNG: {err}");
            }
        }
    }

    fn toggle_syslog_scan(&mut self) {
        if self.scanning_syslog {
            if let Some(flag) = self.scan_stop_flag.take() {
                flag.store(true, Ordering::Relaxed);
            }
            self.scanning_syslog = false;
            self.add_log("Syslog scanning stopped by user.");
            self.status_msg = "Syslog scanning stopped.".to_string();
            return;
        }

        let stop_flag = Arc::new(AtomicBool::new(false));
        self.scan_stop_flag = Some(Arc::clone(&stop_flag));
        self.scanning_syslog = true;
        self.add_log("Initiating syslog monitor session...");
        self.status_msg = "Scanning syslog... Open Wallet or tap your card on iPhone.".to_string();

        let (tx, rx) = channel();
        self.task_rx = Some(rx);
        let udid = self.selected_udid.clone();

        thread::spawn(move || {
            let tx_card = tx.clone();
            let tx_log = tx.clone();
            let res = scan_syslog_for_cards(
                udid.as_deref(),
                stop_flag,
                move |hash, name| {
                    let _ = tx_card.send(BackgroundTaskMessage::CardFound { hash, name });
                },
                move |msg| {
                    let _ = tx_log.send(BackgroundTaskMessage::Log(msg));
                },
            );
            match res {
                Ok(()) => {
                    let _ = tx.send(BackgroundTaskMessage::Done(Ok("Syslog scan finished".into())));
                }
                Err(e) => {
                    let _ = tx.send(BackgroundTaskMessage::Done(Err(e.to_string())));
                }
            }
        });
    }

    fn flash_card(&mut self) {
        let Some(udid) = self.selected_udid.clone() else {
            self.add_log("Flash failed: No connected iPhone selected.");
            self.status_msg = "Please select a connected iPhone.".to_string();
            return;
        };
        let hash = self.card_hash.trim().to_string();
        if hash.is_empty() {
            self.add_log("Flash failed: Target card hash is empty.");
            self.status_msg = "Please enter or scan a target card hash.".to_string();
            return;
        }
        let Some(skin) = self.skin.as_ref() else {
            if self.pdf_bytes.is_none() {
                self.add_log("Flash failed: No skin image prepared.");
                self.status_msg = "Please choose a card skin image first.".to_string();
                return;
            }
            self.start_wallet_flash(udid, hash, WalletArt::Pdf(self.pdf_bytes.clone().unwrap()));
            return;
        };

        let png_bytes = skin.png.clone();
        if let Some(path) = &self.source_path {
            set_card_last_image(&hash, path);
            self.saved_cards = load_saved_cards();
        }
        self.start_wallet_flash(udid, hash, WalletArt::Png(png_bytes));
    }

    fn revert_card(&mut self) {
        let Some(udid) = self.selected_udid.clone() else {
            self.status_msg = "Please select a connected iPhone.".to_string();
            return;
        };
        let hash = self.card_hash.trim().to_string();
        if hash.is_empty() {
            self.status_msg = "Please enter or scan a target card hash.".to_string();
            return;
        }
        let Some(png) = load_previous_png(&hash) else {
            self.status_msg = "No previous AirCard skin stored for this card.".to_string();
            self.add_log("Revert skipped: apply a skin twice to keep a previous copy.");
            return;
        };
        self.start_wallet_flash(udid, hash, WalletArt::Png(png));
    }

    fn start_wallet_flash(&mut self, udid: String, hash: String, art: WalletArt) {
        if let Some(ref flag) = self.scan_stop_flag {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.scanning_syslog = false;
        self.is_busy = true;
        self.progress_step = 0;
        self.progress_total = 3;
        self.progress_msg = "Initiating card flash...".to_string();
        self.status_msg = "Writing card skin to iPhone...".to_string();
        self.add_log(format!("Starting card skin flash for hash: {} (UDID: {})", hash, udid));
        self.refresh_guard();

        let (tx, rx) = channel();
        self.task_rx = Some(rx);

        thread::spawn(move || {
            let tx_progress = tx.clone();
            let tx_log = tx.clone();
            let res = flash_wallet_skin(
                &udid,
                &hash,
                &art,
                move |step, total, msg| {
                    let _ = tx_progress.send(BackgroundTaskMessage::Progress {
                        step,
                        total,
                        message: msg.to_string(),
                    });
                },
                move |msg| {
                    let _ = tx_log.send(BackgroundTaskMessage::Log(msg.to_string()));
                },
            );

            match res {
                Ok(()) => {
                    let _ = tx.send(BackgroundTaskMessage::Done(Ok(
                        "Card skin successfully flashed! Force quit Wallet on iPhone and reopen it.".into(),
                    )));
                }
                Err(e) => {
                    let _ = tx.send(BackgroundTaskMessage::Done(Err(format!("{:#}", e))));
                }
            }
        });
    }

    fn select_theme_file(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Passcode Theme", &["passthm", "passtheme", "zip"])
            .pick_file()
        else {
            return;
        };

        self.load_theme_from_path(ctx, &path);
    }

    fn load_theme_from_path(&mut self, ctx: &egui::Context, path: &Path) {
        self.add_log(format!("Opening passcode theme package: {}", path.display()));
        let target_ver = match self.forced_telephony_ver.as_str() {
            "TelephonyUI-10" => Some("TelephonyUI-10"),
            "TelephonyUI-9" => Some("TelephonyUI-9"),
            "TelephonyUI-8" => Some("TelephonyUI-8"),
            _ => Some("TelephonyUI-10"),
        };

        match parse_passthm_file(path, target_ver, &self.keypad_language) {
            Ok(theme) => {
                self.keypad_textures.clear();
                for (digit, bytes) in &theme.key_previews {
                    if let Ok(img) = image::load_from_memory(bytes) {
                        let rgba = img.to_rgba8();
                        let color_image = egui::ColorImage::from_rgba_unmultiplied(
                            [rgba.width() as usize, rgba.height() as usize],
                            &rgba,
                        );
                        let tex = ctx.load_texture(
                            format!("keypad-{}", digit),
                            color_image,
                            egui::TextureOptions::LINEAR,
                        );
                        self.keypad_textures.push((digit.clone(), tex));
                    }
                }
                self.keypad_textures.sort_by(|a, b| a.0.cmp(&b.0));

                self.add_log(format!(
                    "Passcode theme loaded: '{}' (telephony: {}, lang: {}, {} button asset pairs)",
                    theme.name,
                    theme.detected_version,
                    self.keypad_language,
                    theme.items.len()
                ));
                self.status_msg = format!(
                    "Loaded '{}' with {} assets (target: {}, lang: {})",
                    theme.name,
                    theme.items.len(),
                    theme.detected_version,
                    self.keypad_language
                );
                self.theme_path = Some(path.to_path_buf());
                self.loaded_theme = Some(theme);
            }
            Err(err) => {
                self.add_log(format!("Failed to parse theme: {err:#}"));
                self.status_msg = format!("Failed to parse theme: {err:#}");
            }
        }
    }

    fn telephony_target(&self) -> &'static str {
        match self.forced_telephony_ver.as_str() {
            "TelephonyUI-9" => "TelephonyUI-9",
            "TelephonyUI-8" => "TelephonyUI-8",
            _ => "TelephonyUI-10",
        }
    }

    fn set_sliced_preview(
        &mut self,
        ctx: &egui::Context,
        keys: std::collections::HashMap<String, Vec<u8>>,
    ) {
        self.keypad_textures.clear();
        for (digit, bytes) in &keys {
            if let Ok(img) = image::load_from_memory(bytes) {
                let rgba = img.to_rgba8();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [rgba.width() as usize, rgba.height() as usize],
                    &rgba,
                );
                let tex = ctx.load_texture(
                    format!("keypad-{}", digit),
                    color_image,
                    egui::TextureOptions::LINEAR,
                );
                self.keypad_textures.push((digit.clone(), tex));
            }
        }
        self.keypad_textures.sort_by(|a, b| a.0.cmp(&b.0));
        self.sliced_keys = Some(keys);
        let items = sliced_to_passthm_items(
            self.sliced_keys.as_ref().unwrap(),
            self.telephony_target(),
        );
        self.loaded_theme = Some(PasscodeTheme {
            name: "PosterSlice".into(),
            detected_version: self.telephony_target().into(),
            items,
            key_previews: self.sliced_keys.clone().unwrap_or_default(),
        });
        self.status_msg = format!(
            "Sliced keypad: {} keys, {} cache files (incl. Bold Text)",
            self.keypad_textures.len(),
            self.loaded_theme.as_ref().map(|t| t.items.len()).unwrap_or(0)
        );
    }

    fn select_poster(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "webp"])
            .pick_file()
        else {
            return;
        };
        match image::open(&path) {
            Ok(img) => match slice_poster(&img, self.poster_zoom, self.poster_pan_x, self.poster_pan_y)
            {
                Ok(keys) => {
                    self.add_log(format!("Poster-sliced 10 keypad buttons from {}", path.display()));
                    self.set_sliced_preview(ctx, keys);
                    self.theme_path = Some(path);
                }
                Err(err) => {
                    self.status_msg = format!("Slice failed: {err:#}");
                }
            },
            Err(err) => {
                self.status_msg = format!("Could not open wallpaper: {err}");
            }
        }
    }

    fn reslice_poster(&mut self, ctx: &egui::Context) {
        let Some(path) = self.theme_path.clone() else {
            return;
        };
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("passthm") || e.eq_ignore_ascii_case("zip"))
        {
            return;
        }
        if let Ok(img) = image::open(&path) {
            if let Ok(keys) = slice_poster(&img, self.poster_zoom, self.poster_pan_x, self.poster_pan_y)
            {
                self.set_sliced_preview(ctx, keys);
            }
        }
    }

    fn select_key_folder(&mut self, ctx: &egui::Context) {
        let Some(dir) = rfd::FileDialog::new().pick_folder() else {
            return;
        };
        match load_individual_keys(&dir) {
            Ok(keys) => {
                self.add_log(format!("Loaded 0.png…9.png from {}", dir.display()));
                self.set_sliced_preview(ctx, keys);
                self.theme_path = Some(dir);
            }
            Err(err) => {
                self.status_msg = format!("{err:#}");
            }
        }
    }

    fn flash_theme(&mut self) {
        let Some(udid) = self.selected_udid.clone() else {
            self.add_log("Theme flash failed: No connected iPhone selected.");
            self.status_msg = "Please select a connected iPhone.".to_string();
            return;
        };
        let Some(theme) = self.loaded_theme.as_ref() else {
            self.add_log("Theme flash failed: No .passthm theme loaded.");
            self.status_msg = "Please select a .passthm theme file first.".to_string();
            return;
        };

        let items = theme.items.clone();
        self.is_busy = true;
        self.progress_step = 0;
        self.progress_total = items.len();
        self.progress_msg = "Starting passcode theme flash...".to_string();
        self.status_msg = "Writing passcode theme buttons...".to_string();
        self.add_log(format!("Flashing passcode theme '{}' ({} button assets) to device {}", theme.name, items.len(), udid));
        self.refresh_guard();

        let (tx, rx) = channel();
        self.task_rx = Some(rx);

        thread::spawn(move || {
            let tx_progress = tx.clone();
            let tx_log = tx.clone();
            let res = flash_passcode_theme(
                &udid,
                &items,
                move |step, total, msg| {
                    let _ = tx_progress.send(BackgroundTaskMessage::Progress {
                        step,
                        total,
                        message: msg.to_string(),
                    });
                },
                move |msg| {
                    let _ = tx_log.send(BackgroundTaskMessage::Log(msg.to_string()));
                },
            );

            match res {
                Ok(()) => {
                    let _ = tx.send(BackgroundTaskMessage::Done(Ok(
                        "Passcode theme applied! Lock your iPhone to view the new keypad.".into(),
                    )));
                }
                Err(e) => {
                    let _ = tx.send(BackgroundTaskMessage::Done(Err(format!("{:#}", e))));
                }
            }
        });
    }

    fn probe_handshake(&mut self) {
        let Some(udid) = self.selected_udid.clone() else {
            self.add_log("Probe failed: No connected iPhone selected.");
            self.status_msg = "Please select a connected iPhone.".to_string();
            return;
        };
        self.refresh_guard();
        self.is_busy = true;
        self.progress_step = 0;
        self.progress_total = 1;
        self.progress_msg = "Probing AirTraffic handshake...".to_string();
        self.status_msg = "Handshake probe (no Wallet write)...".to_string();
        self.add_log("Starting AirTraffic handshake probe (no AFC cleanup, no asset write)");

        let (tx, rx) = channel();
        self.task_rx = Some(rx);
        thread::spawn(move || {
            let tx_log = tx.clone();
            let res = airtraffic::sync_assets_via_airtraffic(&udid, &[], move |msg| {
                let _ = tx_log.send(BackgroundTaskMessage::Log(msg.to_string()));
            });
            match res {
                Ok(()) => {
                    let _ = tx.send(BackgroundTaskMessage::Done(Ok(
                        "Handshake OK — ReadyForSync. No Wallet files were written.".into(),
                    )));
                }
                Err(e) => {
                    let _ = tx.send(BackgroundTaskMessage::Done(Err(format!("{:#}", e))));
                }
            }
        });
    }

    fn handle_messages(&mut self) {
        let mut messages = Vec::new();
        if let Some(ref rx) = self.task_rx {
            while let Ok(msg) = rx.try_recv() {
                messages.push(msg);
            }
        }

        let mut finished = false;
        for msg in messages {
            match msg {
                BackgroundTaskMessage::Progress { step, total, message } => {
                    self.progress_step = step;
                    self.progress_total = total;
                    self.progress_msg = message.clone();
                    let msg_str = format!("[{}/{}] {}", step, total, message);
                    self.add_log(&msg_str);
                    self.status_msg = msg_str;
                }
                BackgroundTaskMessage::Log(log_line) => {
                    self.add_log(log_line);
                }
                BackgroundTaskMessage::CardFound { hash, name } => {
                    self.card_hash = hash.clone();
                    self.saved_cards = load_saved_cards();
                    if let Some(card) = self.saved_cards.iter().find(|c| c.hash == hash) {
                        self.card_name_edit = card.name.clone();
                    }
                    let msg_str = format!("Card captured: {} ({})", name, hash);
                    self.add_log(&msg_str);
                    self.status_msg = msg_str;
                }
                BackgroundTaskMessage::Done(res) => {
                    self.is_busy = false;
                    self.scanning_syslog = false;
                    finished = true;
                    match res {
                        Ok(ok_msg) => {
                            self.add_log(format!("Operation completed: {}", ok_msg));
                            self.status_msg = ok_msg;
                        }
                        Err(err_msg) => {
                            self.add_log(format!("Operation failed: {}", err_msg));
                            self.status_msg = format!("Error: {}", err_msg);
                        }
                    }
                }
            }
        }
        if finished {
            self.task_rx = None;
        }
    }
}

pub mod md3 {
    use eframe::egui::Color32;

    // M3 Dark scheme
    pub const SURFACE: Color32 = Color32::from_rgb(18, 18, 20);
    pub const SURFACE_CONTAINER: Color32 = Color32::from_rgb(33, 31, 36);
    pub const SURFACE_CONTAINER_HIGH: Color32 = Color32::from_rgb(43, 41, 48);
    pub const SURFACE_CONTAINER_HIGHEST: Color32 = Color32::from_rgb(54, 52, 59);
    pub const ON_SURFACE: Color32 = Color32::from_rgb(230, 225, 229);
    pub const ON_SURFACE_VARIANT: Color32 = Color32::from_rgb(196, 199, 197);
    pub const OUTLINE: Color32 = Color32::from_rgb(147, 143, 153);
    pub const OUTLINE_VARIANT: Color32 = Color32::from_rgb(73, 69, 79);

    // Primary
    pub const PRIMARY: Color32 = Color32::from_rgb(208, 188, 255);
    pub const ON_PRIMARY: Color32 = Color32::from_rgb(56, 30, 114);
    pub const PRIMARY_CONTAINER: Color32 = Color32::from_rgb(79, 55, 139);
    pub const ON_PRIMARY_CONTAINER: Color32 = Color32::from_rgb(234, 221, 255);

    // Secondary
    pub const SECONDARY_CONTAINER: Color32 = Color32::from_rgb(74, 68, 88);
    pub const ON_SECONDARY_CONTAINER: Color32 = Color32::from_rgb(232, 222, 248);

    // Tertiary
    pub const TERTIARY_CONTAINER: Color32 = Color32::from_rgb(99, 59, 72);
    pub const ON_TERTIARY_CONTAINER: Color32 = Color32::from_rgb(255, 216, 228);

    // Error
    pub const ERROR: Color32 = Color32::from_rgb(242, 184, 181);
    pub const ERROR_CONTAINER: Color32 = Color32::from_rgb(140, 29, 24);

    // Extra
    pub const SUCCESS: Color32 = Color32::from_rgb(120, 220, 120);
}

fn draw_status_dot(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

fn setup_custom_fonts(_ctx: &egui::Context) {
    // default fonts only
}

fn setup_custom_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    visuals.panel_fill = md3::SURFACE;
    visuals.window_fill = md3::SURFACE;
    visuals.extreme_bg_color = md3::SURFACE_CONTAINER;
    visuals.faint_bg_color = md3::SURFACE_CONTAINER;

    visuals.window_corner_radius = 16.into();
    visuals.menu_corner_radius = 12.into();

    visuals.widgets.noninteractive.corner_radius = 12.into();
    visuals.widgets.noninteractive.bg_fill = md3::SURFACE_CONTAINER;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, md3::ON_SURFACE);

    visuals.widgets.inactive.bg_fill = md3::SURFACE_CONTAINER_HIGH;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, md3::ON_SURFACE_VARIANT);
    visuals.widgets.inactive.corner_radius = 12.into();

    visuals.widgets.hovered.bg_fill = md3::SURFACE_CONTAINER_HIGHEST;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, md3::ON_SURFACE);
    visuals.widgets.hovered.corner_radius = 12.into();

    visuals.widgets.active.bg_fill = md3::PRIMARY_CONTAINER;
    visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, md3::ON_PRIMARY_CONTAINER);
    visuals.widgets.active.corner_radius = 12.into();

    visuals.widgets.open.bg_fill = md3::SURFACE_CONTAINER_HIGHEST;
    visuals.widgets.open.corner_radius = 12.into();
    visuals.widgets.open.bg_stroke = egui::Stroke::NONE;

    visuals.selection.bg_fill = md3::PRIMARY_CONTAINER;
    visuals.selection.stroke = egui::Stroke::NONE;

    ctx.set_visuals(visuals);

    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(16.0, 8.0);
    });
}

fn m3_card<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(md3::SURFACE_CONTAINER)
        .corner_radius(16)
        .inner_margin(egui::Margin::same(20))
        .show(ui, |ui| ui.vertical(add_contents).inner)
        .inner
}

fn m3_button_filled(ui: &mut egui::Ui, label: &str) -> bool {
    let btn = egui::Button::new(
        egui::RichText::new(label).size(13.0).color(md3::ON_PRIMARY),
    )
    .fill(md3::PRIMARY)
    .corner_radius(20)
    .stroke(egui::Stroke::NONE);
    ui.add(btn).clicked()
}

fn m3_button_tonal(ui: &mut egui::Ui, label: &str) -> bool {
    let btn = egui::Button::new(
        egui::RichText::new(label).size(13.0).color(md3::ON_SECONDARY_CONTAINER),
    )
    .fill(md3::SECONDARY_CONTAINER)
    .corner_radius(20)
    .stroke(egui::Stroke::NONE);
    ui.add(btn).clicked()
}

fn m3_button_outlined(ui: &mut egui::Ui, label: &str) -> bool {
    let btn = egui::Button::new(
        egui::RichText::new(label).size(13.0).color(md3::PRIMARY),
    )
    .fill(egui::Color32::TRANSPARENT)
    .corner_radius(20)
    .stroke(egui::Stroke::new(1.0_f32, md3::OUTLINE));
    ui.add(btn).clicked()
}

fn m3_tab(ui: &mut egui::Ui, current: &mut AppTab, target: AppTab, label: &str) {
    let selected = *current == target;
    let (bg, fg) = if selected {
        (md3::SECONDARY_CONTAINER, md3::ON_SECONDARY_CONTAINER)
    } else {
        (egui::Color32::TRANSPARENT, md3::ON_SURFACE_VARIANT)
    };
    let btn = egui::Button::new(
        egui::RichText::new(label).size(12.5).color(fg),
    )
    .fill(bg)
    .corner_radius(20)
    .stroke(egui::Stroke::NONE);
    if ui.add(btn).clicked() {
        *current = target;
    }
}

impl eframe::App for AirCardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_messages();

        if self.last_guard_check.elapsed().as_secs() >= 3 {
            self.blocking_apps = host_guard::blocking_sync_apps();
            self.last_guard_check = std::time::Instant::now();
        }

        if self.is_busy || self.scanning_syslog {
            ctx.request_repaint();
        }

        let t = i18n::t(self.lang);

        // Top bar
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::new()
                    .fill(md3::SURFACE)
                    .inner_margin(egui::Margin::symmetric(20, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("AirCard")
                            .strong()
                            .size(18.0)
                            .color(md3::ON_SURFACE),
                    );
                    ui.label(
                        egui::RichText::new("v1.3.1")
                            .size(11.0)
                            .color(md3::ON_SURFACE_VARIANT),
                    );

                    ui.add_space(20.0);
                    m3_tab(ui, &mut self.current_tab, AppTab::Wallet, t.wallet);
                    m3_tab(ui, &mut self.current_tab, AppTab::Passcode, t.passcode);
                    m3_tab(ui, &mut self.current_tab, AppTab::Help, t.help);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if m3_button_outlined(ui, self.lang.code()) {
                            self.cycle_lang();
                        }
                        let can_probe = !self.is_busy && self.selected_udid.is_some();
                        ui.add_enabled_ui(can_probe, |ui| {
                            if m3_button_outlined(ui, t.probe) {
                                self.probe_handshake();
                            }
                        });
                        if m3_button_outlined(ui, "Refresh") {
                            self.refresh_devices();
                            self.refresh_guard();
                        }
                        ui.add_space(4.0);
                        let has_device = !self.devices.is_empty();
                        draw_status_dot(ui, if has_device { md3::SUCCESS } else { md3::ERROR });
                        if has_device {
                            let name = self.devices.iter()
                                .find(|d| Some(&d.udid) == self.selected_udid.as_ref())
                                .map(|d| d.name.clone())
                                .unwrap_or_else(|| "iPhone".into());
                            ui.label(egui::RichText::new(name).size(12.0).color(md3::ON_SURFACE))
                                .on_hover_text(&self.apple_status);
                        } else {
                            ui.label(egui::RichText::new(t.no_device).size(12.0).color(md3::ON_SURFACE_VARIANT))
                                .on_hover_text(&self.apple_status);
                        }
                    });
                });
            });

        // Status bar
        egui::TopBottomPanel::bottom("status_bar")
            .frame(
                egui::Frame::new()
                    .fill(md3::SURFACE)
                    .inner_margin(egui::Margin::symmetric(20, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let dot_col = if self.is_busy || self.scanning_syslog {
                        md3::PRIMARY
                    } else if self.status_msg.starts_with("Error") || self.status_msg.starts_with("Failed") {
                        md3::ERROR
                    } else {
                        md3::SUCCESS
                    };
                    draw_status_dot(ui, dot_col);
                    if self.is_busy || self.scanning_syslog { ui.spinner(); }
                    ui.label(egui::RichText::new(&self.status_msg).size(11.5).color(md3::ON_SURFACE_VARIANT));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn_text = if self.show_logs_window {
                            format!("{} [x]", t.logs)
                        } else {
                            t.logs.to_string()
                        };
                        let btn = egui::Button::new(
                            egui::RichText::new(btn_text).size(11.0).color(
                                if self.show_logs_window { md3::ON_PRIMARY_CONTAINER } else { md3::ON_SURFACE_VARIANT }
                            ),
                        )
                        .fill(if self.show_logs_window { md3::PRIMARY_CONTAINER } else { egui::Color32::TRANSPARENT })
                        .corner_radius(20)
                        .stroke(egui::Stroke::new(1.0_f32, if self.show_logs_window { md3::PRIMARY } else { md3::OUTLINE_VARIANT }));
                        if ui.add(btn).clicked() {
                            self.show_logs_window = !self.show_logs_window;
                        }
                    });
                });
            });

        // Central - same SURFACE fill as header/status for flat look
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(md3::SURFACE)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if !self.blocking_apps.is_empty() {
                        let warn = i18n::t(self.lang).itunes_warn;
                        let apps = self.blocking_apps.join(", ");
                        egui::Frame::new()
                            .fill(md3::ERROR_CONTAINER)
                            .corner_radius(12)
                            .inner_margin(egui::Margin::same(12))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(format!("{warn} ({apps})"))
                                        .size(12.0)
                                        .color(md3::ERROR),
                                );
                            });
                        ui.add_space(8.0);
                    }
                    match self.current_tab {
                        AppTab::Wallet => self.show_wallet_tab(ctx, ui),
                        AppTab::Passcode => self.show_passcode_tab(ctx, ui),
                        AppTab::Help => self.show_help_tab(ui),
                    }
                });
            });

        let mut show_logs = self.show_logs_window;
        let mut file_saved_msg: Option<String> = None;
        if show_logs {
            egui::Window::new(i18n::t(self.lang).logs)
                .open(&mut show_logs)
                .default_size([540.0, 300.0])
                .min_size([360.0, 180.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if m3_button_tonal(ui, "Copy Logs") {
                            ctx.copy_text(self.logs.join("\n"));
                        }
                        if m3_button_outlined(ui, "Save to File...") {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_file_name("aircard-diagnostics.log")
                                .add_filter("Log files", &["log", "txt"])
                                .save_file()
                            {
                                let content = self.logs.join("\r\n");
                                let _ = std::fs::write(&path, content);
                                file_saved_msg = Some(format!("Saved log file to {}", path.display()));
                            }
                        }
                        if m3_button_outlined(ui, "Clear") {
                            self.logs.clear();
                        }
                        ui.label(
                            egui::RichText::new(format!("{} entries", self.logs.len()))
                                .size(11.0)
                                .color(md3::ON_SURFACE_VARIANT),
                        );
                    });
                    ui.add_space(8.0);
                    egui::Frame::new()
                        .fill(md3::SURFACE_CONTAINER_HIGH)
                        .corner_radius(12)
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .stick_to_bottom(true)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    if self.logs.is_empty() {
                                        ui.label(egui::RichText::new("No events logged yet.").size(11.0).color(md3::ON_SURFACE_VARIANT));
                                    } else {
                                        for line in &self.logs {
                                            ui.label(
                                                egui::RichText::new(line)
                                                    .size(10.5)
                                                    .monospace()
                                                    .color(md3::ON_SURFACE),
                                            );
                                        }
                                    }
                                });
                        });
                });
            self.show_logs_window = show_logs;
            if let Some(msg) = file_saved_msg {
                self.add_log(msg);
            }
        }
    }
}

impl AirCardApp {
    fn show_wallet_tab(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let t = i18n::t(self.lang);
        if self.scanning_syslog {
            egui::Frame::new()
                .fill(md3::TERTIARY_CONTAINER)
                .corner_radius(16)
                .inner_margin(egui::Margin::same(16))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(t.scanning).strong().size(13.0).color(md3::ON_TERTIARY_CONTAINER));
                            ui.label(egui::RichText::new(t.scanning_hint).size(11.5).color(md3::ON_TERTIARY_CONTAINER));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let btn = egui::Button::new(egui::RichText::new(t.stop).size(12.0).color(md3::ON_SURFACE))
                                .fill(md3::ERROR_CONTAINER).corner_radius(20).stroke(egui::Stroke::NONE);
                            if ui.add(btn).clicked() { self.toggle_syslog_scan(); }
                        });
                    });
                });
            ui.add_space(8.0);
        }

        ui.columns(2, |cols| {
            let left = &mut cols[0];
            m3_card(left, |ui| {
                ui.label(egui::RichText::new(t.card_config).strong().size(16.0).color(md3::ON_SURFACE));
                ui.add_space(4.0);
                ui.label(egui::RichText::new(t.card_config_sub).size(12.0).color(md3::ON_SURFACE_VARIANT));
                ui.add_space(16.0);

                ui.label(egui::RichText::new(t.target_hash).strong().size(12.0).color(md3::ON_SURFACE));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let btn_w = 90.0;
                    let text_w = (ui.available_width() - btn_w - 12.0).max(150.0);
                    ui.add(egui::TextEdit::singleline(&mut self.card_hash).hint_text("Base64 pass hash...").desired_width(text_w));

                    let scan_label = if self.scanning_syslog { t.stop } else { t.scan };
                    let scan_bg = if self.scanning_syslog { md3::ERROR_CONTAINER } else { md3::PRIMARY_CONTAINER };
                    let scan_fg = if self.scanning_syslog { md3::ERROR } else { md3::ON_PRIMARY_CONTAINER };
                    let scan_btn = egui::Button::new(egui::RichText::new(scan_label).size(12.0).color(scan_fg))
                        .fill(scan_bg).corner_radius(20).stroke(egui::Stroke::NONE);
                    if ui.add(scan_btn).clicked() { self.toggle_syslog_scan(); }
                });

                if !self.saved_cards.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(t.saved_cards).size(11.0).color(md3::ON_SURFACE_VARIANT));
                    ui.add_space(2.0);
                    let combo_w = (ui.available_width() - 4.0).max(150.0);
                    let mut picked: Option<String> = None;
                    egui::ComboBox::from_id_salt("saved_cards_box")
                        .width(combo_w)
                        .selected_text(
                            self.saved_cards.iter()
                                .find(|c| c.hash == self.card_hash)
                                .map(|c| format!("{} ({})", c.name, &c.hash[..8.min(c.hash.len())]))
                                .unwrap_or_else(|| t.select.into()),
                        )
                        .show_ui(ui, |ui| {
                            for card in &self.saved_cards {
                                let label = format!("{} ({}...)", card.name, &card.hash[..8.min(card.hash.len())]);
                                if ui.selectable_label(self.card_hash == card.hash, label).clicked() {
                                    picked = Some(card.hash.clone());
                                }
                            }
                        });
                    if let Some(hash) = picked {
                        self.card_hash = hash.clone();
                        if let Some(card) = self.saved_cards.iter().find(|c| c.hash == hash) {
                            self.card_name_edit = card.name.clone();
                        }
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.card_name_edit).hint_text(t.rename).desired_width(160.0));
                        if m3_button_tonal(ui, t.rename) {
                            self.persist_card_name();
                        }
                        if m3_button_outlined(ui, t.reapply) {
                            self.reapply_last_image(ctx);
                        }
                    });
                }

                ui.add_space(16.0);

                ui.label(egui::RichText::new(t.artwork).strong().size(12.0).color(md3::ON_SURFACE));
                ui.label(egui::RichText::new(t.artwork_sub).size(11.0).color(md3::ON_SURFACE_VARIANT));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if m3_button_filled(ui, t.choose_image) { self.select_skin(ctx); }
                    if self.skin.is_some() {
                        if m3_button_tonal(ui, t.export_png) { self.save_prepared_png(); }
                    }
                });

                if self.pdf_bytes.is_some() {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("PDF → cardBackgroundCombined.pdf").size(11.0).color(md3::PRIMARY));
                } else if let Some(skin) = &self.skin {
                    ui.add_space(4.0);
                    let fname = self.source_path.as_ref()
                        .and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or("image");
                    ui.label(egui::RichText::new(format!("{} - 1536x969 - {:.0} KB", fname, skin.png.len() as f32 / 1024.0)).size(11.0).color(md3::PRIMARY));
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(t.crop).size(11.0).color(md3::ON_SURFACE_VARIANT));
                    let mut crop_changed = false;
                    crop_changed |= ui.add(egui::Slider::new(&mut self.crop_zoom, 1.0..=2.5).text(t.zoom)).changed();
                    crop_changed |= ui.add(egui::Slider::new(&mut self.crop_pan_x, -1.0..=1.0).text(t.pan_x)).changed();
                    crop_changed |= ui.add(egui::Slider::new(&mut self.crop_pan_y, -1.0..=1.0).text(t.pan_y)).changed();
                    if crop_changed {
                        self.reframe_skin(ctx);
                    }
                }

                ui.add_space(16.0);

                ui.label(egui::RichText::new(t.write_iphone).strong().size(12.0).color(md3::ON_SURFACE));
                ui.add_space(4.0);

                let can_flash = !self.is_busy && self.selected_udid.is_some() && !self.card_hash.trim().is_empty() && (self.skin.is_some() || self.pdf_bytes.is_some());
                let flash_btn = egui::Button::new(
                    egui::RichText::new(t.apply_skin).strong().size(14.0)
                        .color(if can_flash { md3::ON_PRIMARY } else { md3::ON_SURFACE_VARIANT }),
                )
                .fill(if can_flash { md3::PRIMARY } else { md3::SURFACE_CONTAINER_HIGH })
                .corner_radius(20).stroke(egui::Stroke::NONE)
                .min_size(egui::vec2(ui.available_width(), 40.0));

                let resp = ui.add_enabled(can_flash, flash_btn);
                if resp.clicked() { self.flash_card(); }
                if !can_flash {
                    let mut r = Vec::new();
                    if self.selected_udid.is_none() { r.push("connect iPhone"); }
                    if self.card_hash.trim().is_empty() { r.push("enter card hash"); }
                    if self.skin.is_none() && self.pdf_bytes.is_none() { r.push("choose image"); }
                    if !r.is_empty() { resp.on_disabled_hover_text(format!("Need: {}", r.join(", "))); }
                }

                ui.add_space(6.0);
                let can_revert = !self.is_busy && self.selected_udid.is_some() && !self.card_hash.trim().is_empty();
                if ui.add_enabled(can_revert, egui::Button::new(egui::RichText::new(t.revert).size(12.0).color(md3::ON_SECONDARY_CONTAINER)).fill(md3::SECONDARY_CONTAINER).corner_radius(20).stroke(egui::Stroke::NONE)).clicked() {
                    self.revert_card();
                }

                if self.is_busy {
                    ui.add_space(8.0);
                    if self.progress_total > 0 {
                        ui.add(egui::ProgressBar::new(self.progress_step as f32 / self.progress_total as f32).animate(true));
                    }
                    ui.label(egui::RichText::new(&self.progress_msg).size(11.0).color(md3::PRIMARY));
                }
            });

            let right = &mut cols[1];
            m3_card(right, |ui| {
                ui.label(egui::RichText::new(t.preview).strong().size(16.0).color(md3::ON_SURFACE));
                ui.add_space(4.0);
                ui.label(egui::RichText::new("1536 x 969 px pass canvas").size(12.0).color(md3::ON_SURFACE_VARIANT));
                ui.add_space(12.0);

                let pass_w = (ui.available_width() - 8.0).clamp(250.0, 400.0);
                let pass_h = pass_w * (969.0 / 1536.0);
                ui.vertical_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(pass_w, pass_h), egui::Sense::hover());
                    let painter = ui.painter();
                    if let Some(tex) = self.skin_texture.as_ref() {
                        painter.image(tex.id(), rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE);
                        painter.rect_stroke(rect, 16.0,
                            egui::Stroke::new(1.0_f32, egui::Color32::from_rgba_premultiplied(255, 255, 255, 30)),
                            egui::StrokeKind::Inside);
                    } else if self.pdf_bytes.is_some() {
                        painter.rect_filled(rect, 16.0, md3::SURFACE_CONTAINER_HIGH);
                        painter.text(rect.center(), egui::Align2::CENTER_CENTER,
                            "PDF artwork", egui::FontId::proportional(14.0), md3::PRIMARY);
                    } else {
                        painter.rect_filled(rect, 16.0, md3::SURFACE_CONTAINER_HIGH);
                        painter.text(rect.center(), egui::Align2::CENTER_CENTER,
                            "No artwork loaded", egui::FontId::proportional(14.0), md3::ON_SURFACE_VARIANT);
                    }
                });

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("1536x969").size(11.0).color(md3::ON_SURFACE_VARIANT));
                    ui.label(egui::RichText::new("|").size(11.0).color(md3::OUTLINE_VARIANT));
                    ui.label(egui::RichText::new("1.585 ratio").size(11.0).color(md3::ON_SURFACE_VARIANT));
                    ui.label(egui::RichText::new("|").size(11.0).color(md3::OUTLINE_VARIANT));
                    if self.skin.is_some() || self.pdf_bytes.is_some() {
                        ui.label(egui::RichText::new(t.ready).size(11.0).color(md3::SUCCESS));
                    } else {
                        ui.label(egui::RichText::new("No image").size(11.0).color(md3::ON_SURFACE_VARIANT));
                    }
                });
                ui.add_space(8.0);
                ui.label(egui::RichText::new(t.force_close).size(11.0).color(md3::ON_SURFACE_VARIANT));
            });
        });
    }

    fn show_passcode_tab(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let t = i18n::t(self.lang);
        ui.columns(2, |cols| {
            // Left: config
            let left = &mut cols[0];
            m3_card(left, |ui| {
                ui.label(egui::RichText::new(t.theme_title).strong().size(16.0).color(md3::ON_SURFACE));
                ui.add_space(4.0);
                ui.label(egui::RichText::new(t.theme_sub).size(12.0).color(md3::ON_SURFACE_VARIANT));
                ui.add_space(16.0);

                ui.label(egui::RichText::new("Theme Package").strong().size(12.0).color(md3::ON_SURFACE));
                ui.label(egui::RichText::new("Choose a .passthm archive containing dialer artwork").size(11.0).color(md3::ON_SURFACE_VARIANT));
                ui.add_space(4.0);
                if m3_button_filled(ui, t.choose_passthm) { self.select_theme_file(ctx); }

                if let Some(theme) = &self.loaded_theme {
                    let fname = self.theme_path.as_ref()
                        .and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or("theme.passthm");
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(format!("{} - {} assets", fname, theme.items.len())).size(11.0).color(md3::PRIMARY));
                }

                ui.add_space(12.0);
                ui.label(egui::RichText::new(t.creator).strong().size(12.0).color(md3::ON_SURFACE));
                ui.label(egui::RichText::new(t.poster).size(11.0).color(md3::ON_SURFACE_VARIANT));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if m3_button_tonal(ui, t.slice_wallpaper) { self.select_poster(ctx); }
                    if m3_button_outlined(ui, "0.png…9.png") { self.select_key_folder(ctx); }
                });
                if self.sliced_keys.is_some() {
                    ui.add_space(4.0);
                    let mut slice_changed = false;
                    slice_changed |= ui.add(egui::Slider::new(&mut self.poster_zoom, 1.0..=2.5).text(t.zoom)).changed();
                    slice_changed |= ui.add(egui::Slider::new(&mut self.poster_pan_x, -1.0..=1.0).text(t.pan_x)).changed();
                    slice_changed |= ui.add(egui::Slider::new(&mut self.poster_pan_y, -1.0..=1.0).text(t.pan_y)).changed();
                    if slice_changed {
                        self.reslice_poster(ctx);
                    }
                }

                ui.add_space(16.0);

                ui.label(egui::RichText::new(t.ios_cache).strong().size(12.0).color(md3::ON_SURFACE));
                ui.label(egui::RichText::new("Select cache format based on connected iOS version").size(11.0).color(md3::ON_SURFACE_VARIANT));
                ui.add_space(4.0);
                let combo_w = (ui.available_width() - 4.0).max(150.0);
                let mut ver_changed = false;
                egui::ComboBox::from_id_salt("telephony_combo")
                    .width(combo_w)
                    .selected_text(&self.forced_telephony_ver)
                    .show_ui(ui, |ui| {
                        ver_changed |= ui.selectable_value(&mut self.forced_telephony_ver, "Auto (TelephonyUI-10)".into(), "Auto (TelephonyUI-10)").clicked();
                        ver_changed |= ui.selectable_value(&mut self.forced_telephony_ver, "TelephonyUI-10".into(), "TelephonyUI-10 (iOS 18+)").clicked();
                        ver_changed |= ui.selectable_value(&mut self.forced_telephony_ver, "TelephonyUI-9".into(), "TelephonyUI-9 (iOS 16-17)").clicked();
                        ver_changed |= ui.selectable_value(&mut self.forced_telephony_ver, "TelephonyUI-8".into(), "TelephonyUI-8 (Legacy)").clicked();
                    });

                if ver_changed {
                    if let Some(path) = self.theme_path.clone() {
                        self.load_theme_from_path(ctx, &path);
                    }
                }

                ui.add_space(16.0);

                // Keypad Language
                ui.label(egui::RichText::new(t.keypad_lang).strong().size(12.0).color(md3::ON_SURFACE));
                ui.label(egui::RichText::new("Subtext alphabet layout (Russian Cyrillic, English, Ukrainian, or Universal)").size(11.0).color(md3::ON_SURFACE_VARIANT));
                ui.add_space(4.0);
                let mut lang_changed = false;
                egui::ComboBox::from_id_salt("keypad_lang_combo")
                    .width(combo_w)
                    .selected_text(&self.keypad_language)
                    .show_ui(ui, |ui| {
                        lang_changed |= ui.selectable_value(&mut self.keypad_language, "Russian (Русский)".into(), "Russian (Русский)").clicked();
                        lang_changed |= ui.selectable_value(&mut self.keypad_language, "English".into(), "English").clicked();
                        lang_changed |= ui.selectable_value(&mut self.keypad_language, "Ukrainian (Українська)".into(), "Ukrainian (Українська)").clicked();
                        lang_changed |= ui.selectable_value(&mut self.keypad_language, "All Languages (Universal)".into(), "All Languages (Universal)").clicked();
                    });

                if lang_changed {
                    if let Some(path) = self.theme_path.clone() {
                        self.load_theme_from_path(ctx, &path);
                    }
                }

                ui.add_space(16.0);

                // Apply
                ui.label(egui::RichText::new(t.write_iphone).strong().size(12.0).color(md3::ON_SURFACE));
                ui.add_space(4.0);

                let can_flash = !self.is_busy && self.selected_udid.is_some() && self.loaded_theme.is_some();
                let flash_btn = egui::Button::new(
                    egui::RichText::new(t.apply_passcode).strong().size(14.0)
                        .color(if can_flash { md3::ON_PRIMARY } else { md3::ON_SURFACE_VARIANT }),
                )
                .fill(if can_flash { md3::PRIMARY } else { md3::SURFACE_CONTAINER_HIGH })
                .corner_radius(20).stroke(egui::Stroke::NONE)
                .min_size(egui::vec2(ui.available_width(), 40.0));

                let resp = ui.add_enabled(can_flash, flash_btn);
                if resp.clicked() { self.flash_theme(); }
                if self.sliced_keys.is_some() {
                    ui.label(egui::RichText::new(t.flash_sliced).size(11.0).color(md3::ON_SURFACE_VARIANT));
                }
                if !can_flash {
                    let mut r = Vec::new();
                    if self.selected_udid.is_none() { r.push("connect iPhone"); }
                    if self.loaded_theme.is_none() { r.push("select theme"); }
                    if !r.is_empty() { resp.on_disabled_hover_text(format!("Need: {}", r.join(", "))); }
                }

                if self.is_busy {
                    ui.add_space(8.0);
                    if self.progress_total > 0 {
                        ui.add(egui::ProgressBar::new(self.progress_step as f32 / self.progress_total as f32).animate(true));
                    }
                    ui.label(egui::RichText::new(&self.progress_msg).size(11.0).color(md3::PRIMARY));
                }
            });

            // Right: preview
            let right = &mut cols[1];
            m3_card(right, |ui| {
                ui.label(egui::RichText::new("Keypad Preview").strong().size(16.0).color(md3::ON_SURFACE));
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Dialer button artwork").size(12.0).color(md3::ON_SURFACE_VARIANT));
                ui.add_space(12.0);

                let pass_w = (ui.available_width() - 8.0).clamp(250.0, 400.0);
                let pass_h = pass_w * (969.0 / 1536.0);

                ui.vertical_centered(|ui| {
                    if self.keypad_textures.is_empty() {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(pass_w, pass_h), egui::Sense::hover());
                        let painter = ui.painter();
                        painter.rect_filled(rect, 16.0, md3::SURFACE_CONTAINER_HIGH);
                        painter.text(rect.center(), egui::Align2::CENTER_CENTER,
                            "No theme loaded", egui::FontId::proportional(14.0), md3::ON_SURFACE_VARIANT);
                    } else {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(pass_w, pass_h), egui::Sense::hover());
                        let painter = ui.painter();
                        painter.rect_filled(rect, 16.0, md3::SURFACE_CONTAINER_HIGH);
                        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                            ui.vertical_centered(|ui| {
                                ui.add_space(10.0);
                                egui::Grid::new("keypad_grid").spacing([14.0, 8.0]).show(ui, |ui| {
                                    for (idx, (digit, tex)) in self.keypad_textures.iter().enumerate() {
                                        ui.vertical_centered(|ui| {
                                            egui::Frame::new()
                                                .fill(md3::SURFACE)
                                                .corner_radius(12).inner_margin(3)
                                                .show(ui, |ui| { ui.image((tex.id(), egui::vec2(40.0, 40.0))); });
                                            ui.label(egui::RichText::new(digit).size(9.5).color(md3::ON_SURFACE_VARIANT));
                                        });
                                        if (idx + 1) % 3 == 0 { ui.end_row(); }
                                    }
                                });
                            });
                        });
                    }
                });

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("3x4 Keypad").size(11.0).color(md3::ON_SURFACE_VARIANT));
                    ui.label(egui::RichText::new("|").size(11.0).color(md3::OUTLINE_VARIANT));
                    ui.label(egui::RichText::new("TelephonyUI").size(11.0).color(md3::ON_SURFACE_VARIANT));
                    ui.label(egui::RichText::new("|").size(11.0).color(md3::OUTLINE_VARIANT));
                    if self.loaded_theme.is_some() {
                        ui.label(egui::RichText::new("Ready").size(11.0).color(md3::SUCCESS));
                    } else {
                        ui.label(egui::RichText::new("No theme").size(11.0).color(md3::ON_SURFACE_VARIANT));
                    }
                });
                ui.add_space(8.0);
                ui.label(egui::RichText::new(t.bold_note).size(11.0).color(md3::ON_SURFACE_VARIANT));
            });
        });
    }

    fn show_help_tab(&mut self, ui: &mut egui::Ui) {
        ui.columns(2, |cols| {
            let left = &mut cols[0];
            m3_card(left, |ui| {
                ui.label(egui::RichText::new("Setup & Card Hash Guide").strong().size(16.0).color(md3::ON_SURFACE));
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Everything you need to connect and capture your card").size(12.0).color(md3::ON_SURFACE_VARIANT));
                ui.add_space(16.0);

                ui.label(egui::RichText::new("Prerequisites").strong().size(12.0).color(md3::ON_SURFACE));
                ui.add_space(6.0);
                ui.label(egui::RichText::new("- 64-bit iTunes or Apple Mobile Device Support installed").size(11.5).color(md3::ON_SURFACE_VARIANT));
                ui.label(egui::RichText::new("- Connect iPhone via USB-C or Lightning cable").size(11.5).color(md3::ON_SURFACE_VARIANT));
                ui.label(egui::RichText::new("- Unlock iPhone and tap \"Trust this Computer\"").size(11.5).color(md3::ON_SURFACE_VARIANT));
                ui.label(egui::RichText::new("- Probe (header) or aircard.exe --probe tests handshake without writing a skin").size(11.5).color(md3::ON_SURFACE_VARIANT));

                ui.add_space(18.0);

                ui.label(egui::RichText::new("Finding Your Card Hash").strong().size(12.0).color(md3::ON_SURFACE));
                ui.add_space(6.0);
                ui.label(egui::RichText::new("1. Click \"Scan\" in the Wallet tab").size(11.5).color(md3::ON_SURFACE_VARIANT));
                ui.label(egui::RichText::new("2. Open Apple Wallet on your iPhone").size(11.5).color(md3::ON_SURFACE_VARIANT));
                ui.label(egui::RichText::new("3. Tap the card you want to customize").size(11.5).color(md3::ON_SURFACE_VARIANT));
                ui.label(egui::RichText::new("4. AirCard captures the pass hash automatically").size(11.5).color(md3::ON_SURFACE_VARIANT));
                ui.label(egui::RichText::new("5. Click \"Stop\" once detected").size(11.5).color(md3::ON_SURFACE_VARIANT));
            });

            let right = &mut cols[1];
            m3_card(right, |ui| {
                ui.label(egui::RichText::new("Activation & Theme Guide").strong().size(16.0).color(md3::ON_SURFACE));
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Applying skins and dialer keypad packages").size(12.0).color(md3::ON_SURFACE_VARIANT));
                ui.add_space(16.0);

                ui.label(egui::RichText::new("Activating Apple Wallet Skin").strong().size(12.0).color(md3::ON_SURFACE));
                ui.add_space(6.0);
                ui.label(egui::RichText::new("1. Click \"Apply Card Skin\" and wait for completion").size(11.5).color(md3::ON_SURFACE_VARIANT));
                ui.label(egui::RichText::new("2. Open App Switcher on iPhone (swipe up from bottom)").size(11.5).color(md3::ON_SURFACE_VARIANT));
                ui.label(egui::RichText::new("3. Force close Apple Wallet by swiping up on it").size(11.5).color(md3::ON_SURFACE_VARIANT));
                ui.label(egui::RichText::new("4. Reopen Wallet - your new skin appears!").size(11.5).color(md3::ON_SURFACE_VARIANT));

                ui.add_space(18.0);

                ui.label(egui::RichText::new("Passcode Themes (.passthm)").strong().size(12.0).color(md3::ON_SURFACE));
                ui.add_space(6.0);
                ui.label(egui::RichText::new("- Compatible with Cowabunga & Nugget theme packages").size(11.5).color(md3::ON_SURFACE_VARIANT));
                ui.label(egui::RichText::new("- iOS 18+: Select \"Auto (TelephonyUI-10)\"").size(11.5).color(md3::ON_SURFACE_VARIANT));
                ui.label(egui::RichText::new("- iOS 16-17: Select \"TelephonyUI-9\"").size(11.5).color(md3::ON_SURFACE_VARIANT));
                ui.label(egui::RichText::new("- Lock screen to verify your updated keypad artwork").size(11.5).color(md3::ON_SURFACE_VARIANT));
            });
        });
    }
}
