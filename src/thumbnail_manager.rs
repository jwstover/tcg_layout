use crate::image_processing::{generate_thumbnail, ThumbnailParams};
use image::ImageBuffer;
use lru::LruCache;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::SystemTime;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    path: PathBuf,
    modified_time: SystemTime,
    bleed_enabled: bool,
    bleed_mm_rounded: u32, // Rounded to avoid float comparison issues
    sharpen_enabled: bool,
    sharpen_amount_rounded: u32, // Rounded to avoid float comparison issues
}

impl CacheKey {
    pub fn new(path: PathBuf, params: &ThumbnailParams) -> Option<Self> {
        let metadata = std::fs::metadata(&path).ok()?;
        let modified_time = metadata.modified().ok()?;
        Some(Self {
            path,
            modified_time,
            bleed_enabled: params.enable_bleed,
            bleed_mm_rounded: (params.bleed_mm * 10.0).round() as u32, // Store as tenths of mm
            sharpen_enabled: params.enable_sharpen,
            sharpen_amount_rounded: (params.sharpen_amount * 100.0).round() as u32, // Hundredths
        })
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

#[derive(Debug)]
pub enum ThumbnailRequest {
    Load {
        path: PathBuf,
        params: ThumbnailParams,
        response_tx: oneshot::Sender<ThumbnailResult>,
    },
}

#[derive(Debug, Clone)]
pub enum ThumbnailResult {
    Success(ImageBuffer<image::Rgba<u8>, Vec<u8>>),
    Error(String),
}

#[derive(Debug)]
pub enum ThumbnailMessage {
    ThumbnailLoaded {
        path: PathBuf,
        params: ThumbnailParams,
        result: ThumbnailResult,
    },
}

pub struct ThumbnailManager {
    request_tx: mpsc::UnboundedSender<ThumbnailRequest>,
    message_rx: mpsc::UnboundedReceiver<ThumbnailMessage>,
    pending_requests: HashMap<PathBuf, oneshot::Receiver<ThumbnailResult>>,
    cache: LruCache<CacheKey, ImageBuffer<image::Rgba<u8>, Vec<u8>>>,
    cache_hits: usize,
    cache_misses: usize,
}

impl ThumbnailManager {
    pub fn new() -> Self {
        Self::with_capacity(100) // Default capacity of 100
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let (request_tx, mut request_rx) = mpsc::unbounded_channel::<ThumbnailRequest>();
        let (message_tx, message_rx) = mpsc::unbounded_channel::<ThumbnailMessage>();

        // Spawn background worker
        tokio::spawn(async move {
            while let Some(request) = request_rx.recv().await {
                match request {
                    ThumbnailRequest::Load {
                        path,
                        params,
                        response_tx,
                    } => {
                        let path_clone = path.clone();
                        let message_tx_clone = message_tx.clone();

                        // Spawn blocking task for CPU-intensive image processing
                        tokio::task::spawn_blocking(move || {
                            let result = match generate_thumbnail(&path_clone, &params) {
                                Ok(thumbnail) => ThumbnailResult::Success(thumbnail),
                                Err(e) => ThumbnailResult::Error(e.to_string()),
                            };

                            // Send result back to the manager
                            let _ = response_tx.send(result.clone());

                            // Also send to the message channel for UI updates
                            let _ = message_tx_clone.send(ThumbnailMessage::ThumbnailLoaded {
                                path: path_clone,
                                params,
                                result,
                            });
                        });
                    }
                }
            }
        });

        Self {
            request_tx,
            message_rx,
            pending_requests: HashMap::new(),
            cache: LruCache::new(NonZeroUsize::new(capacity).unwrap()),
            cache_hits: 0,
            cache_misses: 0,
        }
    }

    pub fn request_thumbnail(
        &mut self,
        path: PathBuf,
        params: &ThumbnailParams,
    ) -> Option<ImageBuffer<image::Rgba<u8>, Vec<u8>>> {
        // Check cache first with file modification time and processing settings
        if let Some(cache_key) = CacheKey::new(path.clone(), params) {
            if let Some(thumbnail) = self.cache.get(&cache_key) {
                self.cache_hits += 1;
                return Some(thumbnail.clone());
            }
        }

        self.cache_misses += 1;

        // Don't request if already pending
        if self.pending_requests.contains_key(&path) {
            return None;
        }

        let (response_tx, response_rx) = oneshot::channel();

        let request = ThumbnailRequest::Load {
            path: path.clone(),
            params: *params,
            response_tx,
        };

        if self.request_tx.send(request).is_ok() {
            self.pending_requests.insert(path, response_rx);
        }

        None
    }

    pub fn try_recv_message(&mut self) -> Option<ThumbnailMessage> {
        match self.message_rx.try_recv() {
            Ok(message) => {
                // Clean up pending request and cache successful results
                let ThumbnailMessage::ThumbnailLoaded {
                    ref path,
                    ref params,
                    ref result,
                } = message;
                self.pending_requests.remove(path);

                // Cache successful thumbnails
                if let ThumbnailResult::Success(ref thumbnail) = result {
                    if let Some(cache_key) = CacheKey::new(path.clone(), params) {
                        self.cache.put(cache_key, thumbnail.clone());
                    }
                }

                Some(message)
            }
            Err(_) => None,
        }
    }

    pub fn has_pending_requests(&self) -> bool {
        !self.pending_requests.is_empty()
    }

    pub fn pending_count(&self) -> usize {
        self.pending_requests.len()
    }

    pub fn cache_stats(&self) -> (usize, usize, f64) {
        let total_requests = self.cache_hits + self.cache_misses;
        let hit_rate = if total_requests > 0 {
            self.cache_hits as f64 / total_requests as f64
        } else {
            0.0
        };
        (self.cache_hits, self.cache_misses, hit_rate)
    }

    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    pub fn cache_capacity(&self) -> usize {
        self.cache.cap().get()
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
}

impl Default for ThumbnailManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};
    use tempfile::NamedTempFile;

    fn create_test_image() -> NamedTempFile {
        let img = RgbImage::from_fn(100, 100, |x, y| {
            if (x + y) % 2 == 0 {
                Rgb([255, 0, 0]) // Red
            } else {
                Rgb([0, 255, 0]) // Green
            }
        });

        let temp_file = NamedTempFile::with_suffix(".png").unwrap();
        img.save(temp_file.path()).unwrap();
        temp_file
    }

    #[tokio::test]
    async fn test_thumbnail_manager_success() {
        let mut manager = ThumbnailManager::new();
        let temp_file = create_test_image();
        let path = temp_file.path().to_path_buf();

        // Request thumbnail
        manager.request_thumbnail(path.clone(), &ThumbnailParams::default());
        assert!(manager.has_pending_requests());
        assert_eq!(manager.pending_count(), 1);

        // Wait a bit for processing
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Try to receive message
        let mut received_message = false;
        for _ in 0..10 {
            if let Some(message) = manager.try_recv_message() {
                match message {
                    ThumbnailMessage::ThumbnailLoaded {
                        path: msg_path,
                        result,
                        ..
                    } => {
                        assert_eq!(msg_path, path);
                        assert!(matches!(result, ThumbnailResult::Success(_)));
                        received_message = true;
                        break;
                    }
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }

        assert!(
            received_message,
            "Should have received thumbnail loaded message"
        );
    }

    #[tokio::test]
    async fn test_thumbnail_manager_failure() {
        let mut manager = ThumbnailManager::new();
        let nonexistent_path = PathBuf::from("/nonexistent/file.jpg");

        // Request thumbnail for nonexistent file
        manager.request_thumbnail(nonexistent_path.clone(), &ThumbnailParams::default());
        assert!(manager.has_pending_requests());

        // Wait a bit for processing
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Try to receive message
        let mut received_error = false;
        for _ in 0..10 {
            if let Some(message) = manager.try_recv_message() {
                match message {
                    ThumbnailMessage::ThumbnailLoaded {
                        path: msg_path,
                        result,
                        ..
                    } => {
                        assert_eq!(msg_path, nonexistent_path);
                        assert!(matches!(result, ThumbnailResult::Error(_)));
                        received_error = true;
                        break;
                    }
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }

        assert!(received_error, "Should have received error message");
    }

    #[tokio::test]
    async fn test_thumbnail_manager_no_duplicate_requests() {
        let mut manager = ThumbnailManager::new();
        let path = PathBuf::from("/test/path.jpg");

        // Request same thumbnail twice
        let result1 = manager.request_thumbnail(path.clone(), &ThumbnailParams::default());
        assert!(result1.is_none()); // Not in cache yet
        assert_eq!(manager.pending_count(), 1);

        let result2 = manager.request_thumbnail(path.clone(), &ThumbnailParams::default());
        assert!(result2.is_none()); // Still not in cache, but no duplicate request
        assert_eq!(manager.pending_count(), 1); // Should still be 1
    }

    #[tokio::test]
    async fn test_cache_key_creation() {
        let temp_file = create_test_image();
        let path = temp_file.path().to_path_buf();

        let bleed_params = ThumbnailParams {
            bleed_mm: 3.0,
            enable_bleed: true,
            ..ThumbnailParams::default()
        };

        let key1 = CacheKey::new(path.clone(), &bleed_params);
        assert!(key1.is_some());

        let key2 = CacheKey::new(path.clone(), &bleed_params);
        assert!(key2.is_some());

        // Keys should be equal for same file with same settings
        assert_eq!(key1, key2);

        // Different bleed settings should create different keys
        let key3 = CacheKey::new(path.clone(), &ThumbnailParams::default());
        assert!(key3.is_some());
        assert_ne!(key1, key3);

        // Different sharpen settings should create different keys
        let sharpen_params = ThumbnailParams {
            sharpen_amount: 1.5,
            enable_sharpen: true,
            ..ThumbnailParams::default()
        };
        let key4 = CacheKey::new(path.clone(), &sharpen_params);
        assert!(key4.is_some());
        assert_ne!(key3, key4);

        // Different sharpen amounts should create different keys
        let stronger_sharpen = ThumbnailParams {
            sharpen_amount: 2.5,
            enable_sharpen: true,
            ..ThumbnailParams::default()
        };
        let key5 = CacheKey::new(path.clone(), &stronger_sharpen);
        assert!(key5.is_some());
        assert_ne!(key4, key5);

        // Test nonexistent file
        let nonexistent = PathBuf::from("/nonexistent/file.jpg");
        let key6 = CacheKey::new(nonexistent, &ThumbnailParams::default());
        assert!(key6.is_none());
    }

    #[tokio::test]
    async fn test_cache_functionality() {
        let mut manager = ThumbnailManager::with_capacity(10);
        let temp_file = create_test_image();
        let path = temp_file.path().to_path_buf();

        // First request should be a cache miss
        let result1 = manager.request_thumbnail(path.clone(), &ThumbnailParams::default());
        assert!(result1.is_none());
        let (hits, misses, _) = manager.cache_stats();
        assert_eq!(hits, 0);
        assert_eq!(misses, 1);

        // Wait for processing and receive the result
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let mut received_result = false;
        for _ in 0..10 {
            if let Some(message) = manager.try_recv_message() {
                match message {
                    ThumbnailMessage::ThumbnailLoaded { result, .. } => {
                        assert!(matches!(result, ThumbnailResult::Success(_)));
                        received_result = true;
                        break;
                    }
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
        assert!(received_result);

        // Second request should be a cache hit
        let result2 = manager.request_thumbnail(path.clone(), &ThumbnailParams::default());
        assert!(result2.is_some());
        let (hits, misses, _) = manager.cache_stats();
        assert_eq!(hits, 1);
        assert_eq!(misses, 1);
    }

    #[tokio::test]
    async fn test_cache_capacity() {
        let manager = ThumbnailManager::with_capacity(50);
        assert_eq!(manager.cache_capacity(), 50);
        assert_eq!(manager.cache_size(), 0);

        let (hits, misses, hit_rate) = manager.cache_stats();
        assert_eq!(hits, 0);
        assert_eq!(misses, 0);
        assert_eq!(hit_rate, 0.0);
    }
}
