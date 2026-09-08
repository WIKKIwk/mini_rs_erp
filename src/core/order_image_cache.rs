//! Bounded, process-local image/thumbnail cache. Authorization and order-image
//! resolution belong to the caller and MUST happen before a cache lookup.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, OnceCell, Semaphore};

use super::calculate_orders::{CalculateOrderError, CalculateOrderStorePort};

pub const THUMBNAIL_EDGE: u32 = 256;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 512;
const TTL: Duration = Duration::from_secs(900);

#[derive(Clone)]
pub struct CachedOrderImage {
    pub body: Bytes,
    pub mime: String,
    pub etag: String,
}

struct Entry {
    cell: Arc<OnceCell<Option<CachedOrderImage>>>,
    created: Instant,
    used: u64,
}

#[derive(Default)]
struct Entries {
    values: HashMap<(Option<String>, String, bool), Entry>,
    clock: u64,
}

pub struct OrderImageCache {
    entries: Mutex<Entries>,
    decoders: Arc<Semaphore>,
    thumbnail_loads: Semaphore,
    full_loads: Semaphore,
}

impl Default for OrderImageCache {
    fn default() -> Self {
        Self {
            entries: Mutex::new(Entries::default()),
            decoders: Arc::new(Semaphore::new(2)),
            thumbnail_loads: Semaphore::new(4),
            full_loads: Semaphore::new(4),
        }
    }
}

impl OrderImageCache {
    pub async fn get(
        &self,
        store: &dyn CalculateOrderStorePort,
        owner: Option<&str>,
        image_id: &str,
        thumbnail: bool,
    ) -> Result<Option<CachedOrderImage>, CalculateOrderError> {
        // Owner-scoped calculate images must not reuse a global operator hit.
        let key = (owner.map(str::to_owned), image_id.to_string(), thumbnail);
        let cell = {
            let mut cache = self.entries.lock().await;
            cache.values.retain(|_, entry| entry.created.elapsed() < TTL);
            cache.clock += 1;
            let used = cache.clock;
            cache.values.entry(key.clone()).or_insert_with(|| Entry {
                cell: Arc::new(OnceCell::new()), created: Instant::now(), used,
            }).used = used;
            cache.values[&key].cell.clone()
        };
        let result = cell.get_or_try_init(|| async {
            // Reserve capacity for zoom even when many workers scroll lists.
            let loads = if thumbnail { &self.thumbnail_loads } else { &self.full_loads };
            let _load = loads.acquire().await.map_err(|_| CalculateOrderError::StoreFailed)?;
            let source = match owner {
                Some(owner) => store.get_image(owner, image_id).await?,
                None => store.get_image_global(image_id).await?,
            };
            let Some(source) = source else { return Ok::<_, CalculateOrderError>(None); };
            let (body, mime) = if thumbnail {
                let permit = self.decoders.clone().acquire_owned().await.map_err(|_| CalculateOrderError::StoreFailed)?;
                let body = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    thumbnail_bytes(&source.body)
                })
                    .await.map_err(|_| CalculateOrderError::StoreFailed)??;
                (body, "image/webp".to_string())
            } else {
                // Full-size is returned byte-for-byte. Never upscale the thumb
                // or recompress the source when opening a detail/zoom view.
                (source.body, source.image_mime)
            };
            let etag = format!("\"{:x}\"", Sha256::digest(&body));
            Ok(Some(CachedOrderImage { body: Bytes::from(body), mime, etag }))
        }).await.cloned();
        let mut cache = self.entries.lock().await;
        // Missing images/errors can be retried after upload, repair or recovery.
        if !matches!(result, Ok(Some(_))) {
            if cache.values.get(&key).is_some_and(|entry| Arc::ptr_eq(&entry.cell, &cell)) {
                cache.values.remove(&key);
            }
        }
        loop {
            let bytes: usize = cache.values.values().filter_map(|entry| entry.cell.get())
                .filter_map(Option::as_ref).map(|image| image.body.len()).sum();
            if cache.values.len() <= MAX_ENTRIES && bytes <= MAX_BYTES { break; }
            let oldest = cache.values.iter().filter(|(_, entry)| entry.cell.initialized())
                .min_by_key(|(_, entry)| entry.used).map(|(key, _)| key.clone());
            let Some(oldest) = oldest else { break; };
            cache.values.remove(&oldest);
        }
        result
    }
}

fn thumbnail_bytes(body: &[u8]) -> Result<Vec<u8>, CalculateOrderError> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(body))
        .with_guessed_format().map_err(|_| CalculateOrderError::StoreFailed)?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(200 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|_| CalculateOrderError::StoreFailed)?;
    let small = if image.width().max(image.height()) > THUMBNAIL_EDGE {
        image.thumbnail(THUMBNAIL_EDGE, THUMBNAIL_EDGE)
    } else { image }.to_rgba8();
    Ok(webp::Encoder::from_rgba(small.as_raw(), small.width(), small.height())
        .encode(82.0).to_vec())
}

#[cfg(test)]
mod tests;
