use crate::types::{LayoutParams, GridLayout, Card, CardPosition, PageLayout, FillOrder};

pub fn calculate_grid(params: &LayoutParams) -> GridLayout {
    let available_width = params.page_size.0 - params.margins.left - params.margins.right;
    let available_height = params.page_size.1 - params.margins.top - params.margins.bottom;
    
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

pub fn distribute_cards(cards: &[Card], grid: &GridLayout, params: &LayoutParams) -> Vec<PageLayout> {
    let mut pages = Vec::new();
    let card_chunks = cards.chunks(grid.cards_per_page);
    let mut page_number = 1;
    
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
        });
        
        page_number += 1;
    }
    
    pages
}

pub fn calculate_card_position(card_index: usize, grid: &GridLayout, params: &LayoutParams) -> CardPosition {
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
    
    let start_x = params.margins.left;
    let start_y = params.margins.top;
    
    CardPosition {
        x: start_x + col as f32 * (params.card_size.0 + params.spacing.0),
        y: start_y + row as f32 * (params.card_size.1 + params.spacing.1),
    }
}

pub fn generate_positions(params: &LayoutParams, grid: &GridLayout) -> Vec<CardPosition> {
    let mut positions = Vec::new();
    
    let start_x = params.margins.left;
    let start_y = params.margins.top;
    
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{LayoutParams, Margins, FillOrder};
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
            target_dpi: 300,
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
            target_dpi: 300,
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
            target_dpi: 300,
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
        assert_eq!(positions[0].x, 5.0);  // start_x
        assert_eq!(positions[0].y, 5.0);  // start_y
        
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
            target_dpi: 300,
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
        assert_eq!(positions[0].x, 5.0);  // start_x
        assert_eq!(positions[0].y, 5.0);  // start_y
        
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
}