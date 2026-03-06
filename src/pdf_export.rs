use crate::layout::{calculate_cut_marks, calculate_grid};
use crate::types::{LayoutParams, PageLayout, PageOrientation};
use anyhow::{Context, Result};
use printpdf::*;
use printpdf::{ImageCompression, ImageOptimizationOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tcg_layout::bleed;

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

        // Image deduplication: embed each unique file once, track pixel dims for sizing
        let mut image_cache: HashMap<PathBuf, (XObjectId, f32, f32)> = HashMap::new();

        let mut pdf_pages = Vec::new();

        for (page_idx, page) in pages.iter().enumerate() {
            let mut ops = Vec::new();

            // Draw cut marks first (background)
            ops.extend(cut_mark_ops.clone());

            // Draw card images
            for (card, position) in &page.cards {
                let (image_bytes, card_w_mm, card_h_mm, img_x_mm, img_y_mm) =
                    if self.params.enable_bleed && self.params.bleed_mm > 0.0 {
                        self.prepare_bleed_image(card, position)?
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
                let (xobject_id, img_pixel_width, img_pixel_height) = if let Some((id, pw, ph)) =
                    image_cache.get(&card.path)
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
                    image_cache.insert(card.path.clone(), (id.clone(), pw, ph));
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

    fn prepare_bleed_image(
        &self,
        card: &crate::types::Card,
        position: &crate::types::CardPosition,
    ) -> Result<(Vec<u8>, f32, f32, f32, f32)> {
        let metadata = tcg_layout::image::load_image_metadata(&card.path)?;

        let (bleed_pixels_x, bleed_pixels_y) = bleed::calculate_bleed_pixels_from_dimensions(
            self.params.bleed_mm,
            self.params.card_size,
            metadata.dimensions,
        );

        let bleed_pixels = ((bleed_pixels_x + bleed_pixels_y) / 2).max(1);
        let bleed_img = bleed::apply_bleed(&card.path, bleed_pixels)?;

        // Encode to PNG in memory
        let mut png_bytes = std::io::Cursor::new(Vec::new());
        bleed_img
            .write_to(&mut png_bytes, ::image::ImageFormat::Png)
            .context("Failed to encode bleed image to PNG")?;

        let bleed_width = self.params.card_size.0 + 2.0 * self.params.bleed_mm;
        let bleed_height = self.params.card_size.1 + 2.0 * self.params.bleed_mm;
        let offset_x = position.x - self.params.bleed_mm;
        let offset_y = position.y - self.params.bleed_mm;

        Ok((
            png_bytes.into_inner(),
            bleed_width,
            bleed_height,
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
        Card, CardPosition, FillOrder, LayoutParams, Margins, PageLayout, PageOrientation,
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
            center_layout: false,
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
            },
            PageLayout {
                page_number: 2,
                cards: vec![(Card::new(img_path), CardPosition { x: 5.0, y: 5.0 })],
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
            },
            PageLayout {
                page_number: 2,
                cards: vec![(Card::new(img_path), CardPosition { x: 5.0, y: 5.0 })],
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
