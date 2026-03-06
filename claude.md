# TCG Card Layout Application

## Project Overview
Desktop application that automatically lays out Trading Card Game (TCG) card images on printable pages. Built in Rust for cross-platform compatibility with focus on macOS development. The app provides a GUI for importing card images, configuring layout parameters, previewing the result, and exporting to SVG or PDF for printing.

## Development Workflow

1. Plan steps to implement new feature / bugfix
2. Implement changes
3. Run tests
4. Ensure that `cargo clippy` doesn't raise any warnings. **Fix all warnings**
5. Ensure that `cargo fmt` doesn't raise any errors

## Development Environment

### Build Commands
- `cargo run --bin tcg_layout` - Run the application
- `cargo test` - Run all tests (78 tests across all modules)
- `cargo clippy` - Run linter checks
- `cargo fmt` - Format code

### Dependencies
- **GUI**: egui 0.29 + eframe 0.29 (glow backend, persistence, accesskit)
- **Image**: image 0.25 with jpeg/png/tiff support
- **Async**: tokio 1.0 (full features) for background thumbnail loading
- **File dialogs**: rfd 0.14 for native file pickers
- **EXIF**: kamadak-exif 0.5 for DPI extraction
- **Cache**: lru 0.12 for thumbnail caching
- **SVG**: svg 0.14 for export generation
- **PDF**: printpdf 0.9 (images, jpeg, png, tiff features)
- **HTTP**: reqwest 0.12 (json, rustls-tls) for OpenAI API calls
- **Serialization**: serde 1.0 + serde_json 1.0
- **Credentials**: keyring 2.0 for secure API key storage
- **Directories**: dirs 5.0 for standard user paths
- **Logging**: log 0.4 + env_logger 0.11
- **Error handling**: anyhow 1.0
- **Dev**: tempfile 3.8 (dev-dependency for tests)

## Architecture

### Module Structure
```
src/
  main.rs              - Application entry point, TcgLayoutApp struct, UI state management
  lib.rs               - Public module declarations
  types.rs             - Core data structures (LayoutParams, Card, GridLayout, etc.)
  layout.rs            - Grid calculation, card distribution, position generation, cut marks
  image_processing.rs  - Thumbnail generation, DPI extraction from EXIF
  image.rs             - Image metadata, format detection, card type detection by aspect ratio
  bleed.rs             - Bleed edge extension using Gaussian blur on edges/corners
  thumbnail_manager.rs - Async thumbnail loading with LRU cache and file mod-time tracking
  svg_export.rs        - SVG export with cut marks, bleed, multi-page Inkscape layers
  pdf_export.rs        - PDF export with image deduplication, DPI scaling, bleed support
  settings.rs          - Settings persistence to ~/.config/tcg_layout/settings.json
  decklist.rs          - Decklist parsing and AI-powered card-to-file matching via OpenAI
  style.rs             - Custom dark mode theme configuration
  ui/
    mod.rs             - PageSizeOption and CardSizeOption enums with preset sizes
    parameters_panel.rs - Layout parameter input forms (page size, card size, margins, etc.)
    card_list_panel.rs  - Card import, removal, reordering, copy count management
    preview_panel.rs    - Grid preview with actual thumbnails and page navigation
    decklist_panel.rs   - Decklist text input, AI matching trigger, result application
  bin/
    layout_demo.rs     - Demonstrates 4 layout scenarios with utilization stats
    image_metadata.rs  - CLI utility for extracting image info
    benchmark_cache.rs - Cache performance benchmarking
```

### Core Data Structures

**LayoutParams** (`types.rs`) - All user-configurable layout settings:
```rust
struct LayoutParams {
    page_size: (f32, f32),           // (width, height) in mm
    card_size: (f32, f32),           // (width, height) in mm
    margins: Margins,                // top/right/bottom/left in mm
    spacing: (f32, f32),             // (horizontal, vertical) spacing in mm
    orientation: FillOrder,          // RowMajor or ColumnMajor
    page_orientation: PageOrientation, // Portrait or Landscape
    target_dpi: u32,                 // Target DPI for export
    bleed_mm: f32,                   // Bleed amount in mm
    enable_bleed: bool,              // Whether bleed is active
    center_layout: bool,             // Center cards on page (ignores margins)
}
```

