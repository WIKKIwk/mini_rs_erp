use sqlx::PgPool;

use super::repository::{authorized_principal_id, by_upload_id};
use super::rows::ChatMediaChunkRow;
use crate::core::auth::models::Principal;
use crate::core::chat_media::{
    ChatMediaError, ChatMediaStatus, ChatMediaUploadMode, ChatMediaUploadRecord,
    ChatMediaUploadedChunk, NewChatMediaUploadedChunk,
};

pub(super) async fn set_multipart_upload_id(
    pool: &PgPool,
    principal: &Principal,
    conversation_id: &str,
    upload_id: &str,
    storage_upload_id: &str,
) -> Result<ChatMediaUploadRecord, ChatMediaError> {
    let storage_upload_id = storage_upload_id.trim();
    if storage_upload_id.is_empty() || storage_upload_id.len() > 1024 {
        return Err(ChatMediaError::StorageFailed);
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?;
    let uploader_id = authorized_principal_id(&mut tx, principal, conversation_id, true).await?;
    let record = by_upload_id(&mut tx, conversation_id, &uploader_id, upload_id, true)
        .await?
        .ok_or(ChatMediaError::NotFound)?;
    if record.upload_mode != ChatMediaUploadMode::Chunked
        || record.status != ChatMediaStatus::Pending
    {
        return Err(ChatMediaError::Conflict);
    }
    if record.storage_multipart_upload_id.is_none() {
        sqlx::query(
            r#"UPDATE mini_chat_media
               SET storage_multipart_upload_id = $2, updated_at = now()
               WHERE media_id = $1 AND storage_multipart_upload_id IS NULL"#,
        )
        .bind(&record.media_id)
        .bind(storage_upload_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?;
    }
    let updated = by_upload_id(&mut tx, conversation_id, &uploader_id, upload_id, false)
        .await?
        .ok_or(ChatMediaError::StoreFailed)?;
    tx.commit().await.map_err(|_| ChatMediaError::StoreFailed)?;
    Ok(updated)
}

pub(super) async fn uploaded_chunks(
    pool: &PgPool,
    principal: &Principal,
    conversation_id: &str,
    upload_id: &str,
    require_can_post: bool,
) -> Result<Vec<ChatMediaUploadedChunk>, ChatMediaError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?;
    let uploader_id =
        authorized_principal_id(&mut tx, principal, conversation_id, require_can_post).await?;
    let record = by_upload_id(&mut tx, conversation_id, &uploader_id, upload_id, false)
        .await?
        .ok_or(ChatMediaError::NotFound)?;
    let rows = sqlx::query_as::<_, ChatMediaChunkRow>(
        r#"SELECT chunk_index, offset_bytes, size_bytes, storage_part_etag,
                  EXTRACT(EPOCH FROM uploaded_at)::BIGINT AS uploaded_at_unix
           FROM mini_chat_media_upload_chunks
           WHERE media_id = $1
           ORDER BY chunk_index"#,
    )
    .bind(&record.media_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| ChatMediaError::StoreFailed)?;
    tx.commit().await.map_err(|_| ChatMediaError::StoreFailed)?;
    Ok(rows
        .into_iter()
        .map(ChatMediaChunkRow::into_model)
        .collect())
}

pub(super) async fn record_uploaded_chunk(
    pool: &PgPool,
    principal: &Principal,
    conversation_id: &str,
    upload_id: &str,
    chunk: NewChatMediaUploadedChunk,
) -> Result<ChatMediaUploadedChunk, ChatMediaError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?;
    let uploader_id = authorized_principal_id(&mut tx, principal, conversation_id, true).await?;
    let record = by_upload_id(&mut tx, conversation_id, &uploader_id, upload_id, true)
        .await?
        .ok_or(ChatMediaError::NotFound)?;
    validate_chunk(&record, &chunk)?;
    let existing = sqlx::query_as::<_, ChatMediaChunkRow>(
        r#"SELECT chunk_index, offset_bytes, size_bytes, storage_part_etag,
                  EXTRACT(EPOCH FROM uploaded_at)::BIGINT AS uploaded_at_unix
           FROM mini_chat_media_upload_chunks
           WHERE media_id = $1 AND chunk_index = $2"#,
    )
    .bind(&record.media_id)
    .bind(chunk.chunk_index)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ChatMediaError::StoreFailed)?;
    if let Some(existing) = existing {
        let existing = existing.into_model();
        if existing.offset_bytes != chunk.offset_bytes
            || existing.size_bytes != chunk.size_bytes
            || existing.storage_part_etag != chunk.storage_part_etag
        {
            return Err(ChatMediaError::Conflict);
        }
        tx.commit().await.map_err(|_| ChatMediaError::StoreFailed)?;
        return Ok(existing);
    }
    let inserted = sqlx::query_as::<_, ChatMediaChunkRow>(
        r#"INSERT INTO mini_chat_media_upload_chunks
             (media_id, chunk_index, offset_bytes, size_bytes, storage_part_etag)
           VALUES ($1, $2, $3, $4, $5)
           RETURNING chunk_index, offset_bytes, size_bytes, storage_part_etag,
                     EXTRACT(EPOCH FROM uploaded_at)::BIGINT AS uploaded_at_unix"#,
    )
    .bind(&record.media_id)
    .bind(chunk.chunk_index)
    .bind(chunk.offset_bytes)
    .bind(chunk.size_bytes)
    .bind(&chunk.storage_part_etag)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ChatMediaError::StoreFailed)?;
    tx.commit().await.map_err(|_| ChatMediaError::StoreFailed)?;
    Ok(inserted.into_model())
}

