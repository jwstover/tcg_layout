use image::ImageBuffer;
use std::collections::VecDeque;
use std::path::PathBuf;
use tcg_layout::color_adjust::HslAdjustment;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum FillOrder {
    #[default]
    RowMajor,
    ColumnMajor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum PageOrientation {
    #[default]
    Portrait,
    Landscape,
}

/// Which edge the sheet is flipped along when printing double-sided.
/// Determines the mirror axis for back-page card positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum FlipEdge {
    #[default]
    LongEdge,
    ShortEdge,
}

/// Whether a page holds card fronts or generated card backs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageSide {
    #[default]
    Front,
    Back,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LayoutParams {
    pub page_size: (f32, f32),             // (width, height) in mm
    pub card_size: (f32, f32),             // (width, height) in mm
    pub margins: Margins,                  // margins in mm
    pub spacing: (f32, f32),               // (horizontal, vertical) spacing in mm
    pub orientation: FillOrder,            // RowMajor vs ColumnMajor
    pub page_orientation: PageOrientation, // Portrait vs Landscape
    pub target_dpi: u32,
    pub bleed_mm: f32,      // Bleed amount in millimeters
    pub enable_bleed: bool, // Whether bleed is enabled
    #[serde(default = "default_sharpen_amount")]
    pub sharpen_amount: f32, // Unsharp mask strength
    #[serde(default = "default_sharpen_radius")]
    pub sharpen_radius: f32, // Unsharp mask radius (Gaussian sigma in px at full resolution)
    #[serde(default = "default_sharpen_threshold")]
    pub sharpen_threshold: f32, // Local contrast below this fraction is left alone
    #[serde(default)]
    pub enable_sharpen: bool, // Whether sharpening is enabled
    pub center_layout: bool, // Whether to center the layout on the page
    #[serde(default = "default_printer_margins")]
    pub printer_margins: Margins, // Unprintable border the printer forces at the sheet edges
    #[serde(default)]
    pub enable_printer_margins: bool, // Whether to lay out and export for the printable area
    #[serde(default)]
    pub hsl_adjustments: Vec<HslAdjustment>, // Targeted color adjustments
    #[serde(default)]
    pub enable_color_adjust: bool, // Whether color adjustments are enabled
    #[serde(default)]
    pub enable_duplex: bool, // Whether double-sided (front/back) pages are generated
    #[serde(default)]
    pub flip_edge: FlipEdge, // Edge the sheet flips along when printed duplex
    #[serde(default)]
    pub back_offset: (f32, f32), // (x, y) mm shift applied to back pages for printer calibration
    #[serde(default)]
    pub default_back_path: Option<PathBuf>, // Back image used when a card has no specific back
}

fn default_sharpen_amount() -> f32 {
    1.0
}

/// Tuned on 600 DPI card scans, whose measured scanner blur is a Gaussian of
/// sigma ~0.9 px. Radii much above this start to show halos on high-contrast
/// edges without recovering any more detail.
fn default_sharpen_radius() -> f32 {
    0.7
}

/// Just above the ~2/255 noise floor measured on the scans, so sharpening
/// stays off flat areas.
fn default_sharpen_threshold() -> f32 {
    0.02
}

/// A plausible forced margin for a consumer inkjet, and only a starting point:
/// the real numbers come from the printer's specification or a test print, and
/// the bottom edge is usually the largest.
fn default_printer_margins() -> Margins {
    Margins::uniform(5.0)
}

