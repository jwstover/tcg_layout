use image::{imageops::blur, DynamicImage, ImageBuffer, Rgba};

/// Maximum sharpening amount exposed in the UI
pub const MAX_SHARPEN_AMOUNT: f32 = 3.0;

/// Radius (Gaussian sigma, in pixels) bounds exposed in the UI.
///
/// The useful range for 600 DPI card scans is roughly 0.5-1.0. Larger radii
/// produce visible halos around high-contrast edges.
pub const MIN_SHARPEN_RADIUS: f32 = 0.1;
pub const MAX_SHARPEN_RADIUS: f32 = 3.0;

/// Maximum threshold exposed in the UI, as a fraction of the full tonal range.
pub const MAX_SHARPEN_THRESHOLD: f32 = 0.2;

/// Radii below this are a no-op on the image being processed, so sharpening is
/// skipped entirely rather than burning a blur pass for nothing. Relevant when
/// a full-resolution radius is scaled down for a small thumbnail.
const NEGLIGIBLE_RADIUS: f32 = 0.3;

/// Unsharp mask settings.
///
/// `radius` is a Gaussian sigma in pixels *at the resolution being processed*.
/// A value tuned on a full-resolution scan means something very different on a
/// 150px thumbnail, so use [`SharpenParams::scaled`] when processing a resized
/// copy of an image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SharpenParams {
    /// Strength multiplier. 0.0 is a no-op, ~1.0-1.6 is a typical print pass.
    pub amount: f32,
    /// Gaussian sigma in pixels.
    pub radius: f32,
    /// Local contrast below this fraction of the full tonal range is left
    /// alone, which keeps sharpening off flat areas and out of scanner noise.
    pub threshold: f32,
}

impl SharpenParams {
    pub fn new(amount: f32, radius: f32, threshold: f32) -> Self {
        Self {
            amount,
            radius,
            threshold,
        }
    }

    /// Whether these settings would actually change an image.
    pub fn is_active(&self) -> bool {
        self.amount > 0.0 && self.radius >= NEGLIGIBLE_RADIUS
    }

    /// Scale the radius for a resized copy of an image.
    ///
    /// `factor` is the resized dimension divided by the original, so a 150px
    /// thumbnail of a 1500px scan uses `0.1`. Amount and threshold are
    /// resolution-independent and carry over unchanged.
    pub fn scaled(&self, factor: f32) -> Self {
        Self {
            amount: self.amount,
            radius: self.radius * factor,
            threshold: self.threshold,
        }
    }
}

impl Default for SharpenParams {
    fn default() -> Self {
        Self {
            amount: 1.0,
            radius: 0.7,
            threshold: 0.02,
        }
    }
}

/// Apply an unsharp mask to an RGBA image buffer.
///
/// `result = original + amount * (luma(original) - luma(blurred))`
///
/// The correction is computed from luminance and added equally to all three
/// colour channels. Sharpening each channel independently would shift hue
/// along coloured edges (visible as red/cyan fringing on card art), so this
/// keeps the sharpening achromatic. Alpha is preserved.
pub fn apply_sharpen_to_buffer(
    img: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    params: &SharpenParams,
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    if !params.is_active() {
        return img.clone();
    }

    let blurred = blur(img, params.radius);
    let (width, height) = img.dimensions();
    let mut out = ImageBuffer::new(width, height);
    let threshold = params.threshold * 255.0;

    for (x, y, pixel) in img.enumerate_pixels() {
        let blurred_pixel = blurred.get_pixel(x, y);
        let delta = luma(pixel) - luma(blurred_pixel);

        if delta.abs() < threshold {
            out.put_pixel(x, y, *pixel);
            continue;
        }

        let correction = params.amount * delta;
        let mut sharpened = [0u8; 4];
        for channel in 0..3 {
            sharpened[channel] = (pixel[channel] as f32 + correction).clamp(0.0, 255.0) as u8;
        }
        sharpened[3] = pixel[3]; // Preserve alpha
        out.put_pixel(x, y, Rgba(sharpened));
    }

    out
}

