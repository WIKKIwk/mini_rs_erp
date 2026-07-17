use sqlx::PgPool;

use super::repository::{prefixed_columns, role_key};
use super::rows::{ChatMediaRow, ChatMediaWorkRow};
use crate::core::auth::models::Principal;
use crate::core::chat_media::{
    ChatMediaError, ChatMediaProcessingWorkItem, ChatMediaReadyInput, ChatMediaUploadRecord,
};

pub(super) async fn claim_processing_jobs(
    pool: &PgPool,
    limit: usize,
) -> Result<Vec<ChatMediaProcessingWorkItem>, ChatMediaError> {
    let query = format!(
        r#"WITH picked AS (
             SELECT job_id
             FROM mini_chat_media_jobs
             WHERE job_status IN ('pending', 'running')
               AND attempts < max_attempts
               AND available_at <= now()
               AND (locked_until IS NULL OR locked_until < now())
             ORDER BY available_at, created_at
             FOR UPDATE SKIP LOCKED
             LIMIT $1
           ), claimed AS (
             UPDATE mini_chat_media_jobs job
             SET job_status = 'running', attempts = attempts + 1,
                 locked_until = now() + interval '10 minutes', updated_at = now()
             FROM picked
             WHERE job.job_id = picked.job_id
             RETURNING job.job_id, job.media_id, job.attempts, job.max_attempts
           )
           SELECT claimed.job_id, claimed.attempts, claimed.max_attempts, {}
           FROM claimed
           JOIN mini_chat_media media ON media.media_id = claimed.media_id
           WHERE media.upload_status = 'processing'"#,
        prefixed_columns("media")
    );
    let rows = sqlx::query_as::<_, ChatMediaWorkRow>(&query)
        .bind(limit.clamp(1, 8) as i64)
        .fetch_all(pool)
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?;
    rows.into_iter().map(ChatMediaWorkRow::into_model).collect()
}

pub(super) async fn mark_processing_ready(
    pool: &PgPool,
    job_id: &str,
    media_id: &str,
    ready: &ChatMediaReadyInput,
) -> Result<(), ChatMediaError> {
    let mut tx = pool.begin().await.map_err(|_| ChatMediaError::StoreFailed)?;
    let updated = sqlx::query(
        r#"UPDATE mini_chat_media
           SET upload_status = 'ready', detected_content_type = $3,
               processed_object_key = $4, processed_content_type = $3,
               processed_size_bytes = $5, processed_etag = $6,
               thumbnail_object_key = $7, width_pixels = $8,
               height_pixels = $9, duration_ms = $10,
               frame_rate_milli = $11, video_codec = $12, audio_codec = $13,
               error_code = NULL, updated_at = now()
           WHERE media_id = $1 AND upload_status = 'processing'
             AND EXISTS (
               SELECT 1 FROM mini_chat_media_jobs
               WHERE job_id = $2 AND media_id = $1 AND job_status = 'running'
             )"#,
    )
    .bind(media_id)
    .bind(job_id)
    .bind(&ready.processed_content_type)
    .bind(&ready.processed_object_key)
    .bind(ready.processed_size_bytes)
    .bind(ready.processed_etag.as_deref())
    .bind(&ready.thumbnail_object_key)
    .bind(ready.width_pixels)
    .bind(ready.height_pixels)
    .bind(ready.duration_ms)
    .bind(ready.frame_rate_milli)
    .bind(ready.video_codec.as_deref())
    .bind(ready.audio_codec.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatMediaError::StoreFailed)?;
    if updated.rows_affected() != 1 {
        return Err(ChatMediaError::Conflict);
    }
    sqlx::query(
        r#"UPDATE mini_chat_media_jobs
           SET job_status = 'succeeded', locked_until = NULL,
               last_error = NULL, updated_at = now()
           WHERE job_id = $1 AND media_id = $2 AND job_status = 'running'"#,
    )
    .bind(job_id)
    .bind(media_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatMediaError::StoreFailed)?;
    tx.commit().await.map_err(|_| ChatMediaError::StoreFailed)
}

pub(super) async fn mark_processing_failed(
    pool: &PgPool,
    job_id: &str,
    media_id: &str,
    error_code: &str,
) -> Result<(), ChatMediaError> {
    let mut tx = pool.begin().await.map_err(|_| ChatMediaError::StoreFailed)?;
    sqlx::query(
        r#"UPDATE mini_chat_media_jobs
           SET job_status = 'failed', locked_until = NULL,
               last_error = $3, updated_at = now()
           WHERE job_id = $1 AND media_id = $2 AND job_status = 'running'"#,
    )
    .bind(job_id)
    .bind(media_id)
    .bind(error_code)
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatMediaError::StoreFailed)?;
    sqlx::query(
        r#"UPDATE mini_chat_media
           SET upload_status = 'failed', error_code = $2, updated_at = now()
           WHERE media_id = $1 AND upload_status = 'processing'"#,
    )
    .bind(media_id)
    .bind(error_code)
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatMediaError::StoreFailed)?;
    tx.commit().await.map_err(|_| ChatMediaError::StoreFailed)
}

pub(super) async fn media_for_access(
    pool: &PgPool,
    principal: &Principal,
    media_id: &str,
) -> Result<ChatMediaUploadRecord, ChatMediaError> {
    let query = format!(
        r#"SELECT {}
           FROM mini_chat_media media
           JOIN mini_chat_message_attachments attachment ON attachment.media_id = media.media_id
           JOIN mini_chat_conversation_members member
             ON member.conversation_id = attachment.conversation_id AND member.left_at IS NULL
           JOIN mini_chat_principals viewer ON viewer.principal_id = member.principal_id
           WHERE media.media_id = $1 AND media.upload_status = 'ready'
             AND viewer.principal_role = $2 AND viewer.principal_ref = $3
             AND viewer.active = TRUE"#,
        prefixed_columns("media")
    );
    sqlx::query_as::<_, ChatMediaRow>(&query)
        .bind(media_id)
        .bind(role_key(&principal.role))
        .bind(principal.ref_.trim())
        .fetch_optional(pool)
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?
        .ok_or(ChatMediaError::NotFound)?
        .into_model()
}
