//! Per-device original Wallet artwork backups. Never overwrite a saved original.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub const ORIGINAL_ASSETS: &[&str] = &[
    "cardBackgroundCombined@3x.png",
    "cardBackgroundCombined@2x.png",
    "cardBackgroundCombined.pdf",
];

fn backup_root() -> PathBuf {
    let local = std::env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| r"C:\Users\Default\AppData\Local".to_string());
    PathBuf::from(local).join("AirCard").join("wallet-originals")
}

fn safe_component(value: &str) -> String {
    let component: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if component.is_empty() {
        "unknown".into()
    } else {
        component
    }
}

pub fn backup_dir(udid: &str, card_hash: &str) -> PathBuf {
    backup_root().join(format!(
        "{}-{}",
        safe_component(udid),
        safe_component(card_hash)
    ))
}

pub fn backup_exists(udid: &str, card_hash: &str) -> bool {
    let dir = backup_dir(udid, card_hash);
    ORIGINAL_ASSETS.iter().any(|asset| dir.join(asset).is_file())
}

pub fn save_original_asset(udid: &str, card_hash: &str, leaf: &str, data: &[u8]) -> Result<bool> {
    if !ORIGINAL_ASSETS.contains(&leaf) {
        bail!("unsupported original asset {leaf}");
    }
    let dir = backup_dir(udid, card_hash);
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let path = dir.join(leaf);
    if path.is_file() {
        return Ok(false);
    }
    atomic_write(&path, data)?;
    Ok(true)
}

pub fn load_original_assets(udid: &str, card_hash: &str) -> Result<Vec<(String, Vec<u8>)>> {
    let dir = backup_dir(udid, card_hash);
    let mut assets = Vec::new();
    for leaf in ORIGINAL_ASSETS {
        let path = dir.join(leaf);
        if path.is_file() {
            let data = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
            if !data.is_empty() {
                assets.push(((*leaf).to_string(), data));
            }
        }
    }
    if assets.is_empty() {
        bail!("No original artwork backup for this card. Save original before flashing.");
    }
    Ok(assets)
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, data)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_overwrites_existing_original() {
        let udid = format!("test-udid-{}", std::process::id());
        let hash = "testHashNeverOverwrite";
        let dir = backup_dir(&udid, hash);
        let _ = fs::remove_dir_all(&dir);
        let leaf = "cardBackgroundCombined@3x.png";
        assert!(save_original_asset(&udid, hash, leaf, b"ORIGINAL").unwrap());
        assert!(!save_original_asset(&udid, hash, leaf, b"OVERWRITE").unwrap());
        assert_eq!(fs::read(dir.join(leaf)).unwrap(), b"ORIGINAL");
        let loaded = load_original_assets(&udid, hash).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].1, b"ORIGINAL");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn backup_dir_is_per_device() {
        let a = backup_dir("UDID-A", "hash+=/");
        let b = backup_dir("UDID-B", "hash+=/");
        assert_ne!(a, b);
        assert!(a.to_string_lossy().contains("UDID-A"));
    }
}
