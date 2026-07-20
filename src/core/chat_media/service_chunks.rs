use bytes::{Bytes, BytesMut};

use super::service::{ensure_chat_principal, validate_identifier, validate_stored_object};
use super::service_support::{map_storage_error, new_id, upload_instruction};
use super::{
    ChatMediaByteStream, ChatMediaChunkUploadResult, ChatMediaCreateResult, ChatMediaError,
    ChatMediaInitialization, ChatMediaKind, ChatMediaStatus, ChatMediaStorageError,
    ChatMediaStoragePart, ChatMediaUploadInstruction, ChatMediaUploadMode, ChatMediaUploadRecord,
    ChatMediaUploadView, ChatMediaUploadedChunk, NewChatMediaUploadedChunk,
};
use crate::core::auth::models::Principal;

impl super::ChatMediaService {
    pub(super) fn upload_configuration(
        &self,
        kind: ChatMediaKind,
        size_bytes: i64,
    ) -> Result<(ChatMediaUploadMode, Option<i64>, Option<i32>), ChatMediaError> {
        if kind != ChatMediaKind::Video {
            return Ok((ChatMediaUploadMode::Single, None, None));
        }
        let chunk_size = self.video_chunk_size_bytes;
        let total = size_bytes
            .checked_add(chunk_size - 1)
            .and_then(|value| value.checked_div(chunk_size))
            .and_then(|value| i32::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or(ChatMediaError::InvalidInput)?;
        Ok((ChatMediaUploadMode::Chunked, Some(chunk_size), Some(total)))
    }

    pub(super) async fn prepare_initialization(
        &self,
        principal: &Principal,
        created: ChatMediaCreateResult,
    ) -> Result<ChatMediaInitialization, ChatMediaError> {
        let was_created = created.created;
        let mut record = created.record;
        if record.upload_mode == ChatMediaUploadMode::Single {
            let storage_upload = self
                .storage
                .prepare_upload(
                    &record.source_object_key,
                    &record.declared_content_type,
                    record.declared_size_bytes,
                )
                .await
                .map_err(map_storage_error)?;
            return Ok(ChatMediaInitialization {
                media: ChatMediaUploadView::from(&record),
                upload: upload_instruction(&record, storage_upload),
                created: was_created,
            });
        }

        if record.storage_multipart_upload_id.is_none() {
            let multipart = self
                .storage
                .begin_multipart_upload(&record.source_object_key, &record.declared_content_type)
                .await
                .map_err(map_storage_error)?;
            let updated = self
                .repository
                .set_multipart_upload_id(
                    principal,
                    &record.conversation_id,
                    &record.upload_id,
                    &multipart.storage_upload_id,
                )
                .await;
            match updated {
                Ok(value) => {
                    if value.storage_multipart_upload_id.as_deref()
                        != Some(multipart.storage_upload_id.as_str())
                    {
                        let _ = self
                            .storage
                            .abort_multipart_upload(
                                &record.source_object_key,
                                &multipart.storage_upload_id,
                            )
                            .await;
                    }
                    record = value;
                }
                Err(error) => {
                    let _ = self
                        .storage
                        .abort_multipart_upload(
                            &record.source_object_key,
                            &multipart.storage_upload_id,
                        )
                        .await;
                    return Err(error);
                }
            }
        }
        let chunks = self
            .repository
            .uploaded_chunks(principal, &record.conversation_id, &record.upload_id, false)
            .await?;
        Ok(ChatMediaInitialization {
            media: ChatMediaUploadView::from(&record).with_uploaded_chunks(chunks),
            upload: chunk_upload_instruction(&record)?,
            created: was_created,
        })
    }

    pub(super) async fn upload_view(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        require_can_post: bool,
    ) -> Result<ChatMediaUploadView, ChatMediaError> {
        let record = self
            .repository
            .upload(principal, conversation_id, upload_id, require_can_post)
            .await?;
        let chunks = if record.upload_mode == ChatMediaUploadMode::Chunked {
            self.repository
                .uploaded_chunks(principal, conversation_id, upload_id, require_can_post)
                .await?
        } else {
            Vec::new()
        };
        Ok(ChatMediaUploadView::from(&record).with_uploaded_chunks(chunks))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upload_chunk(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        chunk_index: i32,
        content_length: Option<i64>,
        content_range: Option<&str>,
        stream: ChatMediaByteStream,
    ) -> Result<ChatMediaChunkUploadResult, ChatMediaError> {
        ensure_chat_principal(principal)?;
        let conversation_id = validate_identifier(conversation_id)?;
        let upload_id = validate_identifier(upload_id)?;
        let record = self
            .repository
            .upload(principal, conversation_id, upload_id, true)
            .await?;
        if record.kind != ChatMediaKind::Video
            || record.upload_mode != ChatMediaUploadMode::Chunked
            || record.status != ChatMediaStatus::Pending
        {
            return Err(ChatMediaError::Conflict);
        }
        let (offset, size) = expected_chunk(&record, chunk_index)?;
        if content_length != Some(size)
            || content_range.map(str::trim)
                != Some(expected_content_range(offset, size, record.declared_size_bytes).as_str())
        {
            return Err(ChatMediaError::InvalidInput);
        }
        let current = self
            .repository
            .uploaded_chunks(principal, conversation_id, upload_id, true)
            .await?;
        if let Some(existing) = current
            .iter()
            .find(|chunk| chunk.chunk_index == chunk_index)
            .cloned()
        {
            if existing.offset_bytes != offset || existing.size_bytes != size {
                return Err(ChatMediaError::Conflict);
            }
            return Ok(ChatMediaChunkUploadResult {
                media: ChatMediaUploadView::from(&record).with_uploaded_chunks(current),
                chunk: existing,
            });
        }
        let content = collect_exact(stream, size).await?;
        let storage_upload_id = record
            .storage_multipart_upload_id
            .as_deref()
            .ok_or(ChatMediaError::Conflict)?;
        let stored = self
            .storage
            .put_multipart_part(
                &record.source_object_key,
                storage_upload_id,
                chunk_index + 1,
                content,
            )
            .await
            .map_err(map_storage_error)?;
        if stored.size_bytes != size || stored.etag.trim().is_empty() {
            return Err(ChatMediaError::StorageFailed);
        }
        let chunk = self
            .repository
            .record_uploaded_chunk(
                principal,
                conversation_id,
                upload_id,
                NewChatMediaUploadedChunk {
                    chunk_index,
                    offset_bytes: offset,
                    size_bytes: size,
                    storage_part_etag: stored.etag,
                },
            )
            .await?;
        let chunks = self
            .repository
            .uploaded_chunks(principal, conversation_id, upload_id, true)
            .await?;
        Ok(ChatMediaChunkUploadResult {
            media: ChatMediaUploadView::from(&record).with_uploaded_chunks(chunks),
            chunk,
        })
    }

    pub(super) async fn complete_chunked_upload(
        &self,
        principal: &Principal,
        conversation_id: &str,
        upload_id: &str,
        record: ChatMediaUploadRecord,
    ) -> Result<ChatMediaUploadView, ChatMediaError> {
        if matches!(
            record.status,
            ChatMediaStatus::Processing | ChatMediaStatus::Ready
        ) {
            return self
                .upload_view(principal, conversation_id, upload_id, false)
                .await;
        }
        if record.status != ChatMediaStatus::Pending {
            return Err(ChatMediaError::Conflict);
        }
        let chunks = self
            .repository
            .uploaded_chunks(principal, conversation_id, upload_id, true)
            .await?;
        validate_complete_chunks(&record, &chunks)?;
        let storage_upload_id = record
            .storage_multipart_upload_id
            .as_deref()
            .ok_or(ChatMediaError::Conflict)?;
        let parts = chunks
            .iter()
            .map(|chunk| ChatMediaStoragePart {
                part_number: chunk.chunk_index + 1,
                size_bytes: chunk.size_bytes,
                etag: chunk.storage_part_etag.clone(),
            })
            .collect::<Vec<_>>();
        let stored = match self
            .storage
            .complete_multipart_upload(
                &record.source_object_key,
                &record.declared_content_type,
                storage_upload_id,
                record.declared_size_bytes,
                &parts,
            )
            .await
        {
            Ok(stored) => stored,
            Err(ChatMediaStorageError::ObjectNotFound) => self
                .storage
                .object_metadata(&record.source_object_key)
                .await
                .map_err(map_storage_error)?,
            Err(error) => return Err(map_storage_error(error)),
        };
        validate_stored_object(&record, &stored)?;
        let updated = self
            .repository
            .complete_upload(
                principal,
                conversation_id,
                upload_id,
                &stored,
                &new_id("media_job"),
            )
            .await?;
        Ok(ChatMediaUploadView::from(&updated).with_uploaded_chunks(chunks))
    }
}

fn chunk_upload_instruction(
    record: &ChatMediaUploadRecord,
) -> Result<ChatMediaUploadInstruction, ChatMediaError> {
    let chunk_size = record.chunk_size_bytes.ok_or(ChatMediaError::StoreFailed)?;
    let total_chunks = record.total_chunks.ok_or(ChatMediaError::StoreFailed)?;
    Ok(ChatMediaUploadInstruction {
        strategy: "resumable_chunks".to_string(),
        method: "PUT".to_string(),
        url: format!(
            "/v1/mobile/chat/conversations/{}/media/uploads/{}/chunks/{{chunk_index}}",
            record.conversation_id, record.upload_id
        ),
        headers: [(
            "content-type".to_string(),
            "application/octet-stream".to_string(),
        )]
        .into_iter()
        .collect(),
        expires_at_unix: record.expires_at_unix,
        chunk_size_bytes: Some(chunk_size),
        total_chunks: Some(total_chunks),
    })
}

fn expected_chunk(
    record: &ChatMediaUploadRecord,
    chunk_index: i32,
) -> Result<(i64, i64), ChatMediaError> {
    let chunk_size = record.chunk_size_bytes.ok_or(ChatMediaError::StoreFailed)?;
    let total_chunks = record.total_chunks.ok_or(ChatMediaError::StoreFailed)?;
    if chunk_index < 0 || chunk_index >= total_chunks {
        return Err(ChatMediaError::InvalidInput);
    }
    let offset = i64::from(chunk_index)
        .checked_mul(chunk_size)
        .ok_or(ChatMediaError::InvalidInput)?;
    let size = (record.declared_size_bytes - offset).min(chunk_size);
    if size <= 0 {
        return Err(ChatMediaError::InvalidInput);
    }
    Ok((offset, size))
}

fn expected_content_range(offset: i64, size: i64, total: i64) -> String {
    format!("bytes {offset}-{}/{total}", offset + size - 1)
}

async fn collect_exact(
    mut stream: ChatMediaByteStream,
    expected_size: i64,
) -> Result<Bytes, ChatMediaError> {
    let capacity = usize::try_from(expected_size).map_err(|_| ChatMediaError::InvalidInput)?;
    let mut bytes = BytesMut::with_capacity(capacity);
    while let Some(chunk) = std::future::poll_fn(|context| stream.as_mut().poll_next(context)).await
    {
        let chunk = chunk.map_err(map_storage_error)?;
        if bytes.len().saturating_add(chunk.len()) > capacity {
            return Err(ChatMediaError::InvalidInput);
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.len() != capacity {
        return Err(ChatMediaError::InvalidInput);
    }
    Ok(bytes.freeze())
}

fn validate_complete_chunks(
    record: &ChatMediaUploadRecord,
    chunks: &[ChatMediaUploadedChunk],
) -> Result<(), ChatMediaError> {
    let total_chunks = record.total_chunks.ok_or(ChatMediaError::StoreFailed)?;
    if chunks.len() != usize::try_from(total_chunks).map_err(|_| ChatMediaError::StoreFailed)? {
        return Err(ChatMediaError::Conflict);
    }
    let mut total = 0_i64;
    for (index, chunk) in chunks.iter().enumerate() {
        let index = i32::try_from(index).map_err(|_| ChatMediaError::StoreFailed)?;
        let (offset, size) = expected_chunk(record, index)?;
        if chunk.chunk_index != index || chunk.offset_bytes != offset || chunk.size_bytes != size {
            return Err(ChatMediaError::Conflict);
        }
        total = total
            .checked_add(chunk.size_bytes)
            .ok_or(ChatMediaError::InvalidInput)?;
    }
    if total != record.declared_size_bytes {
        return Err(ChatMediaError::Conflict);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::expected_content_range;

    #[test]
    fn content_range_is_inclusive() {
        assert_eq!(expected_content_range(8, 4, 20), "bytes 8-11/20");
    }
}
