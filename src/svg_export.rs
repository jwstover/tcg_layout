use crate::layout::calculate_cut_marks;
use crate::types::{LayoutParams, PageLayout, PageOrientation, PageSide};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use svg::node::element::{Definitions, Element, Group, Image, Line, Rectangle};
use svg::{Document, Node};
use tcg_layout::{bleed, color_adjust, sharpen};

pub struct SvgExporter {
    params: LayoutParams,
    processed_dir: Option<PathBuf>,
}

impl SvgExporter {
    pub fn new(params: LayoutParams) -> Self {
        Self {
            params,
            processed_dir: None,
        }
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
    /// adjustment is scoped to a single side. When true, back images get a
    /// distinct processed filename so they can't collide with fronts.
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

    /// Whether any page side needs image processing (used to decide whether
    /// the processed image directory must exist)
    fn needs_image_processing(&self) -> bool {
        self.needs_image_processing_for(false) || self.needs_image_processing_for(true)
    }

    fn setup_processed_directory(&mut self, svg_path: &Path) -> Result<PathBuf> {
        let svg_stem = svg_path
            .file_stem()
            .context("Invalid SVG path")?
            .to_string_lossy();

        let processed_dir = svg_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{svg_stem}_images"));

        std::fs::create_dir_all(&processed_dir).with_context(|| {
            format!(
                "Failed to create processed image directory: {}",
                processed_dir.display()
            )
        })?;

        self.processed_dir = Some(processed_dir.clone());
        Ok(processed_dir)
    }

    /// Export a single page to SVG format
    pub fn export_page(&mut self, page: &PageLayout, output_path: &Path) -> Result<()> {
        if self.needs_image_processing() {
            self.setup_processed_directory(output_path)?;
        }

        let document = self.create_svg_document(page)?;
        svg::save(output_path, &document)
            .with_context(|| format!("Failed to save SVG to {}", output_path.display()))?;
        Ok(())
    }

    /// Export all pages to separate SVG files
    pub fn export_pages(
        &mut self,
        pages: &[PageLayout],
        output_dir: &Path,
    ) -> Result<Vec<PathBuf>> {
        let mut output_paths = Vec::new();

        for page in pages {
            let filename = format!("page_{:03}.svg", page.page_number);
            let output_path = output_dir.join(filename);

            self.export_page(page, &output_path)?;
            output_paths.push(output_path);
        }

        Ok(output_paths)
    }

    /// Export all pages to a single SVG file with multiple pages
    pub fn export_pages_single_file(
        &mut self,
        pages: &[PageLayout],
        output_path: &Path,
    ) -> Result<()> {
        self.export_pages_single_file_with_progress(pages, output_path, |_, _| {})
    }

    /// Export all pages to a single SVG file with progress callback
    pub fn export_pages_single_file_with_progress<F: Fn(usize, usize)>(
        &mut self,
        pages: &[PageLayout],
        output_path: &Path,
        progress: F,
    ) -> Result<()> {
        if self.needs_image_processing() {
            self.setup_processed_directory(output_path)?;
        }

        let document = self.create_multi_page_svg_document(pages, &progress)?;
        svg::save(output_path, &document)
            .with_context(|| format!("Failed to save SVG to {}", output_path.display()))?;
        Ok(())
    }

    fn create_svg_document(&mut self, page: &PageLayout) -> Result<Document> {
        let (page_width, page_height) = self.get_page_dimensions();

        // Create SVG document with proper dimensions in mm
        let mut document = Document::new()
            .set("width", format!("{page_width}mm"))
            .set("height", format!("{page_height}mm"))
            .set("viewBox", format!("0 0 {page_width} {page_height}"))
            .set("xmlns", "http://www.w3.org/2000/svg")
            .set("xmlns:xlink", "http://www.w3.org/1999/xlink");

        // Add definitions section for reusable elements
        let defs = Definitions::new();
        document = document.add(defs);

        // Create main group for the page content
        let mut main_group = Group::new().set("id", format!("page-{}", page.page_number));

        // Add background rectangle (optional, useful for print preview)
        let background = Rectangle::new()
            .set("x", 0)
            .set("y", 0)
            .set("width", page_width)
            .set("height", page_height)
            .set("fill", "white")
            .set("stroke", "none");

        main_group = main_group.add(background);

        // Add cut marks (render first so they appear in background).
        // Back pages get none: cards are cut from the front.
        if page.side == PageSide::Front {
            let cut_marks =
                calculate_cut_marks(&self.params, &crate::layout::calculate_grid(&self.params));
            for cut_mark in &cut_marks {
                let cut_line = Line::new()
                    .set("x1", cut_mark.x1)
                    .set("y1", cut_mark.y1)
                    .set("x2", cut_mark.x2)
                    .set("y2", cut_mark.y2)
                    .set("stroke", "#808080") // Gray color
                    .set("stroke-width", "0.5") // Thin line
                    .set("stroke-linecap", "round");

                main_group = main_group.add(cut_line);
            }
        }

        // Back pages render rotated 180° when the duplex flip is about a
        // horizontal axis, so cut cards come out head-to-head.
        let is_back = page.side == PageSide::Back;
        let rotate_180 = is_back && self.params.backs_rotated_180();

        // Add each card as an image reference (render second so they appear on top)
        for (card, position) in &page.cards {
            let card_element = self.create_card_element(card, position, rotate_180, is_back)?;
            main_group = main_group.add(card_element);
        }

        document = document.add(main_group);
        Ok(document)
    }

    fn create_multi_page_svg_document<F: Fn(usize, usize)>(
        &mut self,
        pages: &[PageLayout],
        progress: &F,
    ) -> Result<Document> {
        if pages.is_empty() {
            return Ok(Document::new());
        }

        let (page_width, page_height) = self.get_page_dimensions();

        // Create SVG document with first page dimensions
        let mut document = Document::new()
            .set("width", format!("{page_width}mm"))
            .set("height", format!("{page_height}mm"))
            .set("viewBox", format!("0 0 {page_width} {page_height}"))
            .set("xmlns", "http://www.w3.org/2000/svg")
            .set("xmlns:xlink", "http://www.w3.org/1999/xlink")
            .set(
                "xmlns:sodipodi",
                "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd",
            )
            .set(
                "xmlns:inkscape",
                "http://www.inkscape.org/namespaces/inkscape",
            );

        // Add definitions section for reusable elements
        let defs = Definitions::new();
        document = document.add(defs);

        // Create Inkscape namedview element containing page definitions
        let mut namedview = Element::new("sodipodi:namedview");
        namedview.assign("id", "base");
        namedview.assign("pagecolor", "#ffffff");
        namedview.assign("bordercolor", "#666666");
        namedview.assign("borderopacity", "1.0");
        namedview.assign("inkscape:pageopacity", "0.0");
        namedview.assign("inkscape:pageshadow", "2");
        namedview.assign("inkscape:document-units", "mm");

        // Create individual page elements within namedview
        for (page_index, _page) in pages.iter().enumerate() {
            let page_x = page_index as f32 * page_width; // Position pages without extra spacing
            let mut page_element = Element::new("inkscape:page");
            page_element.assign("x", format!("{page_x}"));
            page_element.assign("y", "0");
            page_element.assign("width", format!("{page_width}"));
            page_element.assign("height", format!("{page_height}"));
            page_element.assign("id", format!("page{}", page_index + 1));

            namedview.append(page_element);
        }

        document = document.add(namedview);

        // Create content groups for each page
        for (page_index, page) in pages.iter().enumerate() {
            let page_x = page_index as f32 * page_width; // Match page positioning

            // Create content group for this page
            let mut page_group = Group::new()
                .set("id", format!("page-{}-content", page.page_number))
                .set("inkscape:groupmode", "layer")
                .set(
                    "inkscape:label",
                    format!("Page {} Content", page.page_number),
                )
                .set("transform", format!("translate({page_x},0)"));

            // Add background rectangle for this page
            let background = Rectangle::new()
                .set("x", 0)
                .set("y", 1)
                .set("width", page_width)
                .set("height", page_height)
                .set("fill", "white")
                .set("stroke", "none");

            page_group = page_group.add(background);

            // Add cut marks for this page (render first so they appear in
            // background). Back pages get none: cards are cut from the front.
            if page.side == PageSide::Front {
                let cut_marks =
                    calculate_cut_marks(&self.params, &crate::layout::calculate_grid(&self.params));
                for cut_mark in &cut_marks {
                    let cut_line = Line::new()
                        .set("x1", cut_mark.x1)
                        .set("y1", cut_mark.y1)
                        .set("x2", cut_mark.x2)
                        .set("y2", cut_mark.y2)
                        .set("stroke", "#808080") // Gray color
                        .set("stroke-width", "0.5") // Thin line
                        .set("stroke-linecap", "round");

                    page_group = page_group.add(cut_line);
                }
            }

            // Back pages render rotated 180° when the duplex flip is about a
            // horizontal axis, so cut cards come out head-to-head.
            let is_back = page.side == PageSide::Back;
            let rotate_180 = is_back && self.params.backs_rotated_180();

            // Add each card as an image reference (render second so they appear on top)
            for (card, position) in &page.cards {
                let card_element = self.create_card_element(card, position, rotate_180, is_back)?;
                page_group = page_group.add(card_element);
            }

            document = document.add(page_group);
            progress(page_index + 1, pages.len());
        }

        Ok(document)
    }

    fn create_card_element(
        &mut self,
        card: &crate::types::Card,
        position: &crate::types::CardPosition,
        rotate_180: bool,
        is_back: bool,
    ) -> Result<Image> {
        let (image_href, card_width, card_height, img_x, img_y) =
            if self.needs_image_processing_for(is_back) {
                let processed_dir = self
                    .processed_dir
                    .as_ref()
                    .context("Processed image directory not set up")?;

                // Load and process the image: color adjustments first (on the
                // original colors), then sharpening, so bleed strips derive
                // from the fully processed image
                let mut img = image::open(&card.path)
                    .with_context(|| format!("Failed to open image: {}", card.path.display()))?;

                if self.color_adjust_active_for(is_back) {
                    let adjustments =
                        color_adjust::adjustments_for_side(&self.params.hsl_adjustments, is_back);
                    img = color_adjust::apply_hsl_adjustments_to_image(&img, &adjustments);
                }

                if self.sharpen_active() {
                    img = sharpen::apply_sharpen(&img, &self.params.sharpen_params());
                }

                let (card_w, card_h, offset_x, offset_y) = if self.bleed_active() {
                    // Calculate bleed based on actual image dimensions
                    let (bleed_pixels_x, bleed_pixels_y) =
                        bleed::calculate_bleed_pixels_from_dimensions(
                            self.params.bleed_mm,
                            self.params.card_size,
                            (img.width(), img.height()),
                        );

                    // Use average for symmetric bleed application
                    let bleed_pixels = ((bleed_pixels_x + bleed_pixels_y) / 2).max(1);
                    img = image::DynamicImage::ImageRgba8(bleed::apply_bleed_to_image(
                        &img,
                        bleed_pixels,
                    ));

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

                // Save to the processed image directory. When adjustments are
                // side-scoped, an image used as both a front and a back is
                // processed differently per side, so backs get a distinct name.
                let stem = card
                    .path
                    .file_stem()
                    .context("Invalid card path")?
                    .to_string_lossy();
                let filename = if is_back && self.side_scoped_color_adjust() {
                    format!("{stem}_back_processed.png")
                } else {
                    format!("{stem}_processed.png")
                };

                let processed_path = processed_dir.join(&filename);
                img.save(&processed_path).with_context(|| {
                    format!(
                        "Failed to save processed image: {}",
                        processed_path.display()
                    )
                })?;

                // Create relative reference
                let relative_path = format!(
                    "{}/{}",
                    processed_dir.file_name().unwrap().to_string_lossy(),
                    filename
                );

                (relative_path, card_w, card_h, offset_x, offset_y)
            } else {
                // No processing - use original image
                let image_ref = self.get_image_reference(&card.path)?;
                (
                    image_ref,
                    self.params.card_size.0,
                    self.params.card_size.1,
                    position.x,
                    position.y,
                )
            };

        let mut image = Image::new()
            .set("x", img_x)
            .set("y", img_y)
            .set("width", card_width)
            .set("height", card_height)
            .set("href", image_href)
            .set("preserveAspectRatio", "none"); // Changed to ensure bleed fills exactly

        if rotate_180 {
            // Rotate about the image center; the placement rectangle is unchanged
            let center_x = img_x + card_width / 2.0;
            let center_y = img_y + card_height / 2.0;
            image = image.set("transform", format!("rotate(180 {center_x} {center_y})"));
        }

        Ok(image)
    }

    fn get_image_reference(&self, image_path: &Path) -> Result<String> {
        // For desktop usage, use relative paths or file:// URIs
        // This preserves the original files rather than embedding them

        if image_path.is_absolute() {
            // Convert to file:// URI for absolute paths
            let uri = format!("file://{}", image_path.display());
            Ok(uri)
        } else {
            // Use relative path as-is
            Ok(image_path.to_string_lossy().into_owned())
        }
    }

    fn get_page_dimensions(&self) -> (f32, f32) {
        match self.params.page_orientation {
            PageOrientation::Portrait => self.params.page_size,
            PageOrientation::Landscape => (self.params.page_size.1, self.params.page_size.0),
        }
    }
}

/// Utility function to export a single page with default settings
pub fn export_page_to_svg(
    page_layout: &PageLayout,
    params: &LayoutParams,
    output_path: &Path,
) -> Result<()> {
    let mut exporter = SvgExporter::new(params.clone());
    exporter.export_page(page_layout, output_path)
}

/// Utility function to export multiple pages to a directory
pub fn export_pages_to_svg(
    pages: &[PageLayout],
    params: &LayoutParams,
    output_dir: &Path,
) -> Result<Vec<PathBuf>> {
    let mut exporter = SvgExporter::new(params.clone());
    exporter.export_pages(pages, output_dir)
}

/// Utility function to export multiple pages to a single SVG file
pub fn export_pages_to_single_svg(
    pages: &[PageLayout],
    params: &LayoutParams,
    output_path: &Path,
) -> Result<()> {
    export_pages_to_single_svg_with_progress(pages, params, output_path, |_, _| {})
}

/// Utility function to export multiple pages to a single SVG file with progress callback
pub fn export_pages_to_single_svg_with_progress<F: Fn(usize, usize)>(
    pages: &[PageLayout],
    params: &LayoutParams,
    output_path: &Path,
    progress: F,
) -> Result<()> {
    let mut exporter = SvgExporter::new(params.clone());
    exporter.export_pages_single_file_with_progress(pages, output_path, progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        Card, CardPosition, FillOrder, FlipEdge, LayoutParams, Margins, PageLayout, PageOrientation,
    };
    use std::path::PathBuf;
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

    fn create_test_page() -> PageLayout {
        let cards = vec![
            (
                Card::new(PathBuf::from("card1.jpg")),
                CardPosition { x: 5.0, y: 5.0 },
            ),
            (
                Card::new(PathBuf::from("card2.jpg")),
                CardPosition { x: 27.0, y: 5.0 },
            ),
            (
                Card::new(PathBuf::from("card3.jpg")),
                CardPosition { x: 5.0, y: 37.0 },
            ),
        ];

        PageLayout {
            page_number: 1,
            cards,
            side: PageSide::Front,
        }
    }

    fn create_real_test_image(dir: &Path, name: &str) -> PathBuf {
        let img_path = dir.join(name);
        let img = image::ImageBuffer::from_fn(100, 140, |x, _| {
            if x < 50 {
                image::Rgba([50u8, 50, 50, 255])
            } else {
                image::Rgba([200u8, 200, 200, 255])
            }
        });
        img.save(&img_path).unwrap();
        img_path
    }

    #[test]
    fn test_svg_exporter_creation() {
        let params = create_test_params();
        let exporter = SvgExporter::new(params.clone());
        assert_eq!(exporter.params, params);
    }

    #[test]
    fn test_get_page_dimensions_portrait() {
        let params = LayoutParams {
            page_size: (210.0, 297.0),
            page_orientation: PageOrientation::Portrait,
            ..create_test_params()
        };
        let exporter = SvgExporter::new(params);
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
        let exporter = SvgExporter::new(params);
        let (width, height) = exporter.get_page_dimensions();
        assert_eq!(width, 297.0);
        assert_eq!(height, 210.0);
    }

    #[test]
    fn test_get_image_reference_absolute_path() {
        let params = create_test_params();
        let exporter = SvgExporter::new(params);
        let absolute_path = PathBuf::from("/home/user/cards/card.jpg");
        let reference = exporter.get_image_reference(&absolute_path).unwrap();
        assert_eq!(reference, "file:///home/user/cards/card.jpg");
    }

    #[test]
    fn test_get_image_reference_relative_path() {
        let params = create_test_params();
        let exporter = SvgExporter::new(params);
        let relative_path = PathBuf::from("cards/card.jpg");
        let reference = exporter.get_image_reference(&relative_path).unwrap();
        assert_eq!(reference, "cards/card.jpg");
    }

    #[test]
    fn test_export_page_to_svg() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("test_page.svg");
        let params = create_test_params();
        let page = create_test_page();

        let result = export_page_to_svg(&page, &params, &output_path);
        assert!(result.is_ok());
        assert!(output_path.exists());

        // Check that the file contains expected SVG content
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("<svg"));
        assert!(content.contains("width=\"100mm\""));
        assert!(content.contains("height=\"150mm\""));
        assert!(content.contains("card1.jpg"));
        assert!(content.contains("card2.jpg"));
        assert!(content.contains("card3.jpg"));
    }

