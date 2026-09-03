
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

pub(super) async fn put_order_progress_batch_tx(
    tx: &mut Transaction<'_, Postgres>,
    batch: &OrderProgressBatch,
) -> Result<(), ProductionMapError> {
    progress_batch_input_links_from_payload(&batch.payload_json)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut locked_apparatuses = vec![batch.apparatus.as_str()];
    for apparatus in [
        batch.current_apparatus.as_str(),
        batch.next_apparatus.as_str(),
        batch.used_by_apparatus.as_str(),
        batch.processed_by_apparatus.as_str(),
    ] {
        if !apparatus.trim().is_empty() && !is_warehouse_processing_marker(apparatus) {
            locked_apparatuses.push(apparatus);
        }
    }
    lock_order_and_apparatuses_tx(tx, &batch.order_id, &locked_apparatuses).await?;
    let apparatus_id = ApparatusId::new(batch.apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::StoreFailed)?;
    for apparatus in [
        batch.apparatus.as_str(),
        batch.current_apparatus.as_str(),
        batch.next_apparatus.as_str(),
        batch.used_by_apparatus.as_str(),
        batch.processed_by_apparatus.as_str(),
    ] {
        if !apparatus.trim().is_empty() && !is_warehouse_processing_marker(apparatus) {
            require_live_apparatus_id(apparatus)?;
        }
    }
    let result = sqlx::query(
        "INSERT INTO mini_progress_batches (
            batch_id, session_id, apparatus, canonical_apparatus_id, order_id, action, status,
            produced_qty, uom, qr_payload, label_item_code, label_item_name,
            executor_name, worker_role, worker_ref, worker_display_name,
            wip_status, current_apparatus, canonical_current_apparatus_id,
            current_location, next_apparatus, canonical_next_apparatus_id,
            parent_batch_id, used_by_session_id, used_by_apparatus,
            canonical_used_by_apparatus_id, processed_by_session_id, processed_by_apparatus,
            canonical_processed_by_apparatus_id,
            return_ink_kg, lamination_print_leftover_rolls,
            lamination_film_leftover_rolls, rezka_bosma_waste,
            rezka_lamination_waste, rezka_edge_waste, total_waste,
            finished_goods_kg, bobina_kg, finished_goods_meter, diameter, description,
            payload_json, revision, created_at, updated_at
         )
         VALUES ($1, $2,
                 COALESCE((SELECT name FROM mini_apparatus WHERE id = $3), $3), $3,
                 $4, $5, $6,
                 ($7::double precision)::numeric(18,6),
                 $8, $9, $10, $11, $12, $13, $14, $15, $16,
                 $17, NULLIF($17, ''), $18, $19, NULLIF($19, ''),
                 $20, $21, $22,
                 CASE WHEN lower($22) LIKE 'warehouse:%' THEN NULL ELSE NULLIF($22, '') END,
                 $23, $24,
                 CASE WHEN lower($24) LIKE 'warehouse:%' THEN NULL ELSE NULLIF($24, '') END,
                 ($25::double precision)::numeric(18,6),
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
                 $36, $37, $38, now(), now())
         ON CONFLICT (batch_id) DO UPDATE SET
            session_id = excluded.session_id,
            apparatus = excluded.apparatus,
            canonical_apparatus_id = excluded.canonical_apparatus_id,
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
            canonical_current_apparatus_id = excluded.canonical_current_apparatus_id,
            current_location = excluded.current_location,
            next_apparatus = excluded.next_apparatus,
            canonical_next_apparatus_id = excluded.canonical_next_apparatus_id,
            parent_batch_id = excluded.parent_batch_id,
            used_by_session_id = excluded.used_by_session_id,
            used_by_apparatus = excluded.used_by_apparatus,
            canonical_used_by_apparatus_id = excluded.canonical_used_by_apparatus_id,
            processed_by_session_id = excluded.processed_by_session_id,
            processed_by_apparatus = excluded.processed_by_apparatus,
            canonical_processed_by_apparatus_id = excluded.canonical_processed_by_apparatus_id,
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
            updated_at = now()
         WHERE mini_progress_batches.revision = excluded.revision",
    )
    .bind(batch.batch_id.trim())
    .bind(batch.session_id.trim())
    .bind(apparatus_id.as_str())
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
    .bind(i64::try_from(batch.revision).map_err(|_| ProductionMapError::StoreFailed)?)
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
    if result.rows_affected() != 1 {
        return Err(ProductionMapError::ProgressBatchCorrectionConflict);
    }
    Ok(())
}

fn is_warehouse_processing_marker(value: &str) -> bool {
    value.trim().to_ascii_lowercase().starts_with("warehouse:")
}

fn validate_rezka_merge_payload(session: &OrderRunSession) -> Result<(), ProductionMapError> {
    let input_links = order_run_input_links_from_payload(&session.payload_json)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let active_rolls = rezka_active_partial_rolls_from_payload(&session.payload_json)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    if !session.status.is_open() && !active_rolls.is_empty() {
        return Err(ProductionMapError::StoreFailed);
    }
    if !rezka_merge_state_is_consistent(&input_links, &active_rolls) {
        return Err(ProductionMapError::StoreFailed);
    }
    Ok(())
}
