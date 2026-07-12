use sqlx::{PgPool, Postgres, Transaction};

use super::rows::{MessageRow, PrincipalRow, role_key};
use crate::core::auth::models::Principal;
use crate::core::chat::{
    ChatConversation, ChatError, ChatMessage, ChatOutboxEvent, ChatPrincipal, ChatPrincipalInput,
    ChatRealtimeEvent, ChatSendResult,
};

pub(super) async fn ensure_principal(
    pool: &PgPool,
    principal: ChatPrincipalInput,
) -> Result<ChatPrincipal, ChatError> {
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
        tx.commit().await.map_err(|_| ChatError::StoreFailed)?;
        return Ok(ChatSendResult {
            message: existing,
            created: false,
        });
    }
    let last_sequence = sqlx::query_scalar::<_, i64>(
        "SELECT last_message_sequence FROM mini_chat_conversations WHERE conversation_id = $1 FOR UPDATE",
    )
    .bind(conversation_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?
    .ok_or(ChatError::NotFound)?;
    let sequence = last_sequence.saturating_add(1);
    let message_id = new_id("message");
    let row = sqlx::query_as::<_, MessageRow>(
        r#"INSERT INTO mini_chat_messages
             (message_id, conversation_id, sender_principal_id, client_message_id,
              message_sequence, message_type, body)
           VALUES ($1, $2, $3, $4, $5, 'text', $6)
           RETURNING
             message_id,
             conversation_id,
             sender_principal_id,
             $7::TEXT AS sender_role,
             $8::TEXT AS sender_ref,
             $9::TEXT AS sender_display_name,
             client_message_id,
             message_sequence,
             message_type,
             body,
             EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
             NULL::BIGINT AS edited_at_unix,
             NULL::BIGINT AS deleted_at_unix"#,
    )
    .bind(&message_id)
    .bind(conversation_id)
    .bind(&sender.principal_id)
    .bind(client_message_id)
    .bind(sequence)
    .bind(body)
    .bind(role_key(&sender.role))
    .bind(&sender.ref_)
    .bind(&sender.display_name)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    let message = row.into_model()?;
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
    let recipient_keys = sqlx::query_as::<_, (String, String)>(
        r#"SELECT recipient.principal_role, recipient.principal_ref
           FROM mini_chat_conversation_members member
           JOIN mini_chat_principals recipient ON recipient.principal_id = member.principal_id
           WHERE member.conversation_id = $1
             AND member.principal_id <> $2
             AND member.left_at IS NULL
             AND recipient.active = TRUE"#,
    )
    .bind(conversation_id)
    .bind(&sender.principal_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?
    .into_iter()
    .map(|(role, ref_)| format!("{role}:{ref_}"))
    .collect::<Vec<_>>();
    let event = ChatRealtimeEvent {
        event_id: new_id("event"),
        event: "chat.message.created".to_string(),
        conversation_id: conversation_id.to_string(),
        sequence,
        message: message.clone(),
    };
    sqlx::query(
        r#"INSERT INTO mini_chat_outbox_events
             (event_id, topic, conversation_id, message_sequence, recipient_keys, payload_json)
           VALUES ($1, 'chat.message.created', $2, $3, $4, $5)"#,
    )
    .bind(&event.event_id)
    .bind(conversation_id)
    .bind(sequence)
    .bind(serde_json::json!(recipient_keys))
    .bind(serde_json::to_value(&event).map_err(|_| ChatError::StoreFailed)?)
    .execute(&mut *tx)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    tx.commit().await.map_err(|_| ChatError::StoreFailed)?;
    Ok(ChatSendResult {
        message,
        created: true,
    })
}

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

pub(super) async fn claim_outbox(
    pool: &PgPool,
    limit: usize,
) -> Result<Vec<ChatOutboxEvent>, ChatError> {
    let rows = sqlx::query_as::<_, (String, serde_json::Value, serde_json::Value)>(
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
           RETURNING event.event_id, event.recipient_keys, event.payload_json"#,
    )
    .bind(limit.clamp(1, 500) as i64)
    .fetch_all(pool)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    rows.into_iter()
        .map(|(event_id, recipient_keys, payload)| {
            Ok(ChatOutboxEvent {
                event_id,
                recipient_keys: serde_json::from_value(recipient_keys)
                    .map_err(|_| ChatError::StoreFailed)?,
                payload: serde_json::from_value(payload).map_err(|_| ChatError::StoreFailed)?,
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
             AND member.conversation_id = $3
             AND member.left_at IS NULL
             AND member.can_post = TRUE
             AND p.active = TRUE"#,
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
             EXTRACT(EPOCH FROM m.created_at)::BIGINT AS created_at_unix,
             EXTRACT(EPOCH FROM m.edited_at)::BIGINT AS edited_at_unix,
             EXTRACT(EPOCH FROM m.deleted_at)::BIGINT AS deleted_at_unix
           FROM mini_chat_messages m
           JOIN mini_chat_principals sender ON sender.principal_id = m.sender_principal_id
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

fn new_id(prefix: &str) -> String {
    let bytes: [u8; 16] = rand::random();
    format!("{prefix}_{}", data_encoding::HEXLOWER.encode(&bytes))
}
