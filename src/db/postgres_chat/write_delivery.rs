pub(super) async fn mark_read(
    pool: &PgPool,
    principal: &Principal,
    conversation_id: &str,
    sequence: i64,
    device_id: &str,
) -> Result<(), ChatError> {
    let mut tx = pool.begin().await.map_err(|_| ChatError::StoreFailed)?;
    let principal_id = sqlx::query_scalar::<_, String>(
        "SELECT principal_id FROM mini_chat_principals WHERE principal_role = $1 AND principal_ref = $2",
    )
    .bind(role_key(&principal.role))
    .bind(principal.ref_.trim())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?
    .ok_or(ChatError::NotFound)?;
    let read_sequence = sqlx::query_scalar::<_, i64>(
        r#"UPDATE mini_chat_conversation_members member
           SET last_read_sequence = GREATEST(
             member.last_read_sequence,
             LEAST($3, conversation.last_message_sequence)
           )
           FROM mini_chat_conversations conversation
           WHERE member.conversation_id = $1
             AND member.principal_id = $2
             AND member.left_at IS NULL
             AND conversation.conversation_id = member.conversation_id
           RETURNING member.last_read_sequence"#,
    )
    .bind(conversation_id)
    .bind(&principal_id)
    .bind(sequence)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?
    .ok_or(ChatError::Forbidden)?;
    sqlx::query(
        r#"INSERT INTO mini_chat_device_cursors
             (principal_id, device_id, conversation_id, last_delivered_sequence, last_read_sequence)
           VALUES ($1, $2, $3, $4, $4)
           ON CONFLICT (principal_id, device_id, conversation_id) DO UPDATE SET
             last_delivered_sequence = GREATEST(mini_chat_device_cursors.last_delivered_sequence, excluded.last_delivered_sequence),
             last_read_sequence = GREATEST(mini_chat_device_cursors.last_read_sequence, excluded.last_read_sequence),
             last_sync_at = now()"#,
    )
    .bind(&principal_id)
    .bind(device_id)
    .bind(conversation_id)
    .bind(read_sequence)
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    tx.commit().await.map_err(|_| ChatError::StoreFailed)
}

pub(super) async fn mark_delivered(
    pool: &PgPool,
    principal: &Principal,
    conversation_id: &str,
    sequence: i64,
    device_id: &str,
) -> Result<(), ChatError> {
    let result = sqlx::query(
        r#"INSERT INTO mini_chat_device_cursors
             (principal_id, device_id, conversation_id, last_delivered_sequence,
              last_read_sequence, last_sync_at)
           SELECT member.principal_id, $4, member.conversation_id,
                  LEAST($3, conversation.last_message_sequence), 0, now()
           FROM mini_chat_conversation_members member
           JOIN mini_chat_conversations conversation
             ON conversation.conversation_id = member.conversation_id
           JOIN mini_chat_principals principal
             ON principal.principal_id = member.principal_id
           WHERE member.conversation_id = $1
             AND member.left_at IS NULL
             AND principal.principal_role = $2
             AND principal.principal_ref = $5
           ON CONFLICT (principal_id, device_id, conversation_id) DO UPDATE SET
             last_delivered_sequence = GREATEST(
               mini_chat_device_cursors.last_delivered_sequence,
               excluded.last_delivered_sequence
             ),
             last_sync_at = now()"#,
    )
    .bind(conversation_id)
    .bind(role_key(&principal.role))
    .bind(sequence.max(0))
    .bind(device_id)
    .bind(principal.ref_.trim())
    .execute(pool)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    if result.rows_affected() == 0 {
        return Err(ChatError::Forbidden);
    }
    Ok(())
}

