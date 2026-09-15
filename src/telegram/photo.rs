use crate::core::calculate_orders::CalculateOrderImage;

const TELEGRAM_PHOTO_JPEG_QUALITY: u8 = 88;

pub(crate) struct PreparedTelegramPhoto {
    pub(crate) body: Vec<u8>,
    pub(crate) file_name: &'static str,
}

/// Telegram photo uploads must be encoded as a supported photo format.
/// Stored order images are WebP, so create a JPEG delivery copy without
/// changing the optimized image kept in ERP storage.
pub(crate) fn prepare_order_photo(
    image: &CalculateOrderImage,
) -> Result<PreparedTelegramPhoto, &'static str> {
    let decoded = image::load_from_memory(&image.body).map_err(|_| "rasmni ochib bo'lmadi")?;
    let rgb = decoded.to_rgb8();
    let mut body = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut body, TELEGRAM_PHOTO_JPEG_QUALITY)
        .encode_image(&rgb)
        .map_err(|_| "rasmni JPEG formatiga o'tkazib bo'lmadi")?;
    Ok(PreparedTelegramPhoto {
        body,
        file_name: "order-image.jpg",
    })
}

#[cfg(test)]
mod tests {
    use super::prepare_order_photo;
    use crate::core::calculate_orders::CalculateOrderImage;

    #[test]
    fn converts_stored_webp_to_telegram_jpeg() {
        let source = image::RgbImage::from_pixel(32, 20, image::Rgb([120, 80, 40]));
        let webp = webp::Encoder::from_rgb(source.as_raw(), source.width(), source.height())
            .encode(82.0)
            .to_vec();
        let image = CalculateOrderImage {
            image_id: "image-1".to_string(),
            image_name: "image.webp".to_string(),
            image_mime: "image/webp".to_string(),
            image_size_bytes: webp.len() as u64,
            body: webp,
        };

        let prepared = prepare_order_photo(&image).expect("prepare Telegram photo");
        assert_eq!(prepared.file_name, "order-image.jpg");
        assert!(prepared.body.starts_with(&[0xff, 0xd8, 0xff]));
        let decoded = image::load_from_memory(&prepared.body).expect("decode JPEG");
        assert_eq!((decoded.width(), decoded.height()), (32, 20));
    }
}
