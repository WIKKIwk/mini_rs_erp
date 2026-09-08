//! Order image optimization for calculate-page uploads.
//!
//! Phone photos are several megabytes. Storing and serving them as-is makes
//! operators wait on every order sheet. Every upload is therefore normalized
//! to a bounded lossy WebP before it is persisted, so both storage and mobile
//! traffic stay small.

/// Longest edge of a stored order image. Bigger photos are downscaled,
/// smaller ones are kept as-is (never upscaled).
pub(crate) const ORDER_IMAGE_MAX_EDGE_PX: u32 = 1600;
/// Lossy WebP quality for stored order images.
pub(crate) const ORDER_IMAGE_WEBP_QUALITY: f32 = 82.0;
/// Rejects decompression bombs before they can exhaust memory.
const ORDER_IMAGE_MAX_PIXELS: u64 = 50_000_000;

/// Shared HTTP contract for both authorized image routes. The original URL
/// stays full-resolution; clients explicitly opt into the versioned thumbnail.
pub(crate) fn wants_thumbnail(uri: &axum::http::Uri) -> bool {
    uri.query().is_some_and(|query| query.split('&').any(|value| value == "variant=thumb-v1"))
}

pub(crate) fn image_response(
    image: crate::core::order_image_cache::CachedOrderImage,
    headers: &axum::http::HeaderMap,
) -> Result<axum::response::Response, axum::http::Error> {
    use axum::http::{StatusCode, header};
    let unchanged = headers.get(header::IF_NONE_MATCH).and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|tag| {
            let tag = tag.trim().strip_prefix("W/").unwrap_or(tag.trim());
            tag == image.etag || tag == "*"
        }));
    axum::response::Response::builder()
        .status(if unchanged { StatusCode::NOT_MODIFIED } else { StatusCode::OK })
        .header(header::CONTENT_TYPE, image.mime)
        .header(header::ETAG, image.etag)
        // Revalidate mutable order links; Mobile caches known image versions.
        // Private is intentional: never bypass ERP authorization at the CDN.
        .header(header::CACHE_CONTROL, "private, max-age=0, must-revalidate")
        .header(header::VARY, "Authorization")
        .body(if unchanged { axum::body::Body::empty() } else { axum::body::Body::from(image.body) })
}

pub(crate) struct OptimizedOrderImage {
    pub body: Vec<u8>,
    pub file_name: String,
}

/// Decodes an uploaded `jpeg`/`png`/`webp` photo and re-encodes it as a
/// bounded lossy WebP. Returns a static error detail for invalid uploads.
///
/// CPU-heavy: callers must run this inside `spawn_blocking`.
pub(crate) fn optimize_order_image_for_store(
    body: &[u8],
    file_name: &str,
) -> Result<OptimizedOrderImage, &'static str> {
    let image = image::load_from_memory(body).map_err(|_| "rasm o'qilmadi")?;
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > ORDER_IMAGE_MAX_PIXELS {
        return Err("rasm juda katta");
    }
    let image = if width.max(height) > ORDER_IMAGE_MAX_EDGE_PX {
        image.thumbnail(ORDER_IMAGE_MAX_EDGE_PX, ORDER_IMAGE_MAX_EDGE_PX)
    } else {
        image
    };
    let rgb = image.to_rgb8();
    let (width, height) = (rgb.width(), rgb.height());
    let encoded =
        webp::Encoder::from_rgb(rgb.as_raw(), width, height).encode(ORDER_IMAGE_WEBP_QUALITY);
    Ok(OptimizedOrderImage {
        body: encoded.to_vec(),
        file_name: webp_file_name(file_name),
    })
}

fn webp_file_name(file_name: &str) -> String {
    let stem = file_name
        .trim()
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(file_name)
        .trim();
    if stem.is_empty() {
        return "rang.webp".to_string();
    }
    format!("{stem}.webp")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;

    fn sample_jpeg_bytes(width: u32, height: u32) -> Vec<u8> {
        let rgb = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        let dynamic = image::DynamicImage::ImageRgb8(rgb);
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90);
        encoder.encode_image(&dynamic).expect("sample jpeg encodes");
        bytes
    }

    #[test]
    fn order_image_is_stored_as_bounded_webp() {
        let jpeg = sample_jpeg_bytes(3200, 2400);
        let optimized = optimize_order_image_for_store(&jpeg, "rang.jpg").expect("optimize");
        assert_eq!(optimized.file_name, "rang.webp");
        assert!(optimized.body.starts_with(b"RIFF"), "webp container");
        assert_eq!(&optimized.body[8..12], b"WEBP", "webp container");
        let stored = image::load_from_memory(&optimized.body).expect("stored decodes");
        assert_eq!(stored.width(), ORDER_IMAGE_MAX_EDGE_PX);
        assert!(stored.height() <= ORDER_IMAGE_MAX_EDGE_PX);
        assert!(
            (optimized.body.len() as u64) < (jpeg.len() as u64),
            "webp must be smaller than the camera jpeg"
        );
    }

    #[test]
    fn small_order_image_is_not_upscaled() {
        let jpeg = sample_jpeg_bytes(800, 600);
        let optimized = optimize_order_image_for_store(&jpeg, "kichik.png").expect("optimize");
        let stored = image::load_from_memory(&optimized.body).expect("stored decodes");
        assert_eq!((stored.width(), stored.height()), (800, 600));
    }

    #[test]
    fn corrupt_upload_is_rejected() {
        assert!(optimize_order_image_for_store(b"not an image", "x.jpg").is_err());
    }
}