    #[test]
    fn test_export_pages_to_svg() {
        let temp_dir = TempDir::new().unwrap();
        let params = create_test_params();
        let pages = vec![
            create_test_page(),
            PageLayout {
                page_number: 2,
                cards: vec![(
                    Card::new(PathBuf::from("card4.jpg")),
                    CardPosition { x: 5.0, y: 5.0 },
                )],
                side: PageSide::Front,
            },
        ];

        let result = export_pages_to_svg(&pages, &params, temp_dir.path());
        assert!(result.is_ok());

        let output_paths = result.unwrap();
        assert_eq!(output_paths.len(), 2);

        for path in output_paths {
            assert!(path.exists());
            let content = std::fs::read_to_string(&path).unwrap();
            assert!(content.contains("<svg"));
        }
    }

    #[test]
    fn test_create_svg_document() {
        let params = create_test_params();
        let mut exporter = SvgExporter::new(params);
        let page = create_test_page();

        let result = exporter.create_svg_document(&page);
        assert!(result.is_ok());
    }

    #[test]
    fn test_export_pages_single_file() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("multi_page.svg");
        let params = create_test_params();
        let pages = vec![
            create_test_page(),
            PageLayout {
                page_number: 2,
                cards: vec![(
                    Card::new(PathBuf::from("card4.jpg")),
                    CardPosition { x: 5.0, y: 5.0 },
                )],
                side: PageSide::Front,
            },
        ];