**Card** (`types.rs`) - Represents a single card image:
```rust
struct Card {
    path: PathBuf,
    thumbnail_state: ThumbnailState,  // NotLoaded | Loading | Loaded(ImageBuffer) | Failed(String)
    original_dpi: Option<u32>,
    needs_scaling: bool,
    copy_count: u32,                  // Number of copies (default 1)
}
```

**GridLayout** (`types.rs`) - Result of grid calculation:
```rust
struct GridLayout {
    rows: usize,
    cols: usize,
    cards_per_page: usize,  // rows * cols
    total_pages: usize,
}
```

**PageLayout** (`types.rs`) - Cards positioned on a single page:
```rust
struct PageLayout {
    page_number: usize,
    cards: Vec<(Card, CardPosition)>,
}
```

**Other important types in `types.rs`:**
- `Margins` - top/right/bottom/left with `uniform()` and `Default` constructors
- `FillOrder` - `RowMajor` or `ColumnMajor` enum
- `PageOrientation` - `Portrait` or `Landscape` enum
- `CardPosition` - x/y coordinates in mm
- `DpiWarning` - warning info for low-DPI cards
- `CutMark` / `CutMarkType` - cut mark positions and types for print
- `ThumbnailState` - 4-variant enum tracking thumbnail lifecycle

**UI preset enums in `ui/mod.rs`:**
- `PageSizeOption` - A4, USLetter, A3, Custom (each with `get_size()` returning mm)
- `CardSizeOption` - Poker, Bridge, Tarot, Custom (each with `get_size()` returning mm)

### Key Components

**1. Layout Calculator** (`layout.rs` - pure functions, no side effects)
- `calculate_grid(params)` -> `GridLayout` - Determines rows/cols from page constraints. When centering is enabled, margins are ignored for grid calculation. Handles portrait/landscape. Ensures minimum 1x1 grid.
- `distribute_cards(cards, params)` -> `Vec<PageLayout>` - Expands cards by copy_count, chunks into pages, calculates positions per card.
- `calculate_card_position(index, grid, params)` -> `CardPosition` - Computes x,y for a card at a given index. Uses effective_margins() and respects fill order.
- `generate_positions(grid, params)` -> `Vec<CardPosition>` - All positions for one page.
- `calculate_cut_marks(grid, params)` -> `Vec<CutMark>` - Generates vertical and horizontal cut marks at card edges, extending to page boundaries.

**2. Thumbnail Manager** (`thumbnail_manager.rs`)
- Async background loading via tokio::spawn with blocking tasks
- LRU cache with configurable capacity (default 100, app uses 200)
- Cache key includes: path, file modification time, bleed_enabled, bleed_mm (rounded)
- Avoids duplicate requests for the same file
- Message-passing architecture: `ThumbnailRequest` -> background task -> `ThumbnailMessage`
- Key methods: `request_thumbnail()`, `try_recv_message()`, `cache_stats()`, `clear_cache()`

**3. Bleed System** (`bleed.rs`)
- `apply_bleed()` - Applies bleed to full-resolution images during export
- `apply_bleed_to_thumbnail()` - Applies bleed to thumbnail ImageBuffer for preview
- Uses Gaussian blur on edge strips for smooth bleed transitions
- Handles edges (top/bottom/left/right) and corners separately
- `calculate_bleed_pixels_from_dimensions()` - Converts bleed_mm to pixel count based on actual image dimensions vs card_size

**4. Image Processing** (`image_processing.rs` + `image.rs`)
- `generate_thumbnail()` - Creates RGBA8 thumbnail preserving aspect ratio
- `get_image_dpi()` - Extracts DPI from EXIF data
- `ImageMetadata` struct with format, dimensions, DPI, file size
- `extract_dpi_from_metadata()` - Cascades through EXIF, PNG pHYs chunk, TIFF tags
- `CardDimensions` with standard presets: Poker, Bridge, Tarot, Business, Mini
- `detect_card_type_by_aspect_ratio()` - Matches image aspect ratio to known card types