impl Default for LayoutParams {
    fn default() -> Self {
        Self {
            page_size: (210.0, 297.0), // A4 size in mm
            card_size: (63.0, 88.0),   // Standard poker card size in mm
            margins: Margins::uniform(10.0),
            spacing: (2.0, 2.0),
            orientation: FillOrder::RowMajor,
            page_orientation: PageOrientation::Portrait,
            target_dpi: 300,
            bleed_mm: 3.0,       // Standard print bleed
            enable_bleed: false, // Disabled by default
            sharpen_amount: 1.0, // Moderate sharpening when enabled
            sharpen_radius: default_sharpen_radius(),
            sharpen_threshold: default_sharpen_threshold(),
            enable_sharpen: false, // Disabled by default
            center_layout: false,  // Disabled by default
            printer_margins: default_printer_margins(),
            enable_printer_margins: false, // Disabled by default
            hsl_adjustments: Vec::new(),
            enable_color_adjust: false, // Disabled by default
            enable_duplex: false,       // Disabled by default
            flip_edge: FlipEdge::LongEdge,
            back_offset: (0.0, 0.0),
            default_back_path: None,
        }
    }
}

impl LayoutParams {
    /// The unsharp mask settings as a single value.
    ///
    /// Spelled with the absolute `tcg_layout::` path because this module is
    /// compiled into both the lib and bin trees, while `sharpen` only ever
    /// lives in the lib.
    pub fn sharpen_params(&self) -> tcg_layout::sharpen::SharpenParams {
        tcg_layout::sharpen::SharpenParams::new(
            self.sharpen_amount,
            self.sharpen_radius,
            self.sharpen_threshold,
        )
    }

    /// Page dimensions honoring the page orientation (width, height) in mm
    pub fn effective_page_size(&self) -> (f32, f32) {
        match self.page_orientation {
            PageOrientation::Portrait => self.page_size,
            PageOrientation::Landscape => (self.page_size.1, self.page_size.0),
        }
    }

    /// Whether duplex back pages mirror horizontally (same row, opposite
    /// column) rather than vertically (same column, opposite row).
    ///
    /// The flip happens about the physical sheet's edge, so the mirror axis
    /// depends on both the flip edge and the page orientation: the sheet's
    /// long edge is vertical on portrait pages (long-edge flip mirrors x)
    /// but horizontal on landscape pages (long-edge flip mirrors y).
    pub fn back_mirror_is_horizontal(&self) -> bool {
        let (page_width, page_height) = self.effective_page_size();
        let long_edge_is_vertical = page_height >= page_width;
        match self.flip_edge {
            FlipEdge::LongEdge => long_edge_is_vertical,
            FlipEdge::ShortEdge => !long_edge_is_vertical,
        }
    }

    /// Whether back-page images must print rotated 180° so cut cards come
    /// out head-to-head (back upright when the card is flipped about its
    /// vertical axis, like a commercially printed card).
    ///
    /// A duplex flip about a horizontal axis (vertical mirror) puts the back
    /// image's top behind the front image's bottom; without this rotation the
    /// cut card would be head-to-toe.
    pub fn backs_rotated_180(&self) -> bool {
        !self.back_mirror_is_horizontal()
    }

    /// The printer's unprintable border, or all zeros when the feature is off.
    ///
    /// Like `margins`, these are read against the page as it comes out of the
    /// printer, so they are not rotated for landscape orientation.
    pub fn effective_printer_margins(&self) -> Margins {
        if self.enable_printer_margins {
            self.printer_margins
        } else {
            Margins::uniform(0.0)
        }
    }

    /// The border the layout has to keep clear of, in mm.
    ///
    /// Normally the printer's unprintable border. With duplex on it is
    /// symmetrized along the mirror axis: a back page is the front mirrored
    /// about the sheet, but the printer's unprintable border does *not*
    /// mirror with it, so a card only prints in register if its mirror image
    /// also lands inside the printable area. That is the intersection of the
    /// printable area and its own mirror, i.e. the larger of the two facing
    /// margins on both sides.
    pub fn layout_printer_margins(&self) -> Margins {
        let printer = self.effective_printer_margins();

        if !self.enable_duplex {
            return printer;
        }

        if self.back_mirror_is_horizontal() {
            let horizontal = printer.left.max(printer.right);
            Margins {
                left: horizontal,
                right: horizontal,
                ..printer
            }
        } else {
            let vertical = printer.top.max(printer.bottom);
            Margins {
                top: vertical,
                bottom: vertical,
                ..printer
            }
        }
    }