pub(super) async fn issue_socket_ticket(
    pool: &PgPool,
    principal: &Principal,
    ticket: &str,
    expires_at_unix: i64,
) -> Result<(), ChatError> {
    let hash = ticket_hash(ticket);
    let mut tx = pool.begin().await.map_err(|_| ChatError::StoreFailed)?;
    sqlx::query(
        "DELETE FROM mini_chat_socket_tickets WHERE expires_at <= now() OR consumed_at IS NOT NULL",
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    sqlx::query(
        r#"INSERT INTO mini_chat_socket_tickets
             (ticket_hash, principal_role, principal_ref, display_name, legal_name,
              phone, avatar_url, expires_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, to_timestamp($8))"#,
    )
    .bind(hash)
    .bind(role_key(&principal.role))
    .bind(principal.ref_.trim())
    .bind(principal.display_name.trim())
    .bind(principal.legal_name.trim())
    .bind(principal.phone.trim())
    .bind(principal.avatar_url.trim())
    .bind(expires_at_unix)
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    tx.commit().await.map_err(|_| ChatError::StoreFailed)
}

pub(super) async fn consume_socket_ticket(
    pool: &PgPool,
    ticket: &str,
) -> Result<Principal, ChatError> {
    let row = sqlx::query_as::<_, (String, String, String, String, String, String)>(
        r#"UPDATE mini_chat_socket_tickets
           SET consumed_at = now()
           WHERE ticket_hash = $1
             AND consumed_at IS NULL
             AND expires_at > now()
           RETURNING principal_role, principal_ref, display_name, legal_name, phone, avatar_url"#,
    )
    .bind(ticket_hash(ticket))
    .fetch_optional(pool)
    .await
    .map_err(|_| ChatError::StoreFailed)?
    .ok_or(ChatError::NotFound)?;
    Ok(Principal {
        role: parse_role(&row.0)?,
        ref_: row.1,
        display_name: row.2,
        legal_name: row.3,
        phone: row.4,
        avatar_url: row.5,
    })
}

pub(super) async fn claim_push_deliveries(
    pool: &PgPool,
    limit: usize,
) -> Result<Vec<ChatPushDelivery>, ChatError> {
    let rows = sqlx::query_as::<_, (String, String, i32, i64, serde_json::Value)>(
        r#"WITH picked AS (
             SELECT event_id, recipient_key
             FROM mini_chat_push_deliveries
             WHERE delivered_at IS NULL
               AND dead_lettered_at IS NULL
               AND next_attempt_at <= now()
               AND (locked_until IS NULL OR locked_until < now())
               AND recipient_key NOT LIKE 'customer:%'
             ORDER BY next_attempt_at, created_at
             FOR UPDATE SKIP LOCKED
             LIMIT $1
           )
           UPDATE mini_chat_push_deliveries delivery
           SET attempts = delivery.attempts + 1,
               locked_until = now() + interval '5 minutes',
               updated_at = now()
           FROM picked, mini_chat_outbox_events event
           WHERE delivery.event_id = picked.event_id
             AND delivery.recipient_key = picked.recipient_key
             AND event.event_id = delivery.event_id
           RETURNING delivery.event_id, delivery.recipient_key, delivery.attempts,
                     event.event_cursor, event.payload_json"#,
    )
    .bind(limit.clamp(1, 100) as i64)
    .fetch_all(pool)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    rows.into_iter()
        .map(|(event_id, recipient_key, attempts, cursor, payload)| {
            let mut payload: ChatRealtimeEvent =
                serde_json::from_value(payload).map_err(|_| ChatError::StoreFailed)?;
            payload.cursor = cursor;
            Ok(ChatPushDelivery {
                event_id,
                recipient_key,
                attempts,
                payload,
            })
        })
        .collect()
}

pub(super) async fn mark_push_delivered(
    pool: &PgPool,
    event_id: &str,
    recipient_key: &str,
) -> Result<(), ChatError> {
    let mut tx = pool.begin().await.map_err(|_| ChatError::StoreFailed)?;
    sqlx::query(
        r#"UPDATE mini_chat_push_deliveries
           SET delivered_at = now(), locked_until = NULL, last_error = NULL, updated_at = now()
           WHERE event_id = $1 AND recipient_key = $2"#,
    )
    .bind(event_id)
    .bind(recipient_key)
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    finalize_outbox_if_complete(&mut tx, event_id).await?;
    tx.commit().await.map_err(|_| ChatError::StoreFailed)
}

pub(super) async fn reschedule_push_delivery(
    pool: &PgPool,
    event_id: &str,
    recipient_key: &str,
    retry_after_seconds: i64,
    dead_letter: bool,
    error: &str,
) -> Result<(), ChatError> {
    let mut tx = pool.begin().await.map_err(|_| ChatError::StoreFailed)?;
    sqlx::query(
        r#"UPDATE mini_chat_push_deliveries
           SET next_attempt_at = now() + make_interval(secs => $3),
               locked_until = NULL,
               dead_lettered_at = CASE WHEN $4 THEN now() ELSE NULL END,
               last_error = left($5, 500),
               updated_at = now()
           WHERE event_id = $1 AND recipient_key = $2"#,
    )
    .bind(event_id)
    .bind(recipient_key)
    .bind(retry_after_seconds.clamp(1, 3600) as f64)
    .bind(dead_letter)
    .bind(error)
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    if dead_letter {
        finalize_outbox_if_complete(&mut tx, event_id).await?;
    }
    tx.commit().await.map_err(|_| ChatError::StoreFailed)
}