**5. Export Pipeline**
- **SVG** (`svg_export.rs`): `SvgExporter` with `export_page()`, `export_pages()`, `export_pages_single_file()`. Supports cut marks, bleed (processes images into `{name}_bleed/` directory), multi-page with Inkscape layer definitions. Uses file:// URI image references (not embedded).
- **PDF** (`pdf_export.rs`): `PdfExporter` with `export_pages()`. Uses printpdf 0.9. Features image deduplication via HashMap cache, intelligent DPI calculation to fill card width, Y-axis correction (PDF origin is bottom-left), aspect ratio correction via scale_y, bleed image processing (encoded to PNG in-memory), cut marks (gray 0.5pt lines), Flate compression.

**6. Settings** (`settings.rs`)
- Persists to `~/.config/tcg_layout/settings.json`
- Stores: layout_params, page_size_option, card_size_option
- OpenAI API key stored securely via system keyring (keyring crate)
- Graceful defaults when settings file is missing

**7. Decklist Matching** (`decklist.rs`)
- Parses decklist text (format: "N CardName", skips comments/empty lines)
- AI-powered matching via OpenAI GPT API: sends card names + available filenames
- `DecklistEntry` (name, count) and `MatchedCard` (path, confidence) structs
- Async background task for non-blocking UI
- Results applied to card list with proper copy counts

**8. UI** (`main.rs` + `ui/`)
- `TcgLayoutApp` is the main application struct implementing `eframe::App`
- Three-panel layout: Left (card list + decklist), Center (preview), Right (parameters)
- Polls for async messages each frame: thumbnail results, AI matching results
- Bleed setting changes trigger texture cache clear and full thumbnail re-request
- Custom dark theme defined in `style.rs`

### Application State Flow
1. User imports card images -> `Card::new()` extracts DPI synchronously
2. App calls `thumbnail_manager.request_thumbnail()` for each card
3. Manager checks LRU cache (keyed by path + mod_time + bleed settings)
4. Cache miss -> spawns tokio blocking task -> `generate_thumbnail()` + optional `apply_bleed_to_thumbnail()`
5. Result sent via channel -> `ThumbnailMessage::ThumbnailLoaded`
6. Main loop polls `try_recv_message()`, updates card's `ThumbnailState`
7. Preview panel renders thumbnails at calculated grid positions
8. On export: full-resolution images loaded, bleed applied if enabled, positioned on pages

### Units and Conversions
- All internal calculations in **millimeters**
- Screen display: `mm * screen_dpi / 25.4`
- PDF export: `mm * 72 / 25.4` (PDF points)
- Bleed: `bleed_mm * image_pixels / card_size_mm` (pixel conversion)

## Current Implementation Status

### Completed
- Grid layout calculation with margins, spacing, centering
- Multi-page card distribution with copy count expansion
- Row-major and column-major fill orders
- Portrait and landscape page orientations
- Async thumbnail loading with LRU cache (mod-time aware)
- SVG export with image references, cut marks, bleed, multi-page Inkscape layers
- PDF export with image deduplication, DPI scaling, bleed, cut marks
- Bleed system with Gaussian blur edge extension
- Cut marks generation
- Layout centering (ignores margins, centers grid on page)
- Settings persistence (JSON file + secure keyring for API key)
- Decklist parsing and AI-powered card-to-file matching (OpenAI)
- Card reordering in the UI
- DPI detection from EXIF/PNG pHYs/TIFF metadata
- Custom dark UI theme
- Card type detection by aspect ratio

### Not Yet Implemented
- Print dialog integration (direct OS print)
- Mixed card sizes within a single layout
- Layout templates/presets
- DPI warning display in the UI (detection works, UI display not wired up)

## Testing

### Test Structure
- Unit tests colocated in each module using `#[cfg(test)]`
- 78 tests total across all modules
- Async tests in `thumbnail_manager.rs` use tokio runtime

### Running Tests
```bash
cargo test                    # All tests
cargo test types              # Types module tests
cargo test layout             # Layout calculation tests
cargo test bleed              # Bleed processing tests
cargo test thumbnail          # Thumbnail manager tests
cargo test svg                # SVG export tests
cargo test pdf                # PDF export tests
cargo test settings           # Settings persistence tests
```

