use image::{imageops::blur, DynamicImage, ImageBuffer, Rgba};

/// Gaussian blur sigma used for the unsharp mask
/// Smaller values sharpen fine detail, larger values sharpen broader edges
const UNSHARP_SIGMA: f32 = 1.0;

/// Maximum sharpening amount exposed in the UI
pub const MAX_SHARPEN_AMOUNT: f32 = 3.0;

/// Apply an unsharp mask to an RGBA image buffer.
///
/// `result = original + amount * (original - blurred)`
///
/// `amount` controls the strength: 0.0 is a no-op, 1.0 is a typical
/// sharpening pass, values above ~2.0 are aggressive. Alpha is preserved.
pub fn apply_sharpen_to_buffer(
    img: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    amount: f32,
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    if amount <= 0.0 {
        return img.clone();
    }

    let blurred = blur(img, UNSHARP_SIGMA);
    let (width, height) = img.dimensions();
    let mut out = ImageBuffer::new(width, height);

    for (x, y, pixel) in img.enumerate_pixels() {
        let blurred_pixel = blurred.get_pixel(x, y);
        let mut sharpened = [0u8; 4];
        for channel in 0..3 {
            let original = pixel[channel] as f32;
            let diff = original - blurred_pixel[channel] as f32;
            sharpened[channel] = (original + amount * diff).clamp(0.0, 255.0) as u8;
        }
        sharpened[3] = pixel[3]; // Preserve alpha
        out.put_pixel(x, y, Rgba(sharpened));
    }

    out
}

/// Apply an unsharp mask to a DynamicImage (for full-resolution export processing)
pub fn apply_sharpen(img: &DynamicImage, amount: f32) -> DynamicImage {
    DynamicImage::ImageRgba8(apply_sharpen_to_buffer(&img.to_rgba8(), amount))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge_image() -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        // Left half dark, right half light: a strong vertical edge
        ImageBuffer::from_fn(20, 20, |x, _| {
            if x < 10 {
                Rgba([50, 50, 50, 255])
            } else {
                Rgba([200, 200, 200, 255])
            }
        })
    }

    #[test]
    fn test_zero_amount_is_identity() {
        let img = edge_image();
        let result = apply_sharpen_to_buffer(&img, 0.0);
        assert_eq!(img, result);
    }

    #[test]
    fn test_negative_amount_is_identity() {
        let img = edge_image();
        let result = apply_sharpen_to_buffer(&img, -1.0);
        assert_eq!(img, result);
    }

    #[test]
    fn test_flat_image_unchanged() {
        let img = ImageBuffer::from_fn(20, 20, |_, _| Rgba([128u8, 128, 128, 255]));
        let result = apply_sharpen_to_buffer(&img, 2.0);
        // A flat image has no edges, so unsharp masking should not change it
        assert_eq!(img, result);
    }

    #[test]
    fn test_preserves_dimensions() {
        let img = edge_image();
        let result = apply_sharpen_to_buffer(&img, 1.5);
        assert_eq!(img.dimensions(), result.dimensions());
    }

    #[test]
    fn test_increases_edge_contrast() {
        let img = edge_image();
        let result = apply_sharpen_to_buffer(&img, 1.5);

        // Pixels adjacent to the edge should overshoot: darker on the dark
        // side, lighter on the light side
        let dark_side = result.get_pixel(9, 10);
        let light_side = result.get_pixel(10, 10);
        assert!(
            dark_side[0] < 50,
            "Dark side of edge should get darker, got {}",
            dark_side[0]
        );
        assert!(
            light_side[0] > 200,
            "Light side of edge should get lighter, got {}",
            light_side[0]
        );
    }

    #[test]
    fn test_higher_amount_sharpens_more() {
        let img = edge_image();
        let mild = apply_sharpen_to_buffer(&img, 0.5);
        let strong = apply_sharpen_to_buffer(&img, 2.5);

        // Stronger sharpening should push the light edge pixel further up
        let mild_edge = mild.get_pixel(10, 10)[0];
        let strong_edge = strong.get_pixel(10, 10)[0];
        assert!(
            strong_edge >= mild_edge,
            "Stronger amount should sharpen at least as much: mild={mild_edge}, strong={strong_edge}"
        );
    }

    #[test]
    fn test_preserves_alpha() {
        let img = ImageBuffer::from_fn(20, 20, |x, _| {
            if x < 10 {
                Rgba([50, 50, 50, 100])
            } else {
                Rgba([200, 200, 200, 100])
            }
        });
        let result = apply_sharpen_to_buffer(&img, 2.0);
        for pixel in result.pixels() {
            assert_eq!(pixel[3], 100, "Alpha should be preserved");
        }
    }

    #[test]
    fn test_apply_sharpen_dynamic_image() {
        let img = DynamicImage::ImageRgba8(edge_image());
        let result = apply_sharpen(&img, 1.0);
        assert_eq!(img.width(), result.width());
        assert_eq!(img.height(), result.height());
    }
}
