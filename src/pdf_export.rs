use crate::layout::{calculate_cut_marks, calculate_grid};
use crate::types::{LayoutParams, PageLayout, PageOrientation, PageSide};
use anyhow::{Context, Result};
use printpdf::*;
use printpdf::{ImageCompression, ImageOptimizationOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tcg_layout::{bleed, color_adjust, sharpen};

pub struct PdfExporter {
    params: LayoutParams,
}

impl PdfExporter {
    pub fn new(params: LayoutParams) -> Self {
        Self { params }
    }

    pub fn export_pages(&self, pages: &[PageLayout], output_path: &Path) -> Result<()> {
        self.export_pages_with_progress(pages, output_path, |_, _| {})
    }

    pub fn export_pages_with_progress<F: Fn(usize, usize)>(
        &self,
        pages: &[PageLayout],
        output_path: &Path,
        progress: F,
    ) -> Result<()> {
        let mut doc = PdfDocument::new(&format!("TCG Layout - {} pages", pages.len()));
        let mut warnings = Vec::new();

        let (page_width, page_height) = self.get_page_dimensions();

        // Build cut mark operations (same for every page)
        let grid = calculate_grid(&self.params);
        let cut_marks = calculate_cut_marks(&self.params, &grid);
        let cut_mark_ops = self.build_cut_mark_ops(&cut_marks, page_height);

        // Image deduplication: embed each unique (file, rotation, side) once,
        // track pixel dims for sizing. The side only enters the key when
        // adjustments are side-scoped, so an image used on both sides is
        // still embedded once whenever its pixels would be identical.
        let side_scoped = self.side_scoped_color_adjust();
        let mut image_cache: HashMap<(PathBuf, bool, bool), (XObjectId, f32, f32)> = HashMap::new();

        let mut pdf_pages = Vec::new();

        for (page_idx, page) in pages.iter().enumerate() {
            let mut ops = Vec::new();

            let is_back = page.side == PageSide::Back;

            // Back pages print rotated 180° when the duplex flip is about a
            // horizontal axis, so cut cards come out head-to-head.
            let rotate_180 = is_back && self.params.backs_rotated_180();

            // Draw card images
            for (card, position) in &page.cards {
                let (image_bytes, card_w_mm, card_h_mm, img_x_mm, img_y_mm) =
                    if self.needs_image_processing_for(is_back) || rotate_180 {
                        self.prepare_processed_image(card, position, rotate_180, is_back)?
                    } else {
                        let bytes = std::fs::read(&card.path).with_context(|| {
                            format!("Failed to read image: {}", card.path.display())
                        })?;
                        (
                            bytes,
                            self.params.card_size.0,
                            self.params.card_size.1,
                            position.x,
                            position.y,
                        )
                    };

                // Get or create the XObject ID for this image
                let cache_key = (card.path.clone(), rotate_180, is_back && side_scoped);
                let (xobject_id, img_pixel_width, img_pixel_height) = if let Some((id, pw, ph)) =
                    image_cache.get(&cache_key)
                {
                    (id.clone(), *pw, *ph)
                } else {
                    let raw_image = RawImage::decode_from_bytes(&image_bytes, &mut warnings)
                        .map_err(|e| {
                            anyhow::anyhow!("Failed to decode image {}: {e}", card.path.display())
                        })?;
                    let pw = raw_image.width as f32;
                    let ph = raw_image.height as f32;
                    let id = doc.add_image(&raw_image);
                    image_cache.insert(cache_key, (id.clone(), pw, ph));
                    (id, pw, ph)
                };

                // Calculate DPI so the image width fills card_w_mm exactly.
                // printpdf applies this DPI uniformly to both axes, so if the
                // image aspect ratio differs from the card aspect ratio the
                // height won't match. We correct with scale_y.
                let dpi = img_pixel_width * 25.4 / card_w_mm;
                let scale_y = (card_h_mm * img_pixel_width) / (card_w_mm * img_pixel_height);

                // PDF Y-axis is bottom-up; layout Y-axis is top-down.
                // pdf_y = page_height - position.y - card_height
                let pdf_x_pt = Mm(img_x_mm).into_pt();
                let pdf_y_pt = Mm(page_height - img_y_mm - card_h_mm).into_pt();

                ops.push(Op::UseXobject {
                    id: xobject_id,
                    transform: XObjectTransform {
                        translate_x: Some(pdf_x_pt),
                        translate_y: Some(pdf_y_pt),
                        dpi: Some(dpi),
                        scale_y: Some(scale_y),
                        ..XObjectTransform::default()
                    },
                });
            }

            // Draw cut marks last so they sit on top of the card images: with
            // bleed on, the images cover the margins the marks live in, and the
            // marks overlap the trim edges by design. Back pages get none:
            // cards are cut from the front, and marks would need mirroring +
            // the calibration offset to line up.
            if page.side == PageSide::Front {
                ops.extend(cut_mark_ops.clone());
            }

            pdf_pages.push(PdfPage::new(Mm(page_width), Mm(page_height), ops));
            progress(page_idx + 1, pages.len());
        }

        let save_opts = PdfSaveOptions {
            image_optimization: Some(ImageOptimizationOptions {
                format: Some(ImageCompression::Flate),
                quality: None,
                max_image_size: None,
                auto_optimize: Some(false),
                dither_greyscale: None,
                convert_to_greyscale: None,
            }),
            ..PdfSaveOptions::default()
        };

        let pdf_bytes = doc.with_pages(pdf_pages).save(&save_opts, &mut warnings);

        std::fs::write(output_path, pdf_bytes)
            .with_context(|| format!("Failed to write PDF to {}", output_path.display()))?;

        Ok(())
    }

    fn get_page_dimensions(&self) -> (f32, f32) {
        match self.params.page_orientation {
            PageOrientation::Portrait => self.params.page_size,
            PageOrientation::Landscape => (self.params.page_size.1, self.params.page_size.0),
        }
    }

    fn build_cut_mark_ops(&self, cut_marks: &[crate::types::CutMark], page_height: f32) -> Vec<Op> {
        let mut ops = Vec::new();

        if cut_marks.is_empty() {
            return ops;
        }

        // Set cut mark style: gray, thin line
        ops.push(Op::SetOutlineColor {
            col: Color::Rgb(Rgb::new(0.5, 0.5, 0.5, None)),
        });
        ops.push(Op::SetOutlineThickness { pt: Pt(0.5) });

        for mark in cut_marks {
            // Flip Y coordinates for PDF
            let y1 = page_height - mark.y1;
            let y2 = page_height - mark.y2;

            let line = Line {
                points: vec![
                    LinePoint {
                        p: Point::new(Mm(mark.x1), Mm(y1)),
                        bezier: false,
                    },
                    LinePoint {
                        p: Point::new(Mm(mark.x2), Mm(y2)),
                        bezier: false,
                    },
                ],
                is_closed: false,
            };

            ops.push(Op::DrawLine { line });
        }

        ops
    }

    fn bleed_active(&self) -> bool {
        self.params.enable_bleed && self.params.bleed_mm > 0.0
    }

    fn sharpen_active(&self) -> bool {
        self.params.enable_sharpen && self.params.sharpen_params().is_active()
    }

    fn color_adjust_active_for(&self, is_back: bool) -> bool {
        self.params.enable_color_adjust
            && self
                .params
                .hsl_adjustments
                .iter()
                .any(|a| a.is_active_for(is_back))
    }

    /// Whether processed pixels differ between fronts and backs: some active
    /// adjustment is scoped to a single side. When true, the image dedup
    /// cache must not share entries across sides.
    fn side_scoped_color_adjust(&self) -> bool {
        self.params.enable_color_adjust
            && self
                .params
                .hsl_adjustments
                .iter()
                .any(|a| a.is_active() && a.scope != color_adjust::AdjustmentScope::All)
    }

    fn needs_image_processing_for(&self, is_back: bool) -> bool {
        self.bleed_active() || self.sharpen_active() || self.color_adjust_active_for(is_back)
    }

    /// Load an image and apply color adjustments, sharpening, and/or bleed,
    /// returning PNG bytes plus placement dimensions/position in mm. Color
    /// adjustments run first (on the original colors), then sharpening, so
    /// bleed strips derive from the fully processed image. `rotate_180`
    /// rotates the final image in place (for duplex back pages). `is_back`
    /// selects which scoped color adjustments apply.
    fn prepare_processed_image(
        &self,
        card: &crate::types::Card,
        position: &crate::types::CardPosition,
        rotate_180: bool,
        is_back: bool,
    ) -> Result<(Vec<u8>, f32, f32, f32, f32)> {
        let mut img = ::image::open(&card.path)
            .with_context(|| format!("Failed to open image: {}", card.path.display()))?;

        if self.color_adjust_active_for(is_back) {
            let adjustments =
                color_adjust::adjustments_for_side(&self.params.hsl_adjustments, is_back);
            img = color_adjust::apply_hsl_adjustments_to_image(&img, &adjustments);
        }

        if self.sharpen_active() {
            img = sharpen::apply_sharpen(&img, &self.params.sharpen_params());
        }

        let (card_width, card_height, offset_x, offset_y) = if self.bleed_active() {
            let (bleed_pixels_x, bleed_pixels_y) = bleed::calculate_bleed_pixels_from_dimensions(
                self.params.bleed_mm,
                self.params.card_size,
                (img.width(), img.height()),
            );

            let bleed_pixels = ((bleed_pixels_x + bleed_pixels_y) / 2).max(1);
            img =
                ::image::DynamicImage::ImageRgba8(bleed::apply_bleed_to_image(&img, bleed_pixels));

            (
                self.params.card_size.0 + 2.0 * self.params.bleed_mm,
                self.params.card_size.1 + 2.0 * self.params.bleed_mm,
                position.x - self.params.bleed_mm,
                position.y - self.params.bleed_mm,
            )
        } else {
            (
                self.params.card_size.0,
                self.params.card_size.1,
                position.x,
                position.y,
            )
        };

        // Rotate last so the fully processed image (including bleed) turns
        // as one piece; the placement rectangle is unchanged
        if rotate_180 {
            img = img.rotate180();
        }

        // Encode to PNG in memory
        let mut png_bytes = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png_bytes, ::image::ImageFormat::Png)
            .context("Failed to encode processed image to PNG")?;

        Ok((
            png_bytes.into_inner(),
            card_width,
            card_height,
            offset_x,
            offset_y,
        ))
    }
}

