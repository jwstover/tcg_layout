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
- `cargo test` - Run all tests (~351 test executions; shared modules run in both the lib and bin trees)
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
  sharpen.rs           - Luminance-only unsharp mask (amount, radius, threshold)
  color_adjust.rs      - Targeted HSL adjustments (hue band + feather + chroma gating, front/back scope) and dropper color sampling
  thumbnail_manager.rs - Async thumbnail loading with LRU cache and file mod-time tracking
  svg_export.rs        - SVG export with cut marks, bleed/sharpen processing, multi-page Inkscape layers
  pdf_export.rs        - PDF export with image deduplication, DPI scaling, bleed/sharpen support
  project.rs           - Project save/load: layout params + card list (paths only) as JSON (.tcgproj)
  settings.rs          - Settings persistence to ~/.config/tcg_layout/settings.json (incl. recent projects list)
  decklist.rs          - Decklist parsing and AI-powered card-to-file matching via OpenAI
  style.rs             - Custom dark mode theme configuration
  ui/
    mod.rs             - PageSizeOption and CardSizeOption enums with preset sizes
    parameters_panel.rs - Layout parameter input forms (page size, card size, margins, etc.)
    card_list_panel.rs  - Card import, removal, reordering, copy count management
    preview_panel.rs    - Grid preview with actual thumbnails and page navigation
    decklist_panel.rs   - Decklist text input, AI matching trigger, result application
    sharpen_preview.rs  - Full-resolution sharpen preview window with async processing
    color_adjust_preview.rs - Color adjustment editor window with dropper sampling, live preview, and front/back navigation
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
    sharpen_amount: f32,             // Unsharp mask strength (0.0-3.0, serde default 1.0)
    sharpen_radius: f32,             // Unsharp mask Gaussian sigma in px at full res (0.1-3.0, serde default 0.7)
    sharpen_threshold: f32,          // Local contrast below this fraction is left alone (0.0-0.2, serde default 0.02)
    enable_sharpen: bool,            // Whether sharpening is active (serde default false)
    center_layout: bool,             // Center cards on page (ignores margins)
    printer_margins: Margins,        // Printer's unprintable border in mm (serde default 5mm uniform)
    enable_printer_margins: bool,    // Whether to lay out and export for the printable area (serde default false)
    hsl_adjustments: Vec<HslAdjustment>, // Targeted color adjustments (serde default empty)
    enable_color_adjust: bool,       // Whether color adjustments are active (serde default false)
    enable_duplex: bool,             // Double-sided: generate a back page after each front page (serde default false)
    flip_edge: FlipEdge,             // LongEdge or ShortEdge; mirror axis for back pages (serde default LongEdge)
    back_offset: (f32, f32),         // (x, y) mm shift on back pages for printer duplex calibration (serde default 0,0)
    default_back_path: Option<PathBuf>, // Back image used when a card has no specific back (serde default None)
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
    back_path: Option<PathBuf>,       // Per-card back image (overrides default_back_path)
}
```
`Card::new(path)` reads DPI from disk; `Card::placeholder(path)` does no I/O (used for generated back slots).

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
    side: PageSide,  // Front or Back (always Front when duplex is off)
}
```

**Other important types in `types.rs`:**
- `Margins` - top/right/bottom/left with `uniform()` and `Default` constructors
- `FillOrder` - `RowMajor` or `ColumnMajor` enum
- `PageOrientation` - `Portrait` or `Landscape` enum
- `FlipEdge` - `LongEdge` or `ShortEdge` duplex flip axis (serializable)
- `PageSide` - `Front` or `Back`, defaults to `Front`
- `CardPosition` - x/y coordinates in mm
- `DpiWarning` - warning info for low-DPI cards
- `CutMark` / `CutMarkType` - cut mark positions and types for print
- `ThumbnailState` - 4-variant enum tracking thumbnail lifecycle

**UI preset enums in `ui/mod.rs`:**
- `PageSizeOption` - A4, USLetter, A3, Custom (each with `get_size()` returning mm)
- `CardSizeOption` - Poker, Bridge, Tarot, Custom (each with `get_size()` returning mm)

