use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result};
use eframe::egui;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::{DynamicImage, GenericImageView, ImageEncoder, ImageFormat, RgbaImage, imageops::FilterType};

pub const CARD_WIDTH: u32 = 1_536;
pub const CARD_HEIGHT: u32 = 969;
pub const CARD_WIDTH_2X: u32 = 1_024;
pub const CARD_HEIGHT_2X: u32 = 646;
pub const PREVIEW_WIDTH: u32 = 768;
pub const PREVIEW_HEIGHT: u32 = 485;

#[derive(Clone)]
pub struct PreparedSkin {
    pub png: Vec<u8>,
    pub png_2x: Vec<u8>,
    pub preview: egui::ColorImage,
    pub source_width: u32,
    pub source_height: u32,
}

impl PreparedSkin {
    pub fn from_path(path: &Path) -> Result<Self> {
        Self::from_path_framed(path, 1.0, 0.0, 0.0)
    }

    pub fn from_path_framed(path: &Path, zoom: f32, pan_x: f32, pan_y: f32) -> Result<Self> {
        let image = decode_image(path)?;
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
        let rgba = frame_card(&image, zoom, pan_x, pan_y, FilterType::Lanczos3);
        let (png, png_2x) = encode_wallet_pngs(&rgba)?;
        Ok(Self {
            png,
            png_2x,
            preview: preview_color_image(&rgba),
            source_width,
            source_height,
        })
    }
}

pub fn decode_image(path: &Path) -> Result<DynamicImage> {
    image::open(path).with_context(|| format!("Could not decode {}", path.display()))
}

pub fn frame_card(
    image: &DynamicImage,
    zoom: f32,
    pan_x: f32,
    pan_y: f32,
    filter: FilterType,
) -> RgbaImage {
    framed_crop_for_card(image, zoom, pan_x, pan_y)
        .resize_exact(CARD_WIDTH, CARD_HEIGHT, filter)
        .to_rgba8()
}

pub fn preview_color_image(rgba: &RgbaImage) -> egui::ColorImage {
    let preview = DynamicImage::ImageRgba8(rgba.clone()).resize_exact(
        PREVIEW_WIDTH,
        PREVIEW_HEIGHT,
        FilterType::Triangle,
    );
    let pixels = preview.to_rgba8();
    egui::ColorImage::from_rgba_unmultiplied(
        [PREVIEW_WIDTH as usize, PREVIEW_HEIGHT as usize],
        pixels.as_raw(),
    )
}

pub fn encode_wallet_pngs(rgba: &RgbaImage) -> Result<(Vec<u8>, Vec<u8>)> {
    let png = encode_png_fast(rgba).context("Could not encode prepared PNG")?;
    let png_2x_img = DynamicImage::ImageRgba8(rgba.clone()).resize_exact(
        CARD_WIDTH_2X,
        CARD_HEIGHT_2X,
        FilterType::Lanczos3,
    );
    let png_2x = encode_png_fast(&png_2x_img.to_rgba8()).context("Could not encode @2x PNG")?;
    Ok((png, png_2x))
}

fn encode_png_fast(rgba: &RgbaImage) -> Result<Vec<u8>> {
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, PngFilter::Adaptive)
        .write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            image::ExtendedColorType::Rgba8,
        )
        .context("PNG encode failed")?;
    Ok(png)
}

pub fn png_2x_from_3x(png_3x: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory(png_3x).context("Could not decode @3x PNG")?;
    let resized = img.resize_exact(CARD_WIDTH_2X, CARD_HEIGHT_2X, FilterType::Lanczos3);
    let mut png_2x = Vec::new();
    resized
        .write_to(&mut Cursor::new(&mut png_2x), ImageFormat::Png)
        .context("Could not encode downsampled @2x PNG")?;
    Ok(png_2x)
}

fn framed_crop_for_card(image: &DynamicImage, zoom: f32, pan_x: f32, pan_y: f32) -> DynamicImage {
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
        assert!(skin.png_2x.starts_with(b"\x89PNG"));
        let two_x = image::load_from_memory(&skin.png_2x).unwrap();
        assert_eq!(two_x.width(), CARD_WIDTH_2X);
        assert_eq!(two_x.height(), CARD_HEIGHT_2X);
        assert_eq!(skin.preview.width(), PREVIEW_WIDTH as usize);
        assert_eq!(skin.preview.height(), PREVIEW_HEIGHT as usize);
    }

    #[test]
    fn triangle_preview_then_lanczos_encode() {
        let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            800,
            500,
            Rgba([40, 50, 60, 255]),
        ));
        let fast = frame_card(&img, 1.0, 0.0, 0.0, FilterType::Triangle);
        assert_eq!(fast.width(), CARD_WIDTH);
        assert_eq!(fast.height(), CARD_HEIGHT);
        let preview = preview_color_image(&fast);
        assert_eq!(preview.width(), PREVIEW_WIDTH as usize);
        let (png, png_2x) = encode_wallet_pngs(&fast).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        assert!(png_2x.starts_with(b"\x89PNG"));
    }
}
