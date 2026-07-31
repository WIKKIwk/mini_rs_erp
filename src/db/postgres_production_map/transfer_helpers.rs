use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{
    ProductionMapApparatusTransferRecord, ProductionMapApparatusTransferWrite, ProductionMapError,
};

use super::catalog_helpers::save_apparatus_sequence_tx;
use super::map_helpers::put_map_inner_tx;
use super::material_helpers::transfer_raw_material_assignments_tx;
use super::progress_helpers::{put_order_progress_batch_tx, put_order_run_session_tx};
use super::qolip_session_helpers::reject_qolip_in_use_tx;
use super::queue_helpers::put_queue_states_tx;

pub(super) async fn load_apparatus_transfer_by_idempotency_key(
    pool: &PgPool,
    idempotency_key: &str,
) -> Result<Option<ProductionMapApparatusTransferRecord>, ProductionMapError> {
    let payload = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload_json
         FROM mini_apparatus_order_transfers
         WHERE idempotency_key = $1",
    )
    .bind(idempotency_key.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    payload
        .map(|payload| serde_json::from_value(payload).map_err(|_| ProductionMapError::StoreFailed))
        .transpose()
}

pub(super) async fn commit_apparatus_transfer(
    pool: &PgPool,
    write: ProductionMapApparatusTransferWrite,
) -> Result<ProductionMapApparatusTransferRecord, ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;

    let record_payload =
        serde_json::to_value(&write.record).map_err(|_| ProductionMapError::StoreFailed)?;
    let inserted = sqlx::query_scalar::<_, serde_json::Value>(
        "INSERT INTO mini_apparatus_order_transfers (
             transfer_id, idempotency_key, order_id, from_apparatus, to_apparatus,
             reason, actor_role, actor_ref, actor_display_name, session_id,
             progress_batch_id, material_barcodes, payload_json, created_at
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                 to_timestamp($14))
         ON CONFLICT (idempotency_key) DO NOTHING
         RETURNING payload_json",
    )
    .bind(write.record.transfer_id.trim())
    .bind(write.record.idempotency_key.trim())
    .bind(write.record.order_id.trim())
    .bind(write.record.from_apparatus.trim())
    .bind(write.record.to_apparatus.trim())
    .bind(write.record.reason.trim())
    .bind(write.record.actor.role.trim())
    .bind(write.record.actor.ref_.trim())
    .bind(write.record.actor.display_name.trim())
    .bind(write.record.session_id.trim())
    .bind(write.record.progress_batch_id.trim())
    .bind(
        serde_json::to_value(&write.record.material_barcodes)
            .map_err(|_| ProductionMapError::StoreFailed)?,
    )
    .bind(record_payload)
    .bind(write.record.created_at_unix as f64)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    let Some(_) = inserted else {
        let existing = transfer_payload_tx(&mut tx, &write.record.idempotency_key).await?;
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        return existing.ok_or(ProductionMapError::StoreFailed);
    };

    lock_transfer_rows(&mut tx, &write).await?;
    verify_transfer_preconditions(&mut tx, &write).await?;
    reject_qolip_in_use_tx(&mut tx, &write.session).await?;

    put_map_inner_tx(&mut tx, &write.updated_map).await?;
    save_apparatus_sequence_tx(&mut tx, &write.record.from_apparatus, &write.from_sequence).await?;
    save_apparatus_sequence_tx(&mut tx, &write.record.to_apparatus, &write.to_sequence).await?;
    put_queue_states_tx(&mut tx, &write.record.from_apparatus, write.from_states).await?;
    put_queue_states_tx(&mut tx, &write.record.to_apparatus, write.to_states).await?;
    transfer_raw_material_assignments_tx(&mut tx, &write.raw_material_assignments).await?;
    put_order_run_session_tx(&mut tx, &write.session).await?;
    put_order_progress_batch_tx(&mut tx, &write.progress_batch).await?;

    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(write.record)
}

async fn transfer_payload_tx(
    tx: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
) -> Result<Option<ProductionMapApparatusTransferRecord>, ProductionMapError> {
    let payload = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload_json
         FROM mini_apparatus_order_transfers
         WHERE idempotency_key = $1
         FOR UPDATE",
    )
    .bind(idempotency_key.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    payload
        .map(|payload| serde_json::from_value(payload).map_err(|_| ProductionMapError::StoreFailed))
        .transpose()
}

async fn lock_transfer_rows(
    tx: &mut Transaction<'_, Postgres>,
    write: &ProductionMapApparatusTransferWrite,
) -> Result<(), ProductionMapError> {
    sqlx::query(
        "SELECT id
         FROM mini_production_maps
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(write.record.order_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::MapNotFound)?;

    for apparatus in [
        write.record.from_apparatus.trim(),
        write.record.to_apparatus.trim(),
    ] {
        sqlx::query(
            "SELECT apparatus
             FROM mini_queue_sequences
             WHERE apparatus = $1
             FOR UPDATE",
        )
        .bind(apparatus)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        sqlx::query(
            "SELECT order_id
             FROM mini_queue_states
             WHERE apparatus = $1
             FOR UPDATE",
        )
        .bind(apparatus)
        .fetch_all(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(())
}

async fn verify_transfer_preconditions(
    tx: &mut Transaction<'_, Postgres>,
    write: &ProductionMapApparatusTransferWrite,
) -> Result<(), ProductionMapError> {
    let source_state = sqlx::query_scalar::<_, String>(
        "SELECT state
         FROM mini_queue_states
         WHERE apparatus = $1 AND order_id = $2",
    )
    .bind(write.record.from_apparatus.trim())
    .bind(write.record.order_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if source_state.as_deref() != Some("paused") {
        return Err(ProductionMapError::ApparatusTransferOrderNotPaused);
    }

    let target_has_order = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1 FROM mini_queue_states
             WHERE apparatus = $1 AND order_id = $2
         )",
    )
    .bind(write.record.to_apparatus.trim())
    .bind(write.record.order_id.trim())
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if target_has_order {
        return Err(ProductionMapError::ApparatusTransferTargetConflict);
    }

    let session = sqlx::query_as::<_, (String, String, String)>(
        "SELECT apparatus, order_id, status
         FROM mini_order_run_sessions
         WHERE session_id = $1
         FOR UPDATE",
    )
    .bind(write.record.session_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::ApparatusTransferSessionNotFound)?;
    if session.1.trim() != write.record.order_id.trim()
        || !session
            .0
            .trim()
            .eq_ignore_ascii_case(write.record.from_apparatus.trim())
        || session.2 != "paused"
    {
        return Err(ProductionMapError::ApparatusTransferSessionMismatch);
    }

    let batch = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT apparatus, order_id, session_id, action, status
         FROM mini_progress_batches
         WHERE batch_id = $1
         FOR UPDATE",
    )
    .bind(write.record.progress_batch_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::ApparatusTransferProgressNotFound)?;
    if batch.1.trim() != write.record.order_id.trim()
        || batch.2.trim() != write.record.session_id.trim()
        || !batch
            .0
            .trim()
            .eq_ignore_ascii_case(write.record.from_apparatus.trim())
        || batch.3 != "pause"
        || batch.4 != "paused"
    {
        return Err(ProductionMapError::ApparatusTransferProgressMismatch);
    }
    Ok(())
}
