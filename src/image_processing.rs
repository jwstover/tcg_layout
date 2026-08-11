use anyhow::Result;
use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageBuffer, Rgba};
use std::path::Path;

use tcg_layout::{bleed, sharpen};

const THUMBNAIL_WIDTH: u32 = 150;
const THUMBNAIL_HEIGHT: u32 = 200;

/// Settings that affect thumbnail generation.
/// These are part of the thumbnail cache key - changing any of them
/// invalidates cached thumbnails.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThumbnailParams {
    pub bleed_mm: f32,
    pub enable_bleed: bool,
    pub sharpen: sharpen::SharpenParams,
    pub enable_sharpen: bool,
    pub card_size_mm: (f32, f32),
}

impl ThumbnailParams {
    pub fn from_layout(params: &crate::types::LayoutParams) -> Self {
        Self {
            bleed_mm: params.bleed_mm,
            enable_bleed: params.enable_bleed,
            sharpen: params.sharpen_params(),
            enable_sharpen: params.enable_sharpen,
            card_size_mm: params.card_size,
        }
    }
}

impl Default for ThumbnailParams {
    fn default() -> Self {
        Self {
            bleed_mm: 0.0,
            enable_bleed: false,
            sharpen: sharpen::SharpenParams::default(),
            enable_sharpen: false,
            card_size_mm: (63.0, 88.0),
        }
    }
}

pub fn generate_thumbnail(
    image_path: &Path,
    params: &ThumbnailParams,
) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    // Load the image
    let img = image::open(image_path)?;

    // Create thumbnail with aspect ratio preservation
    let thumbnail = resize_with_aspect_ratio(img.clone(), THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT);

    // Convert to RGBA for consistent handling
    let mut thumbnail_rgba = thumbnail.to_rgba8();

    // Apply sharpening before bleed so bleed strips derive from the
    // sharpened image (and the blurred bleed region stays smooth).
    //
    // The radius is specified at full resolution, so it has to be scaled down
    // for the thumbnail or a 0.7 px sigma would sharpen roughly ten times as
    // wide a feature here as it does on export. At thumbnail scale the scaled
    // radius is usually negligible and `apply_sharpen_to_buffer` no-ops, which
    // is correct: sharpening is not visible at 150 px. The full-resolution
    // sharpen preview window is where it can actually be judged.
    if params.enable_sharpen {
        let scale = thumbnail_rgba.width() as f32 / img.width().max(1) as f32;
        thumbnail_rgba =
            sharpen::apply_sharpen_to_buffer(&thumbnail_rgba, &params.sharpen.scaled(scale));
    }

    // Apply bleed if enabled
    if params.enable_bleed && params.bleed_mm > 0.0 {
        let (thumb_width, _) = thumbnail_rgba.dimensions();

        // Calculate bleed as proportion of card size
        let bleed_fraction = params.bleed_mm / params.card_size_mm.0;
        let thumbnail_bleed_px = (bleed_fraction * thumb_width as f32).round() as u32;

        thumbnail_rgba = bleed::apply_bleed_to_thumbnail(&thumbnail_rgba, thumbnail_bleed_px);
    }

    Ok(thumbnail_rgba)
}

fn resize_with_aspect_ratio(
    img: DynamicImage,
    target_width: u32,
    target_height: u32,
) -> DynamicImage {
    let (orig_width, orig_height) = img.dimensions();

    // Calculate scale factors for both dimensions
    let scale_x = target_width as f32 / orig_width as f32;
    let scale_y = target_height as f32 / orig_height as f32;

    // Use the smaller scale factor to maintain aspect ratio
    let scale = scale_x.min(scale_y);

    let new_width = (orig_width as f32 * scale) as u32;
    let new_height = (orig_height as f32 * scale) as u32;

    // Resize the image
    img.resize(new_width, new_height, FilterType::Lanczos3)
}

