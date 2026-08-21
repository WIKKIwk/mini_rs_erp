
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
        crate::core::production_map::canonical_apparatus_key(&batch.current_apparatus)
    } else {
        crate::core::production_map::canonical_apparatus_key(key)
    }
}

fn require_live_apparatus_id(value: &str) -> Result<(), ProductionMapError> {
    if crate::core::production_map::canonical_apparatus_id(value).is_some() {
        Ok(())
    } else {
        Err(ProductionMapError::StoreFailed)
    }
}

include!("../progress_rows.rs");
