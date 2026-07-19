use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::service::ChatMediaService;
use super::{
    ChatMediaError, ChatMediaKind, ChatMediaProcessingError, ChatMediaProcessingWorkItem,
    ChatMediaReadyInput, ChatMediaStorageError, MAX_CHAT_AUDIO_SIZE_BYTES,
    MAX_CHAT_IMAGE_SIZE_BYTES, MAX_CHAT_PROCESSED_AUDIO_SIZE_BYTES,
    MAX_CHAT_PROCESSED_VIDEO_SIZE_BYTES, MAX_CHAT_VIDEO_SIZE_BYTES,
};

const CLEANUP_INTERVAL_SECONDS: u64 = 60 * 60;
const PROCESSING_POLL_MILLIS: u64 = 500;

impl ChatMediaService {
    pub async fn process_pending_jobs(&self, limit: usize) -> Result<usize, ChatMediaError> {
        let jobs = self
            .repository
            .claim_processing_jobs(limit.clamp(1, 8))
            .await?;
        let count = jobs.len();
        for job in jobs {
            if let Err(error) = self.process_job(&job).await {
                let code = processing_error_code(error);
                if let Err(mark_error) = self
                    .repository
                    .mark_processing_failed(&job.job_id, &job.media.media_id, code)
                    .await
                {
                    tracing::warn!(%mark_error, media_id = %job.media.media_id, "chat media failure marker failed");
                }
            }
        }
        Ok(count)
    }

    async fn process_job(
        &self,
        job: &ChatMediaProcessingWorkItem,
    ) -> Result<(), ChatMediaProcessingError> {
        let workspace = ProcessingWorkspace::create(&job.media.media_id, &job.job_id)?;
        let source_path = workspace.path.join("source.upload");
        let content_path = workspace.path.join("canonical.media");
        let thumbnail_path = workspace.path.join("thumbnail.jpg");
        let source = self
            .storage
            .download_object_to_file(&job.media.source_object_key, &source_path)
            .await
            .map_err(storage_processing_error)?;
        validate_source_size(job, source.size_bytes)?;
        let processed = self
            .processor
            .process_file(&job.media, &source_path, &content_path, &thumbnail_path)
            .await?;
        let processed_size = file_size(&processed.content_path).await?;
        if processed_size <= 0 {
            return Err(ChatMediaProcessingError::ProcessingFailed);
        }
        if job.media.kind == ChatMediaKind::Video
            && processed_size > MAX_CHAT_PROCESSED_VIDEO_SIZE_BYTES
        {
            return Err(ChatMediaProcessingError::ProcessedTooLarge);
        }
        if job.media.kind == ChatMediaKind::Audio
            && processed_size > MAX_CHAT_PROCESSED_AUDIO_SIZE_BYTES
        {
            return Err(ChatMediaProcessingError::ProcessedAudioTooLarge);
        }
        if file_size(&processed.thumbnail_path).await? <= 0 {
            return Err(ChatMediaProcessingError::ProcessingFailed);
        }
        let processed_key = format!("chat_media/{}/processed", job.media.media_id);
        let thumbnail_key = format!("chat_media/{}/thumbnail", job.media.media_id);
        let processed_object = self
            .storage
            .put_private_file(
                &processed_key,
                &processed.content_type,
                &processed.content_path,
            )
            .await
            .map_err(storage_processing_error)?;
        if processed_object.size_bytes != processed_size {
            let _ = self.storage.delete_object(&processed_key).await;
            return Err(ChatMediaProcessingError::ProcessingFailed);
        }
        if let Err(error) = self
            .storage
            .put_private_file(
                &thumbnail_key,
                &processed.thumbnail_content_type,
                &processed.thumbnail_path,
            )
            .await
        {
            let _ = self.storage.delete_object(&processed_key).await;
            return Err(storage_processing_error(error));
        }
        let ready = ChatMediaReadyInput {
            processed_object_key: processed_key.clone(),
            processed_content_type: processed.content_type,
            processed_size_bytes: processed_object.size_bytes,
            processed_etag: processed_object.etag,
            thumbnail_object_key: thumbnail_key.clone(),
            width_pixels: processed.width_pixels,
            height_pixels: processed.height_pixels,
            duration_ms: processed.duration_ms,
            frame_rate_milli: processed.frame_rate_milli,
            video_codec: processed.video_codec,
            audio_codec: processed.audio_codec,
        };
        if self
            .repository
            .mark_processing_ready(&job.job_id, &job.media.media_id, &ready)
            .await
            .is_err()
        {
            let _ = self.storage.delete_object(&processed_key).await;
            let _ = self.storage.delete_object(&thumbnail_key).await;
            return Err(ChatMediaProcessingError::ProcessingFailed);
        }
        let _ = self
            .storage
            .delete_object(&job.media.source_object_key)
            .await;
        Ok(())
    }