async fn finalize_outbox_if_complete(
    tx: &mut Transaction<'_, Postgres>,
    event_id: &str,
) -> Result<(), ChatError> {
    sqlx::query(
        r#"UPDATE mini_chat_outbox_events event
           SET published_at = now(), locked_until = NULL
           WHERE event.event_id = $1
             AND NOT EXISTS (
               SELECT 1
               FROM mini_chat_push_deliveries delivery
               WHERE delivery.event_id = event.event_id
                 AND delivery.delivered_at IS NULL
                 AND delivery.dead_lettered_at IS NULL
             )"#,
    )
    .bind(event_id)
    .execute(&mut **tx)
    .await
    .map(|_| ())
    .map_err(|_| ChatError::StoreFailed)
}

pub(super) async fn claim_outbox(
    pool: &PgPool,
    limit: usize,
) -> Result<Vec<ChatOutboxEvent>, ChatError> {
    let rows = sqlx::query_as::<_, (String, i64, serde_json::Value, serde_json::Value)>(
        r#"WITH picked AS (
             SELECT event_id
             FROM mini_chat_outbox_events
             WHERE published_at IS NULL
               AND (locked_until IS NULL OR locked_until < now())
             ORDER BY created_at
             FOR UPDATE SKIP LOCKED
             LIMIT $1
           )
           UPDATE mini_chat_outbox_events event
           SET locked_until = now() + interval '30 seconds',
               attempts = attempts + 1
           FROM picked
           WHERE event.event_id = picked.event_id
           RETURNING event.event_id, event.event_cursor, event.recipient_keys, event.payload_json"#,
    )
    .bind(limit.clamp(1, 500) as i64)
    .fetch_all(pool)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    rows.into_iter()
        .map(|(event_id, cursor, recipient_keys, payload)| {
            let mut payload: ChatRealtimeEvent =
                serde_json::from_value(payload).map_err(|_| ChatError::StoreFailed)?;
            payload.cursor = cursor;
            Ok(ChatOutboxEvent {
                event_id,
                cursor,
                recipient_keys: serde_json::from_value(recipient_keys)
                    .map_err(|_| ChatError::StoreFailed)?,
                payload,
            })
        })
        .collect()
}

pub(super) async fn mark_outbox_published(pool: &PgPool, event_id: &str) -> Result<(), ChatError> {
    sqlx::query(
        "UPDATE mini_chat_outbox_events SET published_at = now(), locked_until = NULL WHERE event_id = $1",
    )
    .bind(event_id)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|_| ChatError::StoreFailed)
}

