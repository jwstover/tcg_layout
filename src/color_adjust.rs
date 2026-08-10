use image::{DynamicImage, ImageBuffer, Rgba};

/// Maximum hue rotation exposed in the UI, in degrees
pub const MAX_HUE_SHIFT: f32 = 180.0;

/// Maximum half-width of the targeted hue band, in degrees
pub const MAX_HUE_RANGE: f32 = 90.0;

/// Default half-width of the targeted hue band, in degrees
pub const DEFAULT_HUE_RANGE: f32 = 20.0;

/// Bounds for the hue range suggested by `sample_region`
pub const MIN_SUGGESTED_HUE_RANGE: f32 = 8.0;
pub const MAX_SUGGESTED_HUE_RANGE: f32 = 40.0;

/// Default pixel radius sampled around a dropper click
pub const SAMPLE_RADIUS: u32 = 4;

/// The adjustment weight fades from zero at CHROMA_DEADZONE to full
/// effect at CHROMA_GATE. Hue is numerically meaningless when chroma is
/// small, so without this gate a shift would speckle noise across
/// near-neutral regions. Chroma (not HSL saturation) is the right gate:
/// HSL saturation blows up to 1.0 for near-black and near-white pixels
/// with the slightest channel imbalance, while chroma goes to zero at
/// both extremes. The dead zone keeps pixels within ~8 levels of neutral
/// exactly unchanged, so lightness shifts can't lift blacks.
const CHROMA_DEADZONE: f32 = 0.03;
const CHROMA_GATE: f32 = 0.12;

/// Which card images an adjustment applies to. Backs are the images used
/// on duplex back pages (per-card backs and the default back).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum AdjustmentScope {
    #[default]
    All,
    FrontsOnly,
    BacksOnly,
}

impl AdjustmentScope {
    /// All scopes, in UI display order
    pub const ALL: [AdjustmentScope; 3] = [
        AdjustmentScope::All,
        AdjustmentScope::FrontsOnly,
        AdjustmentScope::BacksOnly,
    ];

    pub fn applies_to(self, is_back: bool) -> bool {
        match self {
            AdjustmentScope::All => true,
            AdjustmentScope::FrontsOnly => !is_back,
            AdjustmentScope::BacksOnly => is_back,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            AdjustmentScope::All => "All cards",
            AdjustmentScope::FrontsOnly => "Fronts only",
            AdjustmentScope::BacksOnly => "Backs only",
        }
    }
}

/// A targeted HSL adjustment: pixels whose hue falls within
/// `target_hue ± hue_range` receive the full shifts; the effect fades
/// smoothly to zero over an additional `feather` degrees outside the band.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HslAdjustment {
    pub target_hue: f32,       // 0-360 degrees
    pub hue_range: f32,        // half-width of the full-effect band, degrees
    pub feather: f32,          // falloff width beyond the band, degrees
    pub hue_shift: f32,        // -180..180 degrees
    pub saturation_shift: f32, // -1.0..1.0, added to saturation
    pub lightness_shift: f32,  // -1.0..1.0, added to lightness
    /// Temporarily switch the adjustment off without deleting it
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Which card images this adjustment applies to (fronts, backs, or all)
    #[serde(default)]
    pub scope: AdjustmentScope,
}

fn default_enabled() -> bool {
    true
}

impl Default for HslAdjustment {
    fn default() -> Self {
        Self {
            target_hue: 0.0,
            hue_range: DEFAULT_HUE_RANGE,
            feather: DEFAULT_HUE_RANGE * 0.5,
            hue_shift: 0.0,
            saturation_shift: 0.0,
            lightness_shift: 0.0,
            enabled: true,
            scope: AdjustmentScope::All,
        }
    }
}

impl HslAdjustment {
    pub fn is_noop(&self) -> bool {
        self.hue_shift.abs() < 1e-3
            && self.saturation_shift.abs() < 1e-3
            && self.lightness_shift.abs() < 1e-3
    }

