use sqlx::{PgPool, Postgres, Transaction};

use super::rows::ChatMediaRow;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::chat_media::{
    ChatMediaCreateResult, ChatMediaError, ChatMediaKind, ChatMediaStatus, ChatMediaStorageObject,
    ChatMediaUploadRecord, NewChatMediaUpload,
};

const MEDIA_COLUMNS: &str = r#"
media_id,
upload_id,
conversation_id,
uploader_principal_id,
client_upload_id,
media_kind,
upload_status,
original_filename,
declared_content_type,
declared_size_bytes,
declared_duration_ms,
upload_mode,
chunk_size_bytes,
total_chunks,
storage_multipart_upload_id,
source_object_key,
actual_size_bytes,
storage_etag,
detected_content_type,
processed_object_key,
thumbnail_object_key,
processed_content_type,
processed_size_bytes,
processed_etag,
width_pixels,
height_pixels,
duration_ms,
frame_rate_milli,
video_codec,
audio_codec,
error_code,
EXTRACT(EPOCH FROM expires_at)::BIGINT AS expires_at_unix,
EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at_unix
"#;

const AUTHORIZED_PRINCIPAL_SQL: &str = r#"SELECT chat_principal.principal_id
FROM mini_chat_principals chat_principal
JOIN mini_chat_conversation_members member
  ON member.principal_id = chat_principal.principal_id
WHERE chat_principal.principal_role = $1
  AND chat_principal.principal_ref = $2
  AND chat_principal.active = TRUE
  AND member.conversation_id = $3
  AND member.left_at IS NULL
  AND ($4 = FALSE OR member.can_post = TRUE)"#;

pub(super) async fn initialize_upload(
    pool: &PgPool,
    principal: &Principal,
    upload: NewChatMediaUpload,
) -> Result<ChatMediaCreateResult, ChatMediaError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?;
    let uploader_id =
        authorized_principal_id(&mut tx, principal, &upload.conversation_id, true).await?;
    if let Some(record) = by_client_upload_id(
        &mut tx,
        &upload.conversation_id,
        &uploader_id,
        &upload.client_upload_id,
    )
    .await?
    {
        tx.commit().await.map_err(|_| ChatMediaError::StoreFailed)?;
        return Ok(ChatMediaCreateResult {
            record,
            created: false,
        });
    }
    let result = sqlx::query(
        r#"INSERT INTO mini_chat_media
             (media_id, upload_id, conversation_id, uploader_principal_id,
              client_upload_id, media_kind, upload_status, original_filename,
              declared_content_type, declared_size_bytes, declared_duration_ms,
              upload_mode, chunk_size_bytes, total_chunks, source_object_key, expires_at)
           VALUES ($1, $2, $3, $4, $5, $6, 'pending', $7, $8, $9, $10, $11,
                   $12, $13, $14, to_timestamp($15))
           ON CONFLICT (conversation_id, uploader_principal_id, client_upload_id)
           DO NOTHING"#,
    )
    .bind(&upload.media_id)
    .bind(&upload.upload_id)
    .bind(&upload.conversation_id)
    .bind(&uploader_id)
    .bind(&upload.client_upload_id)
    .bind(upload.kind.as_str())
    .bind(&upload.original_filename)
    .bind(&upload.declared_content_type)
    .bind(upload.declared_size_bytes)
    .bind(upload.declared_duration_ms)
    .bind(upload.upload_mode.as_str())
    .bind(upload.chunk_size_bytes)
    .bind(upload.total_chunks)
    .bind(&upload.source_object_key)
    .bind(upload.expires_at_unix)
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatMediaError::StoreFailed)?;
    let record = by_client_upload_id(
        &mut tx,
        &upload.conversation_id,
        &uploader_id,
        &upload.client_upload_id,
    )
    .await?
    .ok_or(ChatMediaError::StoreFailed)?;
    tx.commit().await.map_err(|_| ChatMediaError::StoreFailed)?;
    Ok(ChatMediaCreateResult {
        record,
        created: result.rows_affected() == 1,
    })
}

