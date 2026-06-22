use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, Rgb, RgbImage};

use super::ports::ProfilePortError;

const MAX_AVATAR_SIDE: u32 = 1000;
const AVATAR_JPEG_QUALITY: u8 = 82;

pub(super) struct PreparedProfileAvatar {
    pub filename: String,
    pub content_type: String,
    pub body: Vec<u8>,
}

pub(super) fn prepare_profile_avatar(
    filename: &str,
    content: Vec<u8>,
) -> Result<PreparedProfileAvatar, ProfilePortError> {
    let image = image::load_from_memory(&content).map_err(|_| ProfilePortError::LookupFailed)?;
    let image = resize_avatar(image);
    let mut body = Vec::new();
    JpegEncoder::new_with_quality(&mut body, AVATAR_JPEG_QUALITY)
        .encode_image(&flatten_on_white(&image))
        .map_err(|_| ProfilePortError::LookupFailed)?;
    Ok(PreparedProfileAvatar {
        filename: format!("{}.jpg", filename_stem(filename)),
        content_type: "image/jpeg".to_string(),
        body,
    })
}

fn resize_avatar(image: DynamicImage) -> DynamicImage {
    let (width, height) = image.dimensions();
    let longest = width.max(height);
    if longest <= MAX_AVATAR_SIDE {
        return image;
    }
    let ratio = MAX_AVATAR_SIDE as f32 / longest as f32;
    let resized_width = ((width as f32 * ratio).round() as u32).max(1);
    let resized_height = ((height as f32 * ratio).round() as u32).max(1);
    image.resize(resized_width, resized_height, FilterType::Lanczos3)
}

fn flatten_on_white(image: &DynamicImage) -> RgbImage {
    let rgba = image.to_rgba8();
    let mut rgb = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let alpha = pixel[3] as u16;
        let inv_alpha = 255 - alpha;
        let r = ((pixel[0] as u16 * alpha + 255 * inv_alpha) / 255) as u8;
        let g = ((pixel[1] as u16 * alpha + 255 * inv_alpha) / 255) as u8;
        let b = ((pixel[2] as u16 * alpha + 255 * inv_alpha) / 255) as u8;
        rgb.put_pixel(x, y, Rgb([r, g, b]));
    }
    rgb
}

fn filename_stem(filename: &str) -> String {
    let stem = filename
        .rsplit('/')
        .next()
        .unwrap_or(filename)
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(filename)
        .trim();
    let safe = safe_path_part(stem);
    if safe.is_empty() {
        "avatar".to_string()
    } else {
        safe
    }
}

fn safe_path_part(value: &str) -> String {
    let mut out = String::new();
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '_' | '-') {
            out.push(ch);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::prepare_profile_avatar;
    use image::codecs::png::PngEncoder;
    use image::{ColorType, GenericImageView, ImageEncoder, Rgba, RgbaImage};

    #[test]
    fn prepare_profile_avatar_limits_longest_side_to_1000px_jpeg() {
        let prepared =
            prepare_profile_avatar("Face.PNG", test_png(1600, 800)).expect("prepare avatar");
        let decoded = image::load_from_memory(&prepared.body).expect("decode prepared");

        assert_eq!(prepared.filename, "face.jpg");
        assert_eq!(prepared.content_type, "image/jpeg");
        assert_eq!(decoded.dimensions(), (1000, 500));
    }

    #[test]
    fn prepare_profile_avatar_reencodes_small_image_to_jpeg() {
        let prepared =
            prepare_profile_avatar("avatar.webp", test_png(400, 300)).expect("prepare avatar");
        let decoded = image::load_from_memory(&prepared.body).expect("decode prepared");

        assert_eq!(prepared.filename, "avatar.jpg");
        assert_eq!(prepared.content_type, "image/jpeg");
        assert_eq!(decoded.dimensions(), (400, 300));
    }

    fn test_png(width: u32, height: u32) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width, height, Rgba([120, 80, 40, 255]));
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(image.as_raw(), width, height, ColorType::Rgba8.into())
            .expect("encode png");
        bytes
    }
}
