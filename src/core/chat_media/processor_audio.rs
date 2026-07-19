use std::fs;
use std::path::Path;
use std::process::Command;

use super::processor::SystemChatMediaProcessor;
use super::{
    ChatMediaProcessedFiles, ChatMediaProcessingError, MAX_CHAT_AUDIO_DURATION_MS,
    MAX_CHAT_PROCESSED_AUDIO_SIZE_BYTES,
};

const AUDIO_SAMPLE_RATE: i32 = 48_000;
const AUDIO_CHANNELS: i32 = 1;
const MAX_AUDIO_BIT_RATE: i64 = 96_000;
const WAVEFORM_WIDTH: i32 = 480;
const WAVEFORM_HEIGHT: i32 = 120;

impl SystemChatMediaProcessor {
    pub(super) fn process_audio_files(
        &self,
        source_path: &Path,
        output_path: &Path,
        thumbnail_path: &Path,
    ) -> Result<ChatMediaProcessedFiles, ChatMediaProcessingError> {
        let source_probe = self.probe_audio(source_path)?;
        validate_source_audio(&source_probe)?;
        self.transcode_audio(source_path, output_path)?;
        let output_size = file_size(output_path)?;
        if output_size <= 0 || output_size > MAX_CHAT_PROCESSED_AUDIO_SIZE_BYTES {
            return Err(ChatMediaProcessingError::ProcessedAudioTooLarge);
        }
        let output_probe = self.probe_audio(output_path)?;
        validate_canonical_output(&output_probe)?;
        self.generate_waveform(output_path, thumbnail_path)?;
        if file_size(thumbnail_path)? <= 0 {
            return Err(ChatMediaProcessingError::ProcessingFailed);
        }
        Ok(ChatMediaProcessedFiles {
            content_path: output_path.to_path_buf(),
            content_type: "audio/mp4".to_string(),
            thumbnail_path: thumbnail_path.to_path_buf(),
            thumbnail_content_type: "image/jpeg".to_string(),
            width_pixels: WAVEFORM_WIDTH,
            height_pixels: WAVEFORM_HEIGHT,
            duration_ms: Some(output_probe.duration_ms),
            frame_rate_milli: None,
            video_codec: None,
            audio_codec: Some(output_probe.audio_codec),
        })
    }

    fn transcode_audio(
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
                "0:a:0",
                "-vn",
                "-map_metadata",
                "-1",
                "-map_chapters",
                "-1",
                "-c:a",
                "aac",
                "-profile:a",
                "aac_low",
                "-b:a",
                "64k",
                "-ar",
                "48000",
                "-ac",
                "1",
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
                path_str(output_path)?,
            ],
        )
    }

    fn generate_waveform(
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
                "-i",
                path_str(source_path)?,
                "-filter_complex",
                "aformat=channel_layouts=mono,showwavespic=s=480x120:colors=#607D8B",
                "-frames:v",
                "1",
                "-q:v",
                "3",
                path_str(thumbnail_path)?,
            ],
        )
    }

    fn probe_audio(&self, path: &Path) -> Result<AudioProbe, ChatMediaProcessingError> {
        let output = Command::new(&self.ffprobe_bin)
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type,codec_name,sample_rate,channels,bit_rate:format=duration,format_name,size,bit_rate",
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
struct AudioProbe {
    duration_ms: i64,
    audio_codec: String,
    sample_rate: i32,
    channels: i32,
    audio_bit_rate: Option<i64>,
    format_names: String,
}

fn parse_probe(bytes: &[u8]) -> Result<AudioProbe, ChatMediaProcessingError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| ChatMediaProcessingError::InvalidContent)?;
    let streams = value["streams"]
        .as_array()
        .ok_or(ChatMediaProcessingError::InvalidContent)?;
    let audio_streams = streams
        .iter()
        .filter(|stream| stream["codec_type"].as_str() == Some("audio"))
        .collect::<Vec<_>>();
    let video_count = streams
        .iter()
        .filter(|stream| stream["codec_type"].as_str() == Some("video"))
        .count();
    if audio_streams.len() != 1 || video_count != 0 {
        return Err(ChatMediaProcessingError::InvalidContent);
    }
    let audio = audio_streams[0];
    let duration = value["format"]["duration"]
        .as_str()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or(ChatMediaProcessingError::InvalidContent)?;
    let audio_codec = required_string(&audio["codec_name"])?;
    let sample_rate = audio["sample_rate"]
        .as_str()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|value| *value > 0)
        .ok_or(ChatMediaProcessingError::InvalidContent)?;
    let channels = audio["channels"]
        .as_i64()
        .and_then(|value| i32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or(ChatMediaProcessingError::InvalidContent)?;
    let format_names = required_string(&value["format"]["format_name"])?;
    Ok(AudioProbe {
        duration_ms: (duration * 1000.0).ceil() as i64,
        audio_codec,
        sample_rate,
        channels,
        audio_bit_rate: optional_positive_i64(&audio["bit_rate"]),
        format_names,
    })
}

