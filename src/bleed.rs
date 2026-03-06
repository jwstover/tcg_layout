use anyhow::{Context, Result};
use image::{imageops::blur, DynamicImage, ImageBuffer, Rgba};
use std::path::Path;

/// Depth of edge strip to extract and blur (in pixels)
/// Larger values create smoother transitions but cost more performance
const EDGE_STRIP_DEPTH: u32 = 3;

/// Gaussian blur sigma for edge smoothing
/// Larger values create more blur, smaller values preserve more detail
const BLUR_SIGMA: f32 = 1.5;

/// Calculate bleed pixels based on actual image dimensions and card size
/// This is the correct way to calculate bleed - as a proportion of the actual image
/// rather than relying on DPI metadata which may be incorrect or missing.
///
/// # Arguments
/// * `bleed_mm` - The bleed distance in millimeters
/// * `card_size_mm` - The card dimensions in millimeters (width, height)
/// * `image_dimensions` - The actual image dimensions in pixels (width, height)
///
/// # Returns
/// A tuple of (bleed_pixels_x, bleed_pixels_y) representing the bleed in pixels
pub fn calculate_bleed_pixels_from_dimensions(
    bleed_mm: f32,
    card_size_mm: (f32, f32),
    image_dimensions: (u32, u32),
) -> (u32, u32) {
    // Calculate bleed as a fraction of card size
    let bleed_fraction_x = bleed_mm / card_size_mm.0;
    let bleed_fraction_y = bleed_mm / card_size_mm.1;

    // Apply same fraction to image dimensions
    let bleed_pixels_x = (bleed_fraction_x * image_dimensions.0 as f32).round() as u32;
    let bleed_pixels_y = (bleed_fraction_y * image_dimensions.1 as f32).round() as u32;

    (bleed_pixels_x, bleed_pixels_y)
}

/// Apply bleed to an image loaded from a file path
/// Returns a new DynamicImage with bleed applied
pub fn apply_bleed(image_path: &Path, bleed_pixels: u32) -> Result<DynamicImage> {
    let img = image::open(image_path)
        .with_context(|| format!("Failed to open image: {}", image_path.display()))?;

    let bleed_img = apply_bleed_to_image(&img, bleed_pixels);
    Ok(DynamicImage::ImageRgba8(bleed_img))
}

