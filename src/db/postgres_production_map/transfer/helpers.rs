use sqlx::{PgPool, Postgres, Transaction};

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{
    ProductionMapApparatusTransferRecord, ProductionMapApparatusTransferWrite, ProductionMapError,
};

use super::catalog_helpers::save_apparatus_sequence_tx;
use super::map_helpers::put_map_inner_tx;
use super::material_helpers::transfer_raw_material_assignments_tx;
use super::progress_helpers::{put_order_progress_batch_tx, put_order_run_session_tx};
use super::qolip_session_helpers::reject_qolip_in_use_tx;
use super::transaction_locks::{
    lock_order_and_apparatuses_tx, lock_schedule_reservation_tx, lock_transfer_idempotency_tx,
};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;


pub(super) async fn load_apparatus_transfer_by_idempotency_key(
    pool: &PgPool,
    idempotency_key: &str,
) -> Result<Option<ProductionMapApparatusTransferRecord>, ProductionMapError> {
    let payload = sqlx::query_as::<_, (String, String, serde_json::Value)>(
        "SELECT canonical_from_apparatus_id, canonical_to_apparatus_id, payload_json
         FROM mini_apparatus_order_transfers
         WHERE idempotency_key = $1
           AND canonical_from_apparatus_id IS NOT NULL
           AND canonical_to_apparatus_id IS NOT NULL
           AND EXISTS (
               SELECT 1
               FROM mini_apparatus from_master
               WHERE from_master.id = mini_apparatus_order_transfers.canonical_from_apparatus_id
           )
           AND EXISTS (
               SELECT 1
               FROM mini_apparatus to_master
               WHERE to_master.id = mini_apparatus_order_transfers.canonical_to_apparatus_id
           )",
    )
    .bind(idempotency_key.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    payload
        .map(|(from_apparatus, to_apparatus, payload)| {
            transfer_record_from_payload(from_apparatus, to_apparatus, payload)
        })
        .transpose()
}

pub(super) async fn load_apparatus_transfers_for_audit(
    pool: &PgPool,
) -> Result<Vec<ProductionMapApparatusTransferRecord>, ProductionMapError> {
    let payloads = sqlx::query_as::<_, (String, String, serde_json::Value)>(
        "SELECT canonical_from_apparatus_id, canonical_to_apparatus_id, payload_json
         FROM mini_apparatus_order_transfers
         WHERE canonical_from_apparatus_id IS NOT NULL
           AND canonical_to_apparatus_id IS NOT NULL
           AND EXISTS (
               SELECT 1
               FROM mini_apparatus from_master
               WHERE from_master.id = mini_apparatus_order_transfers.canonical_from_apparatus_id
           )
           AND EXISTS (
               SELECT 1
               FROM mini_apparatus to_master
               WHERE to_master.id = mini_apparatus_order_transfers.canonical_to_apparatus_id
           )
         ORDER BY created_at ASC, transfer_id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    payloads
        .into_iter()
        .map(|(from_apparatus, to_apparatus, payload)| {
            transfer_record_from_payload(from_apparatus, to_apparatus, payload)
        })
        .collect()
}

pub(super) async fn commit_apparatus_transfer(
    pool: &PgPool,
    write: ProductionMapApparatusTransferWrite,
) -> Result<ProductionMapApparatusTransferRecord, ProductionMapError> {
    let from_apparatus = write.record.from_apparatus.trim();
    let to_apparatus = write.record.to_apparatus.trim();
    let from_apparatus_id = ApparatusId::new(from_apparatus.to_string());
    let to_apparatus_id = ApparatusId::new(to_apparatus.to_string());
    if from_apparatus_id.is_err()
        || to_apparatus_id.is_err()
        || write.target_apparatus_id.trim() != to_apparatus
    {
        return Err(ProductionMapError::MoveNotAllowed);
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    lock_transfer_idempotency_tx(&mut tx, &write.record.idempotency_key).await?;
    let reservation_ids = sqlx::query_scalar::<_, String>(
        "SELECT reservation_id
         FROM mini_apparatus_schedule_reservations
         WHERE order_id = $1
           AND status = 'paused'
           AND canonical_apparatus_id = $2
         ORDER BY reservation_id ASC",
    )
    .bind(write.record.order_id.trim())
    .bind(from_apparatus.trim())
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    for reservation_id in reservation_ids {
        lock_schedule_reservation_tx(&mut tx, &reservation_id).await?;
    }
    lock_order_and_apparatuses_tx(
        &mut tx,
        &write.record.order_id,
        &[from_apparatus, to_apparatus],
    )
    .await?;

    let record_payload =
        serde_json::to_value(&write.record).map_err(|_| ProductionMapError::StoreFailed)?;
    let inserted = sqlx::query_scalar::<_, serde_json::Value>(
        "INSERT INTO mini_apparatus_order_transfers (
             transfer_id, idempotency_key, order_id, from_apparatus, to_apparatus,
             canonical_from_apparatus_id, canonical_to_apparatus_id,
             reason, actor_role, actor_ref, actor_display_name, session_id,
             progress_batch_id, material_barcodes, payload_json, created_at
         )
         VALUES ($1, $2, $3,
                 COALESCE((SELECT name FROM mini_apparatus WHERE id = $4), $4),
                 COALESCE((SELECT name FROM mini_apparatus WHERE id = $5), $5),
                 $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
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
    .bind(
        (!write.record.progress_batch_id.trim().is_empty())
            .then_some(write.record.progress_batch_id.trim()),
    )
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
        let existing = existing.ok_or(ProductionMapError::StoreFailed)?;
        if !transfer_idempotency_matches(&existing, &write.record) {
            return Err(ProductionMapError::ApparatusTransferIdempotencyConflict);
        }
        tx.commit()
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        return Ok(existing);
    };

    lock_transfer_rows(&mut tx, &write).await?;
    verify_transfer_preconditions(&mut tx, &write).await?;
    let mut revisions = write.expected_apparatus_revisions.clone();
    revisions.sort_by(|left, right| left.0.cmp(&right.0));
    for (apparatus, expected) in revisions {
        // Hold the canonical runtime row through commit: a concurrent profile
        // change must not invalidate the capacity/tooling validation we used.
        let current = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT source_revision FROM mini_apparatus WHERE id=$1 FOR SHARE",
        ).bind(apparatus).fetch_optional(&mut *tx).await
            .map_err(|_| ProductionMapError::StoreFailed)?.flatten();
        if current.and_then(|value| u64::try_from(value).ok()) != Some(expected) {
            return Err(ProductionMapError::MoveNotAllowed);
        }
    }
    reject_qolip_in_use_tx(&mut tx, &write.session).await?;

    put_map_inner_tx(&mut tx, &write.updated_map).await?;
    let mut from_sequence = queue_sequence_for_update_tx(&mut tx, from_apparatus).await?;
    from_sequence.retain(|order_id| order_id.trim() != write.record.order_id.trim());
    let mut to_sequence = queue_sequence_for_update_tx(&mut tx, to_apparatus).await?;
    if let Some(canonical) = &write.print_target_configuration {
        let maps = super::sequence_move::load_maps_tx(&mut tx).await?;
        let states: Vec<(String, String)> = sqlx::query_as(
            "SELECT order_id,state FROM mini_queue_states WHERE canonical_apparatus_id=$1",
        ).bind(to_apparatus).fetch_all(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        let frozen: Vec<String> = sqlx::query_scalar(
            "SELECT order_id FROM mini_order_control_states WHERE state='frozen'",
        ).fetch_all(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        to_sequence = crate::core::production_map::SequenceMoveState::from_data(
            canonical, &maps, &to_sequence, &states.into_iter().collect(),
            &frozen.into_iter().collect(), &std::collections::BTreeSet::new(),
        ).order_ids;
    }
    to_sequence.retain(|order_id| order_id.trim() != write.record.order_id.trim());
    to_sequence.push(write.record.order_id.trim().to_string());
    save_apparatus_sequence_tx(&mut tx, from_apparatus, &from_sequence).await?;
    save_apparatus_sequence_tx(&mut tx, to_apparatus, &to_sequence).await?;
    transfer_queue_state_tx(
        &mut tx,
        from_apparatus,
        to_apparatus,
        &write.record.order_id,
        write.record.queue_state.as_str(),
    )
    .await?;
    transfer_raw_material_assignments_tx(
        &mut tx,
        &write.raw_material_assignments,
        &write.record.from_apparatus,
        &write.record.transfer_id,
        &write.record.actor,
    )
    .await?;
    put_order_run_session_tx(&mut tx, &write.session).await?;
    if let Some(batch) = &write.progress_batch {
        put_order_progress_batch_tx(&mut tx, batch).await?;
    }
    for batch in &write.progress_batch_updates {
        put_order_progress_batch_tx(&mut tx, batch).await?;
    }
    for batch in &write.opening_wip_batch_updates {
        let updated = sqlx::query(
            "UPDATE mini_opening_wip_batches SET used_by_apparatus=$1, updated_at=to_timestamp($2)
             WHERE batch_id=$3 AND order_id=$4 AND wip_status='in_use'
               AND used_by_session_id=$5 AND used_by_apparatus=$6",
        ).bind(to_apparatus).bind(batch.updated_at_unix as f64).bind(&batch.batch_id)
            .bind(&write.record.order_id).bind(&write.session.session_id).bind(from_apparatus)
            .execute(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        if updated.rows_affected() != 1 { return Err(ProductionMapError::ProgressBatchNotResumable); }
    }
    sqlx::query(
        "UPDATE mini_apparatus_schedule_reservations AS reservation
         SET canonical_apparatus_id = $1,
             apparatus_id = $1,
             apparatus = COALESCE(
                 (SELECT target.name FROM mini_apparatus target WHERE target.id = $1),
                 $2
             ),
             actor_json = $3
         WHERE reservation.order_id = $4
           AND reservation.status = 'paused'
           AND reservation.canonical_apparatus_id = $5",
    )
    .bind(write.target_apparatus_id.trim())
    .bind(write.record.to_apparatus.trim())
    .bind(serde_json::to_value(&write.record.actor).map_err(|_| ProductionMapError::StoreFailed)?)
    .bind(write.record.order_id.trim())
    .bind(write.record.from_apparatus.trim())
    .execute(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    crate::db::postgres_production_map::lifecycle::refresh_production_order_lifecycle_tx(
        &mut tx,
        write.record.order_id.trim(),
        &write.record.actor,
        &write.record.transfer_id,
        "apparatus_transfer",
    )
    .await?;

    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(write.record)
}

fn transfer_idempotency_matches(
    existing: &ProductionMapApparatusTransferRecord,
    incoming: &ProductionMapApparatusTransferRecord,
) -> bool {
    existing.order_id.trim() == incoming.order_id.trim()
        && existing.from_apparatus.trim() == incoming.from_apparatus.trim()
        && existing.to_apparatus.trim() == incoming.to_apparatus.trim()
}

async fn transfer_payload_tx(
    tx: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
) -> Result<Option<ProductionMapApparatusTransferRecord>, ProductionMapError> {
    let payload = sqlx::query_as::<_, (String, String, serde_json::Value)>(
        "SELECT canonical_from_apparatus_id, canonical_to_apparatus_id, payload_json
         FROM mini_apparatus_order_transfers
         WHERE idempotency_key = $1
           AND canonical_from_apparatus_id IS NOT NULL
           AND canonical_to_apparatus_id IS NOT NULL
           AND EXISTS (
               SELECT 1
               FROM mini_apparatus from_master
               WHERE from_master.id = mini_apparatus_order_transfers.canonical_from_apparatus_id
           )
           AND EXISTS (
               SELECT 1
               FROM mini_apparatus to_master
               WHERE to_master.id = mini_apparatus_order_transfers.canonical_to_apparatus_id
           )
         FOR UPDATE",
    )
    .bind(idempotency_key.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    payload
        .map(|(from_apparatus, to_apparatus, payload)| {
            transfer_record_from_payload(from_apparatus, to_apparatus, payload)
        })
        .transpose()
}

fn transfer_record_from_payload(
    from_apparatus: String,
    to_apparatus: String,
    mut payload: serde_json::Value,
) -> Result<ProductionMapApparatusTransferRecord, ProductionMapError> {
    let from_apparatus = ApparatusId::new(from_apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::StoreFailed)?
        .to_string();
    let to_apparatus = ApparatusId::new(to_apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::StoreFailed)?
        .to_string();
    let object = payload
        .as_object_mut()
        .ok_or(ProductionMapError::StoreFailed)?;
    object.insert(
        "from_apparatus".to_string(),
        serde_json::Value::String(from_apparatus),
    );
    object.insert(
        "to_apparatus".to_string(),
        serde_json::Value::String(to_apparatus.clone()),
    );
    if let Some(session) = object
        .get_mut("session")
        .and_then(|value| value.as_object_mut())
    {
        session.insert(
            "apparatus".to_string(),
            serde_json::Value::String(to_apparatus.clone()),
        );
    }
    if let Some(progress_batch) = object
        .get_mut("progress_batch")
        .and_then(|value| value.as_object_mut())
    {
        progress_batch.insert(
            "apparatus".to_string(),
            serde_json::Value::String(to_apparatus),
        );
    }
    serde_json::from_value(payload).map_err(|_| ProductionMapError::StoreFailed)
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
            "SELECT canonical_apparatus_id
             FROM mini_queue_sequences
             WHERE canonical_apparatus_id = $1
             FOR UPDATE",
        )
        .bind(apparatus)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        sqlx::query(
            "SELECT order_id
             FROM mini_queue_states
             WHERE canonical_apparatus_id = $1
             FOR UPDATE",
        )
        .bind(apparatus)
        .fetch_all(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    Ok(())
}

async fn queue_sequence_for_update_tx(
    tx: &mut Transaction<'_, Postgres>,
    apparatus: &str,
) -> Result<Vec<String>, ProductionMapError> {
    let payload = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT order_ids
         FROM mini_queue_sequences
         WHERE canonical_apparatus_id = $1
         FOR UPDATE",
    )
    .bind(apparatus)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    payload
        .map(|payload| serde_json::from_value(payload).map_err(|_| ProductionMapError::StoreFailed))
        .transpose()
        .map(|value| value.unwrap_or_default())
}

async fn transfer_queue_state_tx(
    tx: &mut Transaction<'_, Postgres>,
    from_apparatus: &str,
    to_apparatus: &str,
    order_id: &str,
    state: &str,
) -> Result<(), ProductionMapError> {
    let removed = sqlx::query(
        "DELETE FROM mini_queue_states
         WHERE canonical_apparatus_id = $1 AND order_id = $2",
    )
    .bind(from_apparatus)
    .bind(order_id.trim())
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if removed.rows_affected() != 1 {
        return Err(ProductionMapError::ApparatusTransferOrderNotPaused);
    }
    sqlx::query(
        "INSERT INTO mini_queue_states
            (apparatus, canonical_apparatus_id, order_id, state, updated_at)
         VALUES (COALESCE((SELECT name FROM mini_apparatus WHERE id = $1), $1), $1, $2, $3, now())",
    )
    .bind(to_apparatus)
    .bind(order_id.trim())
    .bind(state)
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}


async fn verify_transfer_preconditions(
    tx: &mut Transaction<'_, Postgres>,
    write: &ProductionMapApparatusTransferWrite,
) -> Result<(), ProductionMapError> {
    let source_state = sqlx::query_scalar::<_, String>(
        "SELECT state
         FROM mini_queue_states
         WHERE canonical_apparatus_id = $1 AND order_id = $2
         FOR UPDATE",
    )
    .bind(write.record.from_apparatus.trim())
    .bind(write.record.order_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    match source_state.as_deref() {
        Some("paused" | "pending") => {}
        Some("frozen") => return Err(ProductionMapError::OrderFrozen),
        Some("completed") => return Err(ProductionMapError::OrderAlreadyCompleted),
        _ => return Err(ProductionMapError::ApparatusTransferOrderNotPaused),
    }
    if let Some(expected) = &write.expected_map {
        let current = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT map_json FROM mini_production_maps WHERE id = $1 FOR UPDATE",
        ).bind(write.record.order_id.trim()).fetch_one(&mut **tx).await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let current: crate::core::production_map::ProductionMapDefinition =
            serde_json::from_value(current).map_err(|_| ProductionMapError::StoreFailed)?;
        if &current != expected { return Err(ProductionMapError::WipRouteChanged); }
    }

    let control_state = sqlx::query_scalar::<_, String>(
        "SELECT state
         FROM mini_order_control_states
         WHERE order_id = $1
         FOR UPDATE",
    )
    .bind(write.record.order_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    match control_state.as_deref() {
        Some("freeze_requested") => return Err(ProductionMapError::OrderFreezeRequested),
        Some("frozen") => return Err(ProductionMapError::OrderFrozen),
        _ => {}
    }

    let target_has_order = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1 FROM mini_queue_states
             WHERE canonical_apparatus_id = $1 AND order_id = $2
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
    let target_session = sqlx::query_scalar::<_, String>(
        "SELECT session_id
         FROM mini_order_run_sessions
         WHERE canonical_apparatus_id = $1
           AND order_id = $2
           AND status IN ('active', 'paused', 'frozen', 'roll_detached')
         FOR UPDATE",
    )
    .bind(write.record.to_apparatus.trim())
    .bind(write.record.order_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if target_session.is_some() {
        return Err(ProductionMapError::ApparatusTransferTargetConflict);
    }

    let session = sqlx::query_as::<_, (String, String, String, serde_json::Value)>(
        "SELECT canonical_apparatus_id, order_id, status, payload_json
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
        || session.0.trim() != write.record.from_apparatus.trim()
        || !(session.2 == "paused" || (write.record.preserves_output_history && session.2 == "roll_detached"))
    {
        return Err(ProductionMapError::ApparatusTransferSessionMismatch);
    }
    if let Some(expected) = &write.expected_session {
        let current = super::order_query_helpers::load_order_run_session(&mut **tx, &write.record.session_id)
            .await?.ok_or(ProductionMapError::ApparatusTransferSessionNotFound)?;
        if &current != expected { return Err(ProductionMapError::ApparatusTransferSessionMismatch); }
        let controls = sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT record_json FROM mini_stage_order_controls WHERE order_id = $1 FOR UPDATE",
        ).bind(write.record.order_id.trim()).fetch_all(&mut **tx).await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        for control in controls {
            let control: crate::core::production_map::OrderControlRecord =
                serde_json::from_value(control).map_err(|_| ProductionMapError::StoreFailed)?;
            if control.freeze_request.as_ref().is_some_and(|request| request.target_apparatus == write.record.from_apparatus) {
                match control.state {
                    crate::core::production_map::OrderControlState::Frozen => return Err(ProductionMapError::OrderFrozen),
                    crate::core::production_map::OrderControlState::FreezeRequested => return Err(ProductionMapError::OrderFreezeRequested),
                    _ => {}
                }
            }
        }
    }
    let requeued = session.3.get("requeued_at_tail")
        .and_then(serde_json::Value::as_bool) == Some(true);
    if (source_state.as_deref() == Some("pending") && !requeued)
        || source_state.as_deref() != Some(write.record.queue_state.as_str())
    {
        return Err(ProductionMapError::ApparatusTransferOrderNotPaused);
    }

    if write.record.preserves_output_history {
        if !write.session.is_print_transfer_continuation() || write.progress_batch.is_some()
            || !write.record.progress_batch_id.is_empty() || write.expected_map.is_none() || write.expected_session.is_none()
        {
            return Err(ProductionMapError::ApparatusTransferProgressMismatch);
        }
        let current = sqlx::query_scalar::<_, String>(
            "SELECT a.barcode FROM mini_raw_material_assignments a
             WHERE a.order_id=$1 AND a.canonical_apparatus_id=$2
               AND NOT EXISTS(SELECT 1 FROM mini_raw_material_stock s
                   WHERE lower(s.barcode)=lower(a.barcode) AND s.status='consumed')
             ORDER BY a.barcode FOR UPDATE OF a",
        ).bind(&write.record.order_id).bind(&write.record.from_apparatus)
            .fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        let normalize = |barcodes: Vec<String>| barcodes.into_iter()
            .map(|barcode| barcode.trim().to_ascii_uppercase()).collect::<std::collections::BTreeSet<_>>();
        if normalize(current) != normalize(write.raw_material_assignments.iter().map(|a| a.barcode.clone()).collect()) {
            return Err(ProductionMapError::RawMaterialAssignmentLocked);
        }
    } else if let Some(expected_batch) = &write.progress_batch {
        let batch = sqlx::query_as::<_, (String, String, String, String, String)>(
            "SELECT canonical_apparatus_id, order_id, session_id, action, status
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
            || batch.0.trim() != write.record.from_apparatus.trim()
            || batch.3 != "pause"
            || batch.4 != "paused"
            || expected_batch.batch_id.trim() != write.record.progress_batch_id.trim()
        {
            return Err(ProductionMapError::ApparatusTransferProgressMismatch);
        }
    } else {
        let has_paused_batch = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM mini_progress_batches
             WHERE session_id = $1 AND canonical_apparatus_id = $2
               AND action = 'pause' AND status = 'paused')",
        )
        .bind(write.record.session_id.trim())
        .bind(write.record.from_apparatus.trim())
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if source_state.as_deref() != Some("pending")
            || !requeued
            || !write.record.progress_batch_id.is_empty()
            || has_paused_batch
        {
            return Err(ProductionMapError::ApparatusTransferProgressMismatch);
        }
    }
    for update in &write.progress_batch_updates {
        if write.record.preserves_output_history {
            let current = super::order_query_helpers::load_progress_batch(&mut **tx, &update.batch_id)
                .await?.ok_or(ProductionMapError::WipRouteChanged)?;
            if !write.expected_input_batches.contains(&current)
                || current.wip_status != crate::core::production_map::OrderProgressBatchWipStatus::InUse
                || current.used_by_session_id != write.session.session_id
                || current.used_by_apparatus != write.record.from_apparatus
            { return Err(ProductionMapError::WipRouteChanged); }
        }
        let update_order_id = sqlx::query_scalar::<_, String>(
            "SELECT order_id
             FROM mini_progress_batches
             WHERE batch_id = $1
             FOR UPDATE",
        )
        .bind(update.batch_id.trim())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or(ProductionMapError::ApparatusTransferProgressMismatch)?;
        if update_order_id.trim() != write.record.order_id.trim() {
            return Err(ProductionMapError::ApparatusTransferProgressMismatch);
        }
    }
    Ok(())
}
