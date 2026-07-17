use std::time::Duration;

use super::{
    ChatMediaError, ChatMediaProcessingError, ChatMediaProcessingWorkItem,
    ChatMediaReadyInput, ChatMediaStorageError,
};
use super::service::ChatMediaService;

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
        let source = self
            .storage
            .read_object(&job.media.source_object_key)
            .await
            .map_err(storage_processing_error)?;
        let processed = self.processor.process(&job.media, source.bytes).await?;
        let processed_key = format!("chat_media/{}/processed", job.media.media_id);
        let thumbnail_key = format!("chat_media/{}/thumbnail", job.media.media_id);
        let processed_object = self
            .storage
            .put_private_object(
                &processed_key,
                &processed.content_type,
                processed.content.clone(),
            )
            .await
            .map_err(storage_processing_error)?;
        if let Err(error) = self
            .storage
            .put_private_object(
                &thumbnail_key,
                &processed.thumbnail_content_type,
                processed.thumbnail.clone(),
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
        ChatMediaStorageError::DirectUploadRequired
        | ChatMediaStorageError::OperationFailed => ChatMediaProcessingError::ProcessingFailed,
    }
}

fn processing_error_code(error: ChatMediaProcessingError) -> &'static str {
    match error {
        ChatMediaProcessingError::Unavailable => "processor_unavailable",
        ChatMediaProcessingError::InvalidContent => "invalid_media_content",
        ChatMediaProcessingError::DurationTooLong => "video_duration_too_long",
        ChatMediaProcessingError::ProcessingFailed => "media_processing_failed",
    }
}