/// Apply bleed to an ImageBuffer (for thumbnail processing)
/// Uses Gaussian blur on edge strips to create smooth, artifact-free bleed regions
pub fn apply_bleed_to_thumbnail(
    img: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    bleed_pixels: u32,
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    if bleed_pixels == 0 {
        return img.clone();
    }

    let (width, height) = img.dimensions();
    let new_width = width + 2 * bleed_pixels;
    let new_height = height + 2 * bleed_pixels;

    let mut canvas = ImageBuffer::new(new_width, new_height);

    // Copy original image to center
    for y in 0..height {
        for x in 0..width {
            let pixel = img.get_pixel(x, y);
            canvas.put_pixel(x + bleed_pixels, y + bleed_pixels, *pixel);
        }
    }

    // Extract and blur edge strips, then sample from them
    let strip_depth = EDGE_STRIP_DEPTH.min(width).min(height);

    // Top edge: extract strip, blur, and extend
    if strip_depth > 0 {
        let mut top_strip = ImageBuffer::new(width, strip_depth);
        for y in 0..strip_depth {
            for x in 0..width {
                top_strip.put_pixel(x, y, *img.get_pixel(x, y));
            }
        }
        let blurred_top = blur(&top_strip, BLUR_SIGMA);
        let sample_y = strip_depth - 1;
        for y in 0..bleed_pixels {
            for x in 0..width {
                let pixel = blurred_top.get_pixel(x, sample_y);
                canvas.put_pixel(x + bleed_pixels, y, *pixel);
            }
        }
    }

    // Bottom edge: extract strip, blur, and extend
    if strip_depth > 0 {
        let mut bottom_strip = ImageBuffer::new(width, strip_depth);
        let start_y = height - strip_depth;
        for y in 0..strip_depth {
            for x in 0..width {
                bottom_strip.put_pixel(x, y, *img.get_pixel(x, start_y + y));
            }
        }
        let blurred_bottom = blur(&bottom_strip, BLUR_SIGMA);
        for y in 0..bleed_pixels {
            for x in 0..width {
                let pixel = blurred_bottom.get_pixel(x, 0);
                canvas.put_pixel(x + bleed_pixels, height + bleed_pixels + y, *pixel);
            }
        }
    }

    // Left edge: extract strip, blur, and extend
    if strip_depth > 0 {
        let mut left_strip = ImageBuffer::new(strip_depth, height);
        for y in 0..height {
            for x in 0..strip_depth {
                left_strip.put_pixel(x, y, *img.get_pixel(x, y));
            }
        }
        let blurred_left = blur(&left_strip, BLUR_SIGMA);
        let sample_x = strip_depth - 1;
        for y in 0..height {
            for x in 0..bleed_pixels {
                let pixel = blurred_left.get_pixel(sample_x, y);
                canvas.put_pixel(x, y + bleed_pixels, *pixel);
            }
        }
    }

    // Right edge: extract strip, blur, and extend
    if strip_depth > 0 {
        let mut right_strip = ImageBuffer::new(strip_depth, height);
        let start_x = width - strip_depth;
        for y in 0..height {
            for x in 0..strip_depth {
                right_strip.put_pixel(x, y, *img.get_pixel(start_x + x, y));
            }
        }
        let blurred_right = blur(&right_strip, BLUR_SIGMA);
        for y in 0..height {
            for x in 0..bleed_pixels {
                let pixel = blurred_right.get_pixel(0, y);
                canvas.put_pixel(width + bleed_pixels + x, y + bleed_pixels, *pixel);
            }
        }
    }

    // Corner handling: extract corner regions, blur, and sample from center
    if strip_depth > 0 {
        // Top-left corner
        let mut top_left_corner = ImageBuffer::new(strip_depth, strip_depth);
        for y in 0..strip_depth {
            for x in 0..strip_depth {
                top_left_corner.put_pixel(x, y, *img.get_pixel(x, y));
            }
        }
        let blurred_tl = blur(&top_left_corner, BLUR_SIGMA);
        let corner_center = strip_depth / 2;
        let tl_pixel = blurred_tl.get_pixel(corner_center, corner_center);
        for y in 0..bleed_pixels {
            for x in 0..bleed_pixels {
                canvas.put_pixel(x, y, *tl_pixel);
            }
        }

        // Top-right corner
        let mut top_right_corner = ImageBuffer::new(strip_depth, strip_depth);
        let start_x = width - strip_depth;
        for y in 0..strip_depth {
            for x in 0..strip_depth {
                top_right_corner.put_pixel(x, y, *img.get_pixel(start_x + x, y));
            }
        }
        let blurred_tr = blur(&top_right_corner, BLUR_SIGMA);
        let tr_pixel = blurred_tr.get_pixel(corner_center, corner_center);
        for y in 0..bleed_pixels {
            for x in 0..bleed_pixels {
                canvas.put_pixel(width + bleed_pixels + x, y, *tr_pixel);
            }
        }

        // Bottom-left corner
        let mut bottom_left_corner = ImageBuffer::new(strip_depth, strip_depth);
        let start_y = height - strip_depth;
        for y in 0..strip_depth {
            for x in 0..strip_depth {
                bottom_left_corner.put_pixel(x, y, *img.get_pixel(x, start_y + y));
            }
        }
        let blurred_bl = blur(&bottom_left_corner, BLUR_SIGMA);
        let bl_pixel = blurred_bl.get_pixel(corner_center, corner_center);
        for y in 0..bleed_pixels {
            for x in 0..bleed_pixels {
                canvas.put_pixel(x, height + bleed_pixels + y, *bl_pixel);
            }
        }

        // Bottom-right corner
        let mut bottom_right_corner = ImageBuffer::new(strip_depth, strip_depth);
        for y in 0..strip_depth {
            for x in 0..strip_depth {
                bottom_right_corner.put_pixel(x, y, *img.get_pixel(start_x + x, start_y + y));
            }
        }
        let blurred_br = blur(&bottom_right_corner, BLUR_SIGMA);
        let br_pixel = blurred_br.get_pixel(corner_center, corner_center);
        for y in 0..bleed_pixels {
            for x in 0..bleed_pixels {
                canvas.put_pixel(
                    width + bleed_pixels + x,
                    height + bleed_pixels + y,
                    *br_pixel,
                );
            }
        }
    }

    canvas
}

