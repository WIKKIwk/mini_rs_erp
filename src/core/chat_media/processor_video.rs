use std::fs;
use std::path::Path;
use std::process::Command;

use super::processor::SystemChatMediaProcessor;
use super::{
    ChatMediaProcessedFiles, ChatMediaProcessingError, MAX_CHAT_PROCESSED_VIDEO_SIZE_BYTES,
    MAX_CHAT_VIDEO_DURATION_MS,
};

const MAX_VIDEO_LONG_EDGE: i32 = 1920;
const MAX_VIDEO_SHORT_EDGE: i32 = 1080;
const MAX_VIDEO_FRAME_RATE_MILLI: i32 = 60_000;
const MAX_VIDEO_BIT_RATE: i64 = 12_000_000;
const MAX_AUDIO_BIT_RATE: i64 = 192_000;

impl SystemChatMediaProcessor {
    pub(super) fn process_video_files(
        &self,
        source_path: &Path,
        output_path: &Path,
        thumbnail_path: &Path,
    ) -> Result<ChatMediaProcessedFiles, ChatMediaProcessingError> {
        let source_probe = self.probe_video(source_path)?;
        validate_source_video(&source_probe)?;
        let source_size = file_size(source_path)?;
        let remux = source_is_canonical(&source_probe)
            && source_size <= MAX_CHAT_PROCESSED_VIDEO_SIZE_BYTES;
        if remux {
            self.remux_video(source_path, output_path)?;
            if file_size(output_path)? > MAX_CHAT_PROCESSED_VIDEO_SIZE_BYTES {
                let _ = fs::remove_file(output_path);
                self.transcode_video(source_path, output_path)?;
            }
        } else {
            self.transcode_video(source_path, output_path)?;
        }
        let output_size = file_size(output_path)?;
        if output_size <= 0 || output_size > MAX_CHAT_PROCESSED_VIDEO_SIZE_BYTES {
            return Err(ChatMediaProcessingError::ProcessedTooLarge);
        }
        let output_probe = self.probe_video(output_path)?;
        validate_canonical_output(&output_probe)?;
        self.generate_thumbnail(output_path, thumbnail_path)?;
        if file_size(thumbnail_path)? <= 0 {
            return Err(ChatMediaProcessingError::ProcessingFailed);
        }
        Ok(ChatMediaProcessedFiles {
            content_path: output_path.to_path_buf(),
            content_type: "video/mp4".to_string(),
            thumbnail_path: thumbnail_path.to_path_buf(),
            thumbnail_content_type: "image/jpeg".to_string(),
            width_pixels: output_probe.width_pixels,
            height_pixels: output_probe.height_pixels,
            duration_ms: Some(output_probe.duration_ms),
            frame_rate_milli: Some(output_probe.frame_rate_milli),
            video_codec: Some(output_probe.video_codec),
            audio_codec: output_probe.audio_codec,
        })
    }

    fn remux_video(
        &self,
        source_path: &Path,
        output_path: &Path,
    ) -> Result<(), ChatMediaProcessingError> {
        run_command(
            &self.ffmpeg_bin,
            &[
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-i",
                path_str(source_path)?,
                "-map",
                "0:v:0",
                "-map",
                "0:a:0?",
                "-map_metadata",
                "-1",
                "-map_chapters",
                "-1",
                "-c",
                "copy",
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
                path_str(output_path)?,
            ],
        )
    }

    fn transcode_video(
        &self,
        source_path: &Path,
        output_path: &Path,
    ) -> Result<(), ChatMediaProcessingError> {
        run_command(
            &self.ffmpeg_bin,
            &[
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-i",
                path_str(source_path)?,
                "-map",
                "0:v:0",
                "-map",
                "0:a:0?",
                "-map_metadata",
                "-1",
                "-map_chapters",
                "-1",
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                "21",
                "-maxrate",
                "12M",
                "-bufsize",
                "6M",
                "-pix_fmt",
                "yuv420p",
                "-profile:v",
                "high",
                "-level:v",
                "4.2",
                "-fps_mode",
                "passthrough",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-ac",
                "2",
                "-tag:v",
                "avc1",
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
                path_str(output_path)?,
            ],
        )
    }

    fn generate_thumbnail(
        &self,
        source_path: &Path,
        thumbnail_path: &Path,
    ) -> Result<(), ChatMediaProcessingError> {
        run_command(
            &self.ffmpeg_bin,
            &[
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-ss",
                "0",
                "-i",
                path_str(source_path)?,
                "-frames:v",
                "1",
                "-vf",
                "scale=480:480:force_original_aspect_ratio=decrease",
                "-q:v",
                "3",
                path_str(thumbnail_path)?,
            ],
        )
    }

