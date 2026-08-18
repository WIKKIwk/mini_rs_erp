use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{
    FinishedGoodsStockEntry, OrderProgressBatch, OrderProgressBatchStatus,
    OrderProgressBatchStatusDetail, OrderProgressBatchWipStatus, OrderProgressEvent,
    OrderRunSession, OrderRunStatus, ProductionMapError, ProductionOrderLogEntry,
    ProgressBatchCorrectionInput, ProgressBatchCorrectionRecord, QueueActionActor, queue_state,
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
            apparatus = excluded.apparatus,
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
            finished_goods_kg, bobina_kg, finished_goods_meter, diameter, description,
            payload_json, created_at
         )
         VALUES ($1, $2, $3, $4, $5, $6,
                 ($7::double precision)::numeric(18,6),
                 $8, $9, $10, $11, $12,
                 ($13::double precision)::numeric(18,6),
                 ($14::double precision)::numeric(18,6),
                 ($15::double precision)::numeric(18,6),
                 ($16::double precision)::numeric(18,6),
                 ($17::double precision)::numeric(18,6),
                 ($18::double precision)::numeric(18,6),
                 ($19::double precision)::numeric(18,6),
                 ($20::double precision)::numeric(18,6),
                 ($21::double precision)::numeric(18,6),
                 ($22::double precision)::numeric(18,6),
                 ($23::double precision)::numeric(18,6),
                 $24, $25, now())
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
            bobina_kg = excluded.bobina_kg,
            finished_goods_meter = excluded.finished_goods_meter,
            diameter = excluded.diameter,
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
    .bind(event.bobina_kg)
    .bind(event.finished_goods_meter)
    .bind(event.diameter)
    .bind(event.description.trim())
    .bind(&event.payload_json)
    .execute(&mut **tx)
    .await
    .map_err(|error| {
        tracing::error!(
            error = %error,
            event_id = %event.event_id,
            order_id = %event.order_id,
            apparatus = %event.apparatus,
            action = ?event.action,
            "failed to store order progress event"
        );
        ProductionMapError::StoreFailed
    })?;
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
            wip_status, current_apparatus, current_apparatus_key, current_location, next_apparatus,
            parent_batch_id, used_by_session_id, used_by_apparatus,
            processed_by_session_id, processed_by_apparatus,
            return_ink_kg, lamination_print_leftover_rolls,
            lamination_film_leftover_rolls, rezka_bosma_waste,
            rezka_lamination_waste, rezka_edge_waste, total_waste,
            finished_goods_kg, bobina_kg, finished_goods_meter, diameter, description,
            payload_json, created_at, updated_at
         )
         VALUES ($1, $2, $3, $4, $5, $6,
                 ($7::double precision)::numeric(18,6),
                 $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18,
                 $19, $20, $21, $22, $23, $24, $25,
                 ($26::double precision)::numeric(18,6),
                 ($27::double precision)::numeric(18,6),
                 ($28::double precision)::numeric(18,6),
                 ($29::double precision)::numeric(18,6),
                 ($30::double precision)::numeric(18,6),
                 ($31::double precision)::numeric(18,6),
                 ($32::double precision)::numeric(18,6),
                 ($33::double precision)::numeric(18,6),
                 ($34::double precision)::numeric(18,6),
                 ($35::double precision)::numeric(18,6),
                 ($36::double precision)::numeric(18,6),
                 $37, $38, now(), now())
         ON CONFLICT (batch_id) DO UPDATE SET
            session_id = excluded.session_id,
            apparatus = excluded.apparatus,
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
            wip_status = excluded.wip_status,
            current_apparatus = excluded.current_apparatus,
            current_apparatus_key = excluded.current_apparatus_key,
            current_location = excluded.current_location,
            next_apparatus = excluded.next_apparatus,
            parent_batch_id = excluded.parent_batch_id,
            used_by_session_id = excluded.used_by_session_id,
            used_by_apparatus = excluded.used_by_apparatus,
            processed_by_session_id = excluded.processed_by_session_id,
            processed_by_apparatus = excluded.processed_by_apparatus,
            return_ink_kg = excluded.return_ink_kg,
            lamination_print_leftover_rolls = excluded.lamination_print_leftover_rolls,
            lamination_film_leftover_rolls = excluded.lamination_film_leftover_rolls,
            rezka_bosma_waste = excluded.rezka_bosma_waste,
            rezka_lamination_waste = excluded.rezka_lamination_waste,
            rezka_edge_waste = excluded.rezka_edge_waste,
            total_waste = excluded.total_waste,
            finished_goods_kg = excluded.finished_goods_kg,
            bobina_kg = excluded.bobina_kg,
            finished_goods_meter = excluded.finished_goods_meter,
            diameter = excluded.diameter,
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
    .bind(batch.wip_status.as_str())
    .bind(batch.current_apparatus.trim())
    .bind(non_empty_current_apparatus_key(batch))
    .bind(batch.current_location.trim())
    .bind(batch.next_apparatus.trim())
    .bind(batch.parent_batch_id.trim())
    .bind(batch.used_by_session_id.trim())
    .bind(batch.used_by_apparatus.trim())
    .bind(batch.processed_by_session_id.trim())
    .bind(batch.processed_by_apparatus.trim())
    .bind(batch.return_ink_kg)
    .bind(batch.lamination_print_leftover_rolls)
    .bind(batch.lamination_film_leftover_rolls)
    .bind(batch.rezka_bosma_waste)
    .bind(batch.rezka_lamination_waste)
    .bind(batch.rezka_edge_waste)
    .bind(batch.total_waste)
    .bind(batch.finished_goods_kg)
    .bind(batch.bobina_kg)
    .bind(batch.finished_goods_meter)
    .bind(batch.diameter)
    .bind(batch.description.trim())
    .bind(&batch.payload_json)
    .execute(&mut **tx)
    .await
    .map_err(|error| {
        tracing::error!(
            error = %error,
            batch_id = %batch.batch_id,
            order_id = %batch.order_id,
            apparatus = %batch.apparatus,
            action = ?batch.action,
            qr_payload = %batch.qr_payload,
            "failed to store order progress batch"
        );
        ProductionMapError::StoreFailed
    })?;
    Ok(())
}

