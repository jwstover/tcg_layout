use crate::types::{
    Card, CardPosition, CutMark, CutMarkType, FillOrder, GridLayout, LayoutParams, PageLayout,
    PageSide,
};
use std::collections::HashMap;
use std::path::PathBuf;

pub fn calculate_grid(params: &LayoutParams) -> GridLayout {
    // Get effective page dimensions based on orientation
    let (page_width, page_height) = params.effective_page_size();

    // The printer's unprintable border is a floor on the margins either way:
    // a card laid out there would be clipped off the print.
    let printer = params.layout_printer_margins();

    // When centering, ignore margins for grid calculation to maximize space
    let (margin_left, margin_right, margin_top, margin_bottom) = if params.center_layout {
        (printer.left, printer.right, printer.top, printer.bottom)
    } else {
        (
            params.margins.left.max(printer.left),
            params.margins.right.max(printer.right),
            params.margins.top.max(printer.top),
            params.margins.bottom.max(printer.bottom),
        )
    };

    let available_width = page_width - margin_left - margin_right;
    let available_height = page_height - margin_top - margin_bottom;

    let card_width_with_spacing = params.card_size.0 + params.spacing.0;
    let card_height_with_spacing = params.card_size.1 + params.spacing.1;

    let cols = ((available_width + params.spacing.0) / card_width_with_spacing).floor() as usize;
    let rows = ((available_height + params.spacing.1) / card_height_with_spacing).floor() as usize;

    let cols = cols.max(1);
    let rows = rows.max(1);

    let cards_per_page = rows * cols;

    GridLayout {
        rows,
        cols,
        cards_per_page,
        total_pages: 1, // Will be calculated by distribute_cards
    }
}

pub fn distribute_cards(
    cards: &[Card],
    grid: &GridLayout,
    params: &LayoutParams,
) -> Vec<PageLayout> {
    distribute_cards_with_backs(cards, grid, params, &HashMap::new())
}

/// Distribute cards into pages. When duplex is enabled, each front page is
/// followed by a generated back page whose card positions are mirrored for
/// the configured flip edge and shifted by the printer calibration offset.
///
/// `back_cards` maps back image paths to Card instances (typically carrying
/// loaded thumbnails); paths not in the map get placeholder Cards, which is
/// sufficient for export since exporters load images from disk by path.
pub fn distribute_cards_with_backs(
    cards: &[Card],
    grid: &GridLayout,
    params: &LayoutParams,
    back_cards: &HashMap<PathBuf, Card>,
) -> Vec<PageLayout> {
    let mut pages = Vec::new();
    let mut page_number = 1;

    // Expand cards based on copy count
    let mut expanded_cards = Vec::new();
    for card in cards {
        for _ in 0..card.copy_count {
            expanded_cards.push(card.clone());
        }
    }

    let card_chunks = expanded_cards.chunks(grid.cards_per_page);

    for chunk in card_chunks {
        let page_cards: Vec<(Card, CardPosition)> = chunk
            .iter()
            .enumerate()
            .map(|(index, card)| {
                let position = calculate_card_position(index, grid, params);
                (card.clone(), position)
            })
            .collect();

        pages.push(PageLayout {
            page_number,
            cards: page_cards,
            side: PageSide::Front,
        });
        page_number += 1;

        if params.enable_duplex {
            // Cards with no back (and no default back) leave a gap so the
            // remaining backs stay aligned with their fronts.
            let back_page_cards: Vec<(Card, CardPosition)> = chunk
                .iter()
                .enumerate()
                .filter_map(|(index, card)| {
                    let back_path = card
                        .back_path
                        .clone()
                        .or_else(|| params.default_back_path.clone())?;
                    let front_position = calculate_card_position(index, grid, params);
                    let position = mirror_position_for_back(&front_position, params);
                    let back_card = back_cards
                        .get(&back_path)
                        .cloned()
                        .unwrap_or_else(|| Card::placeholder(back_path));
                    Some((back_card, position))
                })
                .collect();

            // Always emit the back page (even when empty) so front/back
            // pairing survives duplex printing.
            pages.push(PageLayout {
                page_number,
                cards: back_page_cards,
                side: PageSide::Back,
            });
            page_number += 1;
        }
    }

    pages
}

/// Mirror a front-page card position onto the back page so front and back
/// align when the sheet is flipped along the configured edge, then apply the
/// duplex calibration offset. Flipping rotates the sheet about the axis
/// parallel to the flip edge: a vertical axis mirrors x, a horizontal one
/// mirrors y.
pub fn mirror_position_for_back(position: &CardPosition, params: &LayoutParams) -> CardPosition {
    let (page_width, page_height) = params.effective_page_size();

    let (x, y) = if params.back_mirror_is_horizontal() {
        (page_width - position.x - params.card_size.0, position.y)
    } else {
        (position.x, page_height - position.y - params.card_size.1)
    };

    CardPosition {
        x: x + params.back_offset.0,
        y: y + params.back_offset.1,
    }
}

