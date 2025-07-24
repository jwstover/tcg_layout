use anyhow::Result;
use std::env;
use std::path::Path;
use tcg_layout::image::{
    detect_card_type_by_aspect_ratio, extract_dpi_from_metadata, load_image_metadata,
};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <image_path>", args[0]);
        std::process::exit(1);
    }

    let image_path = Path::new(&args[1]);

    match load_image_metadata(image_path) {
        Ok(metadata) => {
            println!("Image Metadata for: {}", image_path.display());
            println!("  Format: {:?}", metadata.format);
            println!(
                "  Dimensions: {}x{} pixels",
                metadata.dimensions.0, metadata.dimensions.1
            );
            println!("  File Size: {} bytes", metadata.file_size);

            // Show DPI detection details
            let metadata_dpi = extract_dpi_from_metadata(image_path);
            match metadata_dpi {
                Some(dpi) => println!("  DPI (from metadata): {dpi}"),
                None => {
                    println!("  DPI (from metadata): None found");

                    // Try to detect card type and estimate DPI
                    if let Some(card_type) = detect_card_type_by_aspect_ratio(metadata.dimensions) {
                        println!("  Detected card type: {}", card_type.name);
                        println!("  Card aspect ratio: {:.3}", card_type.aspect_ratio());
                        println!(
                            "  Image aspect ratio: {:.3}",
                            metadata.dimensions.0 as f32 / metadata.dimensions.1 as f32
                        );

                        if let Some(estimated_dpi) = metadata.dpi {
                            println!("  DPI (estimated): {estimated_dpi}");
                        }
                    } else {
                        println!("  Could not detect card type from aspect ratio");
                    }
                }
            }

            println!("  Effective DPI: {}", metadata.effective_dpi());
        }
        Err(e) => {
            eprintln!("Error loading image: {e}");
            std::process::exit(1);
        }
    }

    Ok(())
}