pub fn get_image_dpi(image_path: &Path) -> Option<u32> {
    use std::fs::File;
    use std::io::BufReader;

    let file = File::open(image_path).ok()?;
    let mut bufreader = BufReader::new(&file);
    let exifreader = exif::Reader::new();
    let exif = exifreader.read_from_container(&mut bufreader).ok()?;

    // Try to get X resolution first
    if let Some(field) = exif.get_field(exif::Tag::XResolution, exif::In::PRIMARY) {
        if let Some(value) = field.value.get_uint(0) {
            return Some(value);
        }
    }

    // Try Y resolution as fallback
    if let Some(field) = exif.get_field(exif::Tag::YResolution, exif::In::PRIMARY) {
        if let Some(value) = field.value.get_uint(0) {
            return Some(value);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};
    use tempfile::NamedTempFile;

    fn create_test_image(width: u32, height: u32) -> NamedTempFile {
        let img = RgbImage::from_fn(width, height, |x, y| {
            if (x + y) % 2 == 0 {
                Rgb([255, 0, 0]) // Red
            } else {
                Rgb([0, 255, 0]) // Green
            }
        });

        let temp_file = NamedTempFile::with_suffix(".png").unwrap();
        img.save(temp_file.path()).unwrap();
        temp_file
    }

    #[test]
    fn test_generate_thumbnail_preserves_aspect_ratio() {
        let temp_file = create_test_image(300, 600); // 1:2 aspect ratio

        let thumbnail = generate_thumbnail(temp_file.path(), &ThumbnailParams::default()).unwrap();
        let (thumb_width, thumb_height) = thumbnail.dimensions();

        // Should fit within 150x200 bounds
        assert!(thumb_width <= THUMBNAIL_WIDTH);
        assert!(thumb_height <= THUMBNAIL_HEIGHT);

        // Should preserve aspect ratio (1:2)
        let aspect_ratio = thumb_width as f32 / thumb_height as f32;
        let expected_ratio = 300.0 / 600.0;
        assert!((aspect_ratio - expected_ratio).abs() < 0.01);
    }

    #[test]
    fn test_generate_thumbnail_wide_image() {
        let temp_file = create_test_image(600, 300); // 2:1 aspect ratio

        let thumbnail = generate_thumbnail(temp_file.path(), &ThumbnailParams::default()).unwrap();
        let (thumb_width, thumb_height) = thumbnail.dimensions();

        // Should fit within 150x200 bounds
        assert!(thumb_width <= THUMBNAIL_WIDTH);
        assert!(thumb_height <= THUMBNAIL_HEIGHT);

        // Should preserve aspect ratio (2:1)
        let aspect_ratio = thumb_width as f32 / thumb_height as f32;
        let expected_ratio = 600.0 / 300.0;
        assert!((aspect_ratio - expected_ratio).abs() < 0.01);
    }

    #[test]
    fn test_generate_thumbnail_square_image() {
        let temp_file = create_test_image(400, 400); // 1:1 aspect ratio

        let thumbnail = generate_thumbnail(temp_file.path(), &ThumbnailParams::default()).unwrap();
        let (thumb_width, thumb_height) = thumbnail.dimensions();

        // Should fit within 150x200 bounds
        assert!(thumb_width <= THUMBNAIL_WIDTH);
        assert!(thumb_height <= THUMBNAIL_HEIGHT);

        // Should preserve aspect ratio (1:1)
        let aspect_ratio = thumb_width as f32 / thumb_height as f32;
        assert!((aspect_ratio - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_resize_with_aspect_ratio_tall_image() {
        let img = DynamicImage::new_rgb8(100, 300); // 1:3 aspect ratio
        let resized = resize_with_aspect_ratio(img, 150, 200);
        let (width, height) = resized.dimensions();

        // Should be limited by height (200), width should be 200/3 = ~66
        // But in practice, the function limits by the smaller scale factor
        // For 100x300 -> 150x200: scale_x = 1.5, scale_y = 0.666...
        // So it uses scale_y = 0.666... giving us 66x200, but height might be limited
        // Let's check both dimensions are within reasonable bounds
        assert!(height <= 200);
        assert!(width <= 150);
        // Verify aspect ratio is preserved (approximately)
        let aspect_ratio = width as f32 / height as f32;
        let expected_ratio = 100.0 / 300.0;
        assert!((aspect_ratio - expected_ratio).abs() < 0.01);
    }

    #[test]
    fn test_resize_with_aspect_ratio_wide_image() {
        let img = DynamicImage::new_rgb8(300, 100); // 3:1 aspect ratio
        let resized = resize_with_aspect_ratio(img, 150, 200);
        let (width, height) = resized.dimensions();

        // Should be limited by width (150), height should be 150/3 = 50
        assert_eq!(width, 150);
        assert_eq!(height, 50);
    }
}
