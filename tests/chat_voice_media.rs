use std::process::Command;

use mini_rs_erp::core::chat_media::{
    ChatMediaKind, ChatMediaProcessor, ChatMediaStatus, ChatMediaUploadMode, ChatMediaUploadRecord,
    SystemChatMediaProcessor,
};

#[tokio::test]
#[ignore = "requires ffmpeg and ffprobe on PATH"]
async fn voice_media_is_canonicalized_to_mono_aac_with_waveform() {
    let directory = tempfile::tempdir().expect("temporary media directory");
    let source = directory.path().join("source.wav");
    let content = directory.path().join("canonical.m4a");
    let waveform = directory.path().join("waveform.jpg");
    let ffmpeg =
        std::env::var("MOBILE_CHAT_MEDIA_FFMPEG_BIN").unwrap_or_else(|_| "ffmpeg".to_string());
    let status = Command::new(&ffmpeg)
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100:duration=1.25",
            "-ac",
            "2",
            "-c:a",
            "pcm_s16le",
            source.to_str().expect("source path"),
        ])
        .status()
        .expect("run ffmpeg fixture generator");
    assert!(status.success(), "ffmpeg fixture generation failed");
    let size = i64::try_from(std::fs::metadata(&source).unwrap().len()).unwrap();
    let processor = SystemChatMediaProcessor::from_env();
    let processed = processor
        .process_file(&audio_record(size), &source, &content, &waveform)
        .await
        .expect("canonicalize voice message");

    assert_eq!(processed.content_type, "audio/mp4");
    assert_eq!(processed.thumbnail_content_type, "image/jpeg");
    assert_eq!(
        (processed.width_pixels, processed.height_pixels),
        (480, 120)
    );
    assert_eq!(processed.video_codec, None);
    assert_eq!(processed.audio_codec.as_deref(), Some("aac"));
    assert!(
        processed
            .duration_ms
            .is_some_and(|value| (1_200..=1_350).contains(&value))
    );
    assert!(std::fs::metadata(&content).unwrap().len() > 0);
    assert!(std::fs::metadata(&waveform).unwrap().len() > 0);
}

fn audio_record(size: i64) -> ChatMediaUploadRecord {
    ChatMediaUploadRecord {
        media_id: "media_audio_test".to_string(),
        upload_id: "upload_audio_test".to_string(),
        conversation_id: "conversation_test".to_string(),
        uploader_principal_id: "principal_test".to_string(),
        client_upload_id: "client_audio_test".to_string(),
        kind: ChatMediaKind::Audio,
        status: ChatMediaStatus::Processing,
        original_filename: "voice.wav".to_string(),
        declared_content_type: "audio/wav".to_string(),
        declared_size_bytes: size,
        declared_duration_ms: Some(1_250),
        upload_mode: ChatMediaUploadMode::Single,
        chunk_size_bytes: None,
        total_chunks: None,
        storage_multipart_upload_id: None,
        source_object_key: "chat_media/test/source".to_string(),
        actual_size_bytes: Some(size),
        storage_etag: None,
        detected_content_type: None,
        processed_object_key: None,
        thumbnail_object_key: None,
        processed_content_type: None,
        processed_size_bytes: None,
        processed_etag: None,
        width_pixels: None,
        height_pixels: None,
        duration_ms: None,
        frame_rate_milli: None,
        video_codec: None,
        audio_codec: None,
        error_code: None,
        expires_at_unix: 2,
        created_at_unix: 1,
        updated_at_unix: 1,
    }
}
