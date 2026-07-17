use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use bytes::Bytes;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, Rgb, RgbImage};

use super::{
    ChatMediaKind, ChatMediaProcessedContent, ChatMediaProcessedFiles,
    ChatMediaProcessingError, ChatMediaProcessor, ChatMediaUploadRecord,
};

const IMAGE_LONG_EDGE: u32 = 1920;
const THUMBNAIL_LONG_EDGE: u32 = 480;
const IMAGE_QUALITY: u8 = 86;
const THUMBNAIL_QUALITY: u8 = 78;

#[derive(Clone)]
pub struct SystemChatMediaProcessor {
    pub(super) ffmpeg_bin: String,
    pub(super) ffprobe_bin: String,
    temp_root: PathBuf,
}

impl SystemChatMediaProcessor {
    pub fn from_env() -> Self {
        Self {
            ffmpeg_bin: env_or("MOBILE_CHAT_MEDIA_FFMPEG_BIN", "ffmpeg"),
            ffprobe_bin: env_or("MOBILE_CHAT_MEDIA_FFPROBE_BIN", "ffprobe"),
            temp_root: std::env::var("MOBILE_CHAT_MEDIA_TEMP_DIR")
                .ok()
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir),
        }
    }

    #[cfg(test)]
    pub fn new_for_tests(
        ffmpeg_bin: impl Into<String>,
        ffprobe_bin: impl Into<String>,
        temp_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            ffmpeg_bin: ffmpeg_bin.into(),
            ffprobe_bin: ffprobe_bin.into(),
            temp_root: temp_root.into(),
        }
    }
}

#[async_trait]
impl ChatMediaProcessor for SystemChatMediaProcessor {
    async fn process(
        &self,
        media: &ChatMediaUploadRecord,
        source: Bytes,
    ) -> Result<ChatMediaProcessedContent, ChatMediaProcessingError> {
        match media.kind {
            ChatMediaKind::Image => tokio::task::spawn_blocking(move || process_image(&source))
                .await
                .map_err(|_| ChatMediaProcessingError::ProcessingFailed)?,
            ChatMediaKind::Video => {
                let processor = self.clone();
                let media_id = media.media_id.clone();
                tokio::task::spawn_blocking(move || {
                    let workspace = ProcessingWorkspace::create(&processor.temp_root, &media_id)?;
                    let source_path = workspace.path.join("source.upload");
                    let content_path = workspace.path.join("canonical.mp4");
                    let thumbnail_path = workspace.path.join("thumbnail.jpg");
                    fs::write(&source_path, source)
                        .map_err(|_| ChatMediaProcessingError::ProcessingFailed)?;
                    let processed = processor.process_video_files(
                        &source_path,
                        &content_path,
                        &thumbnail_path,
                    )?;
                    Ok(ChatMediaProcessedContent {
                        content: Bytes::from(
                            fs::read(&processed.content_path)
                                .map_err(|_| ChatMediaProcessingError::ProcessingFailed)?,
                        ),
                        content_type: processed.content_type,
                        thumbnail: Bytes::from(
                            fs::read(&processed.thumbnail_path)
                                .map_err(|_| ChatMediaProcessingError::ProcessingFailed)?,
                        ),
                        thumbnail_content_type: processed.thumbnail_content_type,
                        width_pixels: processed.width_pixels,
                        height_pixels: processed.height_pixels,
                        duration_ms: processed.duration_ms,
                        frame_rate_milli: processed.frame_rate_milli,
                        video_codec: processed.video_codec,
                        audio_codec: processed.audio_codec,
                    })
                })
                .await
                .map_err(|_| ChatMediaProcessingError::ProcessingFailed)?
            }
        }
    }

    async fn process_file(
        &self,
        media: &ChatMediaUploadRecord,
        source_path: &Path,
        content_path: &Path,
        thumbnail_path: &Path,
    ) -> Result<ChatMediaProcessedFiles, ChatMediaProcessingError> {
        let processor = self.clone();
        let source_path = source_path.to_path_buf();
        let content_path = content_path.to_path_buf();
        let thumbnail_path = thumbnail_path.to_path_buf();
        let kind = media.kind;
        tokio::task::spawn_blocking(move || match kind {
            ChatMediaKind::Image => {
                let source = fs::read(&source_path)
                    .map_err(|_| ChatMediaProcessingError::ProcessingFailed)?;
                let processed = process_image(&source)?;
                write_file(&content_path, &processed.content)?;
                write_file(&thumbnail_path, &processed.thumbnail)?;
                Ok(ChatMediaProcessedFiles {
                    content_path,
                    content_type: processed.content_type,
                    thumbnail_path,
                    thumbnail_content_type: processed.thumbnail_content_type,
                    width_pixels: processed.width_pixels,
                    height_pixels: processed.height_pixels,
                    duration_ms: processed.duration_ms,
                    frame_rate_milli: processed.frame_rate_milli,
                    video_codec: processed.video_codec,
                    audio_codec: processed.audio_codec,
                })
            }
            ChatMediaKind::Video => {
                processor.process_video_files(&source_path, &content_path, &thumbnail_path)
            }
        })
        .await
        .map_err(|_| ChatMediaProcessingError::ProcessingFailed)?
    }
}