    /// Offset from the sheet origin to the printable area's origin, in mm.
    ///
    /// Layout coordinates are in physical sheet space; exporters subtract this
    /// to re-express them in printable-area space.
    pub fn printable_origin(&self) -> (f32, f32) {
        let printer = self.effective_printer_margins();
        (printer.left, printer.top)
    }

    /// Size of the printable area in mm — the page size exporters emit.
    ///
    /// A PDF page that already matches the printer's printable area is neither
    /// scaled nor re-centered when the driver fits it to that area, so cards
    /// print at exact size and land where the layout put them on the sheet.
    pub fn printable_size(&self) -> (f32, f32) {
        let (page_width, page_height) = self.effective_page_size();
        let printer = self.effective_printer_margins();

        (
            (page_width - printer.left - printer.right).max(MIN_PRINTABLE_MM),
            (page_height - printer.top - printer.bottom).max(MIN_PRINTABLE_MM),
        )
    }

    /// Calculate effective margins based on centering mode, in sheet
    /// coordinates. The printer's unprintable border acts as a floor: nothing
    /// can be laid out where the printer cannot put ink.
    ///
    /// When centering is enabled, returns calculated centered margins
    /// When centering is disabled, returns user-specified margins
    pub fn effective_margins(&self, grid: &GridLayout) -> Margins {
        let printer = self.layout_printer_margins();

        if !self.center_layout {
            return Margins {
                top: self.margins.top.max(printer.top),
                right: self.margins.right.max(printer.right),
                bottom: self.margins.bottom.max(printer.bottom),
                left: self.margins.left.max(printer.left),
            };
        }

        // Calculate actual grid dimensions
        let grid_width = grid.cols as f32 * self.card_size.0
            + grid.cols.saturating_sub(1) as f32 * self.spacing.0;
        let grid_height = grid.rows as f32 * self.card_size.1
            + grid.rows.saturating_sub(1) as f32 * self.spacing.1;

        // Get page dimensions considering orientation
        let (page_width, page_height) = self.effective_page_size();

        // Center on the physical sheet, then pull the grid inside the printable
        // band. The two differ only when the printer's margins are asymmetric.
        let left = center_offset(page_width, grid_width, printer.left, printer.right);
        let top = center_offset(page_height, grid_height, printer.top, printer.bottom);

        Margins {
            left,
            right: (page_width - grid_width - left).max(0.0),
            top,
            bottom: (page_height - grid_height - top).max(0.0),
        }
    }
}

/// Smallest page an export will emit, so absurd printer margins can't produce
/// a zero or negative page size.
const MIN_PRINTABLE_MM: f32 = 1.0;

