use std::collections::BTreeMap;

use sqlx::PgPool;

use crate::core::production_map::{
    CompletedQueueOrder, OrderProgressBatch, OrderRunSession, ProductionMapError,
    ProductionOrderLogEntry,
};

use super::progress_helpers::{
    ProgressBatchRow, ProgressSessionRow, QueueActionLogRow, progress_batch_from_row,
    progress_session_from_row, queue_action_log_from_row,
};

pub(super) async fn load_completed_queue_orders_for_actor(
    pool: &PgPool,
    actor_ref: &str,
    limit: usize,
) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
    let actor_ref = actor_ref.trim();
    if actor_ref.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let limit = i64::try_from(limit.min(500)).unwrap_or(500);
    let rows = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT order_id, apparatus, completed_at_unix
         FROM (
            SELECT DISTINCT ON (order_id)
                order_id,
                apparatus,
                created_at,
                EXTRACT(EPOCH FROM created_at)::bigint AS completed_at_unix
            FROM mini_queue_action_events
            WHERE actor_ref = $1
              AND action = 'complete'
              AND to_state = 'completed'
            ORDER BY order_id, created_at DESC
         ) latest
         ORDER BY created_at DESC
         LIMIT $2",
    )
    .bind(actor_ref)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    Ok(rows
        .into_iter()
        .map(
            |(order_id, apparatus, completed_at_unix)| CompletedQueueOrder {
                apparatus,
                order_id,
                completed_at_unix,
            },
        )
        .collect())
}

pub(super) async fn load_queue_action_logs_for_orders(
    pool: &PgPool,
    order_ids: &[String],
) -> Result<BTreeMap<String, Vec<ProductionOrderLogEntry>>, ProductionMapError> {
    let order_ids = order_ids
        .iter()
        .map(|order_id| order_id.trim().to_string())
        .filter(|order_id| !order_id.is_empty())
        .collect::<Vec<_>>();
    if order_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let rows = sqlx::query_as::<_, QueueActionLogRow>(
        "SELECT event_id, apparatus, order_id, action, from_state, to_state,
                actor_role, actor_ref, actor_display_name,
                EXTRACT(EPOCH FROM created_at)::bigint AS created_at_unix,
                COALESCE((payload_json->>'completed_with_issue')::boolean, false) AS completed_with_issue,
                COALESCE(payload_json->>'issue_note', '') AS issue_note
         FROM mini_queue_action_events
         WHERE order_id = ANY($1)
         ORDER BY created_at ASC, id ASC",
    )
    .bind(&order_ids)
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut logs: BTreeMap<String, Vec<ProductionOrderLogEntry>> = BTreeMap::new();
    for row in rows {
        let entry = queue_action_log_from_row(row)?;
        logs.entry(entry.order_id.clone()).or_default().push(entry);
    }
    Ok(logs)
}

pub(super) async fn load_queue_action_logs_for_worker(
    pool: &PgPool,
    worker_refs: &[String],
    _worker_display_name: &str,
    limit: usize,
) -> Result<Vec<ProductionOrderLogEntry>, ProductionMapError> {
    let worker_refs = normalized_refs(worker_refs);
    if worker_refs.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let limit = i64::try_from(limit.min(500)).unwrap_or(500);
    let rows = sqlx::query_as::<_, QueueActionLogRow>(
        "SELECT event_id, apparatus, order_id, action, from_state, to_state,
                actor_role, actor_ref, actor_display_name,
                EXTRACT(EPOCH FROM created_at)::bigint AS created_at_unix,
                COALESCE((payload_json->>'completed_with_issue')::boolean, false) AS completed_with_issue,
                COALESCE(payload_json->>'issue_note', '') AS issue_note
         FROM mini_queue_action_events AS event
         WHERE event.actor_ref = ANY($1)
            OR EXISTS (
                SELECT 1
                FROM mini_worker_identity_aliases AS alias
                WHERE alias.worker_id = ANY($1)
                  AND alias.alias_type = 'phone'
                  AND event.actor_ref ~ '^[+0-9() .-]+$'
                  AND alias.alias_key = regexp_replace(event.actor_ref, '[^0-9]', '', 'g')
                  AND event.created_at >= alias.valid_from
                  AND (alias.valid_to IS NULL OR event.created_at < alias.valid_to)
            )
         ORDER BY created_at DESC, id DESC
         LIMIT $2",
    )
    .bind(&worker_refs)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.into_iter().map(queue_action_log_from_row).collect()
}

