use tcg_layout::layout::{calculate_grid, generate_positions};
use tcg_layout::types::{FillOrder, LayoutParams, Margins, PageOrientation};

fn main() {
    println!("TCG Card Layout Demo");
    println!("===================");
    println!();

    // Demo 1: Default parameters (A4, poker cards, row-major)
    println!("Demo 1: Default Layout (A4, Poker Cards, Row-Major)");
    println!("---------------------------------------------------");
    demo_layout(LayoutParams::default(), 12);
    println!();

    // Demo 2: Custom parameters with column-major layout
    println!("Demo 2: Custom Layout (100x150mm page, 20x30mm cards, Column-Major)");
    println!("--------------------------------------------------------------------");
    let custom_params = LayoutParams {
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
    demo_layout(custom_params, 8);
    println!();

    // Demo 3: Tight layout with minimal spacing
    println!("Demo 3: Tight Layout (A4, minimal spacing, 12 cards)");
    println!("----------------------------------------------------");
    let tight_params = LayoutParams {
        page_size: (210.0, 297.0),
        card_size: (50.0, 70.0),
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
    demo_layout(tight_params, 12);
    println!();

    // Demo 4: Landscape orientation
    println!("Demo 4: Landscape Layout (A4 landscape, Poker Cards, Row-Major)");
    println!("---------------------------------------------------------------");
    let landscape_params = LayoutParams {
        page_size: (210.0, 297.0), // A4 but will be used as landscape
        card_size: (63.0, 88.0),   // Poker card
        margins: Margins::uniform(10.0),
        spacing: (2.0, 2.0),
        orientation: FillOrder::RowMajor,
        page_orientation: PageOrientation::Landscape,
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
    demo_layout(landscape_params, 12);
}

fn demo_layout(params: LayoutParams, num_cards: usize) {
    // Calculate grid layout
    let grid = calculate_grid(&params);

    // Print layout information
    println!("Layout Parameters:");
    println!(
        "  Page Size: {:.1} x {:.1} mm",
        params.page_size.0, params.page_size.1
    );
    println!(
        "  Card Size: {:.1} x {:.1} mm",
        params.card_size.0, params.card_size.1
    );
    println!(
        "  Margins: T:{:.1} R:{:.1} B:{:.1} L:{:.1} mm",
        params.margins.top, params.margins.right, params.margins.bottom, params.margins.left
    );
    println!(
        "  Spacing: {:.1} x {:.1} mm",
        params.spacing.0, params.spacing.1
    );
    println!("  Fill Order: {:?}", params.orientation);
    println!("  Page Orientation: {:?}", params.page_orientation);
    println!();

    println!("Grid Calculation:");
    println!("  Grid: {} rows x {} columns", grid.rows, grid.cols);
    println!("  Cards per page: {}", grid.cards_per_page);
    println!(
        "  Total pages needed for {} cards: {}",
        num_cards,
        num_cards.div_ceil(grid.cards_per_page)
    );
    println!();

    // Generate positions for first page
    let positions = generate_positions(&params, &grid);
    let cards_on_first_page = num_cards.min(grid.cards_per_page);

    println!("Card Positions (first page, {cards_on_first_page} cards):");
    for (i, position) in positions.iter().enumerate().take(cards_on_first_page) {
        println!(
            "  Card {}: ({:.1}, {:.1}) mm",
            i + 1,
            position.x,
            position.y
        );
    }

    // Show available space utilization
    let (page_width, page_height) = match params.page_orientation {
        PageOrientation::Portrait => params.page_size,
        PageOrientation::Landscape => (params.page_size.1, params.page_size.0),
    };
    let available_width = page_width - params.margins.left - params.margins.right;
    let available_height = page_height - params.margins.top - params.margins.bottom;
    let used_width =
        grid.cols as f32 * params.card_size.0 + (grid.cols - 1) as f32 * params.spacing.0;
    let used_height =
        grid.rows as f32 * params.card_size.1 + (grid.rows - 1) as f32 * params.spacing.1;

    println!();
    println!("Space Utilization:");
    println!("  Available: {available_width:.1} x {available_height:.1} mm");
    println!("  Used: {used_width:.1} x {used_height:.1} mm");
    println!(
        "  Utilization: {:.1}% width, {:.1}% height",
        (used_width / available_width) * 100.0,
        (used_height / available_height) * 100.0
    );
}