async fn sender_for_conversation(
    tx: &mut Transaction<'_, Postgres>,
    principal: &Principal,
    conversation_id: &str,
) -> Result<ChatPrincipal, ChatError> {
    let row = sqlx::query_as::<_, PrincipalRow>(
        r#"SELECT p.principal_id, p.principal_role, p.principal_ref, p.display_name, p.avatar_url
           FROM mini_chat_principals p
           JOIN mini_chat_conversation_members member ON member.principal_id = p.principal_id
           WHERE p.principal_role = $1
             AND p.principal_ref = $2
             AND p.principal_role <> 'customer'
             AND member.conversation_id = $3
             AND member.left_at IS NULL
             AND member.can_post = TRUE
             AND p.active = TRUE
             AND NOT EXISTS (
               SELECT 1
               FROM mini_chat_conversation_members customer_member
               JOIN mini_chat_principals customer_principal
                 ON customer_principal.principal_id = customer_member.principal_id
               WHERE customer_member.conversation_id = member.conversation_id
                 AND customer_member.left_at IS NULL
                 AND customer_principal.principal_role = 'customer'
             )"#,
    )
    .bind(role_key(&principal.role))
    .bind(principal.ref_.trim())
    .bind(conversation_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?
    .ok_or(ChatError::Forbidden)?;
    row.into_model()
}

async fn existing_message(
    tx: &mut Transaction<'_, Postgres>,
    conversation_id: &str,
    sender_principal_id: &str,
    client_message_id: &str,
) -> Result<Option<ChatMessage>, ChatError> {
    let row = sqlx::query_as::<_, MessageRow>(
        r#"SELECT
             m.message_id,
             m.conversation_id,
             m.sender_principal_id,
             sender.principal_role AS sender_role,
             sender.principal_ref AS sender_ref,
             sender.display_name AS sender_display_name,
             m.client_message_id,
             m.message_sequence,
             m.message_type,
             m.body,
             attachment.attachment_id,
             media.media_id,
             media.media_kind,
             media.processed_content_type AS media_content_type,
             media.processed_size_bytes AS media_size_bytes,
             media.width_pixels AS media_width_pixels,
             media.height_pixels AS media_height_pixels,
             media.duration_ms AS media_duration_ms,
             EXTRACT(EPOCH FROM m.created_at)::BIGINT AS created_at_unix,
             EXTRACT(EPOCH FROM m.edited_at)::BIGINT AS edited_at_unix,
             EXTRACT(EPOCH FROM m.deleted_at)::BIGINT AS deleted_at_unix
           FROM mini_chat_messages m
           JOIN mini_chat_principals sender ON sender.principal_id = m.sender_principal_id
           LEFT JOIN mini_chat_message_attachments attachment ON attachment.message_id = m.message_id
           LEFT JOIN mini_chat_media media ON media.media_id = attachment.media_id
           WHERE m.conversation_id = $1
             AND m.sender_principal_id = $2
             AND m.client_message_id = $3"#,
    )
    .bind(conversation_id)
    .bind(sender_principal_id)
    .bind(client_message_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    row.map(MessageRow::into_model).transpose()
}

fn idempotent_send_result(
    existing: ChatMessage,
    body: &str,
    media_id: Option<&str>,
) -> Result<ChatSendResult, ChatError> {
    let existing_media_id = existing
        .attachment
        .as_ref()
        .map(|attachment| attachment.media_id.as_str());
    if existing.body != body || existing_media_id != media_id {
        return Err(ChatError::Conflict);
    }
    Ok(ChatSendResult {
        message: existing,
        created: false,
    })
}

async fn ready_attachment(
    tx: &mut Transaction<'_, Postgres>,
    conversation_id: &str,
    sender_principal_id: &str,
    media_id: &str,
) -> Result<ChatMessageAttachment, ChatError> {
    let row = sqlx::query_as::<_, (String, String, i64, i32, i32, Option<i64>)>(
        r#"SELECT media_kind, processed_content_type, processed_size_bytes,
                  width_pixels, height_pixels, duration_ms
           FROM mini_chat_media media
           WHERE media.media_id = $1
             AND media.conversation_id = $2
             AND media.uploader_principal_id = $3
             AND media.upload_status = 'ready'
             AND media.processed_object_key IS NOT NULL
             AND media.thumbnail_object_key IS NOT NULL
             AND NOT EXISTS (
               SELECT 1 FROM mini_chat_message_attachments attachment
               WHERE attachment.media_id = media.media_id
             )
           FOR UPDATE"#,
    )
    .bind(media_id)
    .bind(conversation_id)
    .bind(sender_principal_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?
    .ok_or(ChatError::Conflict)?;
    let (kind, content_type, size_bytes, width_pixels, height_pixels, duration_ms) = row;
    if !matches!(kind.as_str(), "image" | "video" | "audio") {
        return Err(ChatError::StoreFailed);
    }
    Ok(ChatMessageAttachment {
        attachment_id: new_id("attachment"),
        media_id: media_id.to_string(),
        kind,
        content_type,
        size_bytes,
        width_pixels,
        height_pixels,
        duration_ms,
        content_url: format!("/v1/mobile/chat/media/{media_id}/content"),
        thumbnail_url: format!("/v1/mobile/chat/media/{media_id}/thumbnail"),
    })
}

fn new_id(prefix: &str) -> String {
    let bytes: [u8; 16] = rand::random();
    format!("{prefix}_{}", data_encoding::HEXLOWER.encode(&bytes))
}

fn ticket_hash(ticket: &str) -> Vec<u8> {
    Sha256::digest(ticket.trim().as_bytes()).to_vec()
}