pub(super) async fn upload(
    pool: &PgPool,
    principal: &Principal,
    conversation_id: &str,
    upload_id: &str,
    require_can_post: bool,
) -> Result<ChatMediaUploadRecord, ChatMediaError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?;
    let uploader_id =
        authorized_principal_id(&mut tx, principal, conversation_id, require_can_post).await?;
    let record = by_upload_id(&mut tx, conversation_id, &uploader_id, upload_id, false)
        .await?
        .ok_or(ChatMediaError::NotFound)?;
    tx.commit().await.map_err(|_| ChatMediaError::StoreFailed)?;
    Ok(record)
}

pub(super) async fn mark_uploaded(
    pool: &PgPool,
    principal: &Principal,
    conversation_id: &str,
    upload_id: &str,
    storage: &ChatMediaStorageObject,
) -> Result<ChatMediaUploadRecord, ChatMediaError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?;
    let uploader_id = authorized_principal_id(&mut tx, principal, conversation_id, true).await?;
    let record = by_upload_id(&mut tx, conversation_id, &uploader_id, upload_id, true)
        .await?
        .ok_or(ChatMediaError::NotFound)?;
    if !matches!(
        record.status,
        ChatMediaStatus::Pending | ChatMediaStatus::Uploaded
    ) {
        return Err(ChatMediaError::Conflict);
    }
    if storage.size_bytes != record.declared_size_bytes {
        return Err(ChatMediaError::InvalidInput);
    }
    sqlx::query(
        r#"UPDATE mini_chat_media
           SET upload_status = 'uploaded', actual_size_bytes = $2,
               storage_etag = $3, error_code = NULL, updated_at = now()
           WHERE media_id = $1"#,
    )
    .bind(&record.media_id)
    .bind(storage.size_bytes)
    .bind(storage.etag.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatMediaError::StoreFailed)?;
    let updated = by_upload_id(&mut tx, conversation_id, &uploader_id, upload_id, false)
        .await?
        .ok_or(ChatMediaError::StoreFailed)?;
    tx.commit().await.map_err(|_| ChatMediaError::StoreFailed)?;
    Ok(updated)
}

pub(super) async fn complete_upload(
    pool: &PgPool,
    principal: &Principal,
    conversation_id: &str,
    upload_id: &str,
    storage: &ChatMediaStorageObject,
    job_id: &str,
) -> Result<ChatMediaUploadRecord, ChatMediaError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?;
    let uploader_id = authorized_principal_id(&mut tx, principal, conversation_id, true).await?;
    let record = by_upload_id(&mut tx, conversation_id, &uploader_id, upload_id, true)
        .await?
        .ok_or(ChatMediaError::NotFound)?;
    if matches!(
        record.status,
        ChatMediaStatus::Processing | ChatMediaStatus::Ready
    ) {
        tx.commit().await.map_err(|_| ChatMediaError::StoreFailed)?;
        return Ok(record);
    }
    if !matches!(
        record.status,
        ChatMediaStatus::Pending | ChatMediaStatus::Uploaded
    ) {
        return Err(ChatMediaError::Conflict);
    }
    if storage.size_bytes != record.declared_size_bytes {
        return Err(ChatMediaError::InvalidInput);
    }
    sqlx::query(
        r#"UPDATE mini_chat_media
           SET upload_status = 'processing', actual_size_bytes = $2,
               storage_etag = $3, error_code = NULL, updated_at = now()
           WHERE media_id = $1"#,
    )
    .bind(&record.media_id)
    .bind(storage.size_bytes)
    .bind(storage.etag.as_deref())
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatMediaError::StoreFailed)?;
    sqlx::query(
        r#"INSERT INTO mini_chat_media_jobs
             (job_id, media_id, job_type, job_status)
           VALUES ($1, $2, $3, 'pending')
           ON CONFLICT (media_id) DO NOTHING"#,
    )
    .bind(job_id)
    .bind(&record.media_id)
    .bind(job_type(record.kind))
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatMediaError::StoreFailed)?;
    let updated = by_upload_id(&mut tx, conversation_id, &uploader_id, upload_id, false)
        .await?
        .ok_or(ChatMediaError::StoreFailed)?;
    tx.commit().await.map_err(|_| ChatMediaError::StoreFailed)?;
    Ok(updated)
}