    fn probe_video(&self, path: &Path) -> Result<VideoProbe, ChatMediaProcessingError> {
        let output = Command::new(&self.ffprobe_bin)
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=index,codec_type,codec_name,profile,level,pix_fmt,width,height,avg_frame_rate,r_frame_rate,bit_rate:format=duration,format_name,size,bit_rate",
                "-of",
                "json",
                path_str(path)?,
            ])
            .output()
            .map_err(|_| ChatMediaProcessingError::Unavailable)?;
        if !output.status.success() {
            return Err(ChatMediaProcessingError::InvalidContent);
        }
        parse_probe(&output.stdout)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct VideoProbe {
    width_pixels: i32,
    height_pixels: i32,
    duration_ms: i64,
    frame_rate_milli: i32,
    video_codec: String,
    video_profile: Option<String>,
    video_level: Option<i32>,
    pixel_format: Option<String>,
    audio_codec: Option<String>,
    video_bit_rate: Option<i64>,
    audio_bit_rate: Option<i64>,
    format_names: String,
}

fn parse_probe(bytes: &[u8]) -> Result<VideoProbe, ChatMediaProcessingError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| ChatMediaProcessingError::InvalidContent)?;
    let streams = value["streams"]
        .as_array()
        .ok_or(ChatMediaProcessingError::InvalidContent)?;
    let video_streams = streams
        .iter()
        .filter(|stream| stream["codec_type"].as_str() == Some("video"))
        .collect::<Vec<_>>();
    let audio_streams = streams
        .iter()
        .filter(|stream| stream["codec_type"].as_str() == Some("audio"))
        .collect::<Vec<_>>();
    if video_streams.len() != 1 || audio_streams.len() > 1 {
        return Err(ChatMediaProcessingError::InvalidContent);
    }
    let video = video_streams[0];
    let width_pixels = positive_i32(&video["width"])?;
    let height_pixels = positive_i32(&video["height"])?;
    let average_rate = parse_frame_rate(video["avg_frame_rate"].as_str());
    let nominal_rate = parse_frame_rate(video["r_frame_rate"].as_str());
    let frame_rate_milli = average_rate
        .into_iter()
        .chain(nominal_rate)
        .max()
        .ok_or(ChatMediaProcessingError::InvalidContent)?;
    let video_codec = required_codec(video)?;
    let video_profile = optional_normalized_string(&video["profile"]);
    let video_level = video["level"]
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .filter(|value| *value > 0);
    let pixel_format = optional_normalized_string(&video["pix_fmt"]);
    let audio = audio_streams.first().copied();
    let audio_codec = audio.map(required_codec).transpose()?;
    let duration = value["format"]["duration"]
        .as_str()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or(ChatMediaProcessingError::InvalidContent)?;
    let format_names = value["format"]["format_name"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ChatMediaProcessingError::InvalidContent)?
        .to_ascii_lowercase();
    Ok(VideoProbe {
        width_pixels,
        height_pixels,
        duration_ms: (duration * 1000.0).ceil() as i64,
        frame_rate_milli,
        video_codec,
        video_profile,
        video_level,
        pixel_format,
        audio_codec,
        video_bit_rate: optional_positive_i64(&video["bit_rate"]),
        audio_bit_rate: audio.and_then(|stream| optional_positive_i64(&stream["bit_rate"])),
        format_names,
    })
}

fn validate_source_video(probe: &VideoProbe) -> Result<(), ChatMediaProcessingError> {
    validate_duration(probe.duration_ms)?;
    validate_dimensions(probe.width_pixels, probe.height_pixels)?;
    validate_frame_rate(probe.frame_rate_milli)?;
    if !source_format_supported(&probe.format_names) {
        return Err(ChatMediaProcessingError::InvalidContent);
    }
    Ok(())
}

fn validate_canonical_output(probe: &VideoProbe) -> Result<(), ChatMediaProcessingError> {
    validate_source_video(probe)?;
    if probe.video_codec != "h264"
        || !canonical_h264_shape(probe)
        || probe
            .audio_codec
            .as_deref()
            .is_some_and(|codec| codec != "aac")
        || !is_mp4_format(&probe.format_names)
    {
        return Err(ChatMediaProcessingError::ProcessingFailed);
    }
    if probe
        .video_bit_rate
        .is_some_and(|rate| rate > MAX_VIDEO_BIT_RATE + 500_000)
        || probe
            .audio_bit_rate
            .is_some_and(|rate| rate > MAX_AUDIO_BIT_RATE + 8_000)
    {
        return Err(ChatMediaProcessingError::ProcessingFailed);
    }
    Ok(())
}