/// Apply bleed to a DynamicImage (internal helper)
fn apply_bleed_to_image(img: &DynamicImage, bleed_pixels: u32) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let rgba_img = img.to_rgba8();
    apply_bleed_to_thumbnail(&rgba_img, bleed_pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn test_apply_bleed_to_thumbnail_dimensions() {
        // Create a 100x100 test image
        let img = ImageBuffer::from_fn(100, 100, |x, y| {
            if x < 50 && y < 50 {
                Rgba([255, 0, 0, 255]) // Red
            } else if x >= 50 && y < 50 {
                Rgba([0, 255, 0, 255]) // Green
            } else if x < 50 && y >= 50 {
                Rgba([0, 0, 255, 255]) // Blue
            } else {
                Rgba([255, 255, 0, 255]) // Yellow
            }
        });

        // Apply 10 pixels of bleed
        let bleed_img = apply_bleed_to_thumbnail(&img, 10);

        // Check dimensions: 100x100 + 2*10 = 120x120
        assert_eq!(bleed_img.dimensions(), (120, 120));
    }

    #[test]
    fn test_apply_bleed_to_thumbnail_zero_bleed() {
        // Create a 50x50 test image
        let img = ImageBuffer::from_fn(50, 50, |_, _| Rgba([128, 128, 128, 255]));

        // Apply 0 pixels of bleed (should return identical image)
        let bleed_img = apply_bleed_to_thumbnail(&img, 0);

        assert_eq!(img.dimensions(), bleed_img.dimensions());
        assert_eq!(img, bleed_img);
    }

    #[test]
    fn test_edge_extension() {
        // Create a simple 2x2 test image with distinct colors
        let mut img = ImageBuffer::new(2, 2);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255])); // Top-left: Red
        img.put_pixel(1, 0, Rgba([0, 255, 0, 255])); // Top-right: Green
        img.put_pixel(0, 1, Rgba([0, 0, 255, 255])); // Bottom-left: Blue
        img.put_pixel(1, 1, Rgba([255, 255, 0, 255])); // Bottom-right: Yellow

        // Apply 1 pixel of bleed
        let bleed_img = apply_bleed_to_thumbnail(&img, 1);

        // Check dimensions: 2x2 + 2*1 = 4x4
        assert_eq!(bleed_img.dimensions(), (4, 4));

        // With blur, corners will be blended versions of nearby pixels
        // For very small images (2x2), the corner extraction captures the entire image,
        // so after blur all corners will be mixed. We verify corners have color and alpha.
        // In real-world usage with larger images, corners will better preserve their colors.
        let tl_corner = bleed_img.get_pixel(0, 0);
        assert!(
            tl_corner[0] > 0 || tl_corner[1] > 0 || tl_corner[2] > 0,
            "Top-left corner should have some color"
        );
        assert!(tl_corner[3] == 255, "Alpha should be preserved");

        let tr_corner = bleed_img.get_pixel(3, 0);
        assert!(
            tr_corner[0] > 0 || tr_corner[1] > 0 || tr_corner[2] > 0,
            "Top-right corner should have some color"
        );
        assert!(tr_corner[3] == 255, "Alpha should be preserved");

        let bl_corner = bleed_img.get_pixel(0, 3);
        assert!(
            bl_corner[0] > 0 || bl_corner[1] > 0 || bl_corner[2] > 0,
            "Bottom-left corner should have some color"
        );
        assert!(bl_corner[3] == 255, "Alpha should be preserved");

        let br_corner = bleed_img.get_pixel(3, 3);
        assert!(
            br_corner[0] > 0 || br_corner[1] > 0 || br_corner[2] > 0,
            "Bottom-right corner should have some color"
        );
        assert!(br_corner[3] == 255, "Alpha should be preserved");

        // Verify original image is in center (should be exact)
        assert_eq!(bleed_img.get_pixel(1, 1), &Rgba([255, 0, 0, 255])); // Original top-left
        assert_eq!(bleed_img.get_pixel(2, 1), &Rgba([0, 255, 0, 255])); // Original top-right
        assert_eq!(bleed_img.get_pixel(1, 2), &Rgba([0, 0, 255, 255])); // Original bottom-left
        assert_eq!(bleed_img.get_pixel(2, 2), &Rgba([255, 255, 0, 255])); // Original bottom-right
    }

    #[test]
    fn test_top_edge_replication() {
        // Create a 3x2 image with red top edge
        let mut img = ImageBuffer::new(3, 2);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(2, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(0, 1, Rgba([0, 0, 0, 255]));
        img.put_pixel(1, 1, Rgba([0, 0, 0, 255]));
        img.put_pixel(2, 1, Rgba([0, 0, 0, 255]));

        let bleed_img = apply_bleed_to_thumbnail(&img, 2);

        // Check that top edge bleed area is predominantly red (with blur tolerance)
        // Since blur will blend with the black bottom row, we verify red channel dominates
        // For small images (3x2), blur creates significant blending
        for x in 2..5 {
            let pixel = bleed_img.get_pixel(x, 0);
            assert!(
                pixel[0] > pixel[1] && pixel[0] > pixel[2] && pixel[0] > 50,
                "Top edge bleed should be predominantly red at ({}, 0), got [{}, {}, {}]",
                x,
                pixel[0],
                pixel[1],
                pixel[2]
            );
            assert!(pixel[3] == 255, "Alpha should be preserved");

            let pixel = bleed_img.get_pixel(x, 1);
            assert!(
                pixel[0] > pixel[1] && pixel[0] > pixel[2] && pixel[0] > 50,
                "Top edge bleed should be predominantly red at ({}, 1), got [{}, {}, {}]",
                x,
                pixel[0],
                pixel[1],
                pixel[2]
            );
            assert!(pixel[3] == 255, "Alpha should be preserved");
        }
    }

    #[test]
    fn test_left_edge_replication() {
        // Create a 2x3 image with red left edge
        let mut img = ImageBuffer::new(2, 3);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(0, 1, Rgba([255, 0, 0, 255]));
        img.put_pixel(0, 2, Rgba([255, 0, 0, 255]));
        img.put_pixel(1, 0, Rgba([0, 0, 0, 255]));
        img.put_pixel(1, 1, Rgba([0, 0, 0, 255]));
        img.put_pixel(1, 2, Rgba([0, 0, 0, 255]));

        let bleed_img = apply_bleed_to_thumbnail(&img, 2);

        // Check that left edge bleed area is predominantly red (with blur tolerance)
        // Since blur will blend with the black right column, we verify red channel dominates
        // For small images (2x3), blur creates significant blending
        for y in 2..5 {
            let pixel = bleed_img.get_pixel(0, y);
            assert!(
                pixel[0] > pixel[1] && pixel[0] > pixel[2] && pixel[0] > 50,
                "Left edge bleed should be predominantly red at (0, {}), got [{}, {}, {}]",
                y,
                pixel[0],
                pixel[1],
                pixel[2]
            );
            assert!(pixel[3] == 255, "Alpha should be preserved");

            let pixel = bleed_img.get_pixel(1, y);
            assert!(
                pixel[0] > pixel[1] && pixel[0] > pixel[2] && pixel[0] > 50,
                "Left edge bleed should be predominantly red at (1, {}), got [{}, {}, {}]",
                y,
                pixel[0],
                pixel[1],
                pixel[2]
            );
            assert!(pixel[3] == 255, "Alpha should be preserved");
        }
    }

    #[test]
    fn test_calculate_bleed_pixels_from_dimensions_300dpi() {
        // Card: 63mm × 88mm, Bleed: 3mm, Image: 744×1039 (300 DPI)
        let (bleed_x, bleed_y) =
            calculate_bleed_pixels_from_dimensions(3.0, (63.0, 88.0), (744, 1039));
        // (3/63) * 744 = 35.43 ≈ 35
        // (3/88) * 1039 = 35.42 ≈ 35
        assert_eq!(bleed_x, 35);
        assert_eq!(bleed_y, 35);
    }

    #[test]
    fn test_calculate_bleed_pixels_from_dimensions_403dpi() {
        // Card: 63mm × 88mm, Bleed: 3mm, Image: 1000×1397 (403 DPI)
        let (bleed_x, bleed_y) =
            calculate_bleed_pixels_from_dimensions(3.0, (63.0, 88.0), (1000, 1397));
        // (3/63) * 1000 = 47.62 ≈ 48
        // (3/88) * 1397 = 47.65 ≈ 48
        assert_eq!(bleed_x, 48);
        assert_eq!(bleed_y, 48);
    }

    #[test]
    fn test_calculate_bleed_pixels_from_dimensions_zero_bleed() {
        // Card: 63mm × 88mm, Bleed: 0mm, Image: 744×1039
        let (bleed_x, bleed_y) =
            calculate_bleed_pixels_from_dimensions(0.0, (63.0, 88.0), (744, 1039));
        assert_eq!(bleed_x, 0);
        assert_eq!(bleed_y, 0);
    }

    #[test]
    fn test_calculate_bleed_pixels_from_dimensions_rectangular_card() {
        // Non-square card: 50mm × 100mm, Bleed: 2mm, Image: 600×1200
        let (bleed_x, bleed_y) =
            calculate_bleed_pixels_from_dimensions(2.0, (50.0, 100.0), (600, 1200));
        // (2/50) * 600 = 24
        // (2/100) * 1200 = 24
        assert_eq!(bleed_x, 24);
        assert_eq!(bleed_y, 24);
    }

    #[test]
    fn test_calculate_bleed_pixels_from_dimensions_high_dpi() {
        // Very high DPI image: Card: 63mm × 88mm, Bleed: 3mm, Image: 1488×2078 (600 DPI)
        let (bleed_x, bleed_y) =
            calculate_bleed_pixels_from_dimensions(3.0, (63.0, 88.0), (1488, 2078));
        // (3/63) * 1488 = 70.86 ≈ 71
        // (3/88) * 2078 = 70.84 ≈ 71
        assert_eq!(bleed_x, 71);
        assert_eq!(bleed_y, 71);
    }

    #[test]
    fn test_calculate_bleed_pixels_from_dimensions_small_image() {
        // Small/low-res image: Card: 63mm × 88mm, Bleed: 3mm, Image: 200×279 (approx 81 DPI)
        let (bleed_x, bleed_y) =
            calculate_bleed_pixels_from_dimensions(3.0, (63.0, 88.0), (200, 279));
        // (3/63) * 200 = 9.52 ≈ 10
        // (3/88) * 279 = 9.51 ≈ 10
        assert_eq!(bleed_x, 10);
        assert_eq!(bleed_y, 10);
    }
}
