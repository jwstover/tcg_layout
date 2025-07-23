use std::path::PathBuf;
use std::collections::VecDeque;
use image::ImageBuffer;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillOrder {
    RowMajor,
    ColumnMajor,
}

impl Default for FillOrder {
    fn default() -> Self {
        FillOrder::RowMajor
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margins {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Default for Margins {
    fn default() -> Self {
        Self {
            top: 10.0,
            right: 10.0,
            bottom: 10.0,
            left: 10.0,
        }
    }
}

impl Margins {
    pub fn uniform(margin: f32) -> Self {
        Self {
            top: margin,
            right: margin,
            bottom: margin,
            left: margin,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutParams {
    pub page_size: (f32, f32),      // (width, height) in mm
    pub card_size: (f32, f32),      // (width, height) in mm
    pub margins: Margins,           // margins in mm
    pub spacing: (f32, f32),        // (horizontal, vertical) spacing in mm
    pub orientation: FillOrder,     // RowMajor vs ColumnMajor
    pub target_dpi: u32,
}

impl Default for LayoutParams {
    fn default() -> Self {
        Self {
            page_size: (210.0, 297.0),     // A4 size in mm
            card_size: (63.0, 88.0),       // Standard poker card size in mm
            margins: Margins::uniform(10.0),
            spacing: (2.0, 2.0),
            orientation: FillOrder::RowMajor,
            target_dpi: 300,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Card {
    pub path: PathBuf,
    pub thumbnail: Option<ImageBuffer<image::Rgba<u8>, Vec<u8>>>,
    pub original_dpi: Option<u32>,
    pub needs_scaling: bool,
}

impl Card {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            thumbnail: None,
            original_dpi: None,
            needs_scaling: false,
        }
    }
}

type ThumbnailCache = lru::LruCache<PathBuf, ImageBuffer<image::Rgba<u8>, Vec<u8>>>;

#[derive(Debug)]
pub struct ImageCache {
    pub thumbnails: ThumbnailCache,
    pub loading_queue: VecDeque<PathBuf>,
    pub background_loader: Option<JoinHandle<()>>,
}

impl ImageCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            thumbnails: ThumbnailCache::new(std::num::NonZeroUsize::new(capacity).unwrap()),
            loading_queue: VecDeque::new(),
            background_loader: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GridLayout {
    pub rows: usize,
    pub cols: usize,
    pub cards_per_page: usize,
    pub total_pages: usize,
}

#[derive(Debug, Clone)]
pub struct CardPosition {
    pub x: f32,     // x position in mm
    pub y: f32,     // y position in mm
}

#[derive(Debug, Clone)]
pub struct PageLayout {
    pub page_number: usize,
    pub cards: Vec<(Card, CardPosition)>,
}

#[derive(Debug, Clone)]
pub struct DpiWarning {
    pub card_path: PathBuf,
    pub original_dpi: u32,
    pub target_dpi: u32,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fill_order_default() {
        assert_eq!(FillOrder::default(), FillOrder::RowMajor);
    }

    #[test]
    fn test_fill_order_variants() {
        let row_major = FillOrder::RowMajor;
        let column_major = FillOrder::ColumnMajor;
        
        assert_ne!(row_major, column_major);
        assert_eq!(row_major, FillOrder::RowMajor);
        assert_eq!(column_major, FillOrder::ColumnMajor);
    }

    #[test]
    fn test_margins_default() {
        let margins = Margins::default();
        assert_eq!(margins.top, 10.0);
        assert_eq!(margins.right, 10.0);
        assert_eq!(margins.bottom, 10.0);
        assert_eq!(margins.left, 10.0);
    }

    #[test]
    fn test_margins_uniform() {
        let margins = Margins::uniform(15.0);
        assert_eq!(margins.top, 15.0);
        assert_eq!(margins.right, 15.0);
        assert_eq!(margins.bottom, 15.0);
        assert_eq!(margins.left, 15.0);
    }

    #[test]
    fn test_margins_custom() {
        let margins = Margins {
            top: 5.0,
            right: 10.0,
            bottom: 15.0,
            left: 20.0,
        };
        
        assert_eq!(margins.top, 5.0);
        assert_eq!(margins.right, 10.0);
        assert_eq!(margins.bottom, 15.0);
        assert_eq!(margins.left, 20.0);
    }

    #[test]
    fn test_layout_params_default() {
        let params = LayoutParams::default();
        
        assert_eq!(params.page_size, (210.0, 297.0)); // A4
        assert_eq!(params.card_size, (63.0, 88.0)); // Poker card
        assert_eq!(params.margins, Margins::uniform(10.0));
        assert_eq!(params.spacing, (2.0, 2.0));
        assert_eq!(params.orientation, FillOrder::RowMajor);
        assert_eq!(params.target_dpi, 300);
    }

    #[test]
    fn test_layout_params_custom() {
        let custom_margins = Margins::uniform(5.0);
        let params = LayoutParams {
            page_size: (100.0, 150.0),
            card_size: (50.0, 70.0),
            margins: custom_margins,
            spacing: (1.0, 1.5),
            orientation: FillOrder::ColumnMajor,
            target_dpi: 600,
        };
        
        assert_eq!(params.page_size, (100.0, 150.0));
        assert_eq!(params.card_size, (50.0, 70.0));
        assert_eq!(params.margins, custom_margins);
        assert_eq!(params.spacing, (1.0, 1.5));
        assert_eq!(params.orientation, FillOrder::ColumnMajor);
        assert_eq!(params.target_dpi, 600);
    }

    #[test]
    fn test_card_new() {
        let path = PathBuf::from("/test/path/card.jpg");
        let card = Card::new(path.clone());
        
        assert_eq!(card.path, path);
        assert!(card.thumbnail.is_none());
        assert!(card.original_dpi.is_none());
        assert!(!card.needs_scaling);
    }

    #[test]
    fn test_card_with_properties() {
        let path = PathBuf::from("/test/path/card.png");
        let mut card = Card::new(path.clone());
        
        card.original_dpi = Some(150);
        card.needs_scaling = true;
        
        assert_eq!(card.path, path);
        assert_eq!(card.original_dpi, Some(150));
        assert!(card.needs_scaling);
    }

    #[test]
    fn test_image_cache_new() {
        let cache = ImageCache::new(50);
        
        assert_eq!(cache.thumbnails.cap().get(), 50);
        assert!(cache.loading_queue.is_empty());
        assert!(cache.background_loader.is_none());
    }

    #[test]
    fn test_grid_layout() {
        let layout = GridLayout {
            rows: 3,
            cols: 4,
            cards_per_page: 12,
            total_pages: 2,
        };
        
        assert_eq!(layout.rows, 3);
        assert_eq!(layout.cols, 4);
        assert_eq!(layout.cards_per_page, 12);
        assert_eq!(layout.total_pages, 2);
    }

    #[test]
    fn test_card_position() {
        let position = CardPosition { x: 25.5, y: 40.2 };
        
        assert_eq!(position.x, 25.5);
        assert_eq!(position.y, 40.2);
    }

    #[test]
    fn test_page_layout() {
        let card = Card::new(PathBuf::from("test.jpg"));
        let position = CardPosition { x: 10.0, y: 20.0 };
        
        let page_layout = PageLayout {
            page_number: 1,
            cards: vec![(card.clone(), position)],
        };
        
        assert_eq!(page_layout.page_number, 1);
        assert_eq!(page_layout.cards.len(), 1);
        assert_eq!(page_layout.cards[0].0.path, card.path);
        assert_eq!(page_layout.cards[0].1.x, 10.0);
        assert_eq!(page_layout.cards[0].1.y, 20.0);
    }

    #[test]
    fn test_dpi_warning() {
        let warning = DpiWarning {
            card_path: PathBuf::from("low_res.jpg"),
            original_dpi: 72,
            target_dpi: 300,
            message: "Low resolution detected".to_string(),
        };
        
        assert_eq!(warning.card_path, PathBuf::from("low_res.jpg"));
        assert_eq!(warning.original_dpi, 72);
        assert_eq!(warning.target_dpi, 300);
        assert_eq!(warning.message, "Low resolution detected");
    }

    #[test]
    fn test_margins_equality() {
        let margins1 = Margins::uniform(10.0);
        let margins2 = Margins {
            top: 10.0,
            right: 10.0,
            bottom: 10.0,
            left: 10.0,
        };
        let margins3 = Margins::uniform(15.0);
        
        assert_eq!(margins1, margins2);
        assert_ne!(margins1, margins3);
    }

    #[test]
    fn test_layout_params_equality() {
        let params1 = LayoutParams::default();
        let params2 = LayoutParams::default();
        let mut params3 = LayoutParams::default();
        params3.target_dpi = 600;
        
        assert_eq!(params1, params2);
        assert_ne!(params1, params3);
    }

    #[test]
    fn test_card_clone() {
        let original = Card::new(PathBuf::from("original.jpg"));
        let cloned = original.clone();
        
        assert_eq!(original.path, cloned.path);
        assert_eq!(original.thumbnail.is_none(), cloned.thumbnail.is_none());
        assert_eq!(original.original_dpi, cloned.original_dpi);
        assert_eq!(original.needs_scaling, cloned.needs_scaling);
    }
}