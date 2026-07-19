#[cfg(test)]
mod tests {
    use super::{
        ChatMediaProcessingError, MAX_CHAT_VIDEO_DURATION_MS, parse_probe, source_is_canonical,
        validate_source_video,
    };

    #[test]
    fn accepts_exact_ten_minute_1080p60_video() {
        let probe = parse_probe(&probe_json(1920, 1080, "600.000", "60/1")).expect("probe");
        assert_eq!(probe.duration_ms, MAX_CHAT_VIDEO_DURATION_MS);
        assert_eq!(probe.frame_rate_milli, 60_000);
        assert_eq!(validate_source_video(&probe), Ok(()));
    }

    #[test]
    fn accepts_portrait_1080p60_without_upscale_or_downscale_requirement() {
        let probe = parse_probe(&probe_json(1080, 1920, "10.000", "60000/1000")).expect("probe");
        assert_eq!(validate_source_video(&probe), Ok(()));
    }

    #[test]
    fn rejects_duration_resolution_and_frame_rate_over_limits() {
        let duration =
            parse_probe(&probe_json(1920, 1080, "600.001", "30/1")).expect("duration probe");
        assert_eq!(
            validate_source_video(&duration),
            Err(ChatMediaProcessingError::DurationTooLong)
        );
        let resolution =
            parse_probe(&probe_json(1921, 1080, "10.000", "30/1")).expect("resolution probe");
        assert_eq!(
            validate_source_video(&resolution),
            Err(ChatMediaProcessingError::ResolutionTooLarge)
        );
        let frame_rate =
            parse_probe(&probe_json(1920, 1080, "10.000", "60001/1000")).expect("frame-rate probe");
        assert_eq!(
            validate_source_video(&frame_rate),
            Err(ChatMediaProcessingError::FrameRateTooHigh)
        );
    }

    #[test]
    fn rejects_missing_or_duplicate_video_streams() {
        let no_video = br#"{"streams":[{"codec_type":"audio","codec_name":"aac","bit_rate":"128000"}],"format":{"duration":"10.0","format_name":"mov,mp4"}}"#;
        assert_eq!(
            parse_probe(no_video).unwrap_err(),
            ChatMediaProcessingError::InvalidContent
        );
        let duplicate = br#"{"streams":[{"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"avg_frame_rate":"30/1","r_frame_rate":"30/1"},{"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"avg_frame_rate":"30/1","r_frame_rate":"30/1"}],"format":{"duration":"10.0","format_name":"mov,mp4"}}"#;
        assert_eq!(
            parse_probe(duplicate).unwrap_err(),
            ChatMediaProcessingError::InvalidContent
        );
    }

    #[test]
    fn incompatible_h264_profiles_are_transcoded_instead_of_remuxed() {
        let canonical =
            parse_probe(&probe_json(1920, 1080, "10.0", "30/1")).expect("canonical probe");
        assert!(source_is_canonical(&canonical));

        let high_ten = String::from_utf8(probe_json(1920, 1080, "10.0", "30/1"))
            .expect("probe json")
            .replace(r#""profile":"High""#, r#""profile":"High 10""#)
            .replace(r#""pix_fmt":"yuv420p""#, r#""pix_fmt":"yuv420p10le""#);
        let high_ten = parse_probe(high_ten.as_bytes()).expect("high ten probe");
        assert!(!source_is_canonical(&high_ten));
    }

    fn probe_json(width: i32, height: i32, duration: &str, frame_rate: &str) -> Vec<u8> {
        format!(
            r#"{{"streams":[{{"codec_type":"video","codec_name":"h264","profile":"High","level":42,"pix_fmt":"yuv420p","width":{width},"height":{height},"avg_frame_rate":"{frame_rate}","r_frame_rate":"{frame_rate}","bit_rate":"12000000"}},{{"codec_type":"audio","codec_name":"aac","bit_rate":"192000"}}],"format":{{"duration":"{duration}","format_name":"mov,mp4,m4a,3gp,3g2,mj2"}}}}"#
        )
        .into_bytes()
    }
}