### Key Components

**1. Layout Calculator** (`layout.rs` - pure functions, no side effects)
- `calculate_grid(params)` -> `GridLayout` - Determines rows/cols from page constraints. When centering is enabled, margins are ignored for grid calculation. Handles portrait/landscape. Ensures minimum 1x1 grid. Printer margins (see below) are a floor on the user's margins, since a card laid out inside the unprintable border would be clipped off the print.
- `distribute_cards(cards, params)` -> `Vec<PageLayout>` - Expands cards by copy_count, chunks into pages, calculates positions per card. Delegates to `distribute_cards_with_backs` with an empty back map.
- `distribute_cards_with_backs(cards, grid, params, back_cards)` -> `Vec<PageLayout>` - Same, but when `enable_duplex` is on, emits a `PageSide::Back` page after every front page (interleaved for duplex printing). Back slots use `card.back_path` falling back to `params.default_back_path`; cards with neither leave a gap, and an empty back page is still emitted to preserve front/back pairing. `back_cards: &HashMap<PathBuf, Card>` supplies thumbnail-loaded Card instances (missing paths get `Card::placeholder`).
- `mirror_position_for_back(position, params)` -> `CardPosition` - Mirrors a front position onto the back page for the configured flip edge (long-edge flip mirrors x on portrait pages, y on landscape; short-edge is the opposite), then adds `back_offset`. The axis decision lives in `LayoutParams::back_mirror_is_horizontal()`.
- When the mirror is vertical (horizontal-axis flip), back images must also print rotated 180° or cut cards come out head-to-toe. `LayoutParams::backs_rotated_180()` encodes this; exporters and the preview rotate back-page images when `page.side == Back && backs_rotated_180()` (PDF rotates pixels via `image::rotate180` with the dedup cache keyed by `(path, rotated)`; SVG uses a `rotate(180 cx cy)` transform; the preview inverts texture UVs).
- `calculate_card_position(index, grid, params)` -> `CardPosition` - Computes x,y for a card at a given index. Uses effective_margins() and respects fill order.
- `generate_positions(grid, params)` -> `Vec<CardPosition>` - All positions for one page.
- `calculate_cut_marks(grid, params)` -> `Vec<CutMark>` - Generates cut marks for **every** intersection of a vertical and a horizontal trim line, interior ones included. `cut_line_coords()` builds the distinct trim coordinates per axis (adjacent cards with zero spacing share an edge, so duplicates collapse within `CUT_LINE_EPSILON_MM`); each trim line then gets one segment straddling every crossing line, reaching `CUT_MARK_OVERLAP_MM` (2.0, capped at half the card dimension) past it on both sides. The outermost segments run on out to the page edges — or the printable area's edges when printer margins are configured — instead of stopping at the overlap. Result: a cross at every card corner, so cutting along one line always leaves a stub of every mark perpendicular to it on both sides of the blade. With nonzero spacing a gutter has two distinct trim lines, so the four adjacent card corners render as a hash rather than a single cross; butted cards (zero spacing) give one clean cross. Because the overlap reaches into the card area — and because bleed images cover the margins — all three renderers (SVG, PDF, preview) draw cut marks **after/on top of** the card images.

**1b. Printer Margins** (`types.rs` + all three renderers)

Many printers physically cannot print to the sheet edge, and drivers deal with
that by scaling and re-centering the page into the printable area — which shrinks
the cards (fatal for real card sizes) and shifts the layout off center.
`enable_printer_margins` + `printer_margins` fix this by making the exported page
*be* the printable area, so there is nothing left for the driver to fit: "fit to
printable area" scales by exactly 1.0 and centers exactly.