/// Rec. 601 luma, the same weighting `image`'s grayscale conversion uses.
fn luma(pixel: &Rgba<u8>) -> f32 {
    0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32
}

/// Apply an unsharp mask to a DynamicImage (for full-resolution export processing)
pub fn apply_sharpen(img: &DynamicImage, params: &SharpenParams) -> DynamicImage {
    DynamicImage::ImageRgba8(apply_sharpen_to_buffer(&img.to_rgba8(), params))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Radius large enough to be active, small enough to stay local.
    fn p(amount: f32) -> SharpenParams {
        SharpenParams::new(amount, 1.0, 0.0)
    }

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
        let result = apply_sharpen_to_buffer(&img, &p(0.0));
        assert_eq!(img, result);
    }

    #[test]
    fn test_negative_amount_is_identity() {
        let img = edge_image();
        let result = apply_sharpen_to_buffer(&img, &p(-1.0));
        assert_eq!(img, result);
    }

    #[test]
    fn test_negligible_radius_is_identity() {
        let img = edge_image();
        // A full-resolution radius scaled down for a tiny thumbnail lands here
        let result = apply_sharpen_to_buffer(&img, &SharpenParams::new(2.0, 0.05, 0.0));
        assert_eq!(img, result);
    }

    #[test]
    fn test_flat_image_unchanged() {
        let img = ImageBuffer::from_fn(20, 20, |_, _| Rgba([128u8, 128, 128, 255]));
        let result = apply_sharpen_to_buffer(&img, &p(2.0));
        // A flat image has no edges, so unsharp masking should not change it
        assert_eq!(img, result);
    }

    #[test]
    fn test_preserves_dimensions() {
        let img = edge_image();
        let result = apply_sharpen_to_buffer(&img, &p(1.5));
        assert_eq!(img.dimensions(), result.dimensions());
    }

    #[test]
    fn test_increases_edge_contrast() {
        let img = edge_image();
        let result = apply_sharpen_to_buffer(&img, &p(1.5));

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
        let mild = apply_sharpen_to_buffer(&img, &p(0.5));
        let strong = apply_sharpen_to_buffer(&img, &p(2.5));

        // Stronger sharpening should push the light edge pixel further up
        let mild_edge = mild.get_pixel(10, 10)[0];
        let strong_edge = strong.get_pixel(10, 10)[0];
        assert!(
            strong_edge >= mild_edge,
            "Stronger amount should sharpen at least as much: mild={mild_edge}, strong={strong_edge}"
        );
    }

    #[test]
    fn test_larger_radius_reaches_further() {
        // Wide enough that a large radius's halo does not run off the edge
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_fn(60, 20, |x, _| {
            if x < 30 {
                Rgba([50, 50, 50, 255])
            } else {
                Rgba([200, 200, 200, 255])
            }
        });

        // Furthest column right of the edge whose value actually moved
        let halo_reach = |result: &ImageBuffer<Rgba<u8>, Vec<u8>>| -> u32 {
            (30..60)
                .filter(|&x| {
                    let before = img.get_pixel(x, 10)[0] as i32;
                    let after = result.get_pixel(x, 10)[0] as i32;
                    (after - before).abs() > 1
                })
                .map(|x| x - 30)
                .max()
                .unwrap_or(0)
        };

        let tight = apply_sharpen_to_buffer(&img, &SharpenParams::new(1.5, 0.6, 0.0));
        let wide = apply_sharpen_to_buffer(&img, &SharpenParams::new(1.5, 2.5, 0.0));

        let tight_reach = halo_reach(&tight);
        let wide_reach = halo_reach(&wide);
        assert!(
            wide_reach > tight_reach,
            "Wide radius should spread the halo further: tight={tight_reach}, wide={wide_reach}"
        );
    }

    #[test]
    fn test_threshold_suppresses_low_contrast() {
        // A modest 25-level step. Blurring at sigma 1.0 moves the pixels next
        // to it by roughly 8 levels (~0.03 of the range), so a 0.10 threshold
        // sits above that while 0.0 lets it through.
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_fn(20, 20, |x, _| {
            if x < 10 {
                Rgba([120, 120, 120, 255])
            } else {
                Rgba([145, 145, 145, 255])
            }
        });
        let sharpened = apply_sharpen_to_buffer(&img, &SharpenParams::new(2.0, 1.0, 0.0));
        let thresholded = apply_sharpen_to_buffer(&img, &SharpenParams::new(2.0, 1.0, 0.10));

        assert_ne!(
            img, sharpened,
            "Without a threshold the low-contrast step is touched"
        );
        assert_eq!(
            img, thresholded,
            "A threshold above the local contrast should leave it alone"
        );
    }

    #[test]
    fn test_threshold_still_sharpens_strong_edges() {
        let img = edge_image();
        let result = apply_sharpen_to_buffer(&img, &SharpenParams::new(1.5, 1.0, 0.05));
        // The 50->200 step is far above a 5% threshold, so it must still sharpen
        assert!(result.get_pixel(10, 10)[0] > 200);
    }

    #[test]
    fn test_sharpening_is_achromatic() {
        // A saturated red/blue edge: per-channel sharpening would shift hue,
        // luminance-only sharpening moves all channels together.
        let img = ImageBuffer::from_fn(20, 20, |x, _| {
            if x < 10 {
                Rgba([180, 30, 30, 255])
            } else {
                Rgba([30, 30, 180, 255])
            }
        });
        let result = apply_sharpen_to_buffer(&img, &p(1.5));

        for (x, y, original) in img.enumerate_pixels() {
            let out = result.get_pixel(x, y);
            let d_r = out[0] as i32 - original[0] as i32;
            let d_g = out[1] as i32 - original[1] as i32;
            let d_b = out[2] as i32 - original[2] as i32;
            // Channels that are not clamped must move by the same amount
            let unclamped = |c: usize| original[c] > 5 && original[c] < 250;
            if unclamped(0) && unclamped(1) {
                assert_eq!(d_r, d_g, "R and G should shift together at ({x},{y})");
            }
            if unclamped(1) && unclamped(2) {
                assert_eq!(d_g, d_b, "G and B should shift together at ({x},{y})");
            }
        }
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
        let result = apply_sharpen_to_buffer(&img, &p(2.0));
        for pixel in result.pixels() {
            assert_eq!(pixel[3], 100, "Alpha should be preserved");
        }
    }

    #[test]
    fn test_apply_sharpen_dynamic_image() {
        let img = DynamicImage::ImageRgba8(edge_image());
        let result = apply_sharpen(&img, &p(1.0));
        assert_eq!(img.width(), result.width());
        assert_eq!(img.height(), result.height());
    }

    #[test]
    fn test_scaled_shrinks_radius_only() {
        let full = SharpenParams::new(1.5, 0.8, 0.02);
        let thumb = full.scaled(0.1);
        assert_eq!(thumb.amount, full.amount);
        assert_eq!(thumb.threshold, full.threshold);
        assert!((thumb.radius - 0.08).abs() < 1e-6);
        assert!(
            !thumb.is_active(),
            "A full-res radius scaled to thumbnail size is a no-op"
        );
    }

    #[test]
    fn test_from_layout_reads_all_three_knobs() {
        let params = crate::types::LayoutParams {
            sharpen_amount: 1.4,
            sharpen_radius: 0.9,
            sharpen_threshold: 0.03,
            ..Default::default()
        };
        let sharpen = params.sharpen_params();
        assert_eq!(sharpen.amount, 1.4);
        assert_eq!(sharpen.radius, 0.9);
        assert_eq!(sharpen.threshold, 0.03);
    }
}
