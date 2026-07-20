use sqlx::PgPool;

use super::rows::{ConversationRow, MessageRow, role_key};
use crate::core::auth::models::Principal;
use crate::core::chat::{
    ChatConversation, ChatError, ChatMessagePage, ChatOutboxEvent, ChatRealtimeEvent,
};

const CONVERSATION_SELECT: &str = r#"
SELECT
  c.conversation_id,
  c.kind,
  c.title,
  c.last_message_sequence,
  GREATEST(c.last_message_sequence - me.last_read_sequence, 0)::BIGINT AS unread_count,
  EXTRACT(EPOCH FROM c.updated_at)::BIGINT AS updated_at_unix,
  peer.principal_id AS peer_principal_id,
  peer.principal_role AS peer_role,
  peer.principal_ref AS peer_ref,
  peer.display_name AS peer_display_name,
  peer.avatar_url AS peer_avatar_url,
  lm.message_id,
  sender.principal_id AS sender_principal_id,
  sender.principal_role AS sender_role,
  sender.principal_ref AS sender_ref,
  sender.display_name AS sender_display_name,
  lm.client_message_id,
  lm.message_sequence,
  lm.message_type,
  lm.body,
  attachment.attachment_id,
  media.media_id,
  media.media_kind,
  media.processed_content_type AS media_content_type,
  media.processed_size_bytes AS media_size_bytes,
  media.width_pixels AS media_width_pixels,
  media.height_pixels AS media_height_pixels,
  media.duration_ms AS media_duration_ms,
  EXTRACT(EPOCH FROM lm.created_at)::BIGINT AS message_created_at_unix,
  EXTRACT(EPOCH FROM lm.edited_at)::BIGINT AS edited_at_unix,
  EXTRACT(EPOCH FROM lm.deleted_at)::BIGINT AS deleted_at_unix
FROM mini_chat_principals current_principal
JOIN mini_chat_conversation_members me
  ON me.principal_id = current_principal.principal_id AND me.left_at IS NULL
JOIN mini_chat_conversations c ON c.conversation_id = me.conversation_id
LEFT JOIN LATERAL (
  SELECT p.*
  FROM mini_chat_conversation_members other_member
  JOIN mini_chat_principals p ON p.principal_id = other_member.principal_id
  WHERE other_member.conversation_id = c.conversation_id
    AND other_member.principal_id <> current_principal.principal_id
    AND other_member.left_at IS NULL
  ORDER BY other_member.joined_at
  LIMIT 1
) peer ON TRUE
LEFT JOIN mini_chat_messages lm
  ON lm.conversation_id = c.conversation_id
 AND lm.message_sequence = c.last_message_sequence
LEFT JOIN mini_chat_principals sender ON sender.principal_id = lm.sender_principal_id
LEFT JOIN mini_chat_message_attachments attachment ON attachment.message_id = lm.message_id
LEFT JOIN mini_chat_media media ON media.media_id = attachment.media_id
WHERE current_principal.principal_role = $1
  AND current_principal.principal_ref = $2
  AND c.last_message_sequence > 0
  AND NOT EXISTS (
    SELECT 1
    FROM mini_chat_conversation_members customer_member
    JOIN mini_chat_principals customer_principal
      ON customer_principal.principal_id = customer_member.principal_id
    WHERE customer_member.conversation_id = c.conversation_id
      AND customer_member.left_at IS NULL
      AND customer_principal.principal_role = 'customer'
  )
"#;

pub(super) async fn conversations(
    pool: &PgPool,
    principal: &Principal,
    limit: usize,
    offset: usize,
) -> Result<Vec<ChatConversation>, ChatError> {
    let query = format!("{CONVERSATION_SELECT} ORDER BY c.updated_at DESC LIMIT $3 OFFSET $4");
    let rows = sqlx::query_as::<_, ConversationRow>(&query)
        .bind(role_key(&principal.role))
        .bind(principal.ref_.trim())
        .bind(limit as i64)
        .bind(offset as i64)
        .fetch_all(pool)
        .await
        .map_err(|_| ChatError::StoreFailed)?;
    rows.into_iter().map(ConversationRow::into_model).collect()
}

#[cfg(test)]
mod tests {
    use super::CONVERSATION_SELECT;

    #[test]
    fn conversation_list_excludes_threads_without_messages() {
        assert!(CONVERSATION_SELECT.contains("c.last_message_sequence > 0"));
        assert!(CONVERSATION_SELECT.contains("customer_principal.principal_role = 'customer'"));
    }
}

