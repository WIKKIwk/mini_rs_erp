use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{
    OrderProgressBatch, OrderProgressBatchStatus, OrderProgressEvent, OrderRunSession,
    OrderRunStatus, ProductionMapError, ProductionOrderLogEntry, queue_state,
};

use super::queue_helpers::{queue_action_as_str, queue_action_from_str};

pub(super) async fn put_order_run_session(
    pool: &PgPool,
    session: &OrderRunSession,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    put_order_run_session_tx(&mut tx, session).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn put_order_run_session_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &OrderRunSession,
) -> Result<(), ProductionMapError> {
    sqlx::query(
        "INSERT INTO mini_order_run_sessions (
            session_id, apparatus, order_id, status,
            worker_role, worker_ref, worker_display_name,
            started_at, updated_at, payload_json
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, to_timestamp($8), to_timestamp($9), $10)
         ON CONFLICT (session_id) DO UPDATE SET
            status = excluded.status,
            worker_role = excluded.worker_role,
            worker_ref = excluded.worker_ref,
            worker_display_name = excluded.worker_display_name,
            updated_at = excluded.updated_at,
            payload_json = excluded.payload_json",
    )
    .bind(session.session_id.trim())
    .bind(session.apparatus.trim())
    .bind(session.order_id.trim())
    .bind(session.status.as_str())
    .bind(session.worker_role.trim())
    .bind(session.worker_ref.trim())
    .bind(session.worker_display_name.trim())
    .bind(session.started_at_unix as f64)
    .bind(session.updated_at_unix as f64)
    .bind(&session.payload_json)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn put_order_progress_event(
    pool: &PgPool,
    event: &OrderProgressEvent,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    put_order_progress_event_tx(&mut tx, event).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn put_order_progress_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    event: &OrderProgressEvent,
) -> Result<(), ProductionMapError> {
    sqlx::query(
        "INSERT INTO mini_order_progress_events (
            event_id, session_id, batch_id, apparatus, order_id, action,
            produced_qty, uom, worker_role, worker_ref, worker_display_name,
            qr_payload, return_ink_kg, lamination_print_leftover_rolls,
            lamination_film_leftover_rolls, rezka_bosma_waste,
            rezka_lamination_waste, rezka_edge_waste, total_waste,
            finished_goods_kg, finished_goods_meter, description,
            payload_json, created_at
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, now())
         ON CONFLICT (event_id) DO UPDATE SET
            session_id = excluded.session_id,
            batch_id = excluded.batch_id,
            action = excluded.action,
            produced_qty = excluded.produced_qty,
            uom = excluded.uom,
            worker_role = excluded.worker_role,
            worker_ref = excluded.worker_ref,
            worker_display_name = excluded.worker_display_name,
            qr_payload = excluded.qr_payload,
            return_ink_kg = excluded.return_ink_kg,
            lamination_print_leftover_rolls = excluded.lamination_print_leftover_rolls,
            lamination_film_leftover_rolls = excluded.lamination_film_leftover_rolls,
            rezka_bosma_waste = excluded.rezka_bosma_waste,
            rezka_lamination_waste = excluded.rezka_lamination_waste,
            rezka_edge_waste = excluded.rezka_edge_waste,
            total_waste = excluded.total_waste,
            finished_goods_kg = excluded.finished_goods_kg,
            finished_goods_meter = excluded.finished_goods_meter,
            description = excluded.description,
            payload_json = excluded.payload_json",
    )
    .bind(event.event_id.trim())
    .bind(event.session_id.trim())
    .bind(event.batch_id.trim())
    .bind(event.apparatus.trim())
    .bind(event.order_id.trim())
    .bind(queue_action_as_str(event.action))
    .bind(event.produced_qty)
    .bind(event.uom.trim())
    .bind(event.worker_role.trim())
    .bind(event.worker_ref.trim())
    .bind(event.worker_display_name.trim())
    .bind(event.qr_payload.trim())
    .bind(event.return_ink_kg)
    .bind(event.lamination_print_leftover_rolls)
    .bind(event.lamination_film_leftover_rolls)
    .bind(event.rezka_bosma_waste)
    .bind(event.rezka_lamination_waste)
    .bind(event.rezka_edge_waste)
    .bind(event.total_waste)
    .bind(event.finished_goods_kg)
    .bind(event.finished_goods_meter)
    .bind(event.description.trim())
    .bind(&event.payload_json)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn put_order_progress_batch(
    pool: &PgPool,
    batch: &OrderProgressBatch,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    put_order_progress_batch_tx(&mut tx, batch).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn put_order_progress_batch_tx(
    tx: &mut Transaction<'_, Postgres>,
    batch: &OrderProgressBatch,
) -> Result<(), ProductionMapError> {
    sqlx::query(
        "INSERT INTO mini_progress_batches (
            batch_id, session_id, apparatus, order_id, action, status,
            produced_qty, uom, qr_payload, label_item_code, label_item_name,
            executor_name, worker_role, worker_ref, worker_display_name,
            return_ink_kg, lamination_print_leftover_rolls,
            lamination_film_leftover_rolls, rezka_bosma_waste,
            rezka_lamination_waste, rezka_edge_waste, total_waste,
            finished_goods_kg, finished_goods_meter, description,
            payload_json, created_at, updated_at
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, now(), now())
         ON CONFLICT (batch_id) DO UPDATE SET
            status = excluded.status,
            produced_qty = excluded.produced_qty,
            uom = excluded.uom,
            qr_payload = excluded.qr_payload,
            label_item_code = excluded.label_item_code,
            label_item_name = excluded.label_item_name,
            executor_name = excluded.executor_name,
            worker_role = excluded.worker_role,
            worker_ref = excluded.worker_ref,
            worker_display_name = excluded.worker_display_name,
            return_ink_kg = excluded.return_ink_kg,
            lamination_print_leftover_rolls = excluded.lamination_print_leftover_rolls,
            lamination_film_leftover_rolls = excluded.lamination_film_leftover_rolls,
            rezka_bosma_waste = excluded.rezka_bosma_waste,
            rezka_lamination_waste = excluded.rezka_lamination_waste,
            rezka_edge_waste = excluded.rezka_edge_waste,
            total_waste = excluded.total_waste,
            finished_goods_kg = excluded.finished_goods_kg,
            finished_goods_meter = excluded.finished_goods_meter,
            description = excluded.description,
            payload_json = excluded.payload_json,
            updated_at = now()",
    )
    .bind(batch.batch_id.trim())
    .bind(batch.session_id.trim())
    .bind(batch.apparatus.trim())
    .bind(batch.order_id.trim())
    .bind(queue_action_as_str(batch.action))
    .bind(batch.status.as_str())
    .bind(batch.produced_qty)
    .bind(batch.uom.trim())
    .bind(batch.qr_payload.trim())
    .bind(batch.label_item_code.trim())
    .bind(batch.label_item_name.trim())
    .bind(batch.executor_name.trim())
    .bind(batch.worker_role.trim())
    .bind(batch.worker_ref.trim())
    .bind(batch.worker_display_name.trim())
    .bind(batch.return_ink_kg)
    .bind(batch.lamination_print_leftover_rolls)
    .bind(batch.lamination_film_leftover_rolls)
    .bind(batch.rezka_bosma_waste)
    .bind(batch.rezka_lamination_waste)
    .bind(batch.rezka_edge_waste)
    .bind(batch.total_waste)
    .bind(batch.finished_goods_kg)
    .bind(batch.finished_goods_meter)
    .bind(batch.description.trim())
    .bind(&batch.payload_json)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

#[derive(sqlx::FromRow)]
pub(super) struct ProgressSessionRow {
    pub(super) session_id: String,
    pub(super) apparatus: String,
    pub(super) order_id: String,
    pub(super) status: String,
    pub(super) worker_role: String,
    pub(super) worker_ref: String,
    pub(super) worker_display_name: String,
    pub(super) started_at_unix: i64,
    pub(super) updated_at_unix: i64,
    pub(super) payload_json: serde_json::Value,
}

#[derive(sqlx::FromRow)]
pub(super) struct ProgressBatchRow {
    pub(super) batch_id: String,
    pub(super) session_id: String,
    pub(super) apparatus: String,
    pub(super) order_id: String,
    pub(super) action: String,
    pub(super) status: String,
    pub(super) produced_qty: f64,
    pub(super) uom: String,
    pub(super) qr_payload: String,
    pub(super) label_item_code: String,
    pub(super) label_item_name: String,
    pub(super) executor_name: String,
    pub(super) worker_role: String,
    pub(super) worker_ref: String,
    pub(super) worker_display_name: String,
    pub(super) return_ink_kg: Option<f64>,
    pub(super) lamination_print_leftover_rolls: Option<f64>,
    pub(super) lamination_film_leftover_rolls: Option<f64>,
    pub(super) rezka_bosma_waste: Option<f64>,
    pub(super) rezka_lamination_waste: Option<f64>,
    pub(super) rezka_edge_waste: Option<f64>,
    pub(super) total_waste: Option<f64>,
    pub(super) finished_goods_kg: Option<f64>,
    pub(super) finished_goods_meter: Option<f64>,
    pub(super) description: String,
    pub(super) payload_json: serde_json::Value,
}

#[derive(sqlx::FromRow)]
pub(super) struct QueueActionLogRow {
    pub(super) event_id: String,
    pub(super) apparatus: String,
    pub(super) order_id: String,
    pub(super) action: String,
    pub(super) from_state: String,
    pub(super) to_state: String,
    pub(super) actor_role: String,
    pub(super) actor_ref: String,
    pub(super) actor_display_name: String,
    pub(super) created_at_unix: i64,
    pub(super) completed_with_issue: bool,
    pub(super) issue_note: String,
}

pub(super) fn queue_action_log_from_row(
    row: QueueActionLogRow,
) -> Result<ProductionOrderLogEntry, ProductionMapError> {
    Ok(ProductionOrderLogEntry {
        event_id: row.event_id,
        apparatus: row.apparatus,
        order_id: row.order_id,
        action: queue_action_from_str(&row.action).ok_or(ProductionMapError::StoreFailed)?,
        from_state: queue_state::ApparatusQueueOrderState::parse(&row.from_state)
            .ok_or(ProductionMapError::StoreFailed)?,
        to_state: queue_state::ApparatusQueueOrderState::parse(&row.to_state)
            .ok_or(ProductionMapError::StoreFailed)?,
        actor_role: row.actor_role,
        actor_ref: row.actor_ref,
        actor_display_name: row.actor_display_name,
        created_at_unix: row.created_at_unix,
        completed_with_issue: row.completed_with_issue,
        issue_note: row.issue_note,
    })
}

pub(super) fn progress_session_from_row(
    row: ProgressSessionRow,
) -> Result<OrderRunSession, ProductionMapError> {
    Ok(OrderRunSession {
        session_id: row.session_id,
        apparatus: row.apparatus,
        order_id: row.order_id,
        status: OrderRunStatus::parse(&row.status).ok_or(ProductionMapError::StoreFailed)?,
        worker_role: row.worker_role,
        worker_ref: row.worker_ref,
        worker_display_name: row.worker_display_name,
        started_at_unix: row.started_at_unix,
        updated_at_unix: row.updated_at_unix,
        payload_json: row.payload_json,
    })
}

pub(super) fn progress_batch_from_row(
    row: ProgressBatchRow,
) -> Result<OrderProgressBatch, ProductionMapError> {
    Ok(OrderProgressBatch {
        batch_id: row.batch_id,
        session_id: row.session_id,
        apparatus: row.apparatus,
        order_id: row.order_id,
        action: queue_action_from_str(&row.action).ok_or(ProductionMapError::StoreFailed)?,
        status: OrderProgressBatchStatus::parse(&row.status)
            .ok_or(ProductionMapError::StoreFailed)?,
        produced_qty: row.produced_qty,
        uom: row.uom,
        qr_payload: row.qr_payload,
        label_item_code: row.label_item_code,
        label_item_name: row.label_item_name,
        executor_name: row.executor_name,
        worker_role: row.worker_role,
        worker_ref: row.worker_ref,
        worker_display_name: row.worker_display_name,
        return_ink_kg: row.return_ink_kg,
        lamination_print_leftover_rolls: row.lamination_print_leftover_rolls,
        lamination_film_leftover_rolls: row.lamination_film_leftover_rolls,
        rezka_bosma_waste: row.rezka_bosma_waste,
        rezka_lamination_waste: row.rezka_lamination_waste,
        rezka_edge_waste: row.rezka_edge_waste,
        total_waste: row.total_waste,
        finished_goods_kg: row.finished_goods_kg,
        finished_goods_meter: row.finished_goods_meter,
        description: row.description,
        payload_json: row.payload_json,
    })
}