pub(super) async fn load_active_order_run_session(
    pool: &PgPool,
    apparatus: &str,
    order_id: &str,
) -> Result<Option<OrderRunSession>, ProductionMapError> {
    let row = sqlx::query_as::<_, ProgressSessionRow>(
        "SELECT session_id, apparatus, order_id, status,
                worker_role, worker_ref, worker_display_name,
                EXTRACT(EPOCH FROM started_at)::bigint AS started_at_unix,
                EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at_unix,
                payload_json
         FROM mini_order_run_sessions
         WHERE order_id = $1
           AND lower(apparatus) = lower($2)
           AND status IN ('active', 'paused')
         ORDER BY updated_at DESC
         LIMIT 1",
    )
    .bind(order_id.trim())
    .bind(apparatus.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    row.map(progress_session_from_row).transpose()
}

pub(super) async fn load_active_order_run_session_for_qolip(
    pool: &PgPool,
    qolip_code: &str,
) -> Result<Option<OrderRunSession>, ProductionMapError> {
    let qolip_code = qolip_code.trim();
    if qolip_code.is_empty() {
        return Ok(None);
    }
    let row = sqlx::query_as::<_, ProgressSessionRow>(
        "SELECT session_id, apparatus, order_id, status,
                worker_role, worker_ref, worker_display_name,
                EXTRACT(EPOCH FROM started_at)::bigint AS started_at_unix,
                EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at_unix,
                payload_json
         FROM mini_order_run_sessions
         WHERE status IN ('active', 'paused')
           AND lower(payload_json->>'qolip_code') = lower($1)
         ORDER BY updated_at DESC
         LIMIT 1",
    )
    .bind(qolip_code)
    .fetch_optional(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    row.map(progress_session_from_row).transpose()
}

pub(super) async fn load_active_order_run_sessions_for_worker(
    pool: &PgPool,
    worker_refs: &[String],
    _worker_display_name: &str,
    limit: usize,
) -> Result<Vec<OrderRunSession>, ProductionMapError> {
    let worker_refs = normalized_refs(worker_refs);
    if worker_refs.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let limit = i64::try_from(limit.min(500)).unwrap_or(500);
    let rows = sqlx::query_as::<_, ProgressSessionRow>(
        "SELECT session_id, apparatus, order_id, status,
                worker_role, worker_ref, worker_display_name,
                EXTRACT(EPOCH FROM started_at)::bigint AS started_at_unix,
                EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at_unix,
                payload_json
         FROM mini_order_run_sessions AS session
         WHERE session.status IN ('active', 'paused')
           AND (
               session.worker_ref = ANY($1)
               OR EXISTS (
                   SELECT 1
                   FROM mini_worker_identity_aliases AS alias
                   WHERE alias.worker_id = ANY($1)
                     AND alias.alias_type = 'phone'
                     AND session.worker_ref ~ '^[+0-9() .-]+$'
                     AND alias.alias_key = regexp_replace(session.worker_ref, '[^0-9]', '', 'g')
                     AND session.started_at >= alias.valid_from
                     AND (alias.valid_to IS NULL OR session.started_at < alias.valid_to)
               )
           )
         ORDER BY updated_at DESC, session_id DESC
         LIMIT $2",
    )
    .bind(&worker_refs)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.into_iter().map(progress_session_from_row).collect()
}

pub(super) async fn load_order_run_session(
    pool: &PgPool,
    session_id: &str,
) -> Result<Option<OrderRunSession>, ProductionMapError> {
    let row = sqlx::query_as::<_, ProgressSessionRow>(
        "SELECT session_id, apparatus, order_id, status,
                worker_role, worker_ref, worker_display_name,
                EXTRACT(EPOCH FROM started_at)::bigint AS started_at_unix,
                EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at_unix,
                payload_json
         FROM mini_order_run_sessions
         WHERE session_id = $1",
    )
    .bind(session_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    row.map(progress_session_from_row).transpose()
}

pub(super) async fn load_order_run_sessions_for_order(
    pool: &PgPool,
    order_id: &str,
) -> Result<Vec<OrderRunSession>, ProductionMapError> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, ProgressSessionRow>(
        "SELECT session_id, apparatus, order_id, status,
                worker_role, worker_ref, worker_display_name,
                EXTRACT(EPOCH FROM started_at)::bigint AS started_at_unix,
                EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at_unix,
                payload_json
         FROM mini_order_run_sessions
         WHERE order_id = $1
         ORDER BY started_at ASC, session_id ASC",
    )
    .bind(order_id)
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.into_iter().map(progress_session_from_row).collect()
}

pub(super) async fn load_order_run_sessions_for_audit(
    pool: &PgPool,
) -> Result<Vec<OrderRunSession>, ProductionMapError> {
    let rows = sqlx::query_as::<_, ProgressSessionRow>(
        "SELECT session_id, apparatus, order_id, status,
                worker_role, worker_ref, worker_display_name,
                EXTRACT(EPOCH FROM started_at)::bigint AS started_at_unix,
                EXTRACT(EPOCH FROM updated_at)::bigint AS updated_at_unix,
                payload_json
         FROM mini_order_run_sessions
         ORDER BY started_at ASC, session_id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.into_iter().map(progress_session_from_row).collect()
}

pub(super) async fn load_progress_batch(
    pool: &PgPool,
    batch_id: &str,
) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
    let row = sqlx::query_as::<_, ProgressBatchRow>(
        "SELECT batch_id, session_id, apparatus, order_id, action, status,
                produced_qty::float8 AS produced_qty, uom, qr_payload,
                label_item_code, label_item_name, executor_name,
                worker_role, worker_ref, worker_display_name,
                wip_status, current_apparatus, current_apparatus_key, current_location,
                next_apparatus, parent_batch_id, used_by_session_id,
                used_by_apparatus, processed_by_session_id,
                processed_by_apparatus,
                return_ink_kg::float8 AS return_ink_kg,
                lamination_print_leftover_rolls::float8 AS lamination_print_leftover_rolls,
                lamination_film_leftover_rolls::float8 AS lamination_film_leftover_rolls,
                rezka_bosma_waste::float8 AS rezka_bosma_waste,
                rezka_lamination_waste::float8 AS rezka_lamination_waste,
                rezka_edge_waste::float8 AS rezka_edge_waste,
                total_waste::float8 AS total_waste,
                finished_goods_kg::float8 AS finished_goods_kg,
                finished_goods_meter::float8 AS finished_goods_meter,
                description,
                payload_json
         FROM mini_progress_batches
         WHERE batch_id = $1",
    )
    .bind(batch_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    row.map(progress_batch_from_row).transpose()
}

pub(super) async fn load_progress_batches_for_worker(
    pool: &PgPool,
    worker_refs: &[String],
    _worker_display_name: &str,
    limit: usize,
) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
    let worker_refs = normalized_refs(worker_refs);
    if worker_refs.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let limit = i64::try_from(limit.min(500)).unwrap_or(500);
    let rows = sqlx::query_as::<_, ProgressBatchRow>(
        "SELECT batch_id, session_id, apparatus, order_id, action, status,
                produced_qty::float8 AS produced_qty, uom, qr_payload,
                label_item_code, label_item_name, executor_name,
                worker_role, worker_ref, worker_display_name,
                wip_status, current_apparatus, current_apparatus_key, current_location,
                next_apparatus, parent_batch_id, used_by_session_id,
                used_by_apparatus, processed_by_session_id,
                processed_by_apparatus,
                return_ink_kg::float8 AS return_ink_kg,
                lamination_print_leftover_rolls::float8 AS lamination_print_leftover_rolls,
                lamination_film_leftover_rolls::float8 AS lamination_film_leftover_rolls,
                rezka_bosma_waste::float8 AS rezka_bosma_waste,
                rezka_lamination_waste::float8 AS rezka_lamination_waste,
                rezka_edge_waste::float8 AS rezka_edge_waste,
                total_waste::float8 AS total_waste,
                finished_goods_kg::float8 AS finished_goods_kg,
                finished_goods_meter::float8 AS finished_goods_meter,
                description,
                payload_json
         FROM mini_progress_batches AS batch
         WHERE batch.worker_ref = ANY($1)
            OR EXISTS (
                SELECT 1
                FROM mini_worker_identity_aliases AS alias
                WHERE alias.worker_id = ANY($1)
                  AND alias.alias_type = 'phone'
                  AND batch.worker_ref ~ '^[+0-9() .-]+$'
                  AND alias.alias_key = regexp_replace(batch.worker_ref, '[^0-9]', '', 'g')
                  AND batch.created_at >= alias.valid_from
                  AND (alias.valid_to IS NULL OR batch.created_at < alias.valid_to)
            )
         ORDER BY updated_at DESC, created_at DESC, batch_id DESC
         LIMIT $2",
    )
    .bind(&worker_refs)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.into_iter().map(progress_batch_from_row).collect()
}

pub(super) async fn load_progress_batches_for_order(
    pool: &PgPool,
    order_id: &str,
) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, ProgressBatchRow>(
        "SELECT batch_id, session_id, apparatus, order_id, action, status,
                produced_qty::float8 AS produced_qty, uom, qr_payload,
                label_item_code, label_item_name, executor_name,
                worker_role, worker_ref, worker_display_name,
                wip_status, current_apparatus, current_apparatus_key, current_location,
                next_apparatus, parent_batch_id, used_by_session_id,
                used_by_apparatus, processed_by_session_id,
                processed_by_apparatus,
                return_ink_kg::float8 AS return_ink_kg,
                lamination_print_leftover_rolls::float8 AS lamination_print_leftover_rolls,
                lamination_film_leftover_rolls::float8 AS lamination_film_leftover_rolls,
                rezka_bosma_waste::float8 AS rezka_bosma_waste,
                rezka_lamination_waste::float8 AS rezka_lamination_waste,
                rezka_edge_waste::float8 AS rezka_edge_waste,
                total_waste::float8 AS total_waste,
                finished_goods_kg::float8 AS finished_goods_kg,
                finished_goods_meter::float8 AS finished_goods_meter,
                description,
                payload_json
         FROM mini_progress_batches
         WHERE order_id = $1
         ORDER BY updated_at DESC, created_at DESC, batch_id DESC",
    )
    .bind(order_id)
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.into_iter().map(progress_batch_from_row).collect()
}

pub(super) async fn load_progress_batches_for_audit(
    pool: &PgPool,
) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
    let rows = sqlx::query_as::<_, ProgressBatchRow>(
        "SELECT batch_id, session_id, apparatus, order_id, action, status,
                produced_qty::float8 AS produced_qty, uom, qr_payload,
                label_item_code, label_item_name, executor_name,
                worker_role, worker_ref, worker_display_name,
                wip_status, current_apparatus, current_apparatus_key, current_location,
                next_apparatus, parent_batch_id, used_by_session_id,
                used_by_apparatus, processed_by_session_id,
                processed_by_apparatus,
                return_ink_kg::float8 AS return_ink_kg,
                lamination_print_leftover_rolls::float8 AS lamination_print_leftover_rolls,
                lamination_film_leftover_rolls::float8 AS lamination_film_leftover_rolls,
                rezka_bosma_waste::float8 AS rezka_bosma_waste,
                rezka_lamination_waste::float8 AS rezka_lamination_waste,
                rezka_edge_waste::float8 AS rezka_edge_waste,
                total_waste::float8 AS total_waste,
                finished_goods_kg::float8 AS finished_goods_kg,
                finished_goods_meter::float8 AS finished_goods_meter,
                description,
                payload_json
         FROM mini_progress_batches
         ORDER BY updated_at DESC, created_at DESC, batch_id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.into_iter().map(progress_batch_from_row).collect()
}

pub(super) async fn load_progress_batch_by_qr(
    pool: &PgPool,
    qr_payload: &str,
) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
    let row = sqlx::query_as::<_, ProgressBatchRow>(
        "SELECT batch_id, session_id, apparatus, order_id, action, status,
                produced_qty::float8 AS produced_qty, uom, qr_payload,
                label_item_code, label_item_name, executor_name,
                worker_role, worker_ref, worker_display_name,
                wip_status, current_apparatus, current_apparatus_key, current_location,
                next_apparatus, parent_batch_id, used_by_session_id,
                used_by_apparatus, processed_by_session_id,
                processed_by_apparatus,
                return_ink_kg::float8 AS return_ink_kg,
                lamination_print_leftover_rolls::float8 AS lamination_print_leftover_rolls,
                lamination_film_leftover_rolls::float8 AS lamination_film_leftover_rolls,
                rezka_bosma_waste::float8 AS rezka_bosma_waste,
                rezka_lamination_waste::float8 AS rezka_lamination_waste,
                rezka_edge_waste::float8 AS rezka_edge_waste,
                total_waste::float8 AS total_waste,
                finished_goods_kg::float8 AS finished_goods_kg,
                finished_goods_meter::float8 AS finished_goods_meter,
                description,
                payload_json
         FROM mini_progress_batches
         WHERE lower(qr_payload) = lower($1)",
    )
    .bind(qr_payload.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    row.map(progress_batch_from_row).transpose()
}

fn normalized_refs(worker_refs: &[String]) -> Vec<String> {
    worker_refs
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}
