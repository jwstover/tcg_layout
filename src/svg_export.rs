use crate::types::{LayoutParams, PageLayout, PageOrientation};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use svg::node::element::{Definitions, Group, Image, Rectangle};
use svg::Document;

pub struct SvgExporter {
    params: LayoutParams,
}

impl SvgExporter {
    pub fn new(params: LayoutParams) -> Self {
        Self { params }
    }

    /// Export a single page to SVG format
    pub fn export_page(&self, page: &PageLayout, output_path: &Path) -> Result<()> {
        let document = self.create_svg_document(page)?;
        svg::save(output_path, &document)
            .with_context(|| format!("Failed to save SVG to {}", output_path.display()))?;
        Ok(())
    }

    /// Export all pages to separate SVG files
    pub fn export_pages(&self, pages: &[PageLayout], output_dir: &Path) -> Result<Vec<PathBuf>> {
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
    pub fn export_pages_single_file(&self, pages: &[PageLayout], output_path: &Path) -> Result<()> {
        let document = self.create_multi_page_svg_document(pages)?;
        svg::save(output_path, &document)
            .with_context(|| format!("Failed to save SVG to {}", output_path.display()))?;
        Ok(())
    }

    fn create_svg_document(&self, page: &PageLayout) -> Result<Document> {
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

        // Add each card as an image reference
        for (card, position) in &page.cards {
            let card_element = self.create_card_element(card, position)?;
            main_group = main_group.add(card_element);
        }

        document = document.add(main_group);
        Ok(document)
    }

    fn create_multi_page_svg_document(&self, pages: &[PageLayout]) -> Result<Document> {
        if pages.is_empty() {
            return Ok(Document::new());
        }

        let (page_width, page_height) = self.get_page_dimensions();
        let total_height = page_height * pages.len() as f32;

        // Create SVG document with height for all pages
        let mut document = Document::new()
            .set("width", format!("{page_width}mm"))
            .set("height", format!("{total_height}mm"))
            .set("viewBox", format!("0 0 {page_width} {total_height}"))
            .set("xmlns", "http://www.w3.org/2000/svg")
            .set("xmlns:xlink", "http://www.w3.org/1999/xlink");

        // Add definitions section for reusable elements
        let defs = Definitions::new();
        document = document.add(defs);

        // Create a group for each page
        for (page_index, page) in pages.iter().enumerate() {
            let page_y_offset = page_index as f32 * page_height;

            let mut page_group = Group::new()
                .set("id", format!("page-{}", page.page_number))
                .set("transform", format!("translate(0, {page_y_offset})"));

            // Add background rectangle for this page
            let background = Rectangle::new()
                .set("x", 0)
                .set("y", 0)
                .set("width", page_width)
                .set("height", page_height)
                .set("fill", "white")
                .set("stroke", "#cccccc")
                .set("stroke-width", "0.5");

            page_group = page_group.add(background);

            // Add each card as an image reference
            for (card, position) in &page.cards {
                let card_element = self.create_card_element(card, position)?;
                page_group = page_group.add(card_element);
            }

            document = document.add(page_group);
        }

        Ok(document)
    }

    fn create_card_element(
        &self,
        card: &crate::types::Card,
        position: &crate::types::CardPosition,
    ) -> Result<Image> {
        // Convert the absolute path to a relative path or file URI
        let image_path = self.get_image_reference(&card.path)?;

        let image = Image::new()
            .set("x", position.x)
            .set("y", position.y)
            .set("width", self.params.card_size.0)
            .set("height", self.params.card_size.1)
            .set("href", image_path)
            .set("preserveAspectRatio", "xMidYMid meet");

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
    let exporter = SvgExporter::new(params.clone());
    exporter.export_page(page_layout, output_path)
}

/// Utility function to export multiple pages to a directory
pub fn export_pages_to_svg(
    pages: &[PageLayout],
    params: &LayoutParams,
    output_dir: &Path,
) -> Result<Vec<PathBuf>> {
    let exporter = SvgExporter::new(params.clone());
    exporter.export_pages(pages, output_dir)
}

/// Utility function to export multiple pages to a single SVG file
pub fn export_pages_to_single_svg(
    pages: &[PageLayout],
    params: &LayoutParams,
    output_path: &Path,
) -> Result<()> {
    let exporter = SvgExporter::new(params.clone());
    exporter.export_pages_single_file(pages, output_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        Card, CardPosition, FillOrder, LayoutParams, Margins, PageLayout, PageOrientation,
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
        }
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
        let exporter = SvgExporter::new(params);
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
            },
        ];

        let result = export_pages_to_single_svg(&pages, &params, &output_path);
        assert!(result.is_ok());
        assert!(output_path.exists());

        // Check that the file contains expected SVG content for multiple pages
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("<svg"));
        assert!(content.contains("width=\"100mm\""));
        assert!(content.contains("height=\"300mm\"")); // 2 pages * 150mm each
        assert!(content.contains("page-1"));
        assert!(content.contains("page-2"));
        assert!(content.contains("card1.jpg"));
        assert!(content.contains("card4.jpg"));
        assert!(content.contains("translate(0, 150)")); // Second page offset
    }

    #[test]
    fn test_create_multi_page_svg_document() {
        let params = create_test_params();
        let exporter = SvgExporter::new(params);
        let pages = vec![
            create_test_page(),
            PageLayout {
                page_number: 2,
                cards: vec![(
                    Card::new(PathBuf::from("card4.jpg")),
                    CardPosition { x: 10.0, y: 10.0 },
                )],
            },
        ];

        let result = exporter.create_multi_page_svg_document(&pages);
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_multi_page_svg_document_empty() {
        let params = create_test_params();
        let exporter = SvgExporter::new(params);
        let pages = vec![];

        let result = exporter.create_multi_page_svg_document(&pages);
        assert!(result.is_ok());
    }
}