fn validate_chunk(
    record: &ChatMediaUploadRecord,
    chunk: &NewChatMediaUploadedChunk,
) -> Result<(), ChatMediaError> {
    if record.upload_mode != ChatMediaUploadMode::Chunked
        || record.status != ChatMediaStatus::Pending
        || record.storage_multipart_upload_id.is_none()
        || chunk.storage_part_etag.trim().is_empty()
    {
        return Err(ChatMediaError::Conflict);
    }
    let chunk_size = record.chunk_size_bytes.ok_or(ChatMediaError::StoreFailed)?;
    let total_chunks = record.total_chunks.ok_or(ChatMediaError::StoreFailed)?;
    if chunk.chunk_index < 0 || chunk.chunk_index >= total_chunks {
        return Err(ChatMediaError::InvalidInput);
    }
    let expected_offset = i64::from(chunk.chunk_index)
        .checked_mul(chunk_size)
        .ok_or(ChatMediaError::InvalidInput)?;
    let expected_size = (record.declared_size_bytes - expected_offset).min(chunk_size);
    if chunk.offset_bytes != expected_offset
        || chunk.size_bytes != expected_size
        || expected_size <= 0
    {
        return Err(ChatMediaError::InvalidInput);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_chunk;
    use crate::core::chat_media::{
        ChatMediaKind, ChatMediaStatus, ChatMediaUploadMode, ChatMediaUploadRecord,
        NewChatMediaUploadedChunk,
    };

    #[test]
    fn chunk_shape_is_revalidated_by_repository() {
        let record = ChatMediaUploadRecord {
            media_id: "media_1".into(),
            upload_id: "upload_1".into(),
            conversation_id: "conversation_1".into(),
            uploader_principal_id: "principal_1".into(),
            client_upload_id: "client_1".into(),
            kind: ChatMediaKind::Video,
            status: ChatMediaStatus::Pending,
            original_filename: "incident.mp4".into(),
            declared_content_type: "video/mp4".into(),
            declared_size_bytes: 11,
            declared_duration_ms: Some(1_000),
            upload_mode: ChatMediaUploadMode::Chunked,
            chunk_size_bytes: Some(5),
            total_chunks: Some(3),
            storage_multipart_upload_id: Some("multipart_1".into()),
            source_object_key: "chat_media/source".into(),
            actual_size_bytes: None,
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
        };
        assert!(
            validate_chunk(
                &record,
                &NewChatMediaUploadedChunk {
                    chunk_index: 2,
                    offset_bytes: 10,
                    size_bytes: 1,
                    storage_part_etag: "etag".into(),
                }
            )
            .is_ok()
        );
        assert!(
            validate_chunk(
                &record,
                &NewChatMediaUploadedChunk {
                    chunk_index: 2,
                    offset_bytes: 9,
                    size_bytes: 2,
                    storage_part_etag: "etag".into(),
                }
            )
            .is_err()
        );
    }
}
