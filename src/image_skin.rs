use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result};
use eframe::egui;
use image::{DynamicImage, GenericImageView, ImageFormat, imageops::FilterType};

pub const CARD_WIDTH: u32 = 1_536;
pub const CARD_HEIGHT: u32 = 969;

#[derive(Clone)]
pub struct PreparedSkin {
    pub png: Vec<u8>,
    pub preview: egui::ColorImage,
    pub source_width: u32,
    pub source_height: u32,
}

impl PreparedSkin {
    pub fn from_path(path: &Path) -> Result<Self> {
        Self::from_path_framed(path, 1.0, 0.0, 0.0)
    }

    pub fn from_path_framed(path: &Path, zoom: f32, pan_x: f32, pan_y: f32) -> Result<Self> {
        let image =
            image::open(path).with_context(|| format!("Could not decode {}", path.display()))?;
        Self::from_image_framed(image, zoom, pan_x, pan_y)
    }

    #[allow(dead_code)]
    pub fn from_image(image: DynamicImage) -> Result<Self> {
        Self::from_image_framed(image, 1.0, 0.0, 0.0)
    }

    pub fn from_image_framed(
        image: DynamicImage,
        zoom: f32,
        pan_x: f32,
        pan_y: f32,
    ) -> Result<Self> {
        let (source_width, source_height) = image.dimensions();
        let cropped = framed_crop_for_card(image, zoom, pan_x, pan_y);
        let final_image = cropped.resize_exact(CARD_WIDTH, CARD_HEIGHT, FilterType::Lanczos3);
        let rgba = final_image.to_rgba8();
        let preview = egui::ColorImage::from_rgba_unmultiplied(
            [CARD_WIDTH as usize, CARD_HEIGHT as usize],
            rgba.as_raw(),
        );

        let mut png = Vec::new();
        DynamicImage::ImageRgba8(rgba)
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .context("Could not encode prepared PNG")?;

        Ok(Self {
            png,
            preview,
            source_width,
            source_height,
        })
    }
}

fn framed_crop_for_card(image: DynamicImage, zoom: f32, pan_x: f32, pan_y: f32) -> DynamicImage {
    let (width, height) = image.dimensions();
    let card_ratio = CARD_WIDTH as f64 / CARD_HEIGHT as f64;
    let source_ratio = width as f64 / height as f64;

    let (base_w, base_h, base_x, base_y) = if source_ratio > card_ratio {
        let crop_width = (height as f64 * card_ratio).round();
        let x = (width as f64 - crop_width) / 2.0;
        (crop_width, height as f64, x, 0.0)
    } else {
        let crop_height = (width as f64 / card_ratio).round();
        let y = (height as f64 - crop_height) / 2.0;
        (width as f64, crop_height, 0.0, y)
    };

    let z = (zoom as f64).clamp(1.0, 3.0);
    let view_w = (base_w / z).max(1.0);
    let view_h = (base_h / z).max(1.0);
    let max_ox = (base_w - view_w).max(0.0);
    let max_oy = (base_h - view_h).max(0.0);
    let ox = base_x + ((pan_x as f64 + 1.0) * 0.5 * max_ox);
    let oy = base_y + ((pan_y as f64 + 1.0) * 0.5 * max_oy);

    image.crop_imm(
        ox.round().clamp(0.0, width as f64) as u32,
        oy.round().clamp(0.0, height as f64) as u32,
        view_w.round().max(1.0) as u32,
        view_h.round().max(1.0) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn framed_crop_outputs_card_size() {
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            2000,
            1000,
            Rgba([10, 20, 30, 255]),
        ));
        let skin = PreparedSkin::from_image_framed(img, 1.4, 0.2, -0.1).unwrap();
        assert_eq!(skin.source_width, 2000);
        assert_eq!(skin.source_height, 1000);
        assert!(skin.png.starts_with(b"\x89PNG"));
        assert_eq!(skin.preview.width(), CARD_WIDTH as usize);
        assert_eq!(skin.preview.height(), CARD_HEIGHT as usize);
    }
}
