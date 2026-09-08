use super::*;
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use crate::core::calculate_orders::{CalculateOrderImage, CalculateOrderTemplate};

struct ImageStore {
    body: Vec<u8>,
    reads: AtomicUsize,
    fail: AtomicBool,
}

impl ImageStore {
    fn new() -> Self {
        let rgb = image::RgbImage::from_fn(1200, 800, |x, y| {
            image::Rgb([(x % 251) as u8, (y % 239) as u8, ((x + y) % 227) as u8])
        });
        Self { body: webp::Encoder::from_rgb(rgb.as_raw(), 1200, 800).encode(82.0).to_vec(),
            reads: AtomicUsize::new(0), fail: AtomicBool::new(false) }
    }
}

#[async_trait]
impl CalculateOrderStorePort for ImageStore {
    async fn list(&self, _: &str) -> Result<Vec<CalculateOrderTemplate>, CalculateOrderError> { unreachable!() }
    async fn upsert(&self, _: &str, _: CalculateOrderTemplate) -> Result<CalculateOrderTemplate, CalculateOrderError> { unreachable!() }
    async fn delete(&self, _: &str, _: &str) -> Result<(), CalculateOrderError> { unreachable!() }
    async fn save_image(&self, _: &str, _: CalculateOrderImage) -> Result<CalculateOrderImage, CalculateOrderError> { unreachable!() }
    async fn get_image(&self, owner: &str, id: &str) -> Result<Option<CalculateOrderImage>, CalculateOrderError> {
        if owner != "owner" { return Ok(None); }
        self.get_image_global(id).await
    }
    async fn get_image_global(&self, id: &str) -> Result<Option<CalculateOrderImage>, CalculateOrderError> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        tokio::task::yield_now().await;
        if self.fail.swap(false, Ordering::Relaxed) { return Err(CalculateOrderError::StoreFailed); }
        Ok(Some(CalculateOrderImage { image_id: id.into(), image_name: "photo.webp".into(),
            image_mime: "image/webp".into(), image_size_bytes: self.body.len() as u64,
            body: self.body.clone() }))
    }
}

#[tokio::test]
async fn thumbnails_are_small_and_full_resolution_is_byte_exact() {
    let store = ImageStore::new();
    let cache = OrderImageCache::default();
    let thumb = cache.get(&store, None, "one", true).await.unwrap().unwrap();
    let full = cache.get(&store, None, "one", false).await.unwrap().unwrap();
    assert_eq!(full.body.as_ref(), store.body);
    assert_eq!(image::load_from_memory(&full.body).unwrap().width(), 1200);
    let small = image::load_from_memory(&thumb.body).unwrap();
    assert_eq!(small.width(), THUMBNAIL_EDGE);
    assert!(small.height() <= THUMBNAIL_EDGE);
    assert!(thumb.body.len() < full.body.len() / 2);
    assert_ne!(thumb.etag, full.etag);
    let repeated = cache.get(&store, None, "one", true).await.unwrap().unwrap();
    assert_eq!(thumb.etag, repeated.etag);
    assert_eq!(store.reads.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn concurrent_image_requests_share_work_and_owner_scope_is_preserved() {
    let store = Arc::new(ImageStore::new());
    let cache = Arc::new(OrderImageCache::default());
    let mut tasks = Vec::new();
    for _ in 0..20 {
        let (store, cache) = (store.clone(), cache.clone());
        tasks.push(tokio::spawn(async move { cache.get(store.as_ref(), None, "one", true).await.unwrap() }));
    }
    for task in tasks { assert!(task.await.unwrap().is_some()); }
    assert_eq!(store.reads.load(Ordering::Relaxed), 1);
    assert!(cache.get(store.as_ref(), Some("other-owner"), "one", true).await.unwrap().is_none());
}

#[tokio::test]
async fn full_image_does_not_wait_for_thumbnail_capacity() {
    let store = ImageStore::new();
    let cache = OrderImageCache::default();
    let _busy_thumbnails = cache.thumbnail_loads.acquire_many(4).await.unwrap();
    let full = tokio::time::timeout(
        Duration::from_secs(1),
        cache.get(&store, None, "zoom", false),
    ).await.expect("full image must have independent load capacity").unwrap().unwrap();
    assert_eq!(full.body.as_ref(), store.body);
}

#[tokio::test]
async fn errors_are_retryable_and_cache_entry_count_is_bounded() {
    let mut store = ImageStore::new();
    store.body = vec![0; 8];
    store.fail.store(true, Ordering::Relaxed);
    let cache = OrderImageCache::default();
    assert!(cache.get(&store, None, "one", false).await.is_err());
    assert!(cache.get(&store, None, "one", false).await.unwrap().is_some());
    for id in 0..MAX_ENTRIES + 10 {
        cache.get(&store, None, &id.to_string(), false).await.unwrap();
    }
    assert!(cache.entries.lock().await.values.len() <= MAX_ENTRIES);
}