        let result = export_pages_to_single_svg(&pages, &params, &output_path);
        assert!(result.is_ok());
        assert!(output_path.exists());

        // Check that the file contains expected SVG content for multiple pages
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("<svg"));
        assert!(content.contains("sodipodi:namedview"));
        assert!(content.contains("inkscape:page"));
        assert!(content.contains("page1"));
        assert!(content.contains("page2"));
        assert!(content.contains("card1.jpg"));
        assert!(content.contains("card4.jpg"));
        assert!(content.contains("inkscape:groupmode=\"layer\""));
        assert!(content.contains("Page 1 Content"));
        assert!(content.contains("Page 2 Content"));
    }

    #[test]
    fn test_create_multi_page_svg_document() {
        let params = create_test_params();
        let mut exporter = SvgExporter::new(params);
        let pages = vec![
            create_test_page(),
            PageLayout {
                page_number: 2,
                cards: vec![(
                    Card::new(PathBuf::from("card4.jpg")),
                    CardPosition { x: 10.0, y: 10.0 },
                )],
                side: PageSide::Front,
            },
        ];

        let result = exporter.create_multi_page_svg_document(&pages, &|_, _| {});
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_multi_page_svg_document_empty() {
        let params = create_test_params();
        let mut exporter = SvgExporter::new(params);
        let pages = vec![];

        let result = exporter.create_multi_page_svg_document(&pages, &|_, _| {});
        assert!(result.is_ok());
    }

    #[test]
    fn test_export_with_sharpening_writes_processed_images() {
        let temp_dir = TempDir::new().unwrap();
        let img_path = create_real_test_image(temp_dir.path(), "card_sharp.png");

        let params = LayoutParams {
            sharpen_amount: 1.5,
            sharpen_radius: 0.7,
            sharpen_threshold: 0.02,
            enable_sharpen: true,
            ..create_test_params()
        };

        let page = PageLayout {
            page_number: 1,
            cards: vec![(Card::new(img_path), CardPosition { x: 5.0, y: 5.0 })],
            side: PageSide::Front,
        };

        let output_path = temp_dir.path().join("sharpened.svg");
        let result = export_page_to_svg(&page, &params, &output_path);
        assert!(result.is_ok(), "Export failed: {:?}", result.err());
        assert!(output_path.exists());

        // Sharpening (without bleed) should write processed images to the
        // sidecar directory and reference them in the SVG
        let processed_path = temp_dir
            .path()
            .join("sharpened_images")
            .join("card_sharp_processed.png");
        assert!(
            processed_path.exists(),
            "Processed image should exist at {}",
            processed_path.display()
        );

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("sharpened_images/card_sharp_processed.png"));
    }

    #[test]
    fn test_export_with_sharpening_and_bleed() {
        let temp_dir = TempDir::new().unwrap();
        let img_path = create_real_test_image(temp_dir.path(), "card_both.png");

        let params = LayoutParams {
            bleed_mm: 2.0,
            enable_bleed: true,
            sharpen_amount: 1.0,
            sharpen_radius: 0.7,
            sharpen_threshold: 0.02,
            enable_sharpen: true,
            ..create_test_params()
        };

        let page = PageLayout {
            page_number: 1,
            cards: vec![(Card::new(img_path), CardPosition { x: 5.0, y: 5.0 })],
            side: PageSide::Front,
        };

        let output_path = temp_dir.path().join("both.svg");
        let result = export_page_to_svg(&page, &params, &output_path);
        assert!(result.is_ok(), "Export failed: {:?}", result.err());

        // Processed image should be larger than the original due to bleed
        let processed_path = temp_dir
            .path()
            .join("both_images")
            .join("card_both_processed.png");
        assert!(processed_path.exists());
        let processed = image::open(&processed_path).unwrap();
        assert!(processed.width() > 100);
        assert!(processed.height() > 140);
    }

    #[test]
    fn test_export_with_color_adjustments_writes_processed_images() {
        let temp_dir = TempDir::new().unwrap();
        let img_path = create_real_test_image(temp_dir.path(), "card_color.png");

        let params = LayoutParams {
            enable_color_adjust: true,
            hsl_adjustments: vec![color_adjust::HslAdjustment {
                target_hue: 55.0,
                hue_range: 20.0,
                feather: 10.0,
                hue_shift: 30.0,
                saturation_shift: 0.0,
                lightness_shift: 0.0,
                ..Default::default()
            }],
            ..create_test_params()
        };

        let page = PageLayout {
            page_number: 1,
            cards: vec![(Card::new(img_path), CardPosition { x: 5.0, y: 5.0 })],
            side: PageSide::Front,
        };

        let output_path = temp_dir.path().join("color_adjusted.svg");
        let result = export_page_to_svg(&page, &params, &output_path);
        assert!(result.is_ok(), "Export failed: {:?}", result.err());

        let processed_path = temp_dir
            .path()
            .join("color_adjusted_images")
            .join("card_color_processed.png");
        assert!(
            processed_path.exists(),
            "Processed image should exist at {}",
            processed_path.display()
        );

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("color_adjusted_images/card_color_processed.png"));
    }

    #[test]
    fn test_backs_only_adjustment_processes_backs_not_fronts() {
        let temp_dir = TempDir::new().unwrap();
        let front_path = create_real_test_image(temp_dir.path(), "front.png");
        let back_path = create_real_test_image(temp_dir.path(), "back.png");

        let params = LayoutParams {
            enable_color_adjust: true,
            hsl_adjustments: vec![color_adjust::HslAdjustment {
                target_hue: 55.0,
                hue_range: 20.0,
                feather: 10.0,
                hue_shift: 30.0,
                saturation_shift: 0.0,
                lightness_shift: 0.0,
                scope: color_adjust::AdjustmentScope::BacksOnly,
                ..Default::default()
            }],
            ..create_test_params()
        };

        let pages = vec![
            PageLayout {
                page_number: 1,
                cards: vec![(Card::new(front_path), CardPosition { x: 5.0, y: 5.0 })],
                side: PageSide::Front,
            },
            PageLayout {
                page_number: 2,
                cards: vec![(Card::new(back_path), CardPosition { x: 5.0, y: 5.0 })],
                side: PageSide::Back,
            },
        ];

        let output_path = temp_dir.path().join("scoped.svg");
        let result = export_pages_to_single_svg(&pages, &params, &output_path);
        assert!(result.is_ok(), "Export failed: {:?}", result.err());

        let content = std::fs::read_to_string(&output_path).unwrap();
        // The front is untouched and referenced in place; the back is
        // processed into the images directory with the back-specific name
        assert!(
            content.contains("file://") && content.contains("front.png"),
            "front should reference the original file"
        );
        assert!(content.contains("scoped_images/back_back_processed.png"));
        assert!(temp_dir
            .path()
            .join("scoped_images")
            .join("back_back_processed.png")
            .exists());
        assert!(!temp_dir
            .path()
            .join("scoped_images")
            .join("front_processed.png")
            .exists());
    }

    #[test]
    fn test_noop_color_adjustments_do_not_trigger_processing() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("noop.svg");
        let params = LayoutParams {
            enable_color_adjust: true,
            // All shifts are zero, so no processing should happen
            hsl_adjustments: vec![color_adjust::HslAdjustment::default()],
            ..create_test_params()
        };
        let page = create_test_page();

        let result = export_page_to_svg(&page, &params, &output_path);
        assert!(result.is_ok());
        assert!(!temp_dir.path().join("noop_images").exists());
    }

    #[test]
    fn test_export_without_processing_references_originals() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("plain.svg");
        let params = create_test_params();
        let page = create_test_page();

        let result = export_page_to_svg(&page, &params, &output_path);
        assert!(result.is_ok());

        // No processing enabled: no sidecar directory should be created
        assert!(!temp_dir.path().join("plain_images").exists());
    }

    #[test]
    fn test_back_page_has_no_cut_marks() {
        let temp_dir = TempDir::new().unwrap();
        let params = create_test_params();

        let back_page = PageLayout {
            page_number: 2,
            cards: vec![(
                Card::new(PathBuf::from("back.png")),
                CardPosition { x: 5.0, y: 5.0 },
            )],
            side: PageSide::Back,
        };
        let back_path = temp_dir.path().join("back_page.svg");
        export_page_to_svg(&back_page, &params, &back_path).unwrap();
        let back_content = std::fs::read_to_string(&back_path).unwrap();
        assert!(
            !back_content.contains("<line"),
            "back pages should not contain cut mark lines"
        );

        // A front page with the same params does get cut marks
        let front_path = temp_dir.path().join("front_page.svg");
        export_page_to_svg(&create_test_page(), &params, &front_path).unwrap();
        let front_content = std::fs::read_to_string(&front_path).unwrap();
        assert!(front_content.contains("<line"));
    }

    #[test]
    fn test_back_page_rotation_matches_mirror_axis() {
        let temp_dir = TempDir::new().unwrap();
        let back_page = PageLayout {
            page_number: 2,
            cards: vec![(
                Card::new(PathBuf::from("back.png")),
                CardPosition { x: 5.0, y: 5.0 },
            )],
            side: PageSide::Back,
        };

        // Portrait + short edge: vertical mirror -> backs rotated 180°
        let params = LayoutParams {
            flip_edge: FlipEdge::ShortEdge,
            ..create_test_params()
        };
        let rotated_path = temp_dir.path().join("rotated.svg");
        export_page_to_svg(&back_page, &params, &rotated_path).unwrap();
        let content = std::fs::read_to_string(&rotated_path).unwrap();
        assert!(content.contains("rotate(180"));

        // Portrait + long edge: horizontal mirror -> no rotation
        let unrotated_path = temp_dir.path().join("unrotated.svg");
        export_page_to_svg(&back_page, &create_test_params(), &unrotated_path).unwrap();
        let content = std::fs::read_to_string(&unrotated_path).unwrap();
        assert!(!content.contains("rotate(180"));

        // Front pages never rotate, regardless of flip edge
        let params = LayoutParams {
            flip_edge: FlipEdge::ShortEdge,
            ..create_test_params()
        };
        let front_path = temp_dir.path().join("front.svg");
        export_page_to_svg(&create_test_page(), &params, &front_path).unwrap();
        let content = std::fs::read_to_string(&front_path).unwrap();
        assert!(!content.contains("rotate(180"));
    }

    #[test]
    fn test_export_with_progress_callback() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("progress_test.svg");
        let params = create_test_params();
        let pages = vec![
            create_test_page(),
            PageLayout {
                page_number: 2,
                cards: vec![(
                    Card::new(PathBuf::from("card4.jpg")),
                    CardPosition { x: 5.0, y: 5.0 },
                )],
                side: PageSide::Front,
            },
        ];

        let progress_count = Arc::new(AtomicUsize::new(0));
        let progress_count_clone = progress_count.clone();

        let result = export_pages_to_single_svg_with_progress(
            &pages,
            &params,
            &output_path,
            move |completed, total| {
                assert_eq!(total, 2);
                progress_count_clone.fetch_add(1, Ordering::SeqCst);
                assert_eq!(progress_count_clone.load(Ordering::SeqCst), completed);
            },
        );

        assert!(result.is_ok());
        assert_eq!(progress_count.load(Ordering::SeqCst), 2);
    }
}