pub fn calculate_card_position(
    card_index: usize,
    grid: &GridLayout,
    params: &LayoutParams,
) -> CardPosition {
    let (row, col) = match params.orientation {
        FillOrder::RowMajor => {
            let row = card_index / grid.cols;
            let col = card_index % grid.cols;
            (row, col)
        }
        FillOrder::ColumnMajor => {
            let col = card_index / grid.rows;
            let row = card_index % grid.rows;
            (row, col)
        }
    };

    // Use effective margins for positioning
    let effective_margins = params.effective_margins(grid);
    let start_x = effective_margins.left;
    let start_y = effective_margins.top;

    CardPosition {
        x: start_x + col as f32 * (params.card_size.0 + params.spacing.0),
        y: start_y + row as f32 * (params.card_size.1 + params.spacing.1),
    }
}

pub fn generate_positions(params: &LayoutParams, grid: &GridLayout) -> Vec<CardPosition> {
    let mut positions = Vec::new();

    // Use effective margins for positioning
    let effective_margins = params.effective_margins(grid);
    let start_x = effective_margins.left;
    let start_y = effective_margins.top;

    for card_index in 0..grid.cards_per_page {
        let (row, col) = match params.orientation {
            FillOrder::RowMajor => {
                let row = card_index / grid.cols;
                let col = card_index % grid.cols;
                (row, col)
            }
            FillOrder::ColumnMajor => {
                let col = card_index / grid.rows;
                let row = card_index % grid.rows;
                (row, col)
            }
        };

        let x = start_x + col as f32 * (params.card_size.0 + params.spacing.0);
        let y = start_y + row as f32 * (params.card_size.1 + params.spacing.1);

        positions.push(CardPosition { x, y });
    }

    positions
}

/// How far a cut mark reaches past a trim edge, in mm.
///
/// Marks straddle every trim line they meet rather than stopping at it, so each
/// intersection of a vertical and a horizontal cut line carries a cross. Cutting
/// along one line therefore always leaves a stub of every mark perpendicular to
/// it on both sides of the blade, which is what keeps the remaining cuts
/// registered once the margins are gone.
pub const CUT_MARK_OVERLAP_MM: f32 = 2.0;

/// Tolerance for treating two computed cut-line coordinates as the same line.
const CUT_LINE_EPSILON_MM: f32 = 1e-4;

/// The distinct cut-line coordinates along one axis, ascending.
///
/// Each card contributes a leading and a trailing trim line; with zero spacing
/// adjacent cards share one, so duplicates are collapsed.
fn cut_line_coords(start: f32, count: usize, card_size: f32, spacing: f32) -> Vec<f32> {
    let mut coords = Vec::with_capacity(count * 2);

    for i in 0..count {
        let leading = start + i as f32 * (card_size + spacing);

        for coord in [leading, leading + card_size] {
            let is_new = coords
                .last()
                .is_none_or(|last: &f32| (coord - last).abs() > CUT_LINE_EPSILON_MM);
            if is_new {
                coords.push(coord);
            }
        }
    }

    coords
}

