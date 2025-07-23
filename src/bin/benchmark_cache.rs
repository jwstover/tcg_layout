use tcg_layout::thumbnail_manager::ThumbnailManager;
use std::time::Instant;
use std::fs;
use image::{RgbImage, Rgb};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("TCG Layout Cache Benchmark");
    println!("==========================");
    
    // Create temporary directory with test images
    let temp_dir = std::env::temp_dir().join("tcg_benchmark");
    fs::create_dir_all(&temp_dir)?;
    let mut image_paths = Vec::new();
    
    println!("Creating 100 test images...");
    for i in 0..100 {
        let img = RgbImage::from_fn(300, 200, |x, y| {
            let r = ((x + i * 10) % 256) as u8;
            let g = ((y + i * 5) % 256) as u8;
            let b = ((x * y + i) % 256) as u8;
            Rgb([r, g, b])
        });
        
        let path = temp_dir.join(format!("card_{:03}.png", i));
        img.save(&path)?;
        image_paths.push(path);
    }
    
    println!("Testing thumbnail manager performance...");
    let mut manager = ThumbnailManager::with_capacity(150);
    
    // First pass - cache misses
    let start = Instant::now();
    let mut cache_hits = 0;
    let mut cache_misses = 0;
    
    for path in &image_paths {
        if let Some(_thumbnail) = manager.request_thumbnail(path.clone()) {
            cache_hits += 1;
        } else {
            cache_misses += 1;
        }
    }
    
    let first_pass_time = start.elapsed();
    println!("First pass (cache building):");
    println!("  Time: {:?}", first_pass_time);
    println!("  Cache hits: {}", cache_hits);
    println!("  Cache misses: {}", cache_misses);
    
    // Wait for all thumbnails to be processed
    println!("Waiting for thumbnail processing...");
    let mut processed_count = 0;
    let mut attempts = 0;
    const MAX_ATTEMPTS: usize = 1000;
    
    while processed_count < image_paths.len() && attempts < MAX_ATTEMPTS {
        if let Some(_message) = manager.try_recv_message() {
            processed_count += 1;
        }
        attempts += 1;
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }
    
    println!("Processed {} thumbnails in {} attempts", processed_count, attempts);
    
    // Second pass - should be mostly cache hits
    let start = Instant::now();
    cache_hits = 0;
    cache_misses = 0;
    
    for path in &image_paths {
        if let Some(_thumbnail) = manager.request_thumbnail(path.clone()) {
            cache_hits += 1;
        } else {
            cache_misses += 1;
        }
    }
    
    let second_pass_time = start.elapsed();
    println!("Second pass (cache utilization):");
    println!("  Time: {:?}", second_pass_time);
    println!("  Cache hits: {}", cache_hits);
    println!("  Cache misses: {}", cache_misses);
    
    // Performance improvement
    if first_pass_time.as_nanos() > 0 {
        let improvement = (first_pass_time.as_nanos() as f64) / (second_pass_time.as_nanos() as f64);
        println!("  Speed improvement: {:.2}x", improvement);
    }
    
    // Cache statistics
    let (total_hits, total_misses, hit_rate) = manager.cache_stats();
    println!("Overall cache statistics:");
    println!("  Total hits: {}", total_hits);
    println!("  Total misses: {}", total_misses);
    println!("  Hit rate: {:.1}%", hit_rate * 100.0);
    println!("  Cache size: {}/{}", manager.cache_size(), manager.cache_capacity());
    
    // Memory efficiency test
    let cache_memory_estimate = manager.cache_size() * 150 * 200 * 4; // RGBA bytes
    println!("  Estimated cache memory: {:.2} MB", cache_memory_estimate as f64 / 1_000_000.0);
    
    println!("Benchmark completed successfully!");
    
    // Clean up temporary files
    let _ = fs::remove_dir_all(&temp_dir);
    
    Ok(())
}