- Layout math stays in **physical sheet coordinates** throughout (grid, positions, centering, cut marks, duplex mirroring). Only the exporters re-express coordinates, translating by `printable_origin()` at draw time. This keeps duplex mirroring correct — the sheet flips about the *sheet's* center, not the printable area's — and lets the preview keep showing the real sheet.
- `effective_printer_margins()` - the configured border, or all zeros when disabled. Read against the oriented page (as the sheet comes out of the printer), so they are not rotated for landscape.
- `printable_size()` - `effective_page_size()` minus the border, floored at `MIN_PRINTABLE_MM`. **This is the page size both exporters emit.**
- `printable_origin()` - `(left, top)`, the sheet-to-printable-area translation the exporters apply to every card and cut mark.
- `layout_printer_margins()` - the border the *layout* keeps clear. Same as the printable border, except with duplex on it is symmetrized along the mirror axis: a back page is the front mirrored about the sheet while the unprintable border does **not** mirror with it, so a card only prints in register if its mirror image also lands inside the printable area. That region is the intersection of the printable area and its own mirror, i.e. the larger of the two facing margins on both sides.
- `effective_margins(grid)` floors each side at `layout_printer_margins()`. Consequence worth knowing: turning printer margins on **cannot change an existing layout** unless the printer's border actually exceeds the user's margins on some side.
- With centering on, the grid is centered on the physical **sheet**, then clamped inside the printable band (`center_offset()`). These differ only when the printer's margins are asymmetric — which is common, since the feed edge is usually the largest. If the grid is taller/wider than the band at all, the near edge is kept clear so the overflow clips on one side rather than both.
- Cut marks' outermost runs stop at the printable boundary rather than the sheet edge, so no mark is clipped mid-print and an off-center printable area doesn't clip them asymmetrically.
- The preview still draws the whole sheet and shades the unprintable border over the cards, so it's visible both where the printer can't reach and that nothing was laid out there.
- **Not** part of the thumbnail `CacheKey` — printer margins never change a card's pixels.

**2. Thumbnail Manager** (`thumbnail_manager.rs`)
- Async background loading via tokio::spawn with blocking tasks
- LRU cache with configurable capacity (default 100, app uses 200)
- Requests take a `ThumbnailParams` struct (defined in `image_processing.rs`, built via `ThumbnailParams::from_layout()`) carrying bleed, sharpen, and card size settings
- Cache key includes: path, file modification time, bleed_enabled, bleed_mm (rounded to tenths), sharpen_enabled, sharpen_amount (rounded to hundredths)
- Avoids duplicate requests for the same file
- Message-passing architecture: `ThumbnailRequest` -> background task -> `ThumbnailMessage`
- Key methods: `request_thumbnail()`, `try_recv_message()`, `cache_stats()`, `clear_cache()`

**3. Bleed System** (`bleed.rs`)
- `apply_bleed()` - Applies bleed to full-resolution images during export
- `apply_bleed_to_thumbnail()` - Applies bleed to thumbnail ImageBuffer for preview
- Uses Gaussian blur on edge strips for smooth bleed transitions
- Handles edges (top/bottom/left/right) and corners separately
- `calculate_bleed_pixels_from_dimensions()` - Converts bleed_mm to pixel count based on actual image dimensions vs card_size
- `apply_bleed_to_image()` - Applies bleed to an already-loaded DynamicImage (used when sharpening runs first)