pub fn calculate_cut_marks(params: &LayoutParams, grid: &GridLayout) -> Vec<CutMark> {
    let mut cut_marks = Vec::new();

    // Get effective page dimensions based on orientation
    let (page_width, page_height) = params.effective_page_size();

    // Marks run out as far as the printer can actually put ink: the sheet edge,
    // or the printable area's edge when the printer forces margins. Running
    // them into an unprintable border would just clip the mark short of the
    // page edge anyway, and on an off-center printable area it would clip
    // asymmetrically.
    let printer = params.effective_printer_margins();
    let (min_x, min_y) = (printer.left, printer.top);
    let (max_x, max_y) = (page_width - printer.right, page_height - printer.bottom);

    // Use effective margins for cut mark positioning
    let effective_margins = params.effective_margins(grid);

    // The trim lines the blade follows, in mm from the page origin.
    // Note: when bleed is enabled the images extend past the outermost of these
    // by bleed_mm, which is why all renderers draw the marks over the images.
    let x_lines = cut_line_coords(
        effective_margins.left,
        grid.cols,
        params.card_size.0,
        params.spacing.0,
    );
    let y_lines = cut_line_coords(
        effective_margins.top,
        grid.rows,
        params.card_size.1,
        params.spacing.1,
    );

    // How far a mark reaches past each trim line it crosses. Capped at half a
    // card so the reach from opposite edges can never span a whole card.
    let overlap_x = CUT_MARK_OVERLAP_MM.min(params.card_size.0 / 2.0);
    let overlap_y = CUT_MARK_OVERLAP_MM.min(params.card_size.1 / 2.0);

    // Vertical marks: on every vertical trim line, one segment straddling each
    // horizontal trim line, so the pair forms a cross at that intersection. The
    // outermost segments run on out to the page edges instead of stopping short,
    // which is what makes the marks visible outside the card area.
    for &x in &x_lines {
        for (i, &y) in y_lines.iter().enumerate() {
            cut_marks.push(CutMark {
                x1: x,
                y1: if i == 0 { min_y } else { y - overlap_y },
                x2: x,
                y2: if i == y_lines.len() - 1 {
                    max_y
                } else {
                    y + overlap_y
                },
                mark_type: CutMarkType::Vertical,
            });
        }
    }

    // Horizontal marks: the same, with the axes swapped
    for &y in &y_lines {
        for (i, &x) in x_lines.iter().enumerate() {
            cut_marks.push(CutMark {
                x1: if i == 0 { min_x } else { x - overlap_x },
                y1: y,
                x2: if i == x_lines.len() - 1 {
                    max_x
                } else {
                    x + overlap_x
                },
                y2: y,
                mark_type: CutMarkType::Horizontal,
            });
        }
    }

    cut_marks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FillOrder, FlipEdge, LayoutParams, Margins, PageOrientation};
    use std::path::PathBuf;

    #[test]
    fn test_calculate_grid_default_params() {
        let params = LayoutParams::default();
        let grid = calculate_grid(&params);

        // A4 page (210x297mm) with 10mm margins gives 190x277mm available
        // Poker cards (63x88mm) with 2mm spacing
        // Cols: (190 + 2) / (63 + 2) = 192 / 65 = 2.95 -> 2
        // Rows: (277 + 2) / (88 + 2) = 279 / 90 = 3.1 -> 3
        assert_eq!(grid.cols, 2);
        assert_eq!(grid.rows, 3);
        assert_eq!(grid.cards_per_page, 6);
    }

    #[test]
    fn test_calculate_grid_custom_params() {
        let params = LayoutParams {
            page_size: (100.0, 150.0),
            card_size: (20.0, 30.0),
            margins: Margins::uniform(5.0),
            spacing: (1.0, 1.0),
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
        };

        let grid = calculate_grid(&params);

        // Available: 90x140mm
        // Cards with spacing: 21x31mm
        // Cols: (90 + 1) / 21 = 91 / 21 = 4.33 -> 4
        // Rows: (140 + 1) / 31 = 141 / 31 = 4.54 -> 4
        assert_eq!(grid.cols, 4);
        assert_eq!(grid.rows, 4);
        assert_eq!(grid.cards_per_page, 16);
    }

    #[test]
    fn test_calculate_grid_minimum_one_card() {
        let params = LayoutParams {
            page_size: (50.0, 60.0),
            card_size: (100.0, 120.0), // Larger than page
            margins: Margins::uniform(0.0),
            spacing: (0.0, 0.0),
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
        };

        let grid = calculate_grid(&params);

        // Even if calculations would give 0, we ensure at least 1x1
        assert_eq!(grid.cols, 1);
        assert_eq!(grid.rows, 1);
        assert_eq!(grid.cards_per_page, 1);
    }

    #[test]
    fn test_distribute_cards_single_page() {
        let cards = vec![
            Card::new(PathBuf::from("card1.jpg")),
            Card::new(PathBuf::from("card2.jpg")),
            Card::new(PathBuf::from("card3.jpg")),
        ];

        let grid = GridLayout {
            rows: 2,
            cols: 3,
            cards_per_page: 6,
            total_pages: 1,
        };

        let params = LayoutParams::default();
        let pages = distribute_cards(&cards, &grid, &params);

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].page_number, 1);
        assert_eq!(pages[0].cards.len(), 3);
    }

    #[test]
    fn test_distribute_cards_multiple_pages() {
        let cards = vec![
            Card::new(PathBuf::from("card1.jpg")),
            Card::new(PathBuf::from("card2.jpg")),
            Card::new(PathBuf::from("card3.jpg")),
            Card::new(PathBuf::from("card4.jpg")),
            Card::new(PathBuf::from("card5.jpg")),
        ];

        let grid = GridLayout {
            rows: 1,
            cols: 2,
            cards_per_page: 2,
            total_pages: 3,
        };

        let params = LayoutParams::default();
        let pages = distribute_cards(&cards, &grid, &params);

        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0].cards.len(), 2);
        assert_eq!(pages[1].cards.len(), 2);
        assert_eq!(pages[2].cards.len(), 1);

        assert_eq!(pages[0].page_number, 1);
        assert_eq!(pages[1].page_number, 2);
        assert_eq!(pages[2].page_number, 3);
    }

    #[test]
    fn test_generate_positions_row_major() {
        let params = LayoutParams {
            page_size: (100.0, 150.0),
            card_size: (20.0, 30.0),
            margins: Margins::uniform(5.0),
            spacing: (2.0, 3.0),
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
        };

        let grid = GridLayout {
            rows: 2,
            cols: 2,
            cards_per_page: 4,
            total_pages: 1,
        };

        let positions = generate_positions(&params, &grid);

        assert_eq!(positions.len(), 4);

        // Row major: (0,0), (0,1), (1,0), (1,1)
        assert_eq!(positions[0].x, 5.0); // start_x
        assert_eq!(positions[0].y, 5.0); // start_y

        assert_eq!(positions[1].x, 27.0); // 5 + 20 + 2
        assert_eq!(positions[1].y, 5.0);

        assert_eq!(positions[2].x, 5.0);
        assert_eq!(positions[2].y, 38.0); // 5 + 30 + 3

        assert_eq!(positions[3].x, 27.0);
        assert_eq!(positions[3].y, 38.0);
    }

    #[test]
    fn test_generate_positions_column_major() {
        let params = LayoutParams {
            page_size: (100.0, 150.0),
            card_size: (20.0, 30.0),
            margins: Margins::uniform(5.0),
            spacing: (2.0, 3.0),
            orientation: FillOrder::ColumnMajor,
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
        };

        let grid = GridLayout {
            rows: 2,
            cols: 2,
            cards_per_page: 4,
            total_pages: 1,
        };

        let positions = generate_positions(&params, &grid);

        assert_eq!(positions.len(), 4);

        // Column major: (0,0), (1,0), (0,1), (1,1)
        assert_eq!(positions[0].x, 5.0); // start_x
        assert_eq!(positions[0].y, 5.0); // start_y

        assert_eq!(positions[1].x, 5.0);
        assert_eq!(positions[1].y, 38.0); // 5 + 30 + 3

        assert_eq!(positions[2].x, 27.0); // 5 + 20 + 2
        assert_eq!(positions[2].y, 5.0);

        assert_eq!(positions[3].x, 27.0);
        assert_eq!(positions[3].y, 38.0);
    }

    #[test]
    fn test_generate_positions_empty_grid() {
        let params = LayoutParams::default();
        let grid = GridLayout {
            rows: 0,
            cols: 0,
            cards_per_page: 0,
            total_pages: 1,
        };

        let positions = generate_positions(&params, &grid);
        assert!(positions.is_empty());
    }

    #[test]
    fn test_calculate_grid_landscape_orientation() {
        let params = LayoutParams {
            page_size: (210.0, 297.0), // A4 portrait
            card_size: (63.0, 88.0),   // Poker card
            margins: Margins::uniform(10.0),
            spacing: (2.0, 2.0),
            orientation: FillOrder::RowMajor,
            page_orientation: PageOrientation::Landscape, // Landscape mode
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
        };

        let grid = calculate_grid(&params);

        // In landscape, effective page size is 297x210mm (swapped)
        // Available space: 277x190mm
        // Cols: (277 + 2) / (63 + 2) = 279 / 65 = 4.29 -> 4
        // Rows: (190 + 2) / (88 + 2) = 192 / 90 = 2.13 -> 2
        assert_eq!(grid.cols, 4);
        assert_eq!(grid.rows, 2);
        assert_eq!(grid.cards_per_page, 8);
    }

    #[test]
    fn test_distribute_cards_with_copy_counts() {
        let mut cards = vec![
            Card::new(PathBuf::from("card1.jpg")),
            Card::new(PathBuf::from("card2.jpg")),
        ];

        // Set different copy counts
        cards[0].set_copy_count(3); // 3 copies of card1
        cards[1].set_copy_count(2); // 2 copies of card2

        let grid = GridLayout {
            rows: 2,
            cols: 2,
            cards_per_page: 4,
            total_pages: 2,
        };

        let params = LayoutParams::default();
        let pages = distribute_cards(&cards, &grid, &params);

        // Should have 5 total cards (3 + 2), which needs 2 pages (4 per page)
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].cards.len(), 4); // First page: full
        assert_eq!(pages[1].cards.len(), 1); // Second page: 1 remaining

        // Verify the card distribution
        // First page should have 3 copies of card1 + 1 copy of card2
        assert_eq!(pages[0].cards[0].0.path, PathBuf::from("card1.jpg"));
        assert_eq!(pages[0].cards[1].0.path, PathBuf::from("card1.jpg"));
        assert_eq!(pages[0].cards[2].0.path, PathBuf::from("card1.jpg"));
        assert_eq!(pages[0].cards[3].0.path, PathBuf::from("card2.jpg"));

        // Second page should have remaining copy of card2
        assert_eq!(pages[1].cards[0].0.path, PathBuf::from("card2.jpg"));
    }

    #[test]
    fn test_calculate_grid_with_centering() {
        let params = LayoutParams {
            page_size: (210.0, 297.0),
            card_size: (63.0, 88.0),
            margins: Margins::uniform(10.0), // Should be ignored
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
            center_layout: true, // Centering enabled
            hsl_adjustments: Vec::new(),
            enable_color_adjust: false,
            ..Default::default()
        };

        let grid = calculate_grid(&params);

        // With centering, margins are ignored for grid calculation
        // Available: 210x297mm (full page)
        // Cols: (210 + 2) / (63 + 2) = 212 / 65 = 3.26 -> 3
        // Rows: (297 + 2) / (88 + 2) = 299 / 90 = 3.32 -> 3
        assert_eq!(grid.cols, 3);
        assert_eq!(grid.rows, 3);
        assert_eq!(grid.cards_per_page, 9);
    }

    #[test]
    fn test_card_positioning_with_centering() {
        let params = LayoutParams {
            page_size: (100.0, 150.0),
            card_size: (20.0, 30.0),
            spacing: (2.0, 3.0),
            margins: Margins::uniform(5.0), // Should be ignored
            page_orientation: PageOrientation::Portrait,
            center_layout: true,
            ..Default::default()
        };

        // Create a 2x2 grid
        let grid = GridLayout {
            rows: 2,
            cols: 2,
            cards_per_page: 4,
            total_pages: 1,
        };

        let positions = generate_positions(&params, &grid);

        // Grid dimensions: 2*20 + 1*2 = 42mm wide, 2*30 + 1*3 = 63mm tall
        // Centered margins: H=(100-42)/2=29mm, V=(150-63)/2=43.5mm

        // First card (row 0, col 0) should be at (29, 43.5)
        assert_eq!(positions[0].x, 29.0);
        assert_eq!(positions[0].y, 43.5);

        // Second card (row 0, col 1) should be at (29 + 20 + 2, 43.5) = (51, 43.5)
        assert_eq!(positions[1].x, 51.0);
        assert_eq!(positions[1].y, 43.5);

        // Third card (row 1, col 0) should be at (29, 43.5 + 30 + 3) = (29, 76.5)
        assert_eq!(positions[2].x, 29.0);
        assert_eq!(positions[2].y, 76.5);

        // Fourth card (row 1, col 1) should be at (51, 76.5)
        assert_eq!(positions[3].x, 51.0);
        assert_eq!(positions[3].y, 76.5);
    }

    fn duplex_test_params() -> LayoutParams {
        LayoutParams {
            page_size: (100.0, 150.0),
            card_size: (20.0, 30.0),
            margins: Margins::uniform(5.0),
            spacing: (2.0, 3.0),
            enable_duplex: true,
            default_back_path: Some(PathBuf::from("default_back.png")),
            ..Default::default()
        }
    }

    #[test]
    fn test_mirror_position_long_edge_portrait_mirrors_x() {
        let params = duplex_test_params();
        // Portrait 100x150: long edge is vertical, so x mirrors
        let position = CardPosition { x: 5.0, y: 40.0 };
        let mirrored = mirror_position_for_back(&position, &params);
        assert_eq!(mirrored.x, 100.0 - 5.0 - 20.0); // 75.0
        assert_eq!(mirrored.y, 40.0);
    }

    #[test]
    fn test_mirror_position_short_edge_portrait_mirrors_y() {
        let params = LayoutParams {
            flip_edge: FlipEdge::ShortEdge,
            ..duplex_test_params()
        };
        let position = CardPosition { x: 5.0, y: 40.0 };
        let mirrored = mirror_position_for_back(&position, &params);
        assert_eq!(mirrored.x, 5.0);
        assert_eq!(mirrored.y, 150.0 - 40.0 - 30.0); // 80.0
    }

    #[test]
    fn test_mirror_position_long_edge_landscape_mirrors_y() {
        let params = LayoutParams {
            page_orientation: PageOrientation::Landscape,
            ..duplex_test_params()
        };
        // Landscape: effective page is 150x100, long edge is horizontal, so y mirrors
        let position = CardPosition { x: 5.0, y: 40.0 };
        let mirrored = mirror_position_for_back(&position, &params);
        assert_eq!(mirrored.x, 5.0);
        assert_eq!(mirrored.y, 100.0 - 40.0 - 30.0); // 30.0
    }

    #[test]
    fn test_mirror_position_applies_back_offset() {
        let params = LayoutParams {
            back_offset: (0.5, -0.3),
            ..duplex_test_params()
        };
        let position = CardPosition { x: 5.0, y: 40.0 };
        let mirrored = mirror_position_for_back(&position, &params);
        assert!((mirrored.x - 75.5).abs() < 1e-5);
        assert!((mirrored.y - 39.7).abs() < 1e-5);
    }

    #[test]
    fn test_mirror_is_involution_without_offset() {
        // Mirroring twice must return the original position
        let params = duplex_test_params();
        let position = CardPosition { x: 27.0, y: 38.0 };
        let twice =
            mirror_position_for_back(&mirror_position_for_back(&position, &params), &params);
        assert!((twice.x - position.x).abs() < 1e-5);
        assert!((twice.y - position.y).abs() < 1e-5);
    }

    #[test]
    fn test_duplex_interleaves_front_and_back_pages() {
        let mut cards = vec![
            Card::new(PathBuf::from("card1.jpg")),
            Card::new(PathBuf::from("card2.jpg")),
            Card::new(PathBuf::from("card3.jpg")),
        ];
        cards[2].set_copy_count(3); // 5 cards total

        let grid = GridLayout {
            rows: 2,
            cols: 2,
            cards_per_page: 4,
            total_pages: 2,
        };

        let params = duplex_test_params();
        let pages = distribute_cards(&cards, &grid, &params);

        // 5 cards -> 2 front pages, each followed by a back page
        assert_eq!(pages.len(), 4);
        assert_eq!(pages[0].side, PageSide::Front);
        assert_eq!(pages[1].side, PageSide::Back);
        assert_eq!(pages[2].side, PageSide::Front);
        assert_eq!(pages[3].side, PageSide::Back);
        assert_eq!(
            pages.iter().map(|p| p.page_number).collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );

        // Backs use the default back image and mirror their front positions
        assert_eq!(pages[1].cards.len(), 4);
        for ((front, front_pos), (back, back_pos)) in pages[0].cards.iter().zip(&pages[1].cards) {
            assert_eq!(front.back_path, None);
            assert_eq!(back.path, PathBuf::from("default_back.png"));
            let expected = mirror_position_for_back(front_pos, &params);
            assert_eq!(back_pos.x, expected.x);
            assert_eq!(back_pos.y, expected.y);
        }

        // Partial last page: 1 front card -> 1 back card
        assert_eq!(pages[2].cards.len(), 1);
        assert_eq!(pages[3].cards.len(), 1);
    }

    #[test]
    fn test_duplex_per_card_back_overrides_default() {
        let mut cards = vec![
            Card::new(PathBuf::from("card1.jpg")),
            Card::new(PathBuf::from("card2.jpg")),
        ];
        cards[0].back_path = Some(PathBuf::from("special_back.png"));

        let grid = GridLayout {
            rows: 2,
            cols: 2,
            cards_per_page: 4,
            total_pages: 1,
        };

        let params = duplex_test_params();
        let pages = distribute_cards(&cards, &grid, &params);

        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].cards[0].0.path, PathBuf::from("special_back.png"));
        assert_eq!(pages[1].cards[1].0.path, PathBuf::from("default_back.png"));
    }

    #[test]
    fn test_duplex_without_any_back_emits_empty_back_page() {
        let cards = vec![Card::new(PathBuf::from("card1.jpg"))];

        let grid = GridLayout {
            rows: 1,
            cols: 1,
            cards_per_page: 1,
            total_pages: 1,
        };

        let params = LayoutParams {
            default_back_path: None,
            ..duplex_test_params()
        };
        let pages = distribute_cards(&cards, &grid, &params);

        // Back page still emitted (blank) so duplex pairing is preserved
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].side, PageSide::Back);
        assert!(pages[1].cards.is_empty());
    }

    #[test]
    fn test_duplex_uses_thumbnail_loaded_back_cards() {
        let cards = vec![Card::new(PathBuf::from("card1.jpg"))];

        let grid = GridLayout {
            rows: 1,
            cols: 1,
            cards_per_page: 1,
            total_pages: 1,
        };

        let params = duplex_test_params();
        let mut back_card = Card::placeholder(PathBuf::from("default_back.png"));
        back_card.set_thumbnail_failed("marker".to_string());
        let mut back_cards = HashMap::new();
        back_cards.insert(PathBuf::from("default_back.png"), back_card);

        let pages = distribute_cards_with_backs(&cards, &grid, &params, &back_cards);

        // The provided Card instance (not a fresh placeholder) is used
        assert!(matches!(
            pages[1].cards[0].0.thumbnail_state,
            crate::types::ThumbnailState::Failed(_)
        ));
    }

    #[test]
    fn test_non_duplex_pages_are_all_front() {
        let cards = vec![Card::new(PathBuf::from("card1.jpg"))];
        let grid = GridLayout {
            rows: 1,
            cols: 1,
            cards_per_page: 1,
            total_pages: 1,
        };
        let params = LayoutParams::default();
        let pages = distribute_cards(&cards, &grid, &params);

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].side, PageSide::Front);
    }

    #[test]
    fn test_centering_symmetry() {
        let params = LayoutParams {
            page_size: (200.0, 300.0),
            card_size: (40.0, 60.0),
            spacing: (5.0, 8.0),
            margins: Margins::uniform(10.0),
            page_orientation: PageOrientation::Portrait,
            center_layout: true,
            ..Default::default()
        };

        let grid = GridLayout {
            rows: 3,
            cols: 2,
            cards_per_page: 6,
            total_pages: 1,
        };

        let effective_margins = params.effective_margins(&grid);

        // Verify symmetry: left should equal right, top should equal bottom
        assert_eq!(effective_margins.left, effective_margins.right);
        assert_eq!(effective_margins.top, effective_margins.bottom);

        // Verify non-negative margins
        assert!(effective_margins.left >= 0.0);
        assert!(effective_margins.top >= 0.0);
    }

    #[test]
    fn test_cut_marks_overlap_past_trim_edges() {
        let params = LayoutParams {
            page_size: (200.0, 300.0),
            card_size: (40.0, 60.0),
            margins: Margins::uniform(10.0),
            spacing: (5.0, 8.0),
            ..Default::default()
        };
        let grid = calculate_grid(&params);
        let marks = calculate_cut_marks(&params, &grid);

        let grid_right = 10.0 + (grid.cols as f32 - 1.0) * 45.0 + 40.0;
        let grid_bottom = 10.0 + (grid.rows as f32 - 1.0) * 68.0 + 60.0;

        // The marks that run off the page reach the page edge at one end and
        // CUT_MARK_OVERLAP_MM past the outermost trim line at the other.
        let from_top = marks
            .iter()
            .find(|m| m.mark_type == CutMarkType::Vertical && m.y1 == 0.0)
            .expect("vertical mark from the top page edge");
        assert_eq!(from_top.y2, 10.0 + CUT_MARK_OVERLAP_MM);

        let to_bottom = marks
            .iter()
            .find(|m| m.mark_type == CutMarkType::Vertical && m.y2 == 300.0)
            .expect("vertical mark to the bottom page edge");
        assert_eq!(to_bottom.y1, grid_bottom - CUT_MARK_OVERLAP_MM);

        let from_left = marks
            .iter()
            .find(|m| m.mark_type == CutMarkType::Horizontal && m.x1 == 0.0)
            .expect("horizontal mark from the left page edge");
        assert_eq!(from_left.x2, 10.0 + CUT_MARK_OVERLAP_MM);

        let to_right = marks
            .iter()
            .find(|m| m.mark_type == CutMarkType::Horizontal && m.x2 == 200.0)
            .expect("horizontal mark to the right page edge");
        assert_eq!(to_right.x1, grid_right - CUT_MARK_OVERLAP_MM);

        // Every mark stays on its own axis
        for m in &marks {
            match m.mark_type {
                CutMarkType::Vertical => assert_eq!(m.x1, m.x2),
                CutMarkType::Horizontal => assert_eq!(m.y1, m.y2),
            }
        }
    }

    /// Whether a vertical and a horizontal mark cross at (x, y): each must span
    /// strictly past the other's line, not merely touch it.
    fn has_cross_at(marks: &[CutMark], x: f32, y: f32) -> bool {
        let vertical = marks.iter().any(|m| {
            m.mark_type == CutMarkType::Vertical
                && (m.x1 - x).abs() < 1e-3
                && m.y1.min(m.y2) < y - 1e-3
                && m.y1.max(m.y2) > y + 1e-3
        });
        let horizontal = marks.iter().any(|m| {
            m.mark_type == CutMarkType::Horizontal
                && (m.y1 - y).abs() < 1e-3
                && m.x1.min(m.x2) < x - 1e-3
                && m.x1.max(m.x2) > x + 1e-3
        });
        vertical && horizontal
    }

    #[test]
    fn test_cut_marks_cross_at_every_intersection() {
        // Multi-row, multi-column grid with spacing, so interior trim lines are
        // distinct from the grid's outer bounds.
        let params = LayoutParams {
            page_size: (200.0, 300.0),
            card_size: (40.0, 60.0),
            margins: Margins::uniform(10.0),
            spacing: (5.0, 8.0),
            ..Default::default()
        };
        let grid = calculate_grid(&params);
        assert!(
            grid.cols >= 2 && grid.rows >= 2,
            "need an interior intersection to test"
        );
        let marks = calculate_cut_marks(&params, &grid);

        let x_lines = cut_line_coords(10.0, grid.cols, 40.0, 5.0);
        let y_lines = cut_line_coords(10.0, grid.rows, 60.0, 8.0);

        // Both outer corners and interior intersections carry a cross
        for &x in &x_lines {
            for &y in &y_lines {
                assert!(
                    has_cross_at(&marks, x, y),
                    "no cross at trim intersection ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn test_cut_marks_cross_with_zero_spacing() {
        // Zero spacing makes adjacent cards share a trim line; the shared line
        // must still carry crosses at every row boundary.
        let params = LayoutParams {
            page_size: (200.0, 300.0),
            card_size: (40.0, 60.0),
            margins: Margins::uniform(10.0),
            spacing: (0.0, 0.0),
            ..Default::default()
        };
        let grid = calculate_grid(&params);
        let marks = calculate_cut_marks(&params, &grid);

        let x_lines = cut_line_coords(10.0, grid.cols, 40.0, 0.0);
        let y_lines = cut_line_coords(10.0, grid.rows, 60.0, 0.0);

        // Shared edges collapse: N cards yield N+1 lines, not 2N
        assert_eq!(x_lines.len(), grid.cols + 1);
        assert_eq!(y_lines.len(), grid.rows + 1);

        for &x in &x_lines {
            for &y in &y_lines {
                assert!(
                    has_cross_at(&marks, x, y),
                    "no cross at trim intersection ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn test_cut_line_coords_spacing_variants() {
        // With spacing, every card contributes two distinct lines
        assert_eq!(
            cut_line_coords(10.0, 3, 40.0, 5.0),
            vec![10.0, 50.0, 55.0, 95.0, 100.0, 140.0]
        );
        // Without spacing, shared edges collapse to one line each
        assert_eq!(
            cut_line_coords(10.0, 3, 40.0, 0.0),
            vec![10.0, 50.0, 90.0, 130.0]
        );
        // A single card is just its two edges
        assert_eq!(cut_line_coords(0.0, 1, 63.0, 2.0), vec![0.0, 63.0]);
    }

    #[test]
    fn test_cut_mark_overlap_capped_at_half_card() {
        // Cards smaller than 2 * CUT_MARK_OVERLAP_MM: the overlap must shrink so
        // marks from opposite trim edges can't meet in the middle of a card.
        let params = LayoutParams {
            page_size: (100.0, 100.0),
            card_size: (2.0, 3.0),
            margins: Margins::uniform(10.0),
            spacing: (1.0, 1.0),
            ..Default::default()
        };
        let grid = calculate_grid(&params);
        let marks = calculate_cut_marks(&params, &grid);

        let top_mark = marks
            .iter()
            .find(|m| m.mark_type == CutMarkType::Vertical && m.y1 == 0.0)
            .expect("vertical mark from top of page");
        assert_eq!(top_mark.y2, 10.0 + 1.5);

        let left_mark = marks
            .iter()
            .find(|m| m.mark_type == CutMarkType::Horizontal && m.x1 == 0.0)
            .expect("horizontal mark from left of page");
        assert_eq!(left_mark.x2, 10.0 + 1.0);
    }

    /// A4 with typical inkjet forced margins: a larger bottom edge where the
    /// feed grips the sheet.
    fn printer_margin_params() -> LayoutParams {
        LayoutParams {
            page_size: (210.0, 297.0),
            card_size: (63.0, 88.0),
            margins: Margins::uniform(5.0),
            spacing: (0.0, 0.0),
            page_orientation: PageOrientation::Portrait,
            enable_printer_margins: true,
            printer_margins: Margins {
                top: 3.0,
                right: 3.0,
                bottom: 15.0,
                left: 3.0,
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_grid_unaffected_when_user_margins_exceed_printer_margins() {
        // 10mm all round clears the printer's 3mm sides, so only the 15mm
        // bottom can bite - and here it doesn't change the row count
        let base = LayoutParams {
            margins: Margins::uniform(10.0),
            ..printer_margin_params()
        };
        let without = LayoutParams {
            enable_printer_margins: false,
            ..base.clone()
        };

        let with_printer = calculate_grid(&base);
        let no_printer = calculate_grid(&without);
        assert_eq!(with_printer.rows, no_printer.rows);
        assert_eq!(with_printer.cols, no_printer.cols);
    }

    #[test]
    fn test_grid_shrinks_when_printer_margins_exceed_user_margins() {
        // 5mm user margins vs a 15mm printer bottom: usable height drops from
        // 297 - 10 = 287mm (3 rows of 88) to 297 - 5 - 15 = 277mm (3 rows still
        // fit at 264mm, so widen the card to force the row out)
        let params = LayoutParams {
            card_size: (63.0, 95.0),
            ..printer_margin_params()
        };
        let without = LayoutParams {
            enable_printer_margins: false,
            ..params.clone()
        };

        // 297 - 10 = 287 fits 3 rows of 95 (285mm)
        assert_eq!(calculate_grid(&without).rows, 3);
        // 297 - 5 - 15 = 277 fits only 2
        assert_eq!(calculate_grid(&params).rows, 2);
    }

    #[test]
    fn test_cards_stay_clear_of_unprintable_border() {
        let params = printer_margin_params();
        let grid = calculate_grid(&params);
        let printer = params.effective_printer_margins();
        let (page_width, page_height) = params.effective_page_size();

        for position in generate_positions(&params, &grid) {
            assert!(position.x >= printer.left);
            assert!(position.y >= printer.top);
            assert!(position.x + params.card_size.0 <= page_width - printer.right);
            assert!(position.y + params.card_size.1 <= page_height - printer.bottom);
        }
    }

    #[test]
    fn test_cut_marks_stop_at_printable_bounds() {
        let params = printer_margin_params();
        let grid = calculate_grid(&params);
        let printer = params.effective_printer_margins();
        let (page_width, page_height) = params.effective_page_size();

        let marks = calculate_cut_marks(&params, &grid);
        assert!(!marks.is_empty());

        for mark in &marks {
            for x in [mark.x1, mark.x2] {
                assert!(x >= printer.left - f32::EPSILON, "mark x {x} left of band");
                assert!(x <= page_width - printer.right + f32::EPSILON);
            }
            for y in [mark.y1, mark.y2] {
                assert!(y >= printer.top - f32::EPSILON, "mark y {y} above band");
                assert!(y <= page_height - printer.bottom + f32::EPSILON);
            }
        }

        // The outermost marks still run all the way out, now to the printable
        // edge rather than the sheet edge
        assert!(marks
            .iter()
            .any(|m| m.mark_type == CutMarkType::Vertical && m.y1 == printer.top));
        assert!(marks
            .iter()
            .any(|m| m.mark_type == CutMarkType::Vertical && m.y2 == page_height - printer.bottom));
        assert!(marks
            .iter()
            .any(|m| m.mark_type == CutMarkType::Horizontal && m.x1 == printer.left));
        assert!(marks
            .iter()
            .any(|m| m.mark_type == CutMarkType::Horizontal && m.x2 == page_width - printer.right));
    }

    #[test]
    fn test_duplex_back_positions_stay_inside_printable_area() {
        // Short-edge flip on a portrait page mirrors y, so the asymmetric
        // top/bottom printer margins are what the mirror has to survive
        let params = LayoutParams {
            enable_duplex: true,
            flip_edge: FlipEdge::ShortEdge,
            ..printer_margin_params()
        };
        let grid = calculate_grid(&params);
        let printer = params.effective_printer_margins();
        let (_, page_height) = params.effective_page_size();

        assert!(!params.back_mirror_is_horizontal());

        for position in generate_positions(&params, &grid) {
            let back = mirror_position_for_back(&position, &params);
            assert!(
                back.y >= printer.top,
                "back at y {} is inside the {}mm top border",
                back.y,
                printer.top
            );
            assert!(
                back.y + params.card_size.1 <= page_height - printer.bottom,
                "back at y {} overruns the {}mm bottom border",
                back.y,
                printer.bottom
            );
        }
    }
}