pub(super) async fn correct_progress_batch(
    pool: &PgPool,
    current: &OrderProgressBatch,
    input: &ProgressBatchCorrectionInput,
    actor: &QueueActionActor,
) -> Result<OrderProgressBatch, ProductionMapError> {
    let expected_revision = i64::try_from(input.expected_revision)
        .map_err(|_| ProductionMapError::ProgressBatchCorrectionConflict)?;
    let corrected = current.corrected(input);
    let old_values = serde_json::to_value(current).map_err(|_| ProductionMapError::StoreFailed)?;
    let new_values =
        serde_json::to_value(&corrected).map_err(|_| ProductionMapError::StoreFailed)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let result = sqlx::query(
        "UPDATE mini_progress_batches SET
            produced_qty = ($1::double precision)::numeric(18,6),
            uom = $2,
            return_ink_kg = ($3::double precision)::numeric(18,6),
            lamination_print_leftover_rolls = ($4::double precision)::numeric(18,6),
            lamination_film_leftover_rolls = ($5::double precision)::numeric(18,6),
            rezka_bosma_waste = ($6::double precision)::numeric(18,6),
            rezka_lamination_waste = ($7::double precision)::numeric(18,6),
            rezka_edge_waste = ($8::double precision)::numeric(18,6),
            total_waste = ($9::double precision)::numeric(18,6),
            finished_goods_kg = ($10::double precision)::numeric(18,6),
            bobina_kg = ($11::double precision)::numeric(18,6),
            finished_goods_meter = ($12::double precision)::numeric(18,6),
            diameter = ($13::double precision)::numeric(18,6),
            description = $14,
            payload_json = $15,
            revision = revision + 1,
            updated_at = now()
         WHERE batch_id = $16
           AND revision = $17
           AND wip_status = 'waiting'
           AND (
               worker_ref = $18
               OR EXISTS (
                   SELECT 1
                   FROM mini_worker_identity_aliases AS alias
                   WHERE alias.worker_id = $18
                     AND alias.alias_type = 'phone'
                     AND mini_progress_batches.worker_ref ~ '^[+0-9() .-]+$'
                     AND alias.alias_key = regexp_replace(
                         mini_progress_batches.worker_ref,
                         '[^0-9]',
                         '',
                         'g'
                     )
                     AND mini_progress_batches.created_at >= alias.valid_from
                     AND (
                         alias.valid_to IS NULL
                         OR mini_progress_batches.created_at < alias.valid_to
                     )
               )
           )",
    )
    .bind(corrected.produced_qty)
    .bind(corrected.uom.trim())
    .bind(corrected.return_ink_kg)
    .bind(corrected.lamination_print_leftover_rolls)
    .bind(corrected.lamination_film_leftover_rolls)
    .bind(corrected.rezka_bosma_waste)
    .bind(corrected.rezka_lamination_waste)
    .bind(corrected.rezka_edge_waste)
    .bind(corrected.total_waste)
    .bind(corrected.finished_goods_kg)
    .bind(corrected.bobina_kg)
    .bind(corrected.finished_goods_meter)
    .bind(corrected.diameter)
    .bind(corrected.description.trim())
    .bind(&corrected.payload_json)
    .bind(corrected.batch_id.trim())
    .bind(expected_revision)
    .bind(actor.ref_.trim())
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        tracing::error!(
            error = %error,
            batch_id = %corrected.batch_id,
            expected_revision,
            "failed to correct progress batch"
        );
        ProductionMapError::StoreFailed
    })?;
    if result.rows_affected() == 0 {
        let state = sqlx::query_as::<_, (i64, String, bool)>(
            "SELECT revision,
                    wip_status,
                    (
                        worker_ref = $2
                        OR EXISTS (
                            SELECT 1
                            FROM mini_worker_identity_aliases AS alias
                            WHERE alias.worker_id = $2
                              AND alias.alias_type = 'phone'
                              AND mini_progress_batches.worker_ref ~ '^[+0-9() .-]+$'
                              AND alias.alias_key = regexp_replace(
                                  mini_progress_batches.worker_ref,
                                  '[^0-9]',
                                  '',
                                  'g'
                              )
                              AND mini_progress_batches.created_at >= alias.valid_from
                              AND (
                                  alias.valid_to IS NULL
                                  OR mini_progress_batches.created_at < alias.valid_to
                              )
                        )
                    ) AS owner_matches
             FROM mini_progress_batches
             WHERE batch_id = $1
             FOR UPDATE",
        )
        .bind(corrected.batch_id.trim())
        .bind(actor.ref_.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        return Err(match state {
            None => ProductionMapError::ProgressBatchNotFound,
            Some((_, _, false)) => ProductionMapError::ProgressBatchCorrectionForbidden,
            Some((_, wip_status, true)) if wip_status != "waiting" => {
                ProductionMapError::ProgressBatchCorrectionLocked
            }
            Some(_) => ProductionMapError::ProgressBatchCorrectionConflict,
        });
    }
    sqlx::query(
        "INSERT INTO mini_progress_batch_corrections (
            batch_id, previous_revision, new_revision, reason,
            actor_role, actor_ref, actor_display_name, old_values, new_values
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(corrected.batch_id.trim())
    .bind(expected_revision)
    .bind(expected_revision + 1)
    .bind(input.reason.trim())
    .bind(actor.role.trim())
    .bind(actor.ref_.trim())
    .bind(actor.display_name.trim())
    .bind(old_values)
    .bind(new_values)
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        tracing::error!(
            error = %error,
            batch_id = %corrected.batch_id,
            expected_revision,
            "failed to store progress batch correction audit"
        );
        ProductionMapError::StoreFailed
    })?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(corrected)
}

