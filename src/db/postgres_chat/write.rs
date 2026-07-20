use sqlx::{PgPool, Postgres, Transaction};

use sha2::{Digest, Sha256};

use super::rows::{MessageRow, PrincipalRow, parse_role, role_key};
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::chat::{
    ChatConversation, ChatError, ChatMessage, ChatMessageAttachment, ChatOutboxEvent,
    ChatPrincipal, ChatPrincipalInput, ChatPushDelivery, ChatRealtimeEvent, ChatSendResult,
};

pub(super) async fn ensure_principal(
    pool: &PgPool,
    principal: ChatPrincipalInput,
) -> Result<ChatPrincipal, ChatError> {
    if principal.role == PrincipalRole::Customer {
        return Err(ChatError::Forbidden);
    }
    let row = sqlx::query_as::<_, PrincipalRow>(
        r#"INSERT INTO mini_chat_principals
             (principal_id, principal_role, principal_ref, display_name, avatar_url)
           VALUES ($1, $2, $3, $4, $5)
           ON CONFLICT (principal_role, principal_ref) DO UPDATE SET
             display_name = excluded.display_name,
             avatar_url = excluded.avatar_url,
             active = TRUE,
             updated_at = now()
           RETURNING principal_id, principal_role, principal_ref, display_name, avatar_url"#,
    )
    .bind(new_id("principal"))
    .bind(role_key(&principal.role))
    .bind(principal.ref_.trim())
    .bind(principal.display_name.trim())
    .bind(principal.avatar_url.trim())
    .fetch_one(pool)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    row.into_model()
}