/// Utility function to export pages to a PDF file
pub fn export_pages_to_pdf(
    pages: &[PageLayout],
    params: &LayoutParams,
    output_path: &Path,
) -> Result<()> {
    export_pages_to_pdf_with_progress(pages, params, output_path, |_, _| {})
}

/// Utility function to export pages to a PDF file with progress callback
pub fn export_pages_to_pdf_with_progress<F: Fn(usize, usize)>(
    pages: &[PageLayout],
    params: &LayoutParams,
    output_path: &Path,
    progress: F,
) -> Result<()> {
    let exporter = PdfExporter::new(params.clone());
    exporter.export_pages_with_progress(pages, output_path, progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        Card, CardPosition, FillOrder, FlipEdge, LayoutParams, Margins, PageLayout, PageOrientation,
    };
    use tempfile::TempDir;

    fn create_test_params() -> LayoutParams {
        LayoutParams {
            page_size: (100.0, 150.0),
            card_size: (20.0, 30.0),
            margins: Margins::uniform(5.0),
            spacing: (2.0, 2.0),
            orientation: FillOrder::RowMajor,
            page_orientation: PageOrientation::Portrait,
            target_dpi: 300,
            bleed_mm: 0.0,
            enable_bleed: false,
            sharpen_amount: 1.0,
            sharpen_radius: 0.7,
            sharpen_threshold: 0.02,
            enable_sharpen: false,
            center_layout: false,
            hsl_adjustments: Vec::new(),
            enable_color_adjust: false,
            ..Default::default()
        }
    }

    #[test]
    fn test_pdf_exporter_creation() {
        let params = create_test_params();
        let exporter = PdfExporter::new(params);
        assert_eq!(exporter.params.page_size, (100.0, 150.0));
    }

    #[test]
    fn test_get_page_dimensions_portrait() {
        let params = LayoutParams {
            page_size: (210.0, 297.0),
            page_orientation: PageOrientation::Portrait,
            ..create_test_params()
        };
        let exporter = PdfExporter::new(params);
        let (width, height) = exporter.get_page_dimensions();
        assert_eq!(width, 210.0);
        assert_eq!(height, 297.0);
    }

    #[test]
    fn test_get_page_dimensions_landscape() {
        let params = LayoutParams {
            page_size: (210.0, 297.0),
            page_orientation: PageOrientation::Landscape,
            ..create_test_params()
        };
        let exporter = PdfExporter::new(params);
        let (width, height) = exporter.get_page_dimensions();
        assert_eq!(width, 297.0);
        assert_eq!(height, 210.0);
    }

    #[test]
    fn test_build_cut_mark_ops_empty() {
        let params = create_test_params();
        let exporter = PdfExporter::new(params);
        let ops = exporter.build_cut_mark_ops(&[], 150.0);
        assert!(ops.is_empty());
    }

    #[test]
    fn test_build_cut_mark_ops_has_marks() {
        let params = create_test_params();
        let exporter = PdfExporter::new(params);
        let marks = vec![crate::types::CutMark {
            x1: 5.0,
            y1: 0.0,
            x2: 5.0,
            y2: 5.0,
            mark_type: crate::types::CutMarkType::Vertical,
        }];
        let ops = exporter.build_cut_mark_ops(&marks, 150.0);
        // Should have: SetOutlineColor, SetOutlineThickness, DrawLine
        assert_eq!(ops.len(), 3);
    }

    #[test]
    fn test_export_pages_with_real_image() {
        // Create a small test PNG image
        let temp_dir = TempDir::new().unwrap();
        let img_path = temp_dir.path().join("test_card.png");

        let img = ::image::ImageBuffer::from_fn(100, 140, |_, _| ::image::Rgba([255u8, 0, 0, 255]));
        img.save(&img_path).unwrap();

        let params = create_test_params();
        let pages = vec![PageLayout {
            page_number: 1,
            cards: vec![(Card::new(img_path), CardPosition { x: 5.0, y: 5.0 })],
            side: PageSide::Front,
        }];

        let output_path = temp_dir.path().join("output.pdf");
        let result = export_pages_to_pdf(&pages, &params, &output_path);
        assert!(result.is_ok(), "Export failed: {:?}", result.err());
        assert!(output_path.exists());

        // Verify file is non-empty and starts with PDF header
        let bytes = std::fs::read(&output_path).unwrap();
        assert!(bytes.len() > 100);
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn test_export_with_sharpening() {
        let temp_dir = TempDir::new().unwrap();
        let img_path = temp_dir.path().join("test_card.png");

        let img = ::image::ImageBuffer::from_fn(100, 140, |x, _| {
            if x < 50 {
                ::image::Rgba([50u8, 50, 50, 255])
            } else {
                ::image::Rgba([200u8, 200, 200, 255])
            }
        });
        img.save(&img_path).unwrap();

        let params = LayoutParams {
            sharpen_amount: 1.5,
            sharpen_radius: 0.7,
            sharpen_threshold: 0.02,
            enable_sharpen: true,
            ..create_test_params()
        };
        let pages = vec![PageLayout {
            page_number: 1,
            cards: vec![(Card::new(img_path), CardPosition { x: 5.0, y: 5.0 })],
            side: PageSide::Front,
        }];

        let output_path = temp_dir.path().join("sharpened.pdf");
        let result = export_pages_to_pdf(&pages, &params, &output_path);
        assert!(result.is_ok(), "Export failed: {:?}", result.err());

        let bytes = std::fs::read(&output_path).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn test_export_with_sharpening_and_bleed() {
        let temp_dir = TempDir::new().unwrap();
        let img_path = temp_dir.path().join("test_card.png");

        let img = ::image::ImageBuffer::from_fn(100, 140, |_, _| ::image::Rgba([255u8, 0, 0, 255]));
        img.save(&img_path).unwrap();

        let params = LayoutParams {
            bleed_mm: 2.0,
            enable_bleed: true,
            sharpen_amount: 1.0,
            sharpen_radius: 0.7,
            sharpen_threshold: 0.02,
            enable_sharpen: true,
            ..create_test_params()
        };
        let pages = vec![PageLayout {
            page_number: 1,
            cards: vec![(Card::new(img_path), CardPosition { x: 5.0, y: 5.0 })],
            side: PageSide::Front,
        }];

        let output_path = temp_dir.path().join("both.pdf");
        let result = export_pages_to_pdf(&pages, &params, &output_path);
        assert!(result.is_ok(), "Export failed: {:?}", result.err());

        let bytes = std::fs::read(&output_path).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn test_export_with_color_adjustments() {
        let temp_dir = TempDir::new().unwrap();
        let img_path = temp_dir.path().join("test_card.png");

        let img =
            ::image::ImageBuffer::from_fn(100, 140, |_, _| ::image::Rgba([220u8, 200, 40, 255]));
        img.save(&img_path).unwrap();

        let params = LayoutParams {
            enable_color_adjust: true,
            hsl_adjustments: vec![color_adjust::HslAdjustment {
                target_hue: 55.0,
                hue_range: 20.0,
                feather: 10.0,
                hue_shift: 30.0,
                saturation_shift: -0.2,
                lightness_shift: 0.0,
                ..Default::default()
            }],
            ..create_test_params()
        };
        let pages = vec![PageLayout {
            page_number: 1,
            cards: vec![(Card::new(img_path), CardPosition { x: 5.0, y: 5.0 })],
            side: PageSide::Front,
        }];

        let output_path = temp_dir.path().join("color_adjusted.pdf");
        let result = export_pages_to_pdf(&pages, &params, &output_path);
        assert!(result.is_ok(), "Export failed: {:?}", result.err());

        let bytes = std::fs::read(&output_path).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn test_export_duplex_with_backs_only_color_adjustment() {
        let temp_dir = TempDir::new().unwrap();
        let front_path = temp_dir.path().join("front.png");
        let back_path = temp_dir.path().join("back.png");

        let front =
            ::image::ImageBuffer::from_fn(100, 140, |_, _| ::image::Rgba([220u8, 200, 40, 255]));
        front.save(&front_path).unwrap();
        let back =
            ::image::ImageBuffer::from_fn(100, 140, |_, _| ::image::Rgba([220u8, 40, 60, 255]));
        back.save(&back_path).unwrap();

        let params = LayoutParams {
            enable_duplex: true,
            default_back_path: Some(back_path),
            enable_color_adjust: true,
            hsl_adjustments: vec![color_adjust::HslAdjustment {
                target_hue: 0.0,
                hue_range: 25.0,
                feather: 10.0,
                hue_shift: 30.0,
                saturation_shift: 0.0,
                lightness_shift: 0.0,
                scope: color_adjust::AdjustmentScope::BacksOnly,
                ..Default::default()
            }],
            ..create_test_params()
        };

        let grid = crate::layout::calculate_grid(&params);
        let pages = crate::layout::distribute_cards(&[Card::new(front_path)], &grid, &params);
        assert_eq!(pages[0].side, PageSide::Front);
        assert_eq!(pages[1].side, PageSide::Back);

        let output_path = temp_dir.path().join("backs_only.pdf");
        let result = export_pages_to_pdf(&pages, &params, &output_path);
        assert!(result.is_ok(), "Export failed: {:?}", result.err());

        let bytes = std::fs::read(&output_path).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn test_export_multiple_pages() {
        let temp_dir = TempDir::new().unwrap();
        let img_path = temp_dir.path().join("test_card.png");

        let img =
            ::image::ImageBuffer::from_fn(100, 140, |_, _| ::image::Rgba([0u8, 128, 255, 255]));
        img.save(&img_path).unwrap();

        let params = create_test_params();
        let pages = vec![
            PageLayout {
                page_number: 1,
                cards: vec![
                    (Card::new(img_path.clone()), CardPosition { x: 5.0, y: 5.0 }),
                    (
                        Card::new(img_path.clone()),
                        CardPosition { x: 27.0, y: 5.0 },
                    ),
                ],
                side: PageSide::Front,
            },
            PageLayout {
                page_number: 2,
                cards: vec![(Card::new(img_path), CardPosition { x: 5.0, y: 5.0 })],
                side: PageSide::Front,
            },
        ];

        let output_path = temp_dir.path().join("multi_page.pdf");
        let result = export_pages_to_pdf(&pages, &params, &output_path);
        assert!(result.is_ok(), "Export failed: {:?}", result.err());
        assert!(output_path.exists());

        let bytes = std::fs::read(&output_path).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn test_export_duplex_pages_end_to_end() {
        let temp_dir = TempDir::new().unwrap();
        let front_path = temp_dir.path().join("front.png");
        let back_path = temp_dir.path().join("back.png");

        let front =
            ::image::ImageBuffer::from_fn(100, 140, |_, _| ::image::Rgba([255u8, 0, 0, 255]));
        front.save(&front_path).unwrap();
        let back =
            ::image::ImageBuffer::from_fn(100, 140, |_, _| ::image::Rgba([0u8, 0, 255, 255]));
        back.save(&back_path).unwrap();

        let params = LayoutParams {
            enable_duplex: true,
            default_back_path: Some(back_path),
            back_offset: (0.3, -0.2),
            ..create_test_params()
        };

        let grid = crate::layout::calculate_grid(&params);
        let mut card = Card::new(front_path);
        card.set_copy_count(3);
        let pages = crate::layout::distribute_cards(&[card], &grid, &params);

        // Fronts and backs interleave
        assert!(pages.len() >= 2);
        assert_eq!(pages[0].side, PageSide::Front);
        assert_eq!(pages[1].side, PageSide::Back);

        let output_path = temp_dir.path().join("duplex.pdf");
        let result = export_pages_to_pdf(&pages, &params, &output_path);
        assert!(result.is_ok(), "Export failed: {:?}", result.err());

        let bytes = std::fs::read(&output_path).unwrap();
        assert!(bytes.starts_with(b"%PDF"));

        // Short-edge flip on a portrait page mirrors vertically, which
        // triggers the 180° back-rotation path (image decode + rotate)
        let rotated_params = LayoutParams {
            flip_edge: FlipEdge::ShortEdge,
            ..params.clone()
        };
        assert!(rotated_params.backs_rotated_180());
        let pages = crate::layout::distribute_cards(
            &[Card::new(temp_dir.path().join("front.png"))],
            &grid,
            &rotated_params,
        );
        let rotated_path = temp_dir.path().join("duplex_rotated.pdf");
        let result = export_pages_to_pdf(&pages, &rotated_params, &rotated_path);
        assert!(result.is_ok(), "Export failed: {:?}", result.err());
        assert!(std::fs::read(&rotated_path).unwrap().starts_with(b"%PDF"));
    }

    #[test]
    fn test_export_with_progress_callback() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let temp_dir = TempDir::new().unwrap();
        let img_path = temp_dir.path().join("test_card.png");
        let img = ::image::ImageBuffer::from_fn(100, 140, |_, _| ::image::Rgba([255u8, 0, 0, 255]));
        img.save(&img_path).unwrap();

        let params = create_test_params();
        let pages = vec![
            PageLayout {
                page_number: 1,
                cards: vec![(Card::new(img_path.clone()), CardPosition { x: 5.0, y: 5.0 })],
                side: PageSide::Front,
            },
            PageLayout {
                page_number: 2,
                cards: vec![(Card::new(img_path), CardPosition { x: 5.0, y: 5.0 })],
                side: PageSide::Back,
            },
        ];

        let progress_count = Arc::new(AtomicUsize::new(0));
        let progress_count_clone = progress_count.clone();

        let output_path = temp_dir.path().join("progress_test.pdf");
        let result = export_pages_to_pdf_with_progress(
            &pages,
            &params,
            &output_path,
            move |completed, total| {
                assert_eq!(total, 2);
                progress_count_clone.fetch_add(1, Ordering::SeqCst);
                assert_eq!(progress_count_clone.load(Ordering::SeqCst), completed);
            },
        );

        assert!(result.is_ok(), "Export failed: {:?}", result.err());
        assert_eq!(progress_count.load(Ordering::SeqCst), 2);
    }
}