**3c. Color Adjustment System** (`color_adjust.rs`)
- `HslAdjustment` struct: `target_hue` (0-360°), `hue_range` (full-effect half-width), `feather` (smoothstep falloff beyond the band), `hue_shift` (±180°), `saturation_shift` / `lightness_shift` (±1.0, additive), `enabled` (toggle without deleting; serde default true), `scope` (`AdjustmentScope`: All / FrontsOnly / BacksOnly; serde default All). `is_active()` = enabled && has a non-zero shift; `is_active_for(is_back)` also checks scope — exporters use the latter
- `AdjustmentScope` limits an adjustment to card fronts or duplex back images. `adjustments_for_side(adjs, is_back)` filters the list before applying. Exporters are side-aware: the PDF dedup cache key adds an is_back component (only when some active adjustment is side-scoped), and SVG back images processed with side-scoped adjustments get a `{stem}_back_processed.png` filename so a same-file front/back can't collide. A backs-only adjustment leaves fronts completely unprocessed (original file references / raw bytes)
- `apply_hsl_adjustments()` for RGBA buffers, `apply_hsl_adjustments_to_image()` for DynamicImage (export). One HSL conversion per pixel; all adjustments applied in that single pass so multiple adjustments don't accumulate quantization error
- Weight = hue-band falloff × chroma gate: circular hue distance handles the 0°/360° red seam; the chroma gate (dead zone at 0.03, full effect at 0.12) fades the effect near neutral — gray, near-black, and near-white alike. HSL saturation is deliberately NOT the gate metric (it saturates to 1.0 near black/white)
- `sample_region()` implements the dropper: chroma-weighted circular mean hue over a small patch, plus a `suggested_range` derived from the circular spread
- **Intentionally NOT applied to thumbnails** — no `ThumbnailManager`/`CacheKey` involvement. The effect is visible only in the editor window (`ui/color_adjust_preview.rs`) and in exports
- Editor window mirrors the sharpen preview pattern (2048px cap, one task in flight, latest adjustments win) with add/remove adjustment rows, per-row scope dropdown, a "Pick" dropper mode (click the image to set target hue + range, live hover swatch), hold-to-compare, zoom, "Apply adjustments". It pages through all unique card fronts then all unique backs (◀/▶, wrapping) via `PreviewEntry { path, is_back }`; a `generation` counter discards late results from a previous image, and only adjustments whose scope covers the current side are applied to the preview
- Export order everywhere: **color adjust → sharpen → bleed** (color ops see original colors, bleed derives from the fully processed image)

**3b. Sharpening System** (`sharpen.rs`)
- Unsharp mask: `result = original + amount * (original - blur(original, sigma))`
- `SharpenParams { amount, radius, threshold }` bundles the three knobs; built via `LayoutParams::sharpen_params()` (which lives in `types.rs` and uses the absolute `tcg_layout::sharpen` path, because `types` compiles into both the lib and bin trees while `sharpen` only exists in the lib)
- `apply_sharpen_to_buffer()` for RGBA buffers (thumbnails, preview), `apply_sharpen()` for DynamicImage (export). Both take `&SharpenParams`
- **Luminance-only**: the correction is computed from Rec. 601 luma and added equally to R, G and B. Sharpening channels independently shifts hue along coloured edges (red/cyan fringing on card art)
- `threshold` skips pixels whose local contrast is below the given fraction of the tonal range, keeping sharpening off flat areas and out of scanner noise
- `radius` is a Gaussian sigma **in pixels at the resolution being processed**. `SharpenParams::scaled(factor)` shrinks it for a resized copy; `generate_thumbnail()` applies this so a full-res radius is not ~10x too wide on a 150px thumbnail. At thumbnail scale the scaled radius is usually below `NEGLIGIBLE_RADIUS` and sharpening no-ops, which is correct - it is not visible at 150px, so the full-resolution preview window is the only place to judge it
- `is_active()` gates on both amount and radius; exporters call it via `sharpen_active()`
- `MAX_SHARPEN_AMOUNT` (3.0) bounds the UI slider and validation
- Order of operations everywhere: **sharpen first, then bleed** (so bleed strips derive from the sharpened image)
- Full-resolution preview window (`ui/sharpen_preview.rs`): loads first card capped at 2048px, re-sharpens async on any slider change (amount / radius / threshold; one task in flight, latest settings win), hold-to-compare with original, zoom, "Apply to all cards" commits all three values to `layout_params`

**4. Image Processing** (`image_processing.rs` + `image.rs`)
- `generate_thumbnail()` - Creates RGBA8 thumbnail preserving aspect ratio
- `get_image_dpi()` - Extracts DPI from EXIF data
- `ImageMetadata` struct with format, dimensions, DPI, file size
- `extract_dpi_from_metadata()` - Cascades through EXIF, PNG pHYs chunk, TIFF tags
- `CardDimensions` with standard presets: Poker, Bridge, Tarot, Business, Mini
- `detect_card_type_by_aspect_ratio()` - Matches image aspect ratio to known card types

