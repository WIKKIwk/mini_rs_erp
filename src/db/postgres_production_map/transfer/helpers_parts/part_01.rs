
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
    reject_qolip_in_use_tx(&mut tx, &write.session).await?;

    put_map_inner_tx(&mut tx, &write.updated_map).await?;
    let mut from_sequence = queue_sequence_for_update_tx(&mut tx, from_apparatus).await?;
    from_sequence.retain(|order_id| order_id.trim() != write.record.order_id.trim());
    let mut to_sequence = queue_sequence_for_update_tx(&mut tx, to_apparatus).await?;
    to_sequence.retain(|order_id| order_id.trim() != write.record.order_id.trim());
    to_sequence.push(write.record.order_id.trim().to_string());
    save_apparatus_sequence_tx(&mut tx, from_apparatus, &from_sequence).await?;
    save_apparatus_sequence_tx(&mut tx, to_apparatus, &to_sequence).await?;
    transfer_queue_state_tx(
        &mut tx,
        from_apparatus,
        to_apparatus,
        &write.record.order_id,
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
    put_order_progress_batch_tx(&mut tx, &write.progress_batch).await?;
    for batch in &write.progress_batch_updates {
        put_order_progress_batch_tx(&mut tx, batch).await?;
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
         VALUES (COALESCE((SELECT name FROM mini_apparatus WHERE id = $1), $1), $1, $2, 'paused', now())",
    )
    .bind(to_apparatus)
    .bind(order_id.trim())
    .execute(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}