fn process_image(source: &[u8]) -> Result<ChatMediaProcessedContent, ChatMediaProcessingError> {
    let image = image::load_from_memory(source)
        .map_err(|_| ChatMediaProcessingError::InvalidContent)?;
    let canonical = resize_long_edge(image, IMAGE_LONG_EDGE);
    let (width, height) = canonical.dimensions();
    let thumbnail = resize_long_edge(canonical.clone(), THUMBNAIL_LONG_EDGE);
    Ok(ChatMediaProcessedContent {
        content: Bytes::from(encode_jpeg(&canonical, IMAGE_QUALITY)?),
        content_type: "image/jpeg".to_string(),
        thumbnail: Bytes::from(encode_jpeg(&thumbnail, THUMBNAIL_QUALITY)?),
        thumbnail_content_type: "image/jpeg".to_string(),
        width_pixels: i32::try_from(width)
            .map_err(|_| ChatMediaProcessingError::InvalidContent)?,
        height_pixels: i32::try_from(height)
            .map_err(|_| ChatMediaProcessingError::InvalidContent)?,
        duration_ms: None,
        frame_rate_milli: None,
        video_codec: None,
        audio_codec: None,
    })
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), ChatMediaProcessingError> {
    let mut file =
        fs::File::create(path).map_err(|_| ChatMediaProcessingError::ProcessingFailed)?;
    file.write_all(bytes)
        .and_then(|_| file.flush())
        .map_err(|_| ChatMediaProcessingError::ProcessingFailed)
}

fn resize_long_edge(image: DynamicImage, maximum: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    if width.max(height) <= maximum {
        image
    } else {
        image.resize(maximum, maximum, FilterType::Lanczos3)
    }
}

fn encode_jpeg(image: &DynamicImage, quality: u8) -> Result<Vec<u8>, ChatMediaProcessingError> {
    let mut output = Vec::new();
    JpegEncoder::new_with_quality(&mut output, quality)
        .encode_image(&flatten_on_white(image))
        .map_err(|_| ChatMediaProcessingError::ProcessingFailed)?;
    Ok(output)
}

fn flatten_on_white(image: &DynamicImage) -> RgbImage {
    let rgba = image.to_rgba8();
    let mut rgb = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let alpha = pixel[3] as u16;
        let inverse = 255 - alpha;
        rgb.put_pixel(
            x,
            y,
            Rgb([
                ((pixel[0] as u16 * alpha + 255 * inverse) / 255) as u8,
                ((pixel[1] as u16 * alpha + 255 * inverse) / 255) as u8,
                ((pixel[2] as u16 * alpha + 255 * inverse) / 255) as u8,
            ]),
        );
    }
    rgb
}

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

struct ProcessingWorkspace {
    path: PathBuf,
}

impl ProcessingWorkspace {
    fn create(root: &Path, media_id: &str) -> Result<Self, ChatMediaProcessingError> {
        let path = root.join(format!("mini_chat_{}_{}", media_id, std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).map_err(|_| ChatMediaProcessingError::ProcessingFailed)?;
        Ok(Self { path })
    }
}

impl Drop for ProcessingWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, GenericImageView, ImageFormat, Rgba, RgbaImage};

    use super::{ChatMediaProcessingError, process_image};

    #[test]
    fn image_processing_reencodes_and_limits_both_long_edges() {
        let source = source_png(2400, 1200);
        let processed = process_image(&source).expect("process image");
        let canonical = image::load_from_memory(&processed.content).expect("canonical jpeg");
        let thumbnail = image::load_from_memory(&processed.thumbnail).expect("thumbnail jpeg");

        assert_eq!(processed.content_type, "image/jpeg");
        assert_eq!(processed.thumbnail_content_type, "image/jpeg");
        assert_eq!(canonical.dimensions(), (1920, 960));
        assert_eq!(thumbnail.dimensions(), (480, 240));
        assert_eq!(processed.width_pixels, 1920);
        assert_eq!(processed.height_pixels, 960);
        assert_eq!(processed.duration_ms, None);
    }

    #[test]
    fn image_processing_rejects_non_image_bytes() {
        assert_eq!(
            process_image(b"not an image").unwrap_err(),
            ChatMediaProcessingError::InvalidContent
        );
    }

    fn source_png(width: u32, height: u32) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width, height, Rgba([40, 80, 120, 180]));
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, ImageFormat::Png)
            .expect("encode png");
        bytes.into_inner()
    }
}
