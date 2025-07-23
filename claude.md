# TCG Card Layout Application

## Project Overview
Desktop application that automatically lays out Trading Card Game (TCG) card images on printable pages. Built in Rust for cross-platform compatibility with focus on macOS development.

## Development Environment

### Build Commands
- `cargo run --bin tcg_layout` - Run the application
- `cargo test` - Run all tests  
- `cargo clippy` - Run linter checks
- `cargo fmt` - Format code

### Dependencies
- **GUI**: egui 0.28 + eframe for cross-platform UI
- **Image**: image crate 0.25 with jpeg/png/tiff support
- **Async**: tokio for thumbnail loading
- **File dialogs**: rfd 0.14 for native file pickers
- **EXIF**: kamadak-exif 0.5 for DPI extraction
- **Cache**: lru 0.12 for thumbnail caching
- **SVG**: svg 0.14 for export generation

## Technology Stack
- **Language:** Rust
- **GUI Framework:** egui (immediate mode, simple for parameter forms and preview)
- **Image Processing:** `image` crate for format support and processing
- **Export:** `printpdf` for PDF generation, custom SVG generation
- **Async:** tokio for background thumbnail loading

## Core Requirements

### MVP Features
- Automatic grid-based card layout (no manual positioning)
- Multi-page support when cards don't fit on single page
- Export to SVG (with image references) and PDF
- Support for JPG, PNG, TIFF formats
- User-configurable parameters:
  - Page size (mm)
  - Card size (mm) 
  - Margins (mm)
  - Spacing between cards (mm)
  - Layout orientation (row-major vs column-major)
  - Target DPI for export

### Preview System
- Show actual card thumbnails in layout preview
- Lazy loading for performance with large card counts
- Page navigation (current page N of M)
- DPI warnings display

### Image Handling
- All cards in single layout must be same size
- DPI detection from EXIF data (default 72 if missing)
- If original DPI < target DPI: warn user but use original size (no upscaling)
- Thumbnail generation for preview, full-res processing only during export

## Current Implementation Status

### ✅ Completed Features
- Basic grid layout calculation with margins/spacing
- Multi-page card distribution
- Async thumbnail loading with LRU caching
- SVG export with image references
- UI panels for parameters, card list, and preview
- Copy count support for duplicate cards
- Page orientation support (portrait/landscape)

### 🚧 In Progress
- PDF export (printpdf dependency added but not implemented)
- DPI warning system (detection implemented, UI warnings pending)

### 📋 Planned
- Print dialog integration
- Bleed/crop marks
- Mixed card sizes
- Layout presets

## Architecture

### Current Module Structure
- `main.rs` - Application entry point and UI state management
- `types.rs` - Core data structures (LayoutParams, Card, GridLayout, etc.)
- `layout.rs` - Grid calculation and card distribution logic
- `image_processing.rs` - DPI detection and image metadata
- `thumbnail_manager.rs` - Async thumbnail loading with LRU cache
- `svg_export.rs` - SVG generation for print layouts
- `ui/` - UI component modules
  - `parameters_panel.rs` - Layout parameter input forms
  - `card_list_panel.rs` - Card selection and management
  - `preview_panel.rs` - Layout preview with thumbnails
- `bin/` - Utility binaries for testing and benchmarking

### Core Data Structures
```rust
struct LayoutParams {
    page_size: (f32, f32),      // mm
    card_size: (f32, f32),      // mm  
    margins: Margins,           // mm
    spacing: (f32, f32),        // mm
    orientation: FillOrder,     // RowMajor vs ColumnMajor
    target_dpi: u32,
}

struct Card {
    path: PathBuf,
    thumbnail: Option<ImageBuffer>, 
    original_dpi: Option<u32>,
    needs_scaling: bool,
}

struct ImageCache {
    thumbnails: LRU<PathBuf, ImageBuffer>,
    loading_queue: VecDeque<PathBuf>,
    background_loader: tokio::task::JoinHandle<()>,
}
```

