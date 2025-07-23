use anyhow::{Context, Result};
use image::{ImageFormat, ImageReader};
use exif::{In, Reader, Tag};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

// Standard card dimensions in millimeters (width, height)
#[derive(Debug, Clone, Copy)]
pub struct CardDimensions {
    pub name: &'static str,
    pub width_mm: f32,
    pub height_mm: f32,
}

impl CardDimensions {
    pub fn width_inches(&self) -> f32 {
        self.width_mm / 25.4
    }
    
    pub fn height_inches(&self) -> f32 {
        self.height_mm / 25.4
    }
    
    pub fn aspect_ratio(&self) -> f32 {
        self.width_mm / self.height_mm
    }
}

// Common card types and their standard dimensions
pub const STANDARD_CARD_TYPES: &[CardDimensions] = &[
    CardDimensions { name: "Poker/Trading Card", width_mm: 63.5, height_mm: 88.9 }, // 2.5" x 3.5"
    CardDimensions { name: "Bridge Card", width_mm: 57.0, height_mm: 89.0 }, // 2.25" x 3.5"
    CardDimensions { name: "Tarot Card", width_mm: 70.0, height_mm: 120.0 }, // ~2.75" x 4.75"
    CardDimensions { name: "Business Card", width_mm: 85.6, height_mm: 53.98 }, // 3.37" x 2.125"
    CardDimensions { name: "Mini Card", width_mm: 44.0, height_mm: 67.0 }, // 1.75" x 2.625"
];

#[derive(Debug, Clone)]
pub struct ImageMetadata {
    pub format: ImageFormat,
    pub dimensions: (u32, u32),
    pub dpi: Option<u32>,
    pub file_size: u64,
}

impl ImageMetadata {
    pub fn effective_dpi(&self) -> u32 {
        self.dpi.unwrap_or(72)
    }
}

pub fn detect_image_format(path: &Path) -> Result<ImageFormat> {
    let reader = ImageReader::open(path)
        .with_context(|| format!("Failed to open image file: {}", path.display()))?;
    
    let format = reader.format()
        .with_context(|| format!("Could not detect format for: {}", path.display()))?;
    
    match format {
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Tiff => Ok(format),
        _ => anyhow::bail!("Unsupported format: {:?}. Only JPG, PNG, and TIFF are supported.", format),
    }
}

pub fn extract_dpi_from_metadata(path: &Path) -> Option<u32> {
    // First try EXIF data
    if let Some(dpi) = extract_dpi_from_exif(path) {
        return Some(dpi);
    }
    
    // Try format-specific metadata
    if let Ok(reader) = ImageReader::open(path) {
        if let Some(format) = reader.format() {
            match format {
                ImageFormat::Png => extract_png_dpi(path),
                ImageFormat::Tiff => extract_tiff_dpi(path),
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    }
}

fn extract_dpi_from_exif(path: &Path) -> Option<u32> {
    let file = File::open(path).ok()?;
    let mut bufreader = BufReader::new(&file);
    
    let exifreader = Reader::new();
    let exif = exifreader.read_from_container(&mut bufreader).ok()?;
    
    // Try to get X resolution first
    if let Some(field) = exif.get_field(Tag::XResolution, In::PRIMARY) {
        if let Some(resolution) = field.value.get_uint(0) {
            return Some(resolution);
        }
    }
    
    // Fallback to Y resolution
    if let Some(field) = exif.get_field(Tag::YResolution, In::PRIMARY) {
        if let Some(resolution) = field.value.get_uint(0) {
            return Some(resolution);
        }
    }
    
    None
}

fn extract_png_dpi(path: &Path) -> Option<u32> {
    use std::io::Read;
    
    let mut file = File::open(path).ok()?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).ok()?;
    
    // Look for pHYs chunk in PNG
    let mut pos = 8; // Skip PNG signature
    while pos + 8 < buffer.len() {
        let chunk_length = u32::from_be_bytes([buffer[pos], buffer[pos+1], buffer[pos+2], buffer[pos+3]]) as usize;
        let chunk_type = &buffer[pos+4..pos+8];
        
        if chunk_type == b"pHYs" && chunk_length >= 9 {
            let data_start = pos + 8;
            let pixels_per_unit_x = u32::from_be_bytes([
                buffer[data_start], buffer[data_start+1], 
                buffer[data_start+2], buffer[data_start+3]
            ]);
            let unit_specifier = buffer[data_start + 8];
            
            if unit_specifier == 1 { // pixels per meter
                // Convert pixels per meter to DPI (1 meter = 39.3701 inches)
                let dpi = (pixels_per_unit_x as f64 / 39.3701) as u32;
                return Some(dpi);
            }
        }
        
        pos += 12 + chunk_length; // chunk length + type + data + CRC
    }
    
    None
}

fn extract_tiff_dpi(_path: &Path) -> Option<u32> {
    // TIFF DPI is usually in EXIF, but let's check TIFF tags directly
    // This is a simplified approach - a full implementation would parse TIFF structure
    None
}

pub fn detect_card_type_by_aspect_ratio(dimensions: (u32, u32)) -> Option<&'static CardDimensions> {
    let image_aspect_ratio = dimensions.0 as f32 / dimensions.1 as f32;
    
    // Find the best matching card type (within 5% tolerance)
    STANDARD_CARD_TYPES.iter()
        .min_by(|a, b| {
            let diff_a = (a.aspect_ratio() - image_aspect_ratio).abs();
            let diff_b = (b.aspect_ratio() - image_aspect_ratio).abs();
            diff_a.partial_cmp(&diff_b).unwrap_or(std::cmp::Ordering::Equal)
        })
        .filter(|card_type| {
            let diff = (card_type.aspect_ratio() - image_aspect_ratio).abs();
            diff / image_aspect_ratio < 0.05 // 5% tolerance
        })
}

pub fn estimate_dpi_from_dimensions(pixels: (u32, u32), card_dimensions: &CardDimensions) -> u32 {
    let dpi_x = pixels.0 as f32 / card_dimensions.width_inches();
    let dpi_y = pixels.1 as f32 / card_dimensions.height_inches();
    
    // Return the average DPI, rounded to nearest integer
    ((dpi_x + dpi_y) / 2.0).round() as u32
}

pub fn estimate_dpi_with_fallback(dimensions: (u32, u32)) -> Option<u32> {
    // Try to detect card type by aspect ratio
    if let Some(card_type) = detect_card_type_by_aspect_ratio(dimensions) {
        Some(estimate_dpi_from_dimensions(dimensions, card_type))
    } else {
        None
    }
}

pub fn load_image_metadata(path: &Path) -> Result<ImageMetadata> {
    // Check if file exists
    if !path.exists() {
        anyhow::bail!("File does not exist: {}", path.display());
    }
    
    // Get file size
    let file_size = std::fs::metadata(path)
        .with_context(|| format!("Failed to read file metadata: {}", path.display()))?
        .len();
    
    // Detect format
    let format = detect_image_format(path)?;
    
    // Load image to get dimensions
    let img = ImageReader::open(path)
        .with_context(|| format!("Failed to open image: {}", path.display()))?
        .decode()
        .with_context(|| format!("Failed to decode image: {}", path.display()))?;
    
    let dimensions = (img.width(), img.height());
    
    // Extract DPI from metadata (EXIF + format-specific)
    let mut dpi = extract_dpi_from_metadata(path);
    
    // If no DPI found in metadata, estimate from card dimensions
    if dpi.is_none() {
        dpi = estimate_dpi_with_fallback(dimensions);
    }
    
    Ok(ImageMetadata {
        format,
        dimensions,
        dpi,
        file_size,
    })
}