pub(super) async fn messages(
    pool: &PgPool,
    principal: &Principal,
    conversation_id: &str,
    before_sequence: Option<i64>,
    after_sequence: Option<i64>,
    limit: usize,
) -> Result<ChatMessagePage, ChatError> {
    if before_sequence.is_some() && after_sequence.is_some() {
        return Err(ChatError::InvalidInput);
    }
    let is_member = sqlx::query_scalar::<_, bool>(
        r#"SELECT EXISTS (
             SELECT 1
             FROM mini_chat_conversation_members member
             JOIN mini_chat_principals viewer ON viewer.principal_id = member.principal_id
             WHERE member.conversation_id = $1
               AND member.left_at IS NULL
               AND viewer.principal_role = $2
               AND viewer.principal_ref = $3
               AND NOT EXISTS (
                 SELECT 1
                 FROM mini_chat_conversation_members customer_member
                 JOIN mini_chat_principals customer_principal
                   ON customer_principal.principal_id = customer_member.principal_id
                 WHERE customer_member.conversation_id = member.conversation_id
                   AND customer_member.left_at IS NULL
                   AND customer_principal.principal_role = 'customer'
               )
           )"#,
    )
    .bind(conversation_id)
    .bind(role_key(&principal.role))
    .bind(principal.ref_.trim())
    .fetch_one(pool)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    if !is_member {
        return Err(ChatError::Forbidden);
    }
    let rows = sqlx::query_as::<_, MessageRow>(
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
             AND ($2::BIGINT IS NULL OR m.message_sequence < $2)
             AND ($3::BIGINT IS NULL OR m.message_sequence > $3)
           ORDER BY
             CASE WHEN $3::BIGINT IS NOT NULL THEN m.message_sequence END ASC,
             CASE WHEN $3::BIGINT IS NULL THEN m.message_sequence END DESC
           LIMIT $4"#,
    )
    .bind(conversation_id)
    .bind(before_sequence)
    .bind(after_sequence)
    .bind(limit.saturating_add(1) as i64)
    .fetch_all(pool)
    .await
    .map_err(|_| ChatError::StoreFailed)?;

    let has_more = rows.len() > limit;
    let mut items = rows
        .into_iter()
        .take(limit)
        .map(MessageRow::into_model)
        .collect::<Result<Vec<_>, _>>()?;
    if after_sequence.is_none() {
        items.reverse();
    }
    Ok(ChatMessagePage { items, has_more })
}

pub(super) async fn sync_events(
    pool: &PgPool,
    principal: &Principal,
    after_cursor: i64,
    limit: usize,
) -> Result<(Vec<ChatRealtimeEvent>, i64, bool), ChatError> {
    let key = format!("{}:{}", role_key(&principal.role), principal.ref_.trim());
    // Capture the high watermark first. Event cursors are allocated through the
    // transactionally locked clock, so anything committing later is guaranteed
    // to be above this watermark and will be returned by the next sync.
    let watermark = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(event_cursor), 0) FROM mini_chat_outbox_events",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| ChatError::StoreFailed)?
    .max(after_cursor);
    if watermark <= after_cursor {
        return Ok((Vec::new(), after_cursor, false));
    }
    let rows = sqlx::query_as::<_, (i64, serde_json::Value)>(
        r#"SELECT event.event_cursor, event.payload_json
           FROM mini_chat_outbox_events event
           WHERE event.event_cursor > $1
             AND event.event_cursor <= $2
             AND event.recipient_keys @> to_jsonb(ARRAY[$3]::TEXT[])
             AND NOT EXISTS (
               SELECT 1
               FROM mini_chat_conversation_members customer_member
               JOIN mini_chat_principals customer_principal
                 ON customer_principal.principal_id = customer_member.principal_id
               WHERE customer_member.conversation_id = event.conversation_id
                 AND customer_member.left_at IS NULL
                 AND customer_principal.principal_role = 'customer'
             )
           ORDER BY event.event_cursor ASC
           LIMIT $4"#,
    )
    .bind(after_cursor.max(0))
    .bind(watermark)
    .bind(key)
    .bind(limit.saturating_add(1) as i64)
    .fetch_all(pool)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    let has_more = rows.len() > limit;
    let mut events = Vec::with_capacity(rows.len().min(limit));
    for (cursor, payload) in rows.into_iter().take(limit) {
        let mut event: ChatRealtimeEvent =
            serde_json::from_value(payload).map_err(|_| ChatError::StoreFailed)?;
        event.cursor = cursor;
        events.push(event);
    }
    let next_cursor = if has_more {
        events
            .last()
            .map(|event| event.cursor)
            .unwrap_or(after_cursor)
    } else {
        watermark
    };
    Ok((events, next_cursor, has_more))
}

pub async fn outbox_event(pool: &PgPool, event_id: &str) -> Result<ChatOutboxEvent, ChatError> {
    let (cursor, recipient_keys, payload) =
        sqlx::query_as::<_, (i64, serde_json::Value, serde_json::Value)>(
            r#"SELECT event.event_cursor, event.recipient_keys, event.payload_json
               FROM mini_chat_outbox_events event
               WHERE event.event_id = $1
                 AND NOT EXISTS (
                   SELECT 1
                   FROM mini_chat_conversation_members customer_member
                   JOIN mini_chat_principals customer_principal
                     ON customer_principal.principal_id = customer_member.principal_id
                   WHERE customer_member.conversation_id = event.conversation_id
                     AND customer_member.left_at IS NULL
                     AND customer_principal.principal_role = 'customer'
                 )"#,
        )
        .bind(event_id)
        .fetch_optional(pool)
        .await
        .map_err(|_| ChatError::StoreFailed)?
        .ok_or(ChatError::NotFound)?;
    let mut payload: ChatRealtimeEvent =
        serde_json::from_value(payload).map_err(|_| ChatError::StoreFailed)?;
    payload.cursor = cursor;
    Ok(ChatOutboxEvent {
        event_id: event_id.to_string(),
        cursor,
        recipient_keys: serde_json::from_value(recipient_keys)
            .map_err(|_| ChatError::StoreFailed)?,
        payload,
    })
}
