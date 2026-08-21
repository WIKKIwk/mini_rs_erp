
pub(super) async fn upsert_order_freeze_card(
    pool: &PgPool,
    principal: &Principal,
    conversation_id: &str,
    card_event: &OrderFreezeChatEvent,
) -> Result<ChatSendResult, ChatError> {
    let mut tx = pool.begin().await.map_err(|_| ChatError::StoreFailed)?;
    let sender = sender_for_conversation(&mut tx, principal, conversation_id).await?;
    let client_message_id = format!("order-freeze-request:{}", card_event.request_id.trim());
    let body = card_event.message_body();
    let metadata = card_event.metadata();
    let existing = existing_message(
        &mut tx,
        conversation_id,
        &sender.principal_id,
        &client_message_id,
    )
    .await?;
    let existing_event_sequence = existing
        .as_ref()
        .and_then(|message| message.metadata.get("event_sequence"))
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default();
    if existing_event_sequence >= card_event.event_sequence {
        tx.commit().await.map_err(|_| ChatError::StoreFailed)?;
        return Ok(ChatSendResult {
            message: existing.ok_or(ChatError::Conflict)?,
            created: false,
        });
    }

    let (message, created) = if let Some(existing) = existing {
        let row = sqlx::query_as::<_, MessageRow>(ORDER_FREEZE_UPDATE_MESSAGE_SQL)
            .bind(conversation_id)
            .bind(&sender.principal_id)
            .bind(&client_message_id)
            .bind(&body)
            .bind(&metadata)
            .bind(role_key(&sender.role))
            .bind(&sender.ref_)
            .bind(&sender.display_name)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| ChatError::StoreFailed)?;
        debug_assert_eq!(row.message_id, existing.message_id);
        (row.into_model()?, false)
    } else {
        let last_sequence = sqlx::query_scalar::<_, i64>(
            r#"SELECT last_message_sequence
               FROM mini_chat_conversations
               WHERE conversation_id = $1
               FOR UPDATE"#,
        )
        .bind(conversation_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ChatError::StoreFailed)?
        .ok_or(ChatError::NotFound)?;
        let sequence = last_sequence.saturating_add(1);
        let row = sqlx::query_as::<_, MessageRow>(ORDER_FREEZE_INSERT_MESSAGE_SQL)
            .bind(new_id("message"))
            .bind(conversation_id)
            .bind(&sender.principal_id)
            .bind(&client_message_id)
            .bind(sequence)
            .bind(&body)
            .bind(&metadata)
            .bind(role_key(&sender.role))
            .bind(&sender.ref_)
            .bind(&sender.display_name)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| ChatError::StoreFailed)?;
        sqlx::query(
            r#"UPDATE mini_chat_conversations
               SET last_message_sequence = $2, updated_at = now()
               WHERE conversation_id = $1"#,
        )
        .bind(conversation_id)
        .bind(sequence)
        .execute(&mut *tx)
        .await
        .map_err(|_| ChatError::StoreFailed)?;
        sqlx::query(
            r#"UPDATE mini_chat_conversation_members
               SET last_read_sequence = GREATEST(last_read_sequence, $3)
               WHERE conversation_id = $1
                 AND principal_id = $2
                 AND left_at IS NULL"#,
        )
        .bind(conversation_id)
        .bind(&sender.principal_id)
        .bind(sequence)
        .execute(&mut *tx)
        .await
        .map_err(|_| ChatError::StoreFailed)?;
        (row.into_model()?, true)
    };

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
    let push_recipient_keys = if created {
        recipients
            .iter()
            .filter(|(principal_id, _, _)| principal_id != &sender.principal_id)
            .map(|(_, role, ref_)| format!("{role}:{ref_}"))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
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
        event: if created {
            "chat.message.created".to_string()
        } else {
            "chat.message.updated".to_string()
        },
        conversation_id: conversation_id.to_string(),
        sequence: message.sequence,
        message: message.clone(),
    };
    sqlx::query(
        r#"INSERT INTO mini_chat_outbox_events
             (event_id, event_cursor, topic, conversation_id, message_sequence,
              recipient_keys, push_recipient_keys, payload_json)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
    )
    .bind(&event.event_id)
    .bind(cursor)
    .bind(&event.event)
    .bind(conversation_id)
    .bind(message.sequence)
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
    Ok(ChatSendResult { message, created })
}