pub(super) async fn load_progress_batch_corrections_for_order(
    pool: &PgPool,
    order_id: &str,
) -> Result<Vec<ProgressBatchCorrectionRecord>, ProductionMapError> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, ProgressBatchCorrectionRow>(
        "SELECT correction.batch_id,
                correction.previous_revision,
                correction.new_revision,
                correction.reason,
                correction.actor_role,
                correction.actor_ref,
                correction.actor_display_name,
                correction.old_values,
                correction.new_values,
                EXTRACT(EPOCH FROM correction.created_at)::bigint AS created_at_unix
         FROM mini_progress_batch_corrections AS correction
         INNER JOIN mini_progress_batches AS batch
                 ON batch.batch_id = correction.batch_id
         WHERE batch.order_id = $1
         ORDER BY correction.created_at ASC, correction.id ASC",
    )
    .bind(order_id)
    .fetch_all(pool)
    .await
    .map_err(|error| {
        tracing::error!(
            error = %error,
            order_id,
            "failed to load progress batch correction audit"
        );
        ProductionMapError::StoreFailed
    })?;
    rows.into_iter()
        .map(progress_batch_correction_from_row)
        .collect()
}

pub(super) async fn receive_finished_goods_batch_tx(
    tx: &mut Transaction<'_, Postgres>,
    batch: &OrderProgressBatch,
    stock: &FinishedGoodsStockEntry,
) -> Result<(), ProductionMapError> {
    put_order_progress_batch_tx(tx, batch).await?;
    sqlx::query(
        "INSERT INTO mini_finished_goods_stock (
             id, warehouse, order_id, item_code, item_name, qty, uom, status, payload_json
         )
         VALUES ($1, $2, $3, $4, $5,
                 ($6::double precision)::numeric(18,6), $7, $8, $9)
         ON CONFLICT (id) DO UPDATE SET
           warehouse = excluded.warehouse,
           order_id = excluded.order_id,
           item_code = excluded.item_code,
           item_name = excluded.item_name,
           qty = excluded.qty,
           uom = excluded.uom,
           status = excluded.status,
           payload_json = excluded.payload_json,
           updated_at = now()",
    )
    .bind(stock.id.trim())
    .bind(stock.warehouse.trim())
    .bind(stock.order_id.trim())
    .bind(stock.item_code.trim())
    .bind(stock.item_name.trim())
    .bind(stock.qty)
    .bind(stock.uom.trim())
    .bind(stock.status.trim())
    .bind(&stock.payload_json)
    .execute(&mut **tx)
    .await
    .map_err(|error| {
        tracing::error!(
            error = %error,
            stock_id = %stock.id,
            batch_id = %stock.source_progress_batch_id,
            order_id = %stock.order_id,
            warehouse = %stock.warehouse,
            "failed to store finished goods receipt"
        );
        ProductionMapError::StoreFailed
    })?;
    Ok(())
}

fn non_empty_current_apparatus_key(batch: &OrderProgressBatch) -> String {
    let key = batch.current_apparatus_key.trim();
    if key.is_empty() {
        queue_state::apparatus_search_key(&batch.current_apparatus)
    } else {
        key.to_string()
    }
}

include!("progress_rows.rs");