**5. Export Pipeline**
- Both exporters emit pages of `params.printable_size()` and subtract `params.printable_origin()` from every drawn coordinate. With printer margins off that is the whole sheet and a zero offset, so output is byte-identical to before.
- **SVG** (`svg_export.rs`): `SvgExporter` with `export_page()`, `export_pages()`, `export_pages_single_file()`. Supports cut marks, bleed/sharpen (when either is active, processes images into a `{name}_images/` directory as `{stem}_processed.png`), multi-page with Inkscape layer definitions. Unprocessed images use file:// URI references (not embedded).
- **PDF** (`pdf_export.rs`): `PdfExporter` with `export_pages()`. Uses printpdf 0.9. Features image deduplication via HashMap cache, intelligent DPI calculation to fill card width, Y-axis correction (PDF origin is bottom-left), aspect ratio correction via scale_y, bleed/sharpen image processing via `prepare_processed_image()` (encoded to PNG in-memory), cut marks (gray 0.5pt lines), Flate compression.

**6. Settings** (`settings.rs`)
- Persists to `~/.config/tcg_layout/settings.json`
- Stores: layout_params, page_size_option, card_size_option, recent_projects (MRU list, capped at 10 via `record_recent_project()`)
- OpenAI API key stored securely via system keyring (keyring crate)
- Graceful defaults when settings file is missing

**6b. Projects** (`project.rs`)
- `Project` struct: `layout_params`, `page_size_option`, `card_size_option`, `cards: Vec<ProjectCard>`. Saved as pretty-printed JSON with a `.tcgproj` extension (`PROJECT_FILE_EXTENSION`)
- `ProjectCard` holds only `path`, `back_path`, `copy_count` — no image data or thumbnails. `ProjectCard::to_card()` rebuilds a `Card` via `Card::new()` (re-reads DPI from disk); a moved/deleted file still loads, surfacing as a failed thumbnail rather than aborting the whole project load
- `TcgLayoutApp` (`main.rs`) owns `current_project_path` (`None` = unsaved/untitled) and `recent_projects`. "Save Project" writes to `current_project_path` if set, otherwise falls back to "Save Project As..."; both go through the same native-dialog + `DialogMessage` pattern as image import. The window title reflects the current project name (or "Untitled") every frame via `ViewportCommand::Title`
- Loading/starting a project does **not** touch `layout_params`' bleed/sharpen fields' change-detection (`previous_bleed_*`/`previous_sharpen`) directly — thumbnails for the newly loaded cards are requested explicitly in `load_project_from_path()`, the same way fresh image imports are

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
- Bleed or sharpen setting changes trigger texture cache clear and full thumbnail re-request
- Sharpen preview window state (`SharpenPreviewState`) lives on `TcgLayoutApp`; "Apply to all cards" sets `sharpen_amount` / `sharpen_radius` / `sharpen_threshold` + `enable_sharpen` and saves settings
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
- SVG export with image references, cut marks, bleed, sharpening, multi-page Inkscape layers
- PDF export with image deduplication, DPI scaling, bleed, sharpening, cut marks
- Bleed system with Gaussian blur edge extension
- Sharpening: luminance-only unsharp mask with adjustable amount, radius and threshold, plus a full-resolution single-card preview window
- Targeted HSL color adjustments with dropper sampling, per-adjustment front/back scope, and full-resolution editor window that cycles through all fronts and backs (export-only; not in thumbnails)
- Cut marks generation
- Layout centering (ignores margins, centers grid on page)
- Printer margins: exports sized to the printer's printable area so a forced-margin printer neither scales nor shifts the page
- Double-sided (duplex) printing: interleaved back pages with mirrored positions (flip-edge aware), automatic 180° back rotation when the flip axis requires it (cards always cut head-to-head), per-card and default back images, printer calibration offset, front-only cut marks
- Settings persistence (JSON file + secure keyring for API key)
- Projects: save/open layout params + card list (front/back filenames, copy counts) as `.tcgproj` JSON, "Save"/"Save As", Recent Projects menu, window title reflects current project
- Decklist parsing and AI-powered card-to-file matching (OpenAI)
- Card reordering in the UI
- DPI detection from EXIF/PNG pHYs/TIFF metadata
- Custom dark UI theme
- Card type detection by aspect ratio