pub(super) async fn upsert_inventory_transfer_card(
    pool: &PgPool,
    principal: &Principal,
    conversation_id: &str,
    card_event: &InventoryTransferChatEvent,
) -> Result<ChatSendResult, ChatError> {
    let mut tx = pool.begin().await.map_err(|_| ChatError::StoreFailed)?;
    let sender = sender_for_conversation(&mut tx, principal, conversation_id).await?;
    let client_message_id = format!(
        "inventory-transfer-request:{}",
        card_event.transfer_id.trim()
    );
    let body = card_event.message_body();
    let metadata = card_event.metadata();
    let existing = existing_message(
        &mut tx,
        conversation_id,
        &sender.principal_id,
        &client_message_id,
    )
    .await?;
    let existing_event_sequence = existing
        .as_ref()
        .and_then(|message| message.metadata.get("event_sequence"))
        .and_then(serde_json::Value::as_i64)
        .unwrap_or_default();
    if existing_event_sequence >= card_event.event_sequence {
        tx.commit().await.map_err(|_| ChatError::StoreFailed)?;
        return Ok(ChatSendResult {
            message: existing.ok_or(ChatError::Conflict)?,
            created: false,
        });
    }

    let (message, created) = if let Some(existing) = existing {
        let row = sqlx::query_as::<_, MessageRow>(
            r#"UPDATE mini_chat_messages
               SET body = $4,
                   message_type = 'inventory_transfer_request',
                   metadata_json = $5,
                   edited_at = now()
               WHERE conversation_id = $1
                 AND sender_principal_id = $2
                 AND client_message_id = $3
               RETURNING
                 message_id, conversation_id, sender_principal_id,
                 $6::TEXT AS sender_role, $7::TEXT AS sender_ref,
                 $8::TEXT AS sender_display_name, client_message_id,
                 message_sequence, message_type, body, metadata_json,
                 NULL::TEXT AS attachment_id, NULL::TEXT AS media_id,
                 NULL::TEXT AS media_kind, NULL::TEXT AS media_content_type,
                 NULL::BIGINT AS media_size_bytes, NULL::INTEGER AS media_width_pixels,
                 NULL::INTEGER AS media_height_pixels, NULL::BIGINT AS media_duration_ms,
                 EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
                 EXTRACT(EPOCH FROM edited_at)::BIGINT AS edited_at_unix,
                 NULL::BIGINT AS deleted_at_unix"#,
        )
        .bind(conversation_id)
        .bind(&sender.principal_id)
        .bind(&client_message_id)
        .bind(&body)
        .bind(&metadata)
        .bind(role_key(&sender.role))
        .bind(&sender.ref_)
        .bind(&sender.display_name)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| ChatError::StoreFailed)?;
        debug_assert_eq!(row.message_id, existing.message_id);
        (row.into_model()?, false)
    } else {
        let last_sequence = sqlx::query_scalar::<_, i64>(
            r#"SELECT last_message_sequence
               FROM mini_chat_conversations
               WHERE conversation_id = $1
               FOR UPDATE"#,
        )
        .bind(conversation_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ChatError::StoreFailed)?
        .ok_or(ChatError::NotFound)?;
        let sequence = last_sequence.saturating_add(1);
        let row = sqlx::query_as::<_, MessageRow>(
            r#"INSERT INTO mini_chat_messages
                 (message_id, conversation_id, sender_principal_id, client_message_id,
                  message_sequence, message_type, body, metadata_json)
               VALUES ($1, $2, $3, $4, $5, 'inventory_transfer_request', $6, $7)
               RETURNING
                 message_id, conversation_id, sender_principal_id,
                 $8::TEXT AS sender_role, $9::TEXT AS sender_ref,
                 $10::TEXT AS sender_display_name, client_message_id,
                 message_sequence, message_type, body, metadata_json,
                 NULL::TEXT AS attachment_id, NULL::TEXT AS media_id,
                 NULL::TEXT AS media_kind, NULL::TEXT AS media_content_type,
                 NULL::BIGINT AS media_size_bytes, NULL::INTEGER AS media_width_pixels,
                 NULL::INTEGER AS media_height_pixels, NULL::BIGINT AS media_duration_ms,
                 EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at_unix,
                 NULL::BIGINT AS edited_at_unix, NULL::BIGINT AS deleted_at_unix"#,
        )
        .bind(new_id("message"))
        .bind(conversation_id)
        .bind(&sender.principal_id)
        .bind(&client_message_id)
        .bind(sequence)
        .bind(&body)
        .bind(&metadata)
        .bind(role_key(&sender.role))
        .bind(&sender.ref_)
        .bind(&sender.display_name)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| ChatError::StoreFailed)?;
        sqlx::query(
            r#"UPDATE mini_chat_conversations
               SET last_message_sequence = $2, updated_at = now()
               WHERE conversation_id = $1"#,
        )
        .bind(conversation_id)
        .bind(sequence)
        .execute(&mut *tx)
        .await
        .map_err(|_| ChatError::StoreFailed)?;
        sqlx::query(
            r#"UPDATE mini_chat_conversation_members
               SET last_read_sequence = GREATEST(last_read_sequence, $3)
               WHERE conversation_id = $1
                 AND principal_id = $2
                 AND left_at IS NULL"#,
        )
        .bind(conversation_id)
        .bind(&sender.principal_id)
        .bind(sequence)
        .execute(&mut *tx)
        .await
        .map_err(|_| ChatError::StoreFailed)?;
        (row.into_model()?, true)
    };

    let recipients = sqlx::query_as::<_, (String, String, String)>(ORDER_FREEZE_RECIPIENTS_SQL)
        .bind(conversation_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| ChatError::StoreFailed)?;
    let recipient_keys = recipients
        .iter()
        .map(|(_, role, ref_)| format!("{role}:{ref_}"))
        .collect::<Vec<_>>();
    let push_recipient_keys = if created {
        recipients
            .iter()
            .filter(|(principal_id, _, _)| principal_id != &sender.principal_id)
            .map(|(_, role, ref_)| format!("{role}:{ref_}"))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
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
        event: if created {
            "chat.message.created".to_string()
        } else {
            "chat.message.updated".to_string()
        },
        conversation_id: conversation_id.to_string(),
        sequence: message.sequence,
        message: message.clone(),
    };
    sqlx::query(
        r#"INSERT INTO mini_chat_outbox_events
             (event_id, event_cursor, topic, conversation_id, message_sequence,
              recipient_keys, push_recipient_keys, payload_json)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
    )
    .bind(&event.event_id)
    .bind(cursor)
    .bind(&event.event)
    .bind(conversation_id)
    .bind(message.sequence)
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
    Ok(ChatSendResult { message, created })
}

include!("../write_delivery.rs");
