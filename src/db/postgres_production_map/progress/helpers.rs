use std::collections::BTreeMap;

use sqlx::{PgPool, Postgres, Transaction};

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{
    FinishedGoodsStockEntry, OrderProgressBatch, OrderProgressBatchStatus,
    OrderProgressBatchStatusDetail, OrderProgressBatchWipStatus, OrderProgressEvent,
    OrderRunSession, OrderRunStatus, ProductionMapError, ProductionOrderLogEntry,
    ProgressBatchCorrectionInput, ProgressBatchCorrectionRecord, QueueActionActor,
    order_run_input_links_from_payload, progress_batch_input_links_from_payload, queue_state,
    rezka_active_partial_rolls_from_payload, rezka_merge_state_is_consistent,
};

use super::transaction_locks::lock_order_and_apparatuses_tx;

#[cfg(test)]
#[path = "correction_tests.rs"]
mod correction_tests;

pub(super) async fn reconcile_frozen_qolip_returns(
    pool: &PgPool,
) -> Result<usize, ProductionMapError> {
    let candidates = sqlx::query_as::<_, (String, String, String)>(
        "SELECT session_id, order_id, canonical_apparatus_id FROM mini_order_run_sessions
         WHERE status = 'frozen' AND payload_json->>'qolip_lock_owner' = 'true'
         ORDER BY session_id",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut released = 0;
    for (session_id, order_id, apparatus) in candidates {
        let mut tx = pool
            .begin()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        lock_order_and_apparatuses_tx(&mut tx, &order_id, &[&apparatus]).await?;
        let payload = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT jsonb_build_object(
                'session_id', session_id, 'apparatus', canonical_apparatus_id,
                'order_id', order_id, 'stage_node_id', stage_node_id, 'status', status,
                'worker_role', worker_role, 'worker_ref', worker_ref,
                'worker_display_name', worker_display_name,
                'started_at_unix', EXTRACT(EPOCH FROM started_at)::bigint,
                'updated_at_unix', EXTRACT(EPOCH FROM updated_at)::bigint,
                'payload_json', payload_json)
             FROM mini_order_run_sessions WHERE session_id = $1 AND status = 'frozen'
                AND payload_json->>'qolip_lock_owner' = 'true' FOR UPDATE",
        )
        .bind(&session_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if let Some(payload) = payload {
            let session: OrderRunSession =
                serde_json::from_value(payload).map_err(|_| ProductionMapError::StoreFailed)?;
            put_order_run_session_tx(&mut tx, &session).await?;
            released += 1;
        }
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(released)
}

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
    validate_rezka_merge_payload(session)?;
    lock_order_and_apparatuses_tx(tx, &session.order_id, &[&session.apparatus]).await?;
    let mut session = session.clone();
    super::reactivation::release_reservations_tx(tx, &mut session).await?;
    crate::db::postgres_qolip::mileage::capture_session_cycles_tx(tx, &mut session).await?;
    if session.status == OrderRunStatus::Frozen {
        let previous = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT payload_json FROM mini_order_run_sessions WHERE session_id = $1 FOR UPDATE",
        )
        .bind(&session.session_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        // A repeated freeze must not return a later checkout of the same molds.
        let already_released = previous.as_ref().is_some_and(|payload| {
            payload
                .get("qolip_released_on_freeze")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        });
        if !already_released {
            let returned =
                crate::db::postgres_qolip::return_stopped_session_checkouts_tx(tx, &session)
                    .await
                    .map_err(super::production_map_qolip_checkout_error)?;
            session.payload_json["qolip_returned_checkout_count"] = serde_json::json!(returned);
        } else if let Some(previous) = previous {
            for key in ["qolip_returned_checkout_count", "qolip_released_at_unix"] {
                if let Some(value) = previous.get(key) {
                    session.payload_json[key] = value.clone();
                }
            }
        }
        session.release_frozen_qolips();
    } else if session.status == OrderRunStatus::Completed {
        crate::db::postgres_qolip::return_stopped_session_checkouts_tx(tx, &session)
            .await
            .map_err(super::production_map_qolip_checkout_error)?;
    }
    let apparatus_id = ApparatusId::new(session.apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO mini_order_run_sessions (
            session_id, apparatus, canonical_apparatus_id, order_id, status,
            stage_node_id,
            worker_role, worker_ref, worker_display_name,
            started_at, updated_at, payload_json
         )
         VALUES ($1, COALESCE((SELECT name FROM mini_apparatus WHERE id = $2), $2), $2,
                 $3, $4, $5, $6, $7, $8, to_timestamp($9), to_timestamp($10), $11)
         ON CONFLICT (session_id) DO UPDATE SET
            apparatus = excluded.apparatus,
            canonical_apparatus_id = excluded.canonical_apparatus_id,
            status = excluded.status,
            stage_node_id = excluded.stage_node_id,
            worker_role = excluded.worker_role,
            worker_ref = excluded.worker_ref,
            worker_display_name = excluded.worker_display_name,
            updated_at = excluded.updated_at,
            payload_json = excluded.payload_json",
    )
    .bind(session.session_id.trim())
    .bind(apparatus_id.as_str())
    .bind(session.order_id.trim())
    .bind(session.status.as_str())
    .bind(session.stage_node_id.trim())
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
    lock_order_and_apparatuses_tx(tx, &event.order_id, &[&event.apparatus]).await?;
    let apparatus_id = ApparatusId::new(event.apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO mini_order_progress_events (
            event_id, session_id, batch_id, apparatus, canonical_apparatus_id, order_id, action,
            produced_qty, uom, worker_role, worker_ref, worker_display_name,
            qr_payload, return_ink_kg, lamination_print_leftover_rolls,
            lamination_film_leftover_rolls, rezka_bosma_waste,
            rezka_lamination_waste, rezka_edge_waste, total_waste,
            finished_goods_kg, bobina_kg, finished_goods_meter, diameter, description,
            payload_json, created_at
         )
         VALUES ($1, $2, $3, COALESCE((SELECT name FROM mini_apparatus WHERE id = $4), $4), $4, $5, $6,
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
            event_id = excluded.event_id
         WHERE mini_order_progress_events.session_id IS NOT DISTINCT FROM excluded.session_id
           AND mini_order_progress_events.batch_id IS NOT DISTINCT FROM excluded.batch_id
           AND mini_order_progress_events.canonical_apparatus_id
               IS NOT DISTINCT FROM excluded.canonical_apparatus_id
           AND mini_order_progress_events.order_id IS NOT DISTINCT FROM excluded.order_id
           AND mini_order_progress_events.action IS NOT DISTINCT FROM excluded.action
           AND mini_order_progress_events.produced_qty IS NOT DISTINCT FROM excluded.produced_qty
           AND mini_order_progress_events.uom IS NOT DISTINCT FROM excluded.uom
           AND mini_order_progress_events.worker_role IS NOT DISTINCT FROM excluded.worker_role
           AND mini_order_progress_events.worker_ref IS NOT DISTINCT FROM excluded.worker_ref
           AND mini_order_progress_events.worker_display_name
               IS NOT DISTINCT FROM excluded.worker_display_name
           AND mini_order_progress_events.qr_payload IS NOT DISTINCT FROM excluded.qr_payload
           AND mini_order_progress_events.return_ink_kg
               IS NOT DISTINCT FROM excluded.return_ink_kg
           AND mini_order_progress_events.lamination_print_leftover_rolls
               IS NOT DISTINCT FROM excluded.lamination_print_leftover_rolls
           AND mini_order_progress_events.lamination_film_leftover_rolls
               IS NOT DISTINCT FROM excluded.lamination_film_leftover_rolls
           AND mini_order_progress_events.rezka_bosma_waste
               IS NOT DISTINCT FROM excluded.rezka_bosma_waste
           AND mini_order_progress_events.rezka_lamination_waste
               IS NOT DISTINCT FROM excluded.rezka_lamination_waste
           AND mini_order_progress_events.rezka_edge_waste
               IS NOT DISTINCT FROM excluded.rezka_edge_waste
           AND mini_order_progress_events.total_waste
               IS NOT DISTINCT FROM excluded.total_waste
           AND mini_order_progress_events.finished_goods_kg
               IS NOT DISTINCT FROM excluded.finished_goods_kg
           AND mini_order_progress_events.bobina_kg IS NOT DISTINCT FROM excluded.bobina_kg
           AND mini_order_progress_events.finished_goods_meter
               IS NOT DISTINCT FROM excluded.finished_goods_meter
           AND mini_order_progress_events.diameter IS NOT DISTINCT FROM excluded.diameter
           AND mini_order_progress_events.description IS NOT DISTINCT FROM excluded.description
           AND mini_order_progress_events.payload_json IS NOT DISTINCT FROM excluded.payload_json
         RETURNING event_id",
    )
    .bind(event.event_id.trim())
    .bind(event.session_id.trim())
    .bind(event.batch_id.trim())
    .bind(apparatus_id.as_str())
    .bind(event.order_id.trim())
    .bind(event.action.as_str())
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
    .fetch_optional(&mut **tx)
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
    })?
    .ok_or(ProductionMapError::StoreFailed)?;
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
    let order_id = batch.order_id.trim();
    if !order_id.is_empty() {
        let actor = QueueActionActor {
            role: "system".to_string(),
            ref_: "progress_batch_writer".to_string(),
            display_name: "Progress Batch Writer".to_string(),
        };
        crate::db::postgres_production_map::lifecycle::refresh_production_order_lifecycle_tx(
            &mut tx,
            order_id,
            &actor,
            &batch.batch_id,
            "put_order_progress_batch",
        )
        .await?;
    }
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}

include!("helpers_parts/put_order_progress_batch_tx.rs");

include!("helpers_parts/store_finished_goods_batch_tx.rs");

#[cfg(test)]
#[path = "mileage_tests.rs"]
mod mileage_integration_tests;
