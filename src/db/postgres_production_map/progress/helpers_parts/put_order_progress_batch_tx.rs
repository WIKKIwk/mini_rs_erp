pub(super) async fn put_order_progress_batch_tx(
    tx: &mut Transaction<'_, Postgres>,
    batch: &OrderProgressBatch,
) -> Result<(), ProductionMapError> {
    progress_batch_input_links_from_payload(&batch.payload_json)
        .map_err(|_| ProductionMapError::StoreFailed)?;
    // This write changes a roll, not its producer/next machine's queue.
    // The order lock and revision CAS protect standalone writes too.
    lock_order_and_apparatuses_tx(tx, &batch.order_id, &[]).await?;
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
    let result = sqlx::query_scalar::<_, bool>(
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
            payload_json, revision, production_started_at, production_completed_at,
            created_at, updated_at
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
                 $36, $37, $38,
                 to_timestamp(NULLIF($39::double precision, 0)),
                 to_timestamp($40::double precision), now(), now())
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
         WHERE mini_progress_batches.revision = excluded.revision
         RETURNING (xmax = 0)",
    )
    .bind(batch.batch_id.trim())
    .bind(batch.session_id.trim())
    .bind(apparatus_id.as_str())
    .bind(batch.order_id.trim())
    .bind(batch.action.as_str())
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
    // Deliberately absent from ON CONFLICT: corrections, intake and subsequent
    // work must never change the production interval of an existing QR.
    .bind(batch.started_at_unix as f64)
    .bind(batch.completed_at_unix as f64)
    .fetch_optional(&mut **tx)
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
    let inserted = result.ok_or(ProductionMapError::ProgressBatchCorrectionConflict)?;
    if inserted {
        crate::db::postgres_qolip::mileage::record_new_output_tx(tx, batch).await?;
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

pub(super) async fn correct_progress_batch(
    pool: &PgPool,
    current: &OrderProgressBatch,
    input: &ProgressBatchCorrectionInput,
    actor: &QueueActionActor,
) -> Result<OrderProgressBatch, ProductionMapError> {
    if current.is_layer_remainder() {
        return Err(ProductionMapError::ProgressBatchCorrectionLocked);
    }
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
    // Queue writes take these advisory locks before molds, sessions and WIP.
    // Serialize card corrections with a concurrent record/close before taking
    // any row lock, and invalidate prepared writes using the output revision.
    lock_order_and_apparatuses_tx(&mut tx, &current.order_id, &[&current.apparatus]).await?;
    crate::db::postgres_qolip::mileage::lock_output_sources_tx(&mut tx, &current.batch_id).await?;
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
           AND COALESCE(payload_json->>'origin', '') <> 'layer_remainder'
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
    if let Some(mut payload) = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload_json FROM mini_order_run_sessions
         WHERE session_id = $1 AND order_id = $2 AND canonical_apparatus_id = $3
           AND status IN ('active', 'paused', 'frozen', 'roll_detached') FOR UPDATE",
    )
    .bind(&corrected.session_id)
    .bind(&corrected.order_id)
    .bind(&corrected.apparatus)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
        && crate::core::production_map::correct_recorded_rezka_frame(&mut payload, &corrected)?
    {
        sqlx::query("UPDATE mini_order_run_sessions SET payload_json = $1, updated_at = now() WHERE session_id = $2")
            .bind(payload).bind(&corrected.session_id).execute(&mut *tx).await
            .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    crate::db::postgres_qolip::mileage::correct_output_tx(&mut tx, &corrected).await?;
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
    receive_finished_goods_batches_tx(tx, std::iter::once((batch, stock))).await
}

/// Persist every roll with the existing checks, then refresh each affected
/// order once in the same transaction. A failed refresh rolls back the receipt.
pub(super) async fn receive_finished_goods_batches_tx<'a>(
    tx: &mut Transaction<'_, Postgres>,
    receipts: impl IntoIterator<Item = (&'a OrderProgressBatch, &'a FinishedGoodsStockEntry)>,
) -> Result<(), ProductionMapError> {
    let mut received_orders = BTreeMap::new();
    for (batch, stock) in receipts {
        store_finished_goods_batch_tx(tx, batch, stock).await?;
        let order_id = stock.order_id.trim();
        if !order_id.is_empty() {
            // Keep the first receipt's stock as the lifecycle audit source.
            received_orders.entry(order_id).or_insert(stock);
        }
    }
    for (order_id, stock) in received_orders {
        let actor = QueueActionActor {
            role: "werka".to_string(),
            ref_: "warehouse-worker".to_string(),
            display_name: "Warehouse Worker".to_string(),
        };
        crate::db::postgres_production_map::lifecycle::refresh_production_order_lifecycle_tx(
            tx,
            order_id,
            &actor,
            stock.id.trim(),
            "finished_goods_receipt",
        )
        .await?;
    }
    Ok(())
}