fn source_is_canonical(probe: &VideoProbe) -> bool {
    probe.video_codec == "h264"
        && canonical_h264_shape(probe)
        && probe
            .audio_codec
            .as_deref()
            .is_none_or(|codec| codec == "aac")
        && probe
            .video_bit_rate
            .is_some_and(|rate| rate <= MAX_VIDEO_BIT_RATE)
        && probe
            .audio_bit_rate
            .is_none_or(|rate| rate <= MAX_AUDIO_BIT_RATE)
}

fn canonical_h264_shape(probe: &VideoProbe) -> bool {
    probe.pixel_format.as_deref() == Some("yuv420p")
        && probe.video_profile.as_deref().is_some_and(|profile| {
            matches!(
                profile,
                "baseline" | "constrained baseline" | "main" | "high"
            )
        })
        && probe.video_level.is_some_and(|level| level <= 42)
}

fn validate_duration(duration_ms: i64) -> Result<(), ChatMediaProcessingError> {
    if (1..=MAX_CHAT_VIDEO_DURATION_MS).contains(&duration_ms) {
        Ok(())
    } else {
        Err(ChatMediaProcessingError::DurationTooLong)
    }
}

fn validate_dimensions(width: i32, height: i32) -> Result<(), ChatMediaProcessingError> {
    if width.max(height) <= MAX_VIDEO_LONG_EDGE && width.min(height) <= MAX_VIDEO_SHORT_EDGE {
        Ok(())
    } else {
        Err(ChatMediaProcessingError::ResolutionTooLarge)
    }
}

fn validate_frame_rate(frame_rate_milli: i32) -> Result<(), ChatMediaProcessingError> {
    if (1..=MAX_VIDEO_FRAME_RATE_MILLI).contains(&frame_rate_milli) {
        Ok(())
    } else {
        Err(ChatMediaProcessingError::FrameRateTooHigh)
    }
}

fn source_format_supported(value: &str) -> bool {
    value
        .split(',')
        .any(|format| matches!(format.trim(), "mov" | "mp4" | "webm" | "matroska"))
}

fn is_mp4_format(value: &str) -> bool {
    value
        .split(',')
        .any(|format| matches!(format.trim(), "mov" | "mp4"))
}

fn parse_frame_rate(value: Option<&str>) -> Option<i32> {
    let value = value?.trim();
    let rate = if let Some((numerator, denominator)) = value.split_once('/') {
        let numerator = numerator.parse::<f64>().ok()?;
        let denominator = denominator.parse::<f64>().ok()?;
        (denominator > 0.0).then_some(numerator / denominator)?
    } else {
        value.parse::<f64>().ok()?
    };
    if !rate.is_finite() || rate <= 0.0 {
        return None;
    }
    i32::try_from((rate * 1000.0).ceil() as i64).ok()
}

fn positive_i32(value: &serde_json::Value) -> Result<i32, ChatMediaProcessingError> {
    value
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or(ChatMediaProcessingError::InvalidContent)
}

fn optional_positive_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_str()
        .and_then(|value| value.parse::<i64>().ok())
        .or_else(|| value.as_i64())
        .filter(|value| *value > 0)
}

fn required_codec(stream: &serde_json::Value) -> Result<String, ChatMediaProcessingError> {
    stream["codec_name"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .ok_or(ChatMediaProcessingError::InvalidContent)
}

fn optional_normalized_string(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
}

fn run_command(binary: &str, args: &[&str]) -> Result<(), ChatMediaProcessingError> {
    let status = Command::new(binary)
        .args(args)
        .status()
        .map_err(|_| ChatMediaProcessingError::Unavailable)?;
    if status.success() {
        Ok(())
    } else {
        Err(ChatMediaProcessingError::ProcessingFailed)
    }
}

fn file_size(path: &Path) -> Result<i64, ChatMediaProcessingError> {
    fs::metadata(path)
        .map_err(|_| ChatMediaProcessingError::ProcessingFailed)
        .and_then(|metadata| {
            i64::try_from(metadata.len()).map_err(|_| ChatMediaProcessingError::ProcessingFailed)
        })
}

fn path_str(path: &Path) -> Result<&str, ChatMediaProcessingError> {
    path.to_str()
        .ok_or(ChatMediaProcessingError::ProcessingFailed)
}

include!("processor_video_inline_tests.rs");