/// Offset that centers `content` within `page`, pulled inside the printable
/// band `[near, page - far]` when that band isn't centered on the sheet.
fn center_offset(page: f32, content: f32, near: f32, far: f32) -> f32 {
    let centered = ((page - content) / 2.0).max(0.0);
    // Keep the range non-empty when the content cannot fit the band at all;
    // staying clear of the near edge is the better failure mode, since the
    // grid calculation already sized the grid to the band.
    let furthest = (page - far - content).max(near);

    centered.clamp(near, furthest)
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum ThumbnailState {
    #[default]
    NotLoaded,
    Loading,
    Loaded(ImageBuffer<image::Rgba<u8>, Vec<u8>>),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Card {
    pub path: PathBuf,
    pub thumbnail_state: ThumbnailState,
    pub original_dpi: Option<u32>,
    pub needs_scaling: bool,
    pub copy_count: u32,
    pub back_path: Option<PathBuf>,
}

impl Card {
    pub fn new(path: PathBuf) -> Self {
        let mut card = Self::placeholder(path);

        // Only load DPI info synchronously (it's fast)
        card.load_dpi_info();

        card
    }

    /// Create a card without touching the filesystem. Used for generated
    /// back-side slots whose thumbnail-loaded instances live elsewhere.
    pub fn placeholder(path: PathBuf) -> Self {
        Self {
            path,
            thumbnail_state: ThumbnailState::NotLoaded,
            original_dpi: None,
            needs_scaling: false,
            copy_count: 1,
            back_path: None,
        }
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
    pub x: f32, // x position in mm
    pub y: f32, // y position in mm
}

#[derive(Debug, Clone)]
pub struct PageLayout {
    pub page_number: usize,
    pub cards: Vec<(Card, CardPosition)>,
    pub side: PageSide,
}

#[derive(Debug, Clone)]
pub struct DpiWarning {
    pub card_path: PathBuf,
    pub original_dpi: u32,
    pub target_dpi: u32,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct CutMark {
    pub x1: f32, // start x position in mm
    pub y1: f32, // start y position in mm
    pub x2: f32, // end x position in mm
    pub y2: f32, // end y position in mm
    pub mark_type: CutMarkType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutMarkType {
    Horizontal, // cuts between rows
    Vertical,   // cuts between columns
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
            side: PageSide::Front,
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
        let params3 = LayoutParams {
            target_dpi: 600,
            ..Default::default()
        };

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

    #[test]
    fn test_flip_edge_default() {
        assert_eq!(FlipEdge::default(), FlipEdge::LongEdge);
    }

    #[test]
    fn test_page_side_default() {
        assert_eq!(PageSide::default(), PageSide::Front);
    }

    #[test]
    fn test_layout_params_duplex_defaults() {
        let params = LayoutParams::default();
        assert!(!params.enable_duplex);
        assert_eq!(params.flip_edge, FlipEdge::LongEdge);
        assert_eq!(params.back_offset, (0.0, 0.0));
        assert_eq!(params.default_back_path, None);
    }

    #[test]
    fn test_card_placeholder() {
        let card = Card::placeholder(PathBuf::from("back.png"));
        assert_eq!(card.path, PathBuf::from("back.png"));
        assert!(matches!(card.thumbnail_state, ThumbnailState::NotLoaded));
        assert_eq!(card.original_dpi, None);
        assert_eq!(card.copy_count, 1);
        assert_eq!(card.back_path, None);
    }

    #[test]
    fn test_back_mirror_axis_by_flip_edge_and_orientation() {
        let case = |page_orientation, flip_edge| LayoutParams {
            page_orientation,
            flip_edge,
            ..Default::default()
        };

        // Portrait: sheet's long edge is vertical -> long-edge flip mirrors x
        assert!(case(PageOrientation::Portrait, FlipEdge::LongEdge).back_mirror_is_horizontal());
        assert!(!case(PageOrientation::Portrait, FlipEdge::ShortEdge).back_mirror_is_horizontal());

        // Landscape: sheet's long edge is horizontal -> mirror axes swap
        assert!(!case(PageOrientation::Landscape, FlipEdge::LongEdge).back_mirror_is_horizontal());
        assert!(case(PageOrientation::Landscape, FlipEdge::ShortEdge).back_mirror_is_horizontal());

        // Vertical mirror (horizontal-axis flip) requires a 180° back
        // rotation to keep cut cards head-to-head; horizontal mirror doesn't
        assert!(!case(PageOrientation::Portrait, FlipEdge::LongEdge).backs_rotated_180());
        assert!(case(PageOrientation::Portrait, FlipEdge::ShortEdge).backs_rotated_180());
        assert!(case(PageOrientation::Landscape, FlipEdge::LongEdge).backs_rotated_180());
        assert!(!case(PageOrientation::Landscape, FlipEdge::ShortEdge).backs_rotated_180());
    }

    #[test]
    fn test_effective_page_size() {
        let mut params = LayoutParams::default();
        assert_eq!(params.effective_page_size(), (210.0, 297.0));
        params.page_orientation = PageOrientation::Landscape;
        assert_eq!(params.effective_page_size(), (297.0, 210.0));
    }

    #[test]
    fn test_layout_params_with_bleed() {
        let params = LayoutParams {
            bleed_mm: 3.0,
            enable_bleed: true,
            ..Default::default()
        };
        assert_eq!(params.bleed_mm, 3.0);
        assert!(params.enable_bleed);
    }

    #[test]
    fn test_effective_margins_without_centering() {
        let params = LayoutParams {
            margins: Margins {
                top: 5.0,
                right: 10.0,
                bottom: 15.0,
                left: 20.0,
            },
            center_layout: false,
            ..Default::default()
        };

        let grid = GridLayout {
            rows: 2,
            cols: 3,
            cards_per_page: 6,
            total_pages: 1,
        };

        let effective = params.effective_margins(&grid);

        // Should return user-specified margins
        assert_eq!(effective.top, 5.0);
        assert_eq!(effective.right, 10.0);
        assert_eq!(effective.bottom, 15.0);
        assert_eq!(effective.left, 20.0);
    }

    #[test]
    fn test_effective_margins_with_centering() {
        let params = LayoutParams {
            page_size: (100.0, 150.0),
            card_size: (20.0, 30.0),
            spacing: (2.0, 3.0),
            margins: Margins::uniform(5.0), // Should be ignored when centering
            page_orientation: PageOrientation::Portrait,
            center_layout: true,
            ..Default::default()
        };

        // Grid: 2 cols, 2 rows
        let grid = GridLayout {
            rows: 2,
            cols: 2,
            cards_per_page: 4,
            total_pages: 1,
        };

        let effective = params.effective_margins(&grid);

        // Grid dimensions:
        // Width = 2 * 20 + 1 * 2 = 42mm
        // Height = 2 * 30 + 1 * 3 = 63mm
        // Centered margins:
        // Horizontal = (100 - 42) / 2 = 29mm
        // Vertical = (150 - 63) / 2 = 43.5mm

        assert_eq!(effective.left, 29.0);
        assert_eq!(effective.right, 29.0);
        assert_eq!(effective.top, 43.5);
        assert_eq!(effective.bottom, 43.5);
    }

    #[test]
    fn test_effective_margins_with_centering_landscape() {
        let params = LayoutParams {
            page_size: (100.0, 150.0),
            card_size: (20.0, 30.0),
            spacing: (2.0, 3.0),
            margins: Margins::uniform(5.0), // Should be ignored when centering
            page_orientation: PageOrientation::Landscape, // Page becomes 150x100
            center_layout: true,
            ..Default::default()
        };

        // Grid: 3 cols, 1 row (landscape gives more width)
        let grid = GridLayout {
            rows: 1,
            cols: 3,
            cards_per_page: 3,
            total_pages: 1,
        };

        let effective = params.effective_margins(&grid);

        // Grid dimensions (on landscape page 150x100):
        // Width = 3 * 20 + 2 * 2 = 64mm
        // Height = 1 * 30 + 0 * 3 = 30mm
        // Centered margins:
        // Horizontal = (150 - 64) / 2 = 43mm
        // Vertical = (100 - 30) / 2 = 35mm

        assert_eq!(effective.left, 43.0);
        assert_eq!(effective.right, 43.0);
        assert_eq!(effective.top, 35.0);
        assert_eq!(effective.bottom, 35.0);
    }

    #[test]
    fn test_effective_margins_single_card_centered() {
        let params = LayoutParams {
            page_size: (100.0, 150.0),
            card_size: (50.0, 70.0),
            spacing: (0.0, 0.0),
            margins: Margins::uniform(5.0),
            page_orientation: PageOrientation::Portrait,
            center_layout: true,
            ..Default::default()
        };

        let grid = GridLayout {
            rows: 1,
            cols: 1,
            cards_per_page: 1,
            total_pages: 1,
        };

        let effective = params.effective_margins(&grid);

        // Grid dimensions: 50x70mm (single card, no spacing)
        // Centered margins:
        // Horizontal = (100 - 50) / 2 = 25mm
        // Vertical = (150 - 70) / 2 = 40mm

        assert_eq!(effective.left, 25.0);
        assert_eq!(effective.right, 25.0);
        assert_eq!(effective.top, 40.0);
        assert_eq!(effective.bottom, 40.0);
    }

    /// Printer margins with typical asymmetry: a bigger bottom edge, where the
    /// paper feed grips the sheet.
    fn asymmetric_printer_margins() -> Margins {
        Margins {
            top: 3.0,
            right: 3.0,
            bottom: 15.0,
            left: 3.0,
        }
    }

    #[test]
    fn test_printer_margins_default_off_and_inert() {
        let params = LayoutParams::default();

        assert!(!params.enable_printer_margins);
        assert_eq!(params.effective_printer_margins(), Margins::uniform(0.0));
        assert_eq!(params.printable_origin(), (0.0, 0.0));
        assert_eq!(params.printable_size(), params.effective_page_size());
    }

    #[test]
    fn test_printable_size_and_origin() {
        let params = LayoutParams {
            page_size: (210.0, 297.0),
            enable_printer_margins: true,
            printer_margins: asymmetric_printer_margins(),
            ..Default::default()
        };

        assert_eq!(params.printable_size(), (204.0, 279.0));
        assert_eq!(params.printable_origin(), (3.0, 3.0));
    }

    #[test]
    fn test_printable_size_landscape_uses_oriented_page() {
        let params = LayoutParams {
            page_size: (210.0, 297.0),
            page_orientation: PageOrientation::Landscape,
            enable_printer_margins: true,
            printer_margins: Margins::uniform(5.0),
            ..Default::default()
        };

        // Printer margins read against the page as it comes out of the printer
        assert_eq!(params.printable_size(), (287.0, 200.0));
    }

    #[test]
    fn test_printable_size_never_collapses() {
        let params = LayoutParams {
            page_size: (20.0, 20.0),
            enable_printer_margins: true,
            printer_margins: Margins::uniform(30.0),
            ..Default::default()
        };

        let (width, height) = params.printable_size();
        assert!(width > 0.0 && height > 0.0);
    }

    #[test]
    fn test_effective_margins_floored_by_printer_margins() {
        let params = LayoutParams {
            page_size: (210.0, 297.0),
            margins: Margins::uniform(5.0),
            enable_printer_margins: true,
            printer_margins: asymmetric_printer_margins(),
            center_layout: false,
            ..Default::default()
        };
        let grid = calculate_test_grid();

        let effective = params.effective_margins(&grid);

        // The user's 5mm wins where the printer can reach that far in;
        // the printer's 15mm bottom wins where it cannot
        assert_eq!(effective.top, 5.0);
        assert_eq!(effective.left, 5.0);
        assert_eq!(effective.right, 5.0);
        assert_eq!(effective.bottom, 15.0);
    }

    #[test]
    fn test_effective_margins_unchanged_when_printer_margins_disabled() {
        let with_margins = LayoutParams {
            page_size: (210.0, 297.0),
            margins: Margins::uniform(5.0),
            printer_margins: Margins::uniform(20.0),
            enable_printer_margins: false,
            ..Default::default()
        };
        let grid = calculate_test_grid();

        assert_eq!(with_margins.effective_margins(&grid), Margins::uniform(5.0));
    }

    #[test]
    fn test_centered_layout_clamped_into_printable_band() {
        // Sheet-centered would put the grid 10mm from the top and bottom,
        // which is inside the printer's 15mm bottom border
        let params = LayoutParams {
            page_size: (100.0, 100.0),
            card_size: (40.0, 80.0),
            spacing: (0.0, 0.0),
            center_layout: true,
            enable_printer_margins: true,
            printer_margins: asymmetric_printer_margins(),
            ..Default::default()
        };
        let grid = GridLayout {
            rows: 1,
            cols: 1,
            cards_per_page: 1,
            total_pages: 1,
        };

        let effective = params.effective_margins(&grid);

        // Pulled up until the grid's bottom edge sits on the printable
        // boundary: 100 - 15 - 80 = 5mm from the top
        assert_eq!(effective.top, 5.0);
        assert_eq!(effective.bottom, 15.0);
        // Left/right are symmetric and clear of the border, so untouched
        assert_eq!(effective.left, 30.0);
        assert_eq!(effective.right, 30.0);
    }

    #[test]
    fn test_centered_layout_too_tall_for_band_keeps_near_edge_clear() {
        // A grid taller than the printable band can't be placed legally at all
        // (the grid calculation forces at least one card). Clearing the near
        // edge is the better failure: the overflow is clipped on one side
        // instead of both.
        let params = LayoutParams {
            page_size: (100.0, 100.0),
            card_size: (40.0, 90.0),
            spacing: (0.0, 0.0),
            center_layout: true,
            enable_printer_margins: true,
            printer_margins: asymmetric_printer_margins(),
            ..Default::default()
        };
        let grid = GridLayout {
            rows: 1,
            cols: 1,
            cards_per_page: 1,
            total_pages: 1,
        };

        let effective = params.effective_margins(&grid);

        assert_eq!(effective.top, 3.0);
    }

    #[test]
    fn test_centered_layout_stays_sheet_centered_when_band_allows() {
        let params = LayoutParams {
            page_size: (100.0, 100.0),
            card_size: (40.0, 60.0),
            spacing: (0.0, 0.0),
            center_layout: true,
            enable_printer_margins: true,
            printer_margins: asymmetric_printer_margins(),
            ..Default::default()
        };
        let grid = GridLayout {
            rows: 1,
            cols: 1,
            cards_per_page: 1,
            total_pages: 1,
        };

        let effective = params.effective_margins(&grid);

        // (100 - 60) / 2 = 20mm, clear of the 3mm top and 15mm bottom
        assert_eq!(effective.top, 20.0);
        assert_eq!(effective.bottom, 20.0);
    }

    #[test]
    fn test_layout_printer_margins_symmetrized_for_duplex() {
        let params = LayoutParams {
            page_size: (210.0, 297.0),
            enable_printer_margins: true,
            printer_margins: asymmetric_printer_margins(),
            enable_duplex: true,
            // Portrait + short edge flips about a horizontal axis, mirroring y
            flip_edge: FlipEdge::ShortEdge,
            ..Default::default()
        };

        // A card mirrored onto the back would land in the facing border unless
        // both edges keep clear of the larger of the two
        let layout = params.layout_printer_margins();
        assert_eq!(layout.top, 15.0);
        assert_eq!(layout.bottom, 15.0);
        // The mirror doesn't touch x, so those stay as the printer reports them
        assert_eq!(layout.left, 3.0);
        assert_eq!(layout.right, 3.0);

        // The printable area itself is unchanged: it's what the page is sized to
        assert_eq!(
            params.effective_printer_margins(),
            asymmetric_printer_margins()
        );
    }

    #[test]
    fn test_layout_printer_margins_untouched_without_duplex() {
        let params = LayoutParams {
            enable_printer_margins: true,
            printer_margins: asymmetric_printer_margins(),
            enable_duplex: false,
            ..Default::default()
        };

        assert_eq!(
            params.layout_printer_margins(),
            asymmetric_printer_margins()
        );
    }

    fn calculate_test_grid() -> GridLayout {
        GridLayout {
            rows: 2,
            cols: 2,
            cards_per_page: 4,
            total_pages: 1,
        }
    }
}