fn validate_source_audio(probe: &AudioProbe) -> Result<(), ChatMediaProcessingError> {
    validate_duration(probe.duration_ms)?;
    if !source_format_supported(&probe.format_names) {
        return Err(ChatMediaProcessingError::InvalidContent);
    }
    Ok(())
}

fn validate_canonical_output(probe: &AudioProbe) -> Result<(), ChatMediaProcessingError> {
    validate_duration(probe.duration_ms)?;
    if probe.audio_codec != "aac"
        || probe.sample_rate != AUDIO_SAMPLE_RATE
        || probe.channels != AUDIO_CHANNELS
        || !is_mp4_format(&probe.format_names)
        || probe
            .audio_bit_rate
            .is_some_and(|rate| rate > MAX_AUDIO_BIT_RATE)
    {
        return Err(ChatMediaProcessingError::ProcessingFailed);
    }
    Ok(())
}

fn validate_duration(duration_ms: i64) -> Result<(), ChatMediaProcessingError> {
    if (1..=MAX_CHAT_AUDIO_DURATION_MS).contains(&duration_ms) {
        Ok(())
    } else {
        Err(ChatMediaProcessingError::AudioDurationTooLong)
    }
}

fn source_format_supported(value: &str) -> bool {
    value.split(',').any(|format| {
        matches!(
            format.trim(),
            "mov" | "mp4" | "m4a" | "aac" | "mp3" | "ogg" | "webm" | "matroska" | "wav"
        )
    })
}

fn is_mp4_format(value: &str) -> bool {
    value
        .split(',')
        .any(|format| matches!(format.trim(), "mov" | "mp4" | "m4a"))
}

fn required_string(value: &serde_json::Value) -> Result<String, ChatMediaProcessingError> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .ok_or(ChatMediaProcessingError::InvalidContent)
}

fn optional_positive_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_str()
        .and_then(|value| value.parse::<i64>().ok())
        .or_else(|| value.as_i64())
        .filter(|value| *value > 0)
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

#[cfg(test)]
mod tests {
    use super::{
        ChatMediaProcessingError, parse_probe, validate_canonical_output, validate_source_audio,
    };

    #[test]
    fn accepts_supported_voice_source() {
        let probe = parse_probe(&probe_json(
            "aac",
            "48000",
            1,
            "18.250",
            "mov,mp4,m4a",
            "64000",
        ))
        .expect("parse audio probe");
        assert_eq!(probe.duration_ms, 18_250);
        validate_source_audio(&probe).expect("valid source");
        validate_canonical_output(&probe).expect("canonical output");
    }

    #[test]
    fn rejects_audio_with_video_stream() {
        let value = br#"{
          "streams": [
            {"codec_type":"audio","codec_name":"aac","sample_rate":"48000","channels":1,"bit_rate":"64000"},
            {"codec_type":"video","codec_name":"mjpeg"}
          ],
          "format":{"duration":"2.0","format_name":"mov,mp4,m4a"}
        }"#;
        assert_eq!(
            parse_probe(value).unwrap_err(),
            ChatMediaProcessingError::InvalidContent
        );
    }

    #[test]
    fn rejects_voice_longer_than_ten_minutes() {
        let probe = parse_probe(&probe_json(
            "aac",
            "48000",
            1,
            "600.001",
            "mov,mp4,m4a",
            "64000",
        ))
        .expect("parse audio probe");
        assert_eq!(
            validate_source_audio(&probe).unwrap_err(),
            ChatMediaProcessingError::AudioDurationTooLong
        );
    }

    #[test]
    fn canonical_output_requires_mono_aac_48khz() {
        let stereo = parse_probe(&probe_json(
            "aac",
            "48000",
            2,
            "2.0",
            "mov,mp4,m4a",
            "64000",
        ))
        .expect("parse audio probe");
        assert_eq!(
            validate_canonical_output(&stereo).unwrap_err(),
            ChatMediaProcessingError::ProcessingFailed
        );
    }

    fn probe_json(
        codec: &str,
        sample_rate: &str,
        channels: i32,
        duration: &str,
        format_name: &str,
        bit_rate: &str,
    ) -> Vec<u8> {
        format!(
            r#"{{
              "streams":[{{
                "codec_type":"audio",
                "codec_name":"{codec}",
                "sample_rate":"{sample_rate}",
                "channels":{channels},
                "bit_rate":"{bit_rate}"
              }}],
              "format":{{"duration":"{duration}","format_name":"{format_name}"}}
            }}"#
        )
        .into_bytes()
    }
}