    /// Whether this adjustment would change any pixels: it must be enabled
    /// and have at least one non-zero shift
    pub fn is_active(&self) -> bool {
        self.enabled && !self.is_noop()
    }

    /// Whether this adjustment would change any pixels on the given side
    pub fn is_active_for(&self, is_back: bool) -> bool {
        self.is_active() && self.scope.applies_to(is_back)
    }
}

/// The subset of adjustments whose scope covers the given side. Pass the
/// result to `apply_hsl_adjustments` when processing a front or back image.
pub fn adjustments_for_side(adjustments: &[HslAdjustment], is_back: bool) -> Vec<HslAdjustment> {
    adjustments
        .iter()
        .filter(|a| a.scope.applies_to(is_back))
        .copied()
        .collect()
}

/// Convert 8-bit RGB to HSL: hue in 0-360 degrees, saturation and
/// lightness in 0.0-1.0. Gray pixels report a hue of 0.
pub fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;

    if d == 0.0 {
        return (0.0, 0.0, l);
    }

    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };

    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } * 60.0;

    (h, s, l)
}

/// Convert HSL back to 8-bit RGB. Hue wraps modulo 360; saturation and
/// lightness are clamped to 0.0-1.0.
pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);

    if s == 0.0 {
        let v = (l * 255.0).round() as u8;
        return (v, v, v);
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hk = (h / 360.0).rem_euclid(1.0);

    let to_u8 = |t: f32| (hue_to_rgb(p, q, t) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to_u8(hk + 1.0 / 3.0), to_u8(hk), to_u8(hk - 1.0 / 3.0))
}

fn hue_to_rgb(p: f32, q: f32, t: f32) -> f32 {
    let t = t.rem_euclid(1.0);
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 0.5 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

/// Chroma (max - min channel difference, 0.0-1.0) recovered from HSL.
/// Identically `(1 - |2L - 1|) * S`: for L <= 0.5, S = d / 2L, and for
/// L > 0.5, S = d / (2 - 2L), so the product is exactly d in both cases.
fn hsl_chroma(saturation: f32, lightness: f32) -> f32 {
    (1.0 - (2.0 * lightness - 1.0).abs()) * saturation
}

/// Shortest angular distance between two hues, in degrees (0-180)
fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge1 <= edge0 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Weight of an adjustment for a pixel at the given hue, saturation, and
/// lightness: 1.0 inside the hue band, smoothly falling to 0.0 across the
/// feather, scaled down toward zero chroma so neutral pixels — including
/// near-black and near-white — are untouched.
fn adjustment_weight(adj: &HslAdjustment, hue: f32, saturation: f32, lightness: f32) -> f32 {
    let d = hue_distance(hue, adj.target_hue);
    let hue_weight = if d <= adj.hue_range {
        1.0
    } else {
        1.0 - smoothstep(adj.hue_range, adj.hue_range + adj.feather.max(0.0), d)
    };
    hue_weight
        * smoothstep(
            CHROMA_DEADZONE,
            CHROMA_GATE,
            hsl_chroma(saturation, lightness),
        )
}

/// Apply a set of targeted HSL adjustments to an RGBA buffer. Each pixel
/// is converted to HSL once, all adjustments are applied in order, and the
/// result is converted back — so multiple adjustments don't accumulate
/// quantization error. Alpha is preserved.
pub fn apply_hsl_adjustments(
    img: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    adjustments: &[HslAdjustment],
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let active: Vec<&HslAdjustment> = adjustments.iter().filter(|a| a.is_active()).collect();
    if active.is_empty() {
        return img.clone();
    }

    let mut out = img.clone();
    for pixel in out.pixels_mut() {
        let (mut h, mut s, mut l) = rgb_to_hsl(pixel[0], pixel[1], pixel[2]);

        for adj in &active {
            let weight = adjustment_weight(adj, h, s, l);
            if weight > 0.0 {
                h = (h + weight * adj.hue_shift).rem_euclid(360.0);
                s = (s + weight * adj.saturation_shift).clamp(0.0, 1.0);
                l = (l + weight * adj.lightness_shift).clamp(0.0, 1.0);
            }
        }

        let (r, g, b) = hsl_to_rgb(h, s, l);
        pixel[0] = r;
        pixel[1] = g;
        pixel[2] = b;
    }

    out
}

/// Apply HSL adjustments to a DynamicImage (for full-resolution export processing)
pub fn apply_hsl_adjustments_to_image(
    img: &DynamicImage,
    adjustments: &[HslAdjustment],
) -> DynamicImage {
    DynamicImage::ImageRgba8(apply_hsl_adjustments(&img.to_rgba8(), adjustments))
}

/// Color sampled from a region of an image, used to seed a new adjustment
/// from a dropper click.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SampledColor {
    pub hue: f32,
    pub saturation: f32,
    pub lightness: f32,
    /// Suggested `hue_range` derived from the spread of hues in the sample
    pub suggested_range: f32,
}