### Key Components

**1. Layout Calculator (pure functions)**
- `calculate_grid()` - determine rows/cols from constraints
- `distribute_cards()` - split cards across multiple pages
- `generate_positions()` - calculate x,y coordinates for each card

**2. Image Manager**
- Lazy thumbnail loading with LRU cache
- Background loading thread
- DPI detection and warning generation
- Format detection and validation

**3. Preview Renderer (egui)**
- Custom widget for grid display
- Thumbnail rendering with placeholders
- Page navigation controls
- Parameter input forms

**4. Export Pipeline**
- SVG generation with image references (preserves original files)
- PDF generation with proper print dimensions
- Unit conversions: mm → pixels (display), mm → points (PDF)

## Implementation Details

### Units and Conversions
- All internal calculations in millimeters
- Screen display: `mm * screen_dpi / 25.4`
- PDF export: `mm * 72 / 25.4` (points)

### Performance Considerations
- Lazy thumbnail loading for large card sets
- LRU cache with memory pressure handling
- Only load full-resolution images during export
- Background processing for thumbnail generation

### Assumptions
- Single card always fits on page (no card > page size validation needed)
- All cards in layout are same physical size
- Desktop usage (image references in SVG vs embedding)

## Testing

### Test Structure
- Unit tests are colocated in each module using `#[cfg(test)]`
- Integration tests for layout calculations in `types.rs`
- Test utilities in `bin/` directory for development

### Running Tests
```bash
cargo test                    # All tests
cargo test types             # Specific module
cargo test --bin layout_demo # Binary tests
```

### Test Coverage Areas
- Layout grid calculations
- Card distribution across pages
- UI parameter validation
- Image metadata extraction
- Thumbnail caching behavior

## Development workflow

1. Plan steps to implement new feature / bugfix
2. Implemnt changes
3. Run tests
4. Come up with commit message and prompt for confirmation before committing

## Common Development Tasks

### Adding New Layout Parameters
1. Update `LayoutParams` struct in `types.rs`
2. Add UI controls in `parameters_panel.rs`
3. Update layout calculations in `layout.rs`
4. Add validation logic if needed
5. Update tests in `types.rs`

### Adding New Export Formats
1. Create new export module (e.g., `pdf_export.rs`)
2. Add export option to UI menu in `main.rs`
3. Implement format-specific positioning logic
4. Add unit conversion helpers
5. Update `lib.rs` module declarations

### Debugging Performance Issues
- Enable logging: `RUST_LOG=debug cargo run`
- Use benchmark binary: `cargo run --bin benchmark_cache`
- Profile thumbnail loading with `thumbnail_manager.rs` logs
- Monitor LRU cache hit rates

## Error Handling

### Current Patterns
- `anyhow::Result` for file I/O operations
- `Option<T>` for missing metadata (DPI, thumbnails)
- UI validation errors stored in `Vec<String>`
- Thumbnail loading failures tracked in `ThumbnailState::Failed`

### Adding New Error Types
1. Define error variants for specific failures
2. Use `anyhow::Context` for error chaining
3. Display user-friendly messages in UI
4. Log technical details with `log::error!`

## Code Organization Principles

### Separation of Concerns
- **UI modules** (`ui/`) handle only presentation logic
- **Core logic** (`layout.rs`, `types.rs`) is UI-agnostic
- **I/O operations** are isolated in dedicated modules
- **State management** centralized in main application struct

### Naming Conventions
- Structs: PascalCase (`LayoutParams`, `GridLayout`)
- Functions: snake_case (`calculate_grid`, `distribute_cards`)
- Constants: SCREAMING_SNAKE_CASE
- File names: snake_case matching primary type/function

## Future Enhancements (Post-MVP)
- Bleed and crop marks for print
- Print dialog integration
- Mixed card sizes support
- Additional export formats
- Layout templates/presets
