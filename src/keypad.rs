//! Mac-style poster-slice: one wallpaper → 10 circular keypad buttons.

use std::collections::HashMap;
use std::io::Cursor;

use anyhow::{Context, Result, bail};
use image::{DynamicImage, GenericImageView, ImageFormat, Rgba, RgbaImage, imageops::FilterType};

use crate::passthm::{KEYPAD_SUBTEXTS_EN, export_passthm_archive};

const KEY_PX: u32 = 300;

/// 3×4 iOS keypad: 1 2 3 / 4 5 6 / 7 8 9 /   0
const KEYS: [(&str, u32, u32); 10] = [
    ("1", 0, 0),
    ("2", 1, 0),
    ("3", 2, 0),
    ("4", 0, 1),
    ("5", 1, 1),
    ("6", 2, 1),
    ("7", 0, 2),
    ("8", 1, 2),
    ("9", 2, 2),
    ("0", 1, 3),
];

pub fn slice_poster(image: &DynamicImage, zoom: f32, pan_x: f32, pan_y: f32) -> Result<HashMap<String, Vec<u8>>> {
    let (src_w, src_h) = image.dimensions();
    let grid_w = 3.0_f32;
    let grid_h = 4.0_f32;
    let z = zoom.clamp(1.0, 2.5);
    let view_w = (src_w as f32 / z).max(1.0);
    let view_h = (src_h as f32 / z).max(1.0);
    let max_x = (src_w as f32 - view_w).max(0.0);
    let max_y = (src_h as f32 - view_h).max(0.0);
    let ox = ((pan_x + 1.0) * 0.5 * max_x).clamp(0.0, max_x);
    let oy = ((pan_y + 1.0) * 0.5 * max_y).clamp(0.0, max_y);

    let cropped = image.crop_imm(ox as u32, oy as u32, view_w as u32, view_h as u32);
    let scaled = cropped.resize_exact(
        (KEY_PX as f32 * grid_w) as u32,
        (KEY_PX as f32 * grid_h) as u32,
        FilterType::Lanczos3,
    );
    let rgba = scaled.to_rgba8();

    let mut out = HashMap::new();
    let r = KEY_PX as i32 / 2;
    let r2 = r * r;
    for (digit, col, row) in KEYS {
        let x0 = col * KEY_PX;
        let y0 = row * KEY_PX;
        let mut button = RgbaImage::new(KEY_PX, KEY_PX);
        for y in 0..KEY_PX {
            for x in 0..KEY_PX {
                let dx = x as i32 - r;
                let dy = y as i32 - r;
                if dx * dx + dy * dy <= r2 {
                    let px = rgba.get_pixel(x0 + x, y0 + y);
                    button.put_pixel(x, y, *px);
                } else {
                    button.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                }
            }
        }
        let mut png = Vec::new();
        DynamicImage::ImageRgba8(button)
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .context("encode keypad PNG")?;
        out.insert(digit.to_string(), png);
    }
    Ok(out)
}

pub fn sliced_to_passthm_items(
    digits: &HashMap<String, Vec<u8>>,
    telephony: &str,
) -> Vec<(String, String, Vec<u8>)> {
    let en: HashMap<&str, &str> = KEYPAD_SUBTEXTS_EN.iter().copied().collect();
    let tdir = format!("/var/mobile/Library/Caches/{telephony}");
    let mut items = Vec::new();
    for (digit, png) in digits {
        let sub = en.get(digit.as_str()).copied().unwrap_or("");
        let names = if sub.is_empty() {
            vec![
                format!("en-{digit}---white.png"),
                format!("en-{digit}---white-bold.png"),
                format!("other-{digit}---white.png"),
                format!("other-{digit}---white-bold.png"),
            ]
        } else {
            vec![
                format!("en-{digit}-{sub}--white.png"),
                format!("en-{digit}-{sub}--white-bold.png"),
                format!("other-{digit}-{sub}--white.png"),
                format!("other-{digit}-{sub}--white-bold.png"),
                format!("en-{digit}---white.png"),
                format!("other-{digit}---white.png"),
            ]
        };
        for name in names {
            items.push((tdir.clone(), name, png.clone()));
        }
    }
    items
}

pub fn load_individual_keys(dir: &std::path::Path) -> Result<HashMap<String, Vec<u8>>> {
    let mut out = HashMap::new();
    for digit in ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"] {
        let mut found = None;
        for ext in ["png", "jpg", "jpeg", "webp"] {
            let path = dir.join(format!("{digit}.{ext}"));
            if path.is_file() {
                found = Some(std::fs::read(&path).with_context(|| format!("read {}", path.display()))?);
                break;
            }
        }
        if let Some(bytes) = found {
            out.insert(digit.to_string(), bytes);
        }
    }
    if out.len() < 10 {
        bail!(
            "Need 0.png … 9.png in {} (found {} keys)",
            dir.display(),
            out.len()
        );
    }
    Ok(out)
}

#[allow(dead_code)]
pub fn export_sliced_passthm(
    path: &std::path::Path,
    digits: &HashMap<String, Vec<u8>>,
) -> Result<()> {
    let files: Vec<(String, Vec<u8>)> = sliced_to_passthm_items(digits, "TelephonyUI-10")
        .into_iter()
        .map(|(_, leaf, data)| (format!("TelephonyUI-10/{leaf}"), data))
        .collect();
    export_passthm_archive(path, &files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn poster_slice_makes_ten_circular_keys() {
        let mut img = RgbaImage::new(900, 1200);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = Rgba([(x % 255) as u8, (y % 255) as u8, 80, 255]);
        }
        let keys = slice_poster(&DynamicImage::ImageRgba8(img), 1.0, 0.0, 0.0).unwrap();
        assert_eq!(keys.len(), 10);
        for d in ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"] {
            assert!(keys[d].starts_with(b"\x89PNG"));
        }
        let items = sliced_to_passthm_items(&keys, "TelephonyUI-10");
        assert!(items.iter().any(|(_, leaf, _)| leaf.contains("white-bold")));
    }
}