### Not Yet Implemented
- Multiple projects open simultaneously (tabs/switcher) — currently one project active at a time, save/open like a normal document
- Unsaved-changes tracking (no dirty flag / prompt-to-save on "New Project" or quit)
- Print dialog integration (direct OS print)
- Mixed card sizes within a single layout
- Layout templates/presets
- DPI warning display in the UI (detection works, UI display not wired up)
- Duplex calibration test sheet (front/back page pair with ruled markers to measure printer offset directly)

## Testing

### Test Structure
- Unit tests colocated in each module using `#[cfg(test)]`
- ~351 test executions total (shared modules compile into both lib and bin trees)
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
cargo test project            # Project save/load tests
```

### Test Coverage Areas
- Layout grid calculations (49 tests): grid sizing, distribution, positions, centering, cut marks (page-edge runs, crosses at every intersection incl. interior, zero-spacing edge collapse, `cut_line_coords` coordinates, half-card overlap cap), duplex mirroring/interleaving, printer margins (grid unchanged when user margins are larger, grid shrinks when they aren't, cards and cut marks stay inside the printable area, duplex backs survive the mirror)
- Image processing (6 tests): thumbnails, aspect ratio, DPI
- Bleed (11 tests): pixel calculations, edge replication, zero bleed
- Sharpen (8 tests): identity at zero, flat-image invariance, edge contrast, alpha preservation
- Color adjust (25 tests): RGB↔HSL round trip, chroma recovery, noop/empty identity, gray and near-black/near-white invariance, hue targeting and wraparound, feather bounds, alpha preservation, scope filtering (per-side activity, legacy JSON defaults to All), dropper sampling (circular mean, seam, gray, near-black noise rejection, bounds)
- Thumbnail manager (7 tests): async loading, caching, deduplication, cache key discrimination
- SVG export (24 tests): single/multi-page, cut marks (front pages only, emitted after images), bleed, sharpening, processed image directory, backs-only adjustment scoping, printer margins (page size is the printable area, content shifted into it, unshifted when disabled)
- PDF export (17 tests): multi-page, real images, sharpening, sharpening + bleed, duplex end-to-end, backs-only adjustment scoping, printer margins (MediaBox is the printable area / the whole sheet when disabled, duplex end-to-end)
- Settings (12 tests): serialization, defaults, duplex backwards compatibility, printer margin backwards compatibility (absent fields load with the feature off) and round trip, recent-projects backwards compatibility/dedup/cap
- Projects (6 tests): round trip through JSON, `Card`⇄`ProjectCard` conversion, `cards` defaults to empty when absent (backwards compat), missing-file load error, display name derivation, per-card default `copy_count` when absent
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
- Bleed settings (enabled + mm amount) and sharpen settings (enabled + amount + radius + threshold) are part of the cache key - changing either triggers full cache miss
- The app clears the texture cache and re-requests thumbnails when bleed or sharpen settings change (`thumbnails_outdated` check in `main.rs`)

### Image Deduplication in PDF Export
- PDF exporter maintains a `HashMap<PathBuf, (printpdf::Image, dimensions)>` to avoid embedding the same image multiple times (important when copy_count > 1)

### Multi-page SVG Export
- `export_pages_single_file()` uses Inkscape-specific XML attributes for page definitions and layer grouping
- Standard SVG viewers will show all pages stacked; Inkscape renders them as separate pages

### OpenAI Integration
- API key stored in system keyring via `keyring` crate (service: "tcg_layout", user: "openai_api_key")
- Decklist matching runs as async background task to avoid blocking UI
- Results delivered via message channel, similar to thumbnail loading pattern
