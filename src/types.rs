use std::path::PathBuf;
use std::collections::VecDeque;
use image::ImageBuffer;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillOrder {
    RowMajor,
    ColumnMajor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageOrientation {
    Portrait,
    Landscape,
}

impl Default for FillOrder {
    fn default() -> Self {
        FillOrder::RowMajor
    }
}

impl Default for PageOrientation {
    fn default() -> Self {
        PageOrientation::Portrait
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
    pub page_orientation: PageOrientation, // Portrait vs Landscape
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
            page_orientation: PageOrientation::Portrait,
            target_dpi: 300,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ThumbnailState {
    NotLoaded,
    Loading,
    Loaded(ImageBuffer<image::Rgba<u8>, Vec<u8>>),
    Failed(String),
}

impl Default for ThumbnailState {
    fn default() -> Self {
        ThumbnailState::NotLoaded
    }
}

#[derive(Debug, Clone)]
pub struct Card {
    pub path: PathBuf,
    pub thumbnail_state: ThumbnailState,
    pub original_dpi: Option<u32>,
    pub needs_scaling: bool,
    pub copy_count: u32,
}

impl Card {
    pub fn new(path: PathBuf) -> Self {
        let mut card = Self {
            path: path.clone(),
            thumbnail_state: ThumbnailState::NotLoaded,
            original_dpi: None,
            needs_scaling: false,
            copy_count: 1,
        };
        
        // Only load DPI info synchronously (it's fast)
        card.load_dpi_info();
        
        card
    }
    
    pub fn set_thumbnail_loading(&mut self) {
        self.thumbnail_state = ThumbnailState::Loading;
    }
    
    pub fn set_thumbnail_loaded(&mut self, thumbnail: ImageBuffer<image::Rgba<u8>, Vec<u8>>) {
        self.thumbnail_state = ThumbnailState::Loaded(thumbnail);
    }
    
    pub fn set_thumbnail_failed(&mut self, error: String) {
        self.thumbnail_state = ThumbnailState::Failed(error);
    }
    
    pub fn get_thumbnail(&self) -> Option<&ImageBuffer<image::Rgba<u8>, Vec<u8>>> {
        match &self.thumbnail_state {
            ThumbnailState::Loaded(thumbnail) => Some(thumbnail),
            _ => None,
        }
    }
    
    pub fn is_thumbnail_loaded(&self) -> bool {
        matches!(self.thumbnail_state, ThumbnailState::Loaded(_))
    }
    
    pub fn is_thumbnail_loading(&self) -> bool {
        matches!(self.thumbnail_state, ThumbnailState::Loading)
    }
    
    pub fn set_copy_count(&mut self, count: u32) {
        self.copy_count = count.max(1); // Ensure minimum of 1
    }
    
    pub fn get_copy_count(&self) -> u32 {
        self.copy_count
    }
    
    pub fn increment_copy_count(&mut self) {
        self.copy_count += 1;
    }
    
    pub fn decrement_copy_count(&mut self) {
        if self.copy_count > 1 {
            self.copy_count -= 1;
        }
    }
    
    fn load_dpi_info(&mut self) {
        self.original_dpi = super::image_processing::get_image_dpi(&self.path);
        // Default to 72 DPI if not found in EXIF
        if self.original_dpi.is_none() {
            self.original_dpi = Some(72);
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
    fn test_page_orientation_default() {
        assert_eq!(PageOrientation::default(), PageOrientation::Portrait);
    }

    #[test]
    fn test_page_orientation_variants() {
        let portrait = PageOrientation::Portrait;
        let landscape = PageOrientation::Landscape;
        
        assert_ne!(portrait, landscape);
        assert_eq!(portrait, PageOrientation::Portrait);
        assert_eq!(landscape, PageOrientation::Landscape);
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
        assert_eq!(params.page_orientation, PageOrientation::Portrait);
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
            page_orientation: PageOrientation::Landscape,
            target_dpi: 600,
        };
        
        assert_eq!(params.page_size, (100.0, 150.0));
        assert_eq!(params.card_size, (50.0, 70.0));
        assert_eq!(params.margins, custom_margins);
        assert_eq!(params.spacing, (1.0, 1.5));
        assert_eq!(params.orientation, FillOrder::ColumnMajor);
        assert_eq!(params.page_orientation, PageOrientation::Landscape);
        assert_eq!(params.target_dpi, 600);
    }

    #[test]
    fn test_card_new() {
        let path = PathBuf::from("/test/path/card.jpg");
        let card = Card::new(path.clone());
        
        assert_eq!(card.path, path);
        // Thumbnail state will be NotLoaded initially
        assert!(matches!(card.thumbnail_state, ThumbnailState::NotLoaded));
        // DPI will be Some(72) because we default to 72 DPI when EXIF isn't available
        assert_eq!(card.original_dpi, Some(72));
        assert!(!card.needs_scaling);
        assert_eq!(card.copy_count, 1);
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
        assert!(matches!(card.thumbnail_state, ThumbnailState::NotLoaded));
        assert_eq!(card.copy_count, 1);
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
        assert_eq!(original.thumbnail_state, cloned.thumbnail_state);
        assert_eq!(original.original_dpi, cloned.original_dpi);
        assert_eq!(original.needs_scaling, cloned.needs_scaling);
        assert_eq!(original.copy_count, cloned.copy_count);
    }

    #[test]
    fn test_card_copy_count_methods() {
        let mut card = Card::new(PathBuf::from("test.jpg"));
        
        // Test initial copy count
        assert_eq!(card.get_copy_count(), 1);
        
        // Test increment
        card.increment_copy_count();
        assert_eq!(card.get_copy_count(), 2);
        
        // Test set copy count
        card.set_copy_count(5);
        assert_eq!(card.get_copy_count(), 5);
        
        // Test decrement
        card.decrement_copy_count();
        assert_eq!(card.get_copy_count(), 4);
        
        // Test minimum of 1
        card.set_copy_count(0);
        assert_eq!(card.get_copy_count(), 1);
        
        // Test decrement at minimum
        card.decrement_copy_count();
        assert_eq!(card.get_copy_count(), 1);
    }
}