#[derive(Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Lang {
    #[default]
    En,
    Id,
}

impl Lang {
    pub fn cycle(self) -> Self {
        match self {
            Lang::En => Lang::Id,
            Lang::Id => Lang::En,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "EN",
            Lang::Id => "ID",
        }
    }
}

#[derive(Clone, Copy)]
pub struct T {
    pub wallet: &'static str,
    pub passcode: &'static str,
    pub help: &'static str,
    pub no_device: &'static str,
    pub logs: &'static str,
    pub scan: &'static str,
    pub stop: &'static str,
    pub scanning: &'static str,
    pub scanning_hint: &'static str,
    pub card_config: &'static str,
    pub card_config_sub: &'static str,
    pub target_hash: &'static str,
    pub saved_cards: &'static str,
    pub select: &'static str,
    pub rename: &'static str,
    pub reapply: &'static str,
    pub revert: &'static str,
    pub artwork: &'static str,
    pub artwork_sub: &'static str,
    pub choose_image: &'static str,
    pub export_png: &'static str,
    pub crop: &'static str,
    pub zoom: &'static str,
    pub pan_x: &'static str,
    pub pan_y: &'static str,
    pub write_iphone: &'static str,
    pub apply_skin: &'static str,
    pub preview: &'static str,
    pub force_close: &'static str,
    pub itunes_warn: &'static str,
    pub theme_title: &'static str,
    pub theme_sub: &'static str,
    pub choose_passthm: &'static str,
    pub creator: &'static str,
    pub poster: &'static str,
    pub slice_wallpaper: &'static str,
    pub flash_sliced: &'static str,
    pub ios_cache: &'static str,
    pub keypad_lang: &'static str,
    pub apply_passcode: &'static str,
    pub bold_note: &'static str,
    pub ready: &'static str,
}

pub fn t(lang: Lang) -> T {
    match lang {
        Lang::En => T {
            wallet: "Wallet",
            passcode: "Passcode",
            help: "Help",
            no_device: "No device",
            logs: "Logs",
            scan: "Scan",
            stop: "Stop",
            scanning: "Scanning syslog...",
            scanning_hint: "Open Wallet on iPhone and tap your card",
            card_config: "Card Configuration",
            card_config_sub: "Target your card and choose replacement artwork",
            target_hash: "Target Card Hash",
            saved_cards: "Saved cards",
            select: "Select...",
            rename: "Rename",
            reapply: "Apply last image",
            revert: "Revert last AirCard skin",
            artwork: "Card Skin Artwork",
            artwork_sub: "PNG, JPG, WebP, PDF — pan/zoom then flash",
            choose_image: "Choose Image...",
            export_png: "Export PNG",
            crop: "Frame",
            zoom: "Zoom",
            pan_x: "Pan X",
            pan_y: "Pan Y",
            write_iphone: "Write to iPhone",
            apply_skin: "Apply Card Skin",
            preview: "Wallet Preview",
            force_close: "After applying, force close Apple Wallet and reopen it.",
            itunes_warn: "Quit iTunes / Apple Devices first — they block AirTraffic.",
            theme_title: "Passcode Theme",
            theme_sub: "Cowabunga/Nugget .passthm, or slice a wallpaper (Mac-style)",
            choose_passthm: "Choose .passthm...",
            creator: "Theme Creator",
            poster: "Poster Slice",
            slice_wallpaper: "Choose wallpaper...",
            flash_sliced: "Flash sliced keypad",
            ios_cache: "Target iOS Cache",
            keypad_lang: "Keypad Language",
            apply_passcode: "Apply Passcode Theme",
            bold_note: "Bold Text caches are flashed too (--white-bold). Lock the phone to view.",
            ready: "Ready",
        },
        Lang::Id => T {
            wallet: "Dompet",
            passcode: "Kode sandi",
            help: "Bantuan",
            no_device: "Tidak ada perangkat",
            logs: "Log",
            scan: "Pindai",
            stop: "Stop",
            scanning: "Memindai syslog...",
            scanning_hint: "Buka Wallet di iPhone lalu ketuk kartunya",
            card_config: "Pengaturan kartu",
            card_config_sub: "Pilih kartu dan gambar pengganti",
            target_hash: "Hash kartu",
            saved_cards: "Kartu tersimpan",
            select: "Pilih...",
            rename: "Ganti nama",
            reapply: "Pasang gambar terakhir",
            revert: "Kembalikan kulit AirCard terakhir",
            artwork: "Gambar kartu",
            artwork_sub: "PNG, JPG, WebP, PDF — geser/zoom lalu pasang",
            choose_image: "Pilih gambar...",
            export_png: "Ekspor PNG",
            crop: "Bingkai",
            zoom: "Zoom",
            pan_x: "Geser X",
            pan_y: "Geser Y",
            write_iphone: "Tulis ke iPhone",
            apply_skin: "Pasang kulit kartu",
            preview: "Pratinjau Wallet",
            force_close: "Setelah selesai, paksa tutup Wallet lalu buka lagi.",
            itunes_warn: "Tutup iTunes / Apple Devices dulu — mereka memblokir AirTraffic.",
            theme_title: "Tema kode sandi",
            theme_sub: ".passthm Cowabunga/Nugget, atau potong wallpaper (gaya Mac)",
            choose_passthm: "Pilih .passthm...",
            creator: "Pembuat tema",
            poster: "Potong poster",
            slice_wallpaper: "Pilih wallpaper...",
            flash_sliced: "Pasang keypad hasil potong",
            ios_cache: "Cache iOS",
            keypad_lang: "Bahasa keypad",
            apply_passcode: "Pasang tema kode sandi",
            bold_note: "Cache Teks Tebal ikut dipasang (--white-bold). Kunci layar untuk melihat.",
            ready: "Siap",
        },
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub lang: Lang,
}

impl Default for Settings {
    fn default() -> Self {
        Self { lang: Lang::En }
    }
}

fn settings_path() -> std::path::PathBuf {
    let local = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| r"C:\Users\Default\AppData\Local".to_string());
    let dir = std::path::PathBuf::from(local).join("AirCard");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("settings.json")
}

pub fn load_settings() -> Settings {
    let path = settings_path();
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Ok(s) = serde_json::from_str(&text) {
            return s;
        }
    }
    Settings::default()
}

pub fn save_settings(settings: &Settings) {
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(settings_path(), json);
    }
}
