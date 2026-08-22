
pub(super) async fn mark_order_freeze_chat_event_delivered(
    pool: &PgPool,
    event_id: &str,
) -> Result<(), ChatError> {
    sqlx::query(
        r#"UPDATE mini_order_freeze_chat_outbox
           SET delivered_at = now(), locked_until = NULL, last_error = ''
           WHERE event_id = $1"#,
    )
    .bind(event_id.trim())
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|_| ChatError::StoreFailed)
}

pub(super) async fn reschedule_order_freeze_chat_event(
    pool: &PgPool,
    event_id: &str,
    error: &str,
) -> Result<(), ChatError> {
    sqlx::query(
        r#"UPDATE mini_order_freeze_chat_outbox
           SET locked_until = now() + interval '2 seconds',
               last_error = left($2, 1000)
           WHERE event_id = $1 AND delivered_at IS NULL"#,
    )
    .bind(event_id.trim())
    .bind(error.trim())
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|_| ChatError::StoreFailed)
}

pub(super) async fn claim_inventory_transfer_chat_events(
    pool: &PgPool,
    limit: usize,
) -> Result<Vec<InventoryTransferChatEvent>, ChatError> {
    let rows = sqlx::query_as::<_, super::rows::InventoryTransferChatEventRow>(
        CLAIM_INVENTORY_TRANSFER_CHAT_EVENTS_SQL,
    )
    .bind(limit.clamp(1, 100) as i64)
    .fetch_all(pool)
    .await
    .map_err(|_| ChatError::StoreFailed)?;
    Ok(rows
        .into_iter()
        .map(super::rows::InventoryTransferChatEventRow::into_model)
        .collect())
}

pub(super) async fn mark_inventory_transfer_chat_event_delivered(
    pool: &PgPool,
    event_id: &str,
) -> Result<(), ChatError> {
    sqlx::query(
        r#"UPDATE mini_inventory_transfer_chat_outbox
           SET delivered_at = now(), locked_until = NULL, last_error = ''
           WHERE event_id = $1"#,
    )
    .bind(event_id.trim())
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|_| ChatError::StoreFailed)
}

pub(super) async fn reschedule_inventory_transfer_chat_event(
    pool: &PgPool,
    event_id: &str,
    error: &str,
) -> Result<(), ChatError> {
    sqlx::query(
        r#"UPDATE mini_inventory_transfer_chat_outbox
           SET locked_until = now() + interval '2 seconds',
               last_error = left($2, 1000)
           WHERE event_id = $1 AND delivered_at IS NULL"#,
    )
    .bind(event_id.trim())
    .bind(error.trim())
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|_| ChatError::StoreFailed)
}

fn new_id(prefix: &str) -> String {
    let bytes: [u8; 16] = rand::random();
    format!("{prefix}_{}", data_encoding::HEXLOWER.encode(&bytes))
}

fn ticket_hash(ticket: &str) -> Vec<u8> {
    Sha256::digest(ticket.trim().as_bytes()).to_vec()
}
