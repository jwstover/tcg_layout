use crate::types::{
    Card, CardPosition, CutMark, CutMarkType, FillOrder, GridLayout, LayoutParams, PageLayout,
    PageOrientation, PageSide,
};
use std::collections::HashMap;
use std::path::PathBuf;

pub fn calculate_grid(params: &LayoutParams) -> GridLayout {
    // Get effective page dimensions based on orientation
    let (page_width, page_height) = match params.page_orientation {
        PageOrientation::Portrait => params.page_size,
        PageOrientation::Landscape => (params.page_size.1, params.page_size.0), // Swap width and height
    };

    // When centering, ignore margins for grid calculation to maximize space
    let (margin_left, margin_right, margin_top, margin_bottom) = if params.center_layout {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        (
            params.margins.left,
            params.margins.right,
            params.margins.top,
            params.margins.bottom,
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

pub fn calculate_cut_marks(params: &LayoutParams, grid: &GridLayout) -> Vec<CutMark> {
    let mut cut_marks = Vec::new();

    // Get effective page dimensions based on orientation
    let (page_width, page_height) = match params.page_orientation {
        PageOrientation::Portrait => params.page_size,
        PageOrientation::Landscape => (params.page_size.1, params.page_size.0),
    };

    // Use effective margins for cut mark positioning
    let effective_margins = params.effective_margins(grid);
    let start_x = effective_margins.left;
    let start_y = effective_margins.top;
    let card_width_with_spacing = params.card_size.0 + params.spacing.0;
    let card_height_with_spacing = params.card_size.1 + params.spacing.1;

    // Calculate the bounds of the card grid (using trim positions, not bleed positions)
    // Note: When bleed is enabled, actual images extend beyond these bounds by bleed_mm
    let grid_bottom =
        start_y + (grid.rows as f32 - 1.0) * card_height_with_spacing + params.card_size.1;
    let grid_right =
        start_x + (grid.cols as f32 - 1.0) * card_width_with_spacing + params.card_size.0;

    // Vertical cut marks - at the left and right edge of each card
    for col in 0..grid.cols {
        let card_left_x = start_x + col as f32 * card_width_with_spacing;
        let card_right_x = card_left_x + params.card_size.0;

        // Left edge of card - extend from page edge to top margin, and from bottom margin to page edge
        cut_marks.push(CutMark {
            x1: card_left_x,
            y1: 0.0, // Top edge of page
            x2: card_left_x,
            y2: start_y, // Top margin edge
            mark_type: CutMarkType::Vertical,
        });
        cut_marks.push(CutMark {
            x1: card_left_x,
            y1: grid_bottom, // Bottom of card grid
            x2: card_left_x,
            y2: page_height, // Bottom edge of page
            mark_type: CutMarkType::Vertical,
        });

        // Right edge of card - extend from page edge to top margin, and from bottom margin to page edge
        cut_marks.push(CutMark {
            x1: card_right_x,
            y1: 0.0, // Top edge of page
            x2: card_right_x,
            y2: start_y, // Top margin edge
            mark_type: CutMarkType::Vertical,
        });
        cut_marks.push(CutMark {
            x1: card_right_x,
            y1: grid_bottom, // Bottom of card grid
            x2: card_right_x,
            y2: page_height, // Bottom edge of page
            mark_type: CutMarkType::Vertical,
        });
    }

    // Horizontal cut marks - at the top and bottom edge of each card
    for row in 0..grid.rows {
        let card_top_y = start_y + row as f32 * card_height_with_spacing;
        let card_bottom_y = card_top_y + params.card_size.1;

        // Top edge of card - extend from page edge to left margin, and from right margin to page edge
        cut_marks.push(CutMark {
            x1: 0.0, // Left edge of page
            y1: card_top_y,
            x2: start_x, // Left margin edge
            y2: card_top_y,
            mark_type: CutMarkType::Horizontal,
        });
        cut_marks.push(CutMark {
            x1: grid_right, // Right of card grid
            y1: card_top_y,
            x2: page_width, // Right edge of page
            y2: card_top_y,
            mark_type: CutMarkType::Horizontal,
        });

        // Bottom edge of card - extend from page edge to left margin, and from right margin to page edge
        cut_marks.push(CutMark {
            x1: 0.0, // Left edge of page
            y1: card_bottom_y,
            x2: start_x, // Left margin edge
            y2: card_bottom_y,
            mark_type: CutMarkType::Horizontal,
        });
        cut_marks.push(CutMark {
            x1: grid_right, // Right of card grid
            y1: card_bottom_y,
            x2: page_width, // Right edge of page
            y2: card_bottom_y,
            mark_type: CutMarkType::Horizontal,
        });
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
}