pub(super) async fn cancel_upload(
    pool: &PgPool,
    principal: &Principal,
    conversation_id: &str,
    upload_id: &str,
) -> Result<ChatMediaUploadRecord, ChatMediaError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?;
    let uploader_id = authorized_principal_id(&mut tx, principal, conversation_id, false).await?;
    let record = by_upload_id(&mut tx, conversation_id, &uploader_id, upload_id, true)
        .await?
        .ok_or(ChatMediaError::NotFound)?;
    if record.status == ChatMediaStatus::Ready {
        return Err(ChatMediaError::Conflict);
    }
    if record.status != ChatMediaStatus::Cancelled {
        sqlx::query(
            r#"UPDATE mini_chat_media
               SET upload_status = 'cancelled', updated_at = now()
               WHERE media_id = $1"#,
        )
        .bind(&record.media_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?;
        sqlx::query(
            r#"UPDATE mini_chat_media_jobs
               SET job_status = 'cancelled', locked_until = NULL, updated_at = now()
               WHERE media_id = $1 AND job_status IN ('pending', 'running', 'failed')"#,
        )
        .bind(&record.media_id)
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

pub(super) async fn claim_orphaned_uploads(
    pool: &PgPool,
    now_unix: i64,
    limit: usize,
) -> Result<Vec<ChatMediaUploadRecord>, ChatMediaError> {
    let query = format!(
        r#"WITH picked AS (
             SELECT media_id
             FROM mini_chat_media
             WHERE cleaned_at IS NULL
               AND expires_at <= to_timestamp($1)
               AND upload_status IN ('pending', 'uploaded', 'failed', 'cancelled', 'ready')
               AND NOT EXISTS (
                 SELECT 1 FROM mini_chat_message_attachments attachment
                 WHERE attachment.media_id = mini_chat_media.media_id
               )
               AND (cleanup_locked_until IS NULL OR cleanup_locked_until < now())
             ORDER BY expires_at
             FOR UPDATE SKIP LOCKED
             LIMIT $2
           )
           UPDATE mini_chat_media media
           SET cleanup_locked_until = now() + interval '5 minutes', updated_at = now()
           FROM picked
           WHERE media.media_id = picked.media_id
           RETURNING {}"#,
        prefixed_columns("media")
    );
    let rows = sqlx::query_as::<_, ChatMediaRow>(&query)
        .bind(now_unix)
        .bind(limit.clamp(1, 100) as i64)
        .fetch_all(pool)
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?;
    rows.into_iter().map(ChatMediaRow::into_model).collect()
}

pub(super) async fn mark_orphan_cleaned(
    pool: &PgPool,
    media_id: &str,
) -> Result<(), ChatMediaError> {
    sqlx::query(
        r#"UPDATE mini_chat_media
           SET upload_status = 'cancelled', cleaned_at = now(),
               cleanup_locked_until = NULL, updated_at = now()
           WHERE media_id = $1"#,
    )
    .bind(media_id)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|_| ChatMediaError::StoreFailed)
}

pub(super) async fn release_orphan_cleanup(
    pool: &PgPool,
    media_id: &str,
) -> Result<(), ChatMediaError> {
    sqlx::query(
        "UPDATE mini_chat_media SET cleanup_locked_until = NULL, updated_at = now() WHERE media_id = $1",
    )
    .bind(media_id)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|_| ChatMediaError::StoreFailed)
}