pub(super) async fn create_or_get_dm(
    pool: &PgPool,
    actor: &ChatPrincipal,
    target: &ChatPrincipal,
) -> Result<ChatConversation, ChatError> {
    if actor.role == PrincipalRole::Customer || target.role == PrincipalRole::Customer {
        return Err(ChatError::Forbidden);
    }
    let mut ids = [actor.principal_id.as_str(), target.principal_id.as_str()];
    ids.sort_unstable();
    let dm_key = format!("{}:{}", ids[0], ids[1]);
    let mut tx = pool.begin().await.map_err(|_| ChatError::StoreFailed)?;
    let conversation_id = sqlx::query_scalar::<_, String>(
        r#"INSERT INTO mini_chat_conversations
             (conversation_id, kind, dm_key, created_by_principal_id)
           VALUES ($1, 'dm', $2, $3)
           ON CONFLICT (dm_key) WHERE dm_key IS NOT NULL
           DO UPDATE SET dm_key = excluded.dm_key
           RETURNING conversation_id"#,
    )
    .bind(new_id("conversation"))
    .bind(&dm_key)
    .bind(&actor.principal_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    for principal in [actor, target] {
        sqlx::query(
            r#"INSERT INTO mini_chat_conversation_members
                 (conversation_id, principal_id, member_role)
               VALUES ($1, $2, $3)
               ON CONFLICT (conversation_id, principal_id) DO UPDATE SET left_at = NULL"#,
        )
        .bind(&conversation_id)
        .bind(&principal.principal_id)
        .bind(if principal.principal_id == actor.principal_id {
            "owner"
        } else {
            "member"
        })
        .execute(&mut *tx)
        .await
        .map_err(|_| ChatError::StoreFailed)?;
    }
    tx.commit().await.map_err(|_| ChatError::StoreFailed)?;
    Ok(ChatConversation {
        conversation_id,
        kind: "dm".to_string(),
        title: String::new(),
        peer: Some(target.clone()),
        last_message: None,
        last_message_sequence: 0,
        unread_count: 0,
        updated_at_unix: time::OffsetDateTime::now_utc().unix_timestamp(),
    })
}

pub(super) async fn send_message(
    pool: &PgPool,
    principal: &Principal,
    conversation_id: &str,
    client_message_id: &str,
    body: &str,
    media_id: Option<&str>,
) -> Result<ChatSendResult, ChatError> {
    let mut tx = pool.begin().await.map_err(|_| ChatError::StoreFailed)?;
    let sender = sender_for_conversation(&mut tx, principal, conversation_id).await?;
    if let Some(existing) = existing_message(
        &mut tx,
        conversation_id,
        &sender.principal_id,
        client_message_id,
    )
    .await?
    {
        let result = idempotent_send_result(existing, body, media_id)?;
        tx.commit().await.map_err(|_| ChatError::StoreFailed)?;
        return Ok(result);
    }
    let last_sequence = sqlx::query_scalar::<_, i64>(
        "SELECT last_message_sequence FROM mini_chat_conversations WHERE conversation_id = $1 FOR UPDATE",
    )
    .bind(conversation_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?
    .ok_or(ChatError::NotFound)?;
    // The first idempotency lookup happens before the conversation write lock.
    // A concurrent retry can therefore commit while this transaction is waiting
    // for the lock. Recheck after acquiring it so both callers resolve to the
    // same persisted message instead of leaking the unique-constraint race as a
    // store failure.
    if let Some(existing) = existing_message(
        &mut tx,
        conversation_id,
        &sender.principal_id,
        client_message_id,
    )
    .await?
    {
        let result = idempotent_send_result(existing, body, media_id)?;
        tx.commit().await.map_err(|_| ChatError::StoreFailed)?;
        return Ok(result);
    }
    let sequence = last_sequence.saturating_add(1);
    let ready_attachment = match media_id {
        Some(media_id) => {
            Some(ready_attachment(&mut tx, conversation_id, &sender.principal_id, media_id).await?)
        }
        None => None,
    };
    let message_type = ready_attachment
        .as_ref()
        .map(|attachment| attachment.kind.as_str())
        .unwrap_or("text");
    let message_id = new_id("message");
    let row = sqlx::query_as::<_, MessageRow>(
        r#"INSERT INTO mini_chat_messages
             (message_id, conversation_id, sender_principal_id, client_message_id,
              message_sequence, message_type, body)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING
             message_id,
             conversation_id,
             sender_principal_id,
             $8::TEXT AS sender_role,
             $9::TEXT AS sender_ref,
             $10::TEXT AS sender_display_name,
             client_message_id,
             message_sequence,
             message_type,
             body,
             NULL::TEXT AS attachment_id,
             NULL::TEXT AS media_id,
             NULL::TEXT AS media_kind,
             NULL::TEXT AS media_content_type,
             NULL::BIGINT AS media_size_bytes,
             NULL::INTEGER AS media_width_pixels,
             NULL::INTEGER AS media_height_pixels,
             NULL::BIGINT AS media_duration_ms,
             EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
             NULL::BIGINT AS edited_at_unix,
             NULL::BIGINT AS deleted_at_unix"#,
    )
    .bind(&message_id)
    .bind(conversation_id)
    .bind(&sender.principal_id)
    .bind(client_message_id)
    .bind(sequence)
    .bind(message_type)
    .bind(body)
    .bind(role_key(&sender.role))
    .bind(&sender.ref_)
    .bind(&sender.display_name)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    let mut message = row.into_model()?;
    if let Some(attachment) = ready_attachment {
        sqlx::query(
            r#"INSERT INTO mini_chat_message_attachments
                 (attachment_id, message_id, conversation_id, media_id, ordinal)
               VALUES ($1, $2, $3, $4, 0)"#,
        )
        .bind(&attachment.attachment_id)
        .bind(&message_id)
        .bind(conversation_id)
        .bind(&attachment.media_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ChatError::Conflict)?;
        message.attachment = Some(attachment);
    }
    sqlx::query(
        "UPDATE mini_chat_conversations SET last_message_sequence = $2, updated_at = now() WHERE conversation_id = $1",
    )
    .bind(conversation_id)
    .bind(sequence)
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    sqlx::query(
        r#"UPDATE mini_chat_conversation_members
           SET last_read_sequence = GREATEST(last_read_sequence, $3)
           WHERE conversation_id = $1 AND principal_id = $2 AND left_at IS NULL"#,
    )
    .bind(conversation_id)
    .bind(&sender.principal_id)
    .bind(sequence)
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    let recipients = sqlx::query_as::<_, (String, String, String)>(
        r#"SELECT recipient.principal_id, recipient.principal_role, recipient.principal_ref
           FROM mini_chat_conversation_members member
           JOIN mini_chat_principals recipient ON recipient.principal_id = member.principal_id
           WHERE member.conversation_id = $1
             AND member.left_at IS NULL
             AND recipient.principal_role <> 'customer'
             AND recipient.active = TRUE"#,
    )
    .bind(conversation_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    let recipient_keys = recipients
        .iter()
        .map(|(_, role, ref_)| format!("{role}:{ref_}"))
        .collect::<Vec<_>>();
    let push_recipient_keys = recipients
        .iter()
        .filter(|(principal_id, _, _)| principal_id != &sender.principal_id)
        .map(|(_, role, ref_)| format!("{role}:{ref_}"))
        .collect::<Vec<_>>();
    let cursor = sqlx::query_scalar::<_, i64>(
        r#"UPDATE mini_chat_event_clock
           SET cursor = cursor + 1
           WHERE singleton = TRUE
           RETURNING cursor"#,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    let event = ChatRealtimeEvent {
        event_id: new_id("event"),
        cursor,
        event: "chat.message.created".to_string(),
        conversation_id: conversation_id.to_string(),
        sequence,
        message: message.clone(),
    };
    sqlx::query(
        r#"INSERT INTO mini_chat_outbox_events
             (event_id, event_cursor, topic, conversation_id, message_sequence,
              recipient_keys, push_recipient_keys, payload_json)
           VALUES ($1, $2, 'chat.message.created', $3, $4, $5, $6, $7)"#,
    )
    .bind(&event.event_id)
    .bind(cursor)
    .bind(conversation_id)
    .bind(sequence)
    .bind(serde_json::json!(recipient_keys))
    .bind(serde_json::json!(push_recipient_keys))
    .bind(serde_json::to_value(&event).map_err(|_| ChatError::StoreFailed)?)
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    sqlx::query(
        r#"INSERT INTO mini_chat_push_deliveries (event_id, recipient_key)
           SELECT $1, value
           FROM jsonb_array_elements_text($2::JSONB) value
           ON CONFLICT (event_id, recipient_key) DO NOTHING"#,
    )
    .bind(&event.event_id)
    .bind(serde_json::json!(push_recipient_keys))
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    finalize_outbox_if_complete(&mut tx, &event.event_id).await?;
    sqlx::query("SELECT pg_notify('mini_chat_realtime', $1)")
        .bind(&event.event_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ChatError::StoreFailed)?;
    tx.commit().await.map_err(|_| ChatError::StoreFailed)?;
    Ok(ChatSendResult {
        message,
        created: true,
    })
}

include!("write_delivery.rs");
