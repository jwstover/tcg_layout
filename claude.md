# TCG Card Layout Application

## Project Overview
Desktop application that automatically lays out Trading Card Game (TCG) card images on printable pages. Built in Rust for cross-platform compatibility with focus on macOS development.

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

## Architecture

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

## Future Enhancements (Post-MVP)
- Bleed and crop marks for print
- Print dialog integration
- Mixed card sizes support
- Additional export formats
- Layout templates/presets