    pub fn start_cleanup_worker(&self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let service = self.clone();
        handle.spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(CLEANUP_INTERVAL_SECONDS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if let Err(error) = service.cleanup_orphaned_uploads(100).await {
                    tracing::warn!(%error, "chat media orphan cleanup failed");
                }
            }
        });
    }

    pub fn start_processing_worker(&self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let service = self.clone();
        handle.spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(PROCESSING_POLL_MILLIS));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                if let Err(error) = service.process_pending_jobs(2).await {
                    tracing::warn!(%error, "chat media processing worker failed");
                }
            }
        });
    }
}

fn storage_processing_error(error: ChatMediaStorageError) -> ChatMediaProcessingError {
    match error {
        ChatMediaStorageError::Unavailable => ChatMediaProcessingError::Unavailable,
        ChatMediaStorageError::ObjectNotFound
        | ChatMediaStorageError::SizeMismatch
        | ChatMediaStorageError::InvalidObjectKey => ChatMediaProcessingError::InvalidContent,
        ChatMediaStorageError::DirectUploadRequired | ChatMediaStorageError::OperationFailed => {
            ChatMediaProcessingError::ProcessingFailed
        }
    }
}

fn processing_error_code(error: ChatMediaProcessingError) -> &'static str {
    match error {
        ChatMediaProcessingError::Unavailable => "processor_unavailable",
        ChatMediaProcessingError::InvalidContent => "invalid_media_content",
        ChatMediaProcessingError::DurationTooLong => "video_duration_too_long",
        ChatMediaProcessingError::AudioDurationTooLong => "audio_duration_too_long",
        ChatMediaProcessingError::ResolutionTooLarge => "video_resolution_too_large",
        ChatMediaProcessingError::FrameRateTooHigh => "video_frame_rate_too_high",
        ChatMediaProcessingError::ProcessedTooLarge => "processed_video_too_large",
        ChatMediaProcessingError::ProcessedAudioTooLarge => "processed_audio_too_large",
        ChatMediaProcessingError::ProcessingFailed => "media_processing_failed",
    }
}

fn validate_source_size(
    job: &ChatMediaProcessingWorkItem,
    actual_size: i64,
) -> Result<(), ChatMediaProcessingError> {
    let maximum = match job.media.kind {
        ChatMediaKind::Image => MAX_CHAT_IMAGE_SIZE_BYTES,
        ChatMediaKind::Video => MAX_CHAT_VIDEO_SIZE_BYTES,
        ChatMediaKind::Audio => MAX_CHAT_AUDIO_SIZE_BYTES,
    };
    if actual_size != job.media.declared_size_bytes || !(1..=maximum).contains(&actual_size) {
        Err(ChatMediaProcessingError::InvalidContent)
    } else {
        Ok(())
    }
}

async fn file_size(path: &Path) -> Result<i64, ChatMediaProcessingError> {
    tokio::fs::metadata(path)
        .await
        .map_err(|_| ChatMediaProcessingError::ProcessingFailed)
        .and_then(|metadata| {
            i64::try_from(metadata.len()).map_err(|_| ChatMediaProcessingError::ProcessingFailed)
        })
}

struct ProcessingWorkspace {
    path: PathBuf,
}

impl ProcessingWorkspace {
    fn create(media_id: &str, job_id: &str) -> Result<Self, ChatMediaProcessingError> {
        let path = std::env::var("MOBILE_CHAT_MEDIA_TEMP_DIR")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("mini_chat_job_{media_id}_{job_id}"));
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