/// Sample a square region of `radius` pixels around (cx, cy), clamped to the
/// image bounds, and compute a representative color. The hue is a circular
/// mean weighted by chroma, so near-neutral pixels (gray, near-black,
/// near-white) don't pollute it and reds straddling the 0°/360° seam
/// average correctly. The suggested range grows with the spread of
/// sampled hues.
pub fn sample_region(
    img: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    cx: u32,
    cy: u32,
    radius: u32,
) -> Option<SampledColor> {
    let (width, height) = img.dimensions();
    if width == 0 || height == 0 {
        return None;
    }

    let cx = cx.min(width - 1);
    let cy = cy.min(height - 1);
    let x0 = cx.saturating_sub(radius);
    let y0 = cy.saturating_sub(radius);
    let x1 = (cx + radius).min(width - 1);
    let y1 = (cy + radius).min(height - 1);

    let mut hues = Vec::new();
    let mut sum_s = 0.0f32;
    let mut sum_l = 0.0f32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            let p = img.get_pixel(x, y);
            let (h, s, l) = rgb_to_hsl(p[0], p[1], p[2]);
            hues.push((h, hsl_chroma(s, l)));
            sum_s += s;
            sum_l += l;
        }
    }

    let count = hues.len() as f32;
    let circular_mean = |weighted: bool| -> (f32, f32) {
        let mut sum_x = 0.0f32;
        let mut sum_y = 0.0f32;
        let mut total_weight = 0.0f32;
        for &(h, c) in &hues {
            let w = if weighted { c } else { 1.0 };
            let rad = h.to_radians();
            sum_x += w * rad.cos();
            sum_y += w * rad.sin();
            total_weight += w;
        }
        if total_weight <= 0.0 {
            return (0.0, 1.0);
        }
        let mean = sum_y.atan2(sum_x).to_degrees().rem_euclid(360.0);
        let resultant = (sum_x * sum_x + sum_y * sum_y).sqrt() / total_weight;
        (mean, resultant)
    };

    // Weight hues by chroma; fall back to unweighted if the whole
    // sample is essentially neutral
    let (mean_hue, resultant) = {
        let total_chroma: f32 = hues.iter().map(|&(_, c)| c).sum();
        if total_chroma > 1e-4 {
            circular_mean(true)
        } else {
            circular_mean(false)
        }
    };

    // Circular standard deviation from the resultant length: tight samples
    // have resultant near 1, scattered samples near 0
    let resultant = resultant.clamp(1e-6, 1.0);
    let circular_std_deg = (-2.0 * resultant.ln()).sqrt().to_degrees();
    let suggested_range =
        (2.0 * circular_std_deg).clamp(MIN_SUGGESTED_HUE_RANGE, MAX_SUGGESTED_HUE_RANGE);

    Some(SampledColor {
        hue: mean_hue,
        saturation: sum_s / count,
        lightness: sum_l / count,
        suggested_range,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yellow_adjustment(hue_shift: f32) -> HslAdjustment {
        HslAdjustment {
            target_hue: 55.0,
            hue_range: 20.0,
            feather: 10.0,
            hue_shift,
            saturation_shift: 0.0,
            lightness_shift: 0.0,
            ..Default::default()
        }
    }

    fn solid_image(color: [u8; 4]) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        ImageBuffer::from_fn(10, 10, |_, _| Rgba(color))
    }

    #[test]
    fn test_rgb_hsl_known_values() {
        assert_eq!(rgb_to_hsl(255, 0, 0), (0.0, 1.0, 0.5)); // red
        assert_eq!(rgb_to_hsl(0, 255, 0), (120.0, 1.0, 0.5)); // green
        assert_eq!(rgb_to_hsl(0, 0, 255), (240.0, 1.0, 0.5)); // blue
        let (h, s, _) = rgb_to_hsl(128, 128, 128);
        assert_eq!((h, s), (0.0, 0.0)); // gray has no hue or saturation
    }

    #[test]
    fn test_rgb_hsl_round_trip() {
        // Sample the color cube; every value should survive the round trip
        // within one step per channel
        for r in (0..=255).step_by(15) {
            for g in (0..=255).step_by(15) {
                for b in (0..=255).step_by(15) {
                    let (h, s, l) = rgb_to_hsl(r as u8, g as u8, b as u8);
                    let (r2, g2, b2) = hsl_to_rgb(h, s, l);
                    assert!(
                        (r - r2 as i32).abs() <= 1
                            && (g - g2 as i32).abs() <= 1
                            && (b - b2 as i32).abs() <= 1,
                        "round trip failed for ({r}, {g}, {b}) -> ({r2}, {g2}, {b2})"
                    );
                }
            }
        }
    }

    #[test]
    fn test_empty_adjustments_is_identity() {
        let img = solid_image([200, 180, 40, 255]);
        assert_eq!(apply_hsl_adjustments(&img, &[]), img);
    }

    #[test]
    fn test_noop_adjustment_is_identity() {
        let img = solid_image([200, 180, 40, 255]);
        let noop = HslAdjustment {
            target_hue: 55.0,
            ..Default::default()
        };
        assert_eq!(apply_hsl_adjustments(&img, &[noop]), img);
    }

    #[test]
    fn test_disabled_adjustment_is_identity() {
        let img = solid_image([220, 200, 40, 255]);
        let disabled = HslAdjustment {
            enabled: false,
            ..yellow_adjustment(60.0)
        };
        assert_eq!(apply_hsl_adjustments(&img, &[disabled]), img);

        // Disabled adjustments don't block others in the list
        let result = apply_hsl_adjustments(&img, &[disabled, yellow_adjustment(30.0)]);
        assert_ne!(result, img);
    }

    #[test]
    fn test_enabled_defaults_to_true_when_missing_from_json() {
        // Settings saved before the enabled flag existed must deserialize
        // as enabled
        let json = r#"{
            "target_hue": 55.0,
            "hue_range": 20.0,
            "feather": 10.0,
            "hue_shift": 30.0,
            "saturation_shift": 0.0,
            "lightness_shift": 0.0
        }"#;
        let adj: HslAdjustment = serde_json::from_str(json).unwrap();
        assert!(adj.enabled);
        assert!(adj.is_active());
        // Settings saved before the scope field existed apply to all cards
        assert_eq!(adj.scope, AdjustmentScope::All);
    }

    #[test]
    fn test_scope_applies_to_sides() {
        assert!(AdjustmentScope::All.applies_to(false));
        assert!(AdjustmentScope::All.applies_to(true));
        assert!(AdjustmentScope::FrontsOnly.applies_to(false));
        assert!(!AdjustmentScope::FrontsOnly.applies_to(true));
        assert!(!AdjustmentScope::BacksOnly.applies_to(false));
        assert!(AdjustmentScope::BacksOnly.applies_to(true));
    }

    #[test]
    fn test_adjustments_for_side_filters_by_scope() {
        let all = yellow_adjustment(10.0);
        let fronts = HslAdjustment {
            scope: AdjustmentScope::FrontsOnly,
            ..yellow_adjustment(20.0)
        };
        let backs = HslAdjustment {
            scope: AdjustmentScope::BacksOnly,
            ..yellow_adjustment(30.0)
        };
        let adjustments = [all, fronts, backs];

        let front_set = adjustments_for_side(&adjustments, false);
        assert_eq!(front_set, vec![all, fronts]);

        let back_set = adjustments_for_side(&adjustments, true);
        assert_eq!(back_set, vec![all, backs]);
    }

    #[test]
    fn test_is_active_for_respects_scope_and_enabled() {
        let backs_only = HslAdjustment {
            scope: AdjustmentScope::BacksOnly,
            ..yellow_adjustment(30.0)
        };
        assert!(backs_only.is_active_for(true));
        assert!(!backs_only.is_active_for(false));

        let disabled = HslAdjustment {
            enabled: false,
            ..backs_only
        };
        assert!(!disabled.is_active_for(true));
    }

    #[test]
    fn test_gray_untouched_by_hue_shift() {
        // Warm gray: numerically inside the yellow band but nearly unsaturated
        let img = solid_image([129, 128, 126, 255]);
        let result = apply_hsl_adjustments(&img, &[yellow_adjustment(60.0)]);
        for (orig, out) in img.pixels().zip(result.pixels()) {
            for c in 0..3 {
                assert!(
                    (orig[c] as i32 - out[c] as i32).abs() <= 1,
                    "gray pixel changed: {orig:?} -> {out:?}"
                );
            }
        }
    }

    #[test]
    fn test_hsl_chroma_matches_max_minus_min() {
        for &(r, g, b) in &[(10u8, 2u8, 2u8), (220, 200, 40), (255, 250, 250), (0, 0, 0)] {
            let (_, s, l) = rgb_to_hsl(r, g, b);
            let expected = (r.max(g).max(b) as f32 - r.min(g).min(b) as f32) / 255.0;
            assert!(
                (hsl_chroma(s, l) - expected).abs() < 1e-4,
                "chroma mismatch for ({r}, {g}, {b})"
            );
        }
    }

    #[test]
    fn test_near_black_untouched() {
        // Visually black, but with a slight red cast: HSL saturation is
        // ~0.67 here, so a saturation-only gate would pass it at full
        // weight and a lightness shift would visibly lift the shadows
        let img = solid_image([10, 2, 2, 255]);
        let adj = HslAdjustment {
            target_hue: 0.0,
            hue_range: 25.0,
            feather: 10.0,
            hue_shift: 30.0,
            saturation_shift: 0.3,
            lightness_shift: 0.3,
            ..Default::default()
        };
        let result = apply_hsl_adjustments(&img, &[adj]);
        for (orig, out) in img.pixels().zip(result.pixels()) {
            for c in 0..3 {
                assert!(
                    (orig[c] as i32 - out[c] as i32).abs() <= 2,
                    "near-black pixel changed: {orig:?} -> {out:?}"
                );
            }
        }
    }

    #[test]
    fn test_near_white_untouched() {
        // Visually white with a slight warm cast; HSL saturation is ~1.0
        let img = solid_image([255, 250, 248, 255]);
        let adj = HslAdjustment {
            target_hue: 17.0,
            hue_range: 25.0,
            feather: 10.0,
            hue_shift: 30.0,
            saturation_shift: 0.3,
            lightness_shift: -0.3,
            ..Default::default()
        };
        let result = apply_hsl_adjustments(&img, &[adj]);
        for (orig, out) in img.pixels().zip(result.pixels()) {
            for c in 0..3 {
                assert!(
                    (orig[c] as i32 - out[c] as i32).abs() <= 2,
                    "near-white pixel changed: {orig:?} -> {out:?}"
                );
            }
        }
    }

    #[test]
    fn test_sample_region_ignores_near_black_noise() {
        // A saturated yellow patch peppered with near-black pixels whose
        // noise hue is blue-ish; chroma weighting should keep the mean hue
        // on yellow
        let img = ImageBuffer::from_fn(10, 10, |x, y| {
            if (x + y) % 3 == 0 {
                Rgba([2, 3, 10, 255]) // near-black, hue ~232
            } else {
                Rgba([220, 200, 40, 255]) // hue ~53
            }
        });
        let sample = sample_region(&img, 5, 5, 4).unwrap();
        let (h_yellow, _, _) = rgb_to_hsl(220, 200, 40);
        assert!(
            (sample.hue - h_yellow).abs() < 5.0,
            "near-black noise polluted the mean hue: {} vs {h_yellow}",
            sample.hue
        );
    }

    #[test]
    fn test_targeted_hue_shifted_others_untouched() {
        let yellow = solid_image([220, 200, 40, 255]); // hue ~53
        let blue = solid_image([40, 80, 220, 255]); // hue ~227

        let adj = [yellow_adjustment(30.0)];
        let yellow_out = apply_hsl_adjustments(&yellow, &adj);
        let blue_out = apply_hsl_adjustments(&blue, &adj);

        assert_ne!(yellow_out, yellow, "yellow should be shifted");
        assert_eq!(blue_out, blue, "blue is far outside the band");

        let (h_before, _, _) = rgb_to_hsl(220, 200, 40);
        let p = yellow_out.get_pixel(0, 0);
        let (h_after, _, _) = rgb_to_hsl(p[0], p[1], p[2]);
        assert!(
            (h_after - h_before - 30.0).abs() < 3.0,
            "expected ~30 degree shift, got {h_before} -> {h_after}"
        );
    }

    #[test]
    fn test_hue_wraparound_at_red_seam() {
        // Hue ~350 sits within range 20 of target 0 across the seam
        let img = solid_image([220, 30, 60, 255]);
        let (h, _, _) = rgb_to_hsl(220, 30, 60);
        assert!(h > 330.0, "test pixel should be near the seam, got {h}");

        let adj = HslAdjustment {
            target_hue: 0.0,
            hue_range: 25.0,
            feather: 10.0,
            hue_shift: 20.0,
            saturation_shift: 0.0,
            lightness_shift: 0.0,
            ..Default::default()
        };
        let result = apply_hsl_adjustments(&img, &[adj]);
        assert_ne!(result, img, "red across the 0/360 seam should be targeted");
    }

    #[test]
    fn test_saturation_shift_desaturates_target() {
        let img = solid_image([220, 200, 40, 255]);
        let adj = HslAdjustment {
            target_hue: 55.0,
            hue_range: 20.0,
            feather: 10.0,
            hue_shift: 0.0,
            saturation_shift: -0.5,
            lightness_shift: 0.0,
            ..Default::default()
        };
        let result = apply_hsl_adjustments(&img, &[adj]);
        let (_, s_before, _) = rgb_to_hsl(220, 200, 40);
        let p = result.get_pixel(0, 0);
        let (_, s_after, _) = rgb_to_hsl(p[0], p[1], p[2]);
        assert!(
            s_after < s_before - 0.3,
            "saturation should drop: {s_before} -> {s_after}"
        );
    }

    #[test]
    fn test_pixel_beyond_feather_untouched() {
        // Hue ~120 (green) vs yellow band ending at 55 + 20 + 10 = 85
        let img = solid_image([40, 220, 40, 255]);
        let result = apply_hsl_adjustments(&img, &[yellow_adjustment(60.0)]);
        assert_eq!(result, img);
    }

    #[test]
    fn test_alpha_preserved() {
        let img = solid_image([220, 200, 40, 100]);
        let result = apply_hsl_adjustments(&img, &[yellow_adjustment(45.0)]);
        for pixel in result.pixels() {
            assert_eq!(pixel[3], 100, "alpha should be preserved");
        }
    }

    #[test]
    fn test_apply_to_dynamic_image() {
        let img = DynamicImage::ImageRgba8(solid_image([220, 200, 40, 255]));
        let result = apply_hsl_adjustments_to_image(&img, &[yellow_adjustment(30.0)]);
        assert_eq!(result.width(), img.width());
        assert_eq!(result.height(), img.height());
    }

    #[test]
    fn test_sample_region_uniform_patch() {
        let img = solid_image([220, 200, 40, 255]);
        let sample = sample_region(&img, 5, 5, 4).unwrap();
        let (h, s, _) = rgb_to_hsl(220, 200, 40);
        assert!((sample.hue - h).abs() < 1.0, "hue {} vs {h}", sample.hue);
        assert!((sample.saturation - s).abs() < 0.05);
        // A uniform patch has zero spread, so the range clamps to the floor
        assert_eq!(sample.suggested_range, MIN_SUGGESTED_HUE_RANGE);
    }

    #[test]
    fn test_sample_region_circular_mean_across_seam() {
        // Alternate two saturated reds straddling the 0/360 seam; a naive
        // arithmetic mean would land near cyan (~180)
        let img = ImageBuffer::from_fn(10, 10, |x, _| {
            if x % 2 == 0 {
                Rgba([230, 25, 45, 255]) // hue ~354
            } else {
                Rgba([230, 45, 25, 255]) // hue ~6
            }
        });
        let sample = sample_region(&img, 5, 5, 4).unwrap();
        let dist = {
            let d = sample.hue.rem_euclid(360.0);
            d.min(360.0 - d)
        };
        assert!(
            dist < 10.0,
            "circular mean should be near 0, got {}",
            sample.hue
        );
    }

    #[test]
    fn test_sample_region_gray_patch() {
        let img = solid_image([128, 128, 128, 255]);
        let sample = sample_region(&img, 5, 5, 4).unwrap();
        assert!(sample.saturation < 0.01);
        assert!((sample.lightness - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_sample_region_clamps_to_bounds() {
        let img = solid_image([220, 200, 40, 255]);
        // Corner and out-of-bounds coordinates should not panic
        assert!(sample_region(&img, 0, 0, 4).is_some());
        assert!(sample_region(&img, 100, 100, 4).is_some());
    }

    #[test]
    fn test_multiple_adjustments_apply_together() {
        // Half yellow, half blue; one adjustment per color
        let img = ImageBuffer::from_fn(10, 10, |x, _| {
            if x < 5 {
                Rgba([220, 200, 40, 255])
            } else {
                Rgba([40, 80, 220, 255])
            }
        });
        let adjustments = [
            yellow_adjustment(30.0),
            HslAdjustment {
                target_hue: 227.0,
                hue_range: 15.0,
                feather: 10.0,
                hue_shift: 0.0,
                saturation_shift: -0.4,
                lightness_shift: 0.0,
                ..Default::default()
            },
        ];
        let result = apply_hsl_adjustments(&img, &adjustments);

        let yellow_out = result.get_pixel(0, 0);
        let (h_yellow, _, _) = rgb_to_hsl(yellow_out[0], yellow_out[1], yellow_out[2]);
        let (h_orig, _, _) = rgb_to_hsl(220, 200, 40);
        assert!((h_yellow - h_orig - 30.0).abs() < 3.0);

        let blue_out = result.get_pixel(9, 0);
        let (_, s_blue, _) = rgb_to_hsl(blue_out[0], blue_out[1], blue_out[2]);
        let (_, s_orig, _) = rgb_to_hsl(40, 80, 220);
        assert!(s_blue < s_orig - 0.2);
    }
}
