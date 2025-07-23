use anyhow::Result;
use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageBuffer, Rgba};
use std::path::Path;

const THUMBNAIL_WIDTH: u32 = 150;
const THUMBNAIL_HEIGHT: u32 = 200;

pub fn generate_thumbnail(image_path: &Path) -> Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    // Load the image
    let img = image::open(image_path)?;

    // Create thumbnail with aspect ratio preservation
    let thumbnail = resize_with_aspect_ratio(img, THUMBNAIL_WIDTH, THUMBNAIL_HEIGHT);

    // Convert to RGBA for consistent handling
    Ok(thumbnail.to_rgba8())
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

        let thumbnail = generate_thumbnail(temp_file.path()).unwrap();
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

        let thumbnail = generate_thumbnail(temp_file.path()).unwrap();
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

        let thumbnail = generate_thumbnail(temp_file.path()).unwrap();
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