### Test Coverage Areas
- Layout grid calculations (29 tests): grid sizing, distribution, positions, centering, cut marks
- Image processing (6 tests): thumbnails, aspect ratio, DPI
- Bleed (11 tests): pixel calculations, edge replication, zero bleed
- Thumbnail manager (7 tests): async loading, caching, deduplication
- SVG export (11 tests): single/multi-page, cut marks, bleed
- PDF export (6 tests): multi-page, real images
- Settings (4 tests): serialization, defaults
- Types (various): enum behavior, defaults, validation

## Common Development Tasks

### Adding New Layout Parameters
1. Add field to `LayoutParams` in `types.rs` (include in `Default` impl and serialization)
2. Add UI control in `ui/parameters_panel.rs`
3. Update layout calculations in `layout.rs` if the parameter affects positioning
4. Update `settings.rs` serialization if needed (ensure backwards-compatible defaults)
5. Add tests
6. If it affects thumbnails (like bleed), update `ThumbnailManager`'s `CacheKey`

### Adding New Export Formats
1. Create new export module (e.g., `png_export.rs`)
2. Declare module in `lib.rs`
3. Add export option to UI in `main.rs`
4. Implement format-specific positioning (reference `pdf_export.rs` for coordinate systems)
5. Handle bleed if applicable (see `bleed.rs`)
6. Add unit tests

### Adding New Card Size Presets
1. Add variant to `CardSizeOption` in `ui/mod.rs`
2. Implement `get_size()` returning `(width_mm, height_mm)`
3. Update `display_name()` for the UI label
4. Optionally add to `CardDimensions` in `image.rs` for aspect ratio detection

### Modifying Bleed Behavior
- Bleed affects both thumbnails (preview) and full-res images (export)
- `CacheKey` in `thumbnail_manager.rs` includes bleed settings - changes invalidate cache
- `main.rs` clears texture cache and re-requests all thumbnails when bleed settings change
- Edge blur uses Gaussian convolution - parameters are in `bleed.rs`

## Error Handling Patterns
- `anyhow::Result` for file I/O and export operations
- `Option<T>` for missing metadata (DPI, thumbnails)
- `ThumbnailState::Failed(String)` for thumbnail loading errors
- Settings loading falls back to defaults on any error
- Export errors surfaced to user via UI (not panics)

## Code Organization Principles
- **UI modules** (`ui/`) handle only presentation logic
- **Core logic** (`layout.rs`, `types.rs`) is UI-agnostic and purely functional
- **I/O operations** isolated in dedicated modules (`image_processing.rs`, `settings.rs`)
- **State management** centralized in `TcgLayoutApp` struct in `main.rs`
- **Async work** uses message-passing (channels), not shared mutable state
- Structs: PascalCase, Functions: snake_case, Constants: SCREAMING_SNAKE_CASE
- File names: snake_case matching primary type/function

## Important Implementation Notes for Future Work

### Coordinate Systems
- Layout calculations use top-left origin (0,0 at top-left of page)
- PDF uses bottom-left origin - `pdf_export.rs` applies Y-axis correction: `page_height - y - card_height`
- SVG uses top-left origin (matches layout calculations directly)

### Thumbnail Cache Invalidation
- Cache keys include file modification time, so editing an image file on disk automatically invalidates its cache entry
- Bleed settings (enabled + mm amount) are part of the cache key - changing bleed triggers full cache miss
- The app must call `clear_cache()` and re-request thumbnails when bleed settings change (done in `main.rs`)

### Image Deduplication in PDF Export
- PDF exporter maintains a `HashMap<PathBuf, (printpdf::Image, dimensions)>` to avoid embedding the same image multiple times (important when copy_count > 1)

### Multi-page SVG Export
- `export_pages_single_file()` uses Inkscape-specific XML attributes for page definitions and layer grouping
- Standard SVG viewers will show all pages stacked; Inkscape renders them as separate pages

### OpenAI Integration
- API key stored in system keyring via `keyring` crate (service: "tcg_layout", user: "openai_api_key")
- Decklist matching runs as async background task to avoid blocking UI
- Results delivered via message channel, similar to thumbnail loading pattern