pub(super) async fn authorized_principal_id(
    tx: &mut Transaction<'_, Postgres>,
    principal: &Principal,
    conversation_id: &str,
    require_can_post: bool,
) -> Result<String, ChatMediaError> {
    sqlx::query_scalar::<_, String>(AUTHORIZED_PRINCIPAL_SQL)
        .bind(role_key(&principal.role))
        .bind(principal.ref_.trim())
        .bind(conversation_id)
        .bind(require_can_post)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?
        .ok_or(ChatMediaError::Forbidden)
}

#[cfg(test)]
mod tests {
    use super::{AUTHORIZED_PRINCIPAL_SQL, prefixed_columns};

    #[test]
    fn media_authorization_requires_active_conversation_membership_and_can_post_when_requested() {
        let query = AUTHORIZED_PRINCIPAL_SQL.to_ascii_lowercase();

        assert!(query.contains("mini_chat_conversation_members"));
        assert!(query.contains("chat_principal.active = true"));
        assert!(query.contains("member.conversation_id = $3"));
        assert!(query.contains("member.left_at is null"));
        assert!(query.contains("member.can_post = true"));
    }

    #[test]
    fn orphan_returning_columns_are_qualified_for_update_query() {
        let columns = prefixed_columns("media").to_ascii_lowercase();

        assert!(columns.contains("media.media_id"));
        assert!(columns.contains("extract(epoch from media.expires_at)"));
        assert!(!columns.contains("\nmedia.extract"));
    }
}

pub(super) async fn by_upload_id(
    tx: &mut Transaction<'_, Postgres>,
    conversation_id: &str,
    uploader_id: &str,
    upload_id: &str,
    for_update: bool,
) -> Result<Option<ChatMediaUploadRecord>, ChatMediaError> {
    let lock = if for_update { " FOR UPDATE" } else { "" };
    let query = format!(
        "SELECT {MEDIA_COLUMNS} FROM mini_chat_media WHERE conversation_id = $1 AND uploader_principal_id = $2 AND upload_id = $3{lock}"
    );
    sqlx::query_as::<_, ChatMediaRow>(&query)
        .bind(conversation_id)
        .bind(uploader_id)
        .bind(upload_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?
        .map(ChatMediaRow::into_model)
        .transpose()
}

async fn by_client_upload_id(
    tx: &mut Transaction<'_, Postgres>,
    conversation_id: &str,
    uploader_id: &str,
    client_upload_id: &str,
) -> Result<Option<ChatMediaUploadRecord>, ChatMediaError> {
    let query = format!(
        "SELECT {MEDIA_COLUMNS} FROM mini_chat_media WHERE conversation_id = $1 AND uploader_principal_id = $2 AND client_upload_id = $3"
    );
    sqlx::query_as::<_, ChatMediaRow>(&query)
        .bind(conversation_id)
        .bind(uploader_id)
        .bind(client_upload_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ChatMediaError::StoreFailed)?
        .map(ChatMediaRow::into_model)
        .transpose()
}

pub(super) fn prefixed_columns(prefix: &str) -> String {
    MEDIA_COLUMNS
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let line = line.trim();
            if line.starts_with("EXTRACT(") {
                line.replace("FROM ", &format!("FROM {prefix}."))
            } else {
                format!("{prefix}.{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn job_type(kind: ChatMediaKind) -> &'static str {
    match kind {
        ChatMediaKind::Image => "canonicalize_image",
        ChatMediaKind::Video => "canonicalize_video",
        ChatMediaKind::Audio => "canonicalize_audio",
    }
}

pub(super) fn role_key(role: &PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Supplier => "supplier",
        PrincipalRole::Werka => "werka",
        PrincipalRole::Customer => "customer",
        PrincipalRole::Aparatchi => "aparatchi",
        PrincipalRole::Qolipchi => "qolipchi",
        PrincipalRole::Boyoqchi => "boyoqchi",
        PrincipalRole::MaterialTaminotchi => "material_taminotchi",
        PrincipalRole::Admin => "admin",
    }
}
