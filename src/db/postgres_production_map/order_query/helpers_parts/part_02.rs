
pub(super) async fn load_progress_batches_for_order(
    pool: &PgPool,
    order_id: &str,
) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Ok(Vec::new());
    }
    let order_ids = vec![order_id.to_string()];
    let mut batches = load_progress_batches_for_orders(pool, &order_ids).await?;
    Ok(batches.remove(order_id).unwrap_or_default())
}

pub(super) async fn load_progress_batches_for_orders(
    pool: &PgPool,
    order_ids: &[String],
) -> Result<BTreeMap<String, Vec<OrderProgressBatch>>, ProductionMapError> {
    let order_ids = order_ids
        .iter()
        .map(|order_id| order_id.trim().to_string())
        .filter(|order_id| !order_id.is_empty())
        .collect::<Vec<_>>();
    if order_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let rows = sqlx::query_as::<_, ProgressBatchRow>(
        "SELECT batch.batch_id, batch.revision, batch.session_id,
                COALESCE(EXTRACT(EPOCH FROM session.started_at)::bigint,
                         EXTRACT(EPOCH FROM batch.created_at)::bigint) AS started_at_unix,
                COALESCE(EXTRACT(EPOCH FROM session.session_updated_at)::bigint,
                         EXTRACT(EPOCH FROM batch.updated_at)::bigint) AS completed_at_unix,
                batch.canonical_apparatus_id AS apparatus, batch.order_id, batch.action, batch.status,
                produced_qty::float8 AS produced_qty, uom, qr_payload,
                label_item_code, label_item_name, executor_name,
                worker_role, worker_ref, worker_display_name,
                wip_status, COALESCE(batch.canonical_current_apparatus_id, '') AS current_apparatus,
                current_location,
                COALESCE(batch.canonical_next_apparatus_id, '') AS next_apparatus,
                parent_batch_id, used_by_session_id,
                COALESCE(batch.canonical_used_by_apparatus_id, '') AS used_by_apparatus,
                processed_by_session_id,
                COALESCE(batch.canonical_processed_by_apparatus_id, '') AS processed_by_apparatus,
                return_ink_kg::float8 AS return_ink_kg,
                lamination_print_leftover_rolls::float8 AS lamination_print_leftover_rolls,
                lamination_film_leftover_rolls::float8 AS lamination_film_leftover_rolls,
                rezka_bosma_waste::float8 AS rezka_bosma_waste,
                rezka_lamination_waste::float8 AS rezka_lamination_waste,
                rezka_edge_waste::float8 AS rezka_edge_waste,
                total_waste::float8 AS total_waste,
                finished_goods_kg::float8 AS finished_goods_kg,
                bobina_kg::float8 AS bobina_kg,
                finished_goods_meter::float8 AS finished_goods_meter,
                diameter::float8 AS diameter,
                description,
                payload_json
         FROM mini_progress_batches AS batch
         LEFT JOIN (
             SELECT session_id, started_at, updated_at AS session_updated_at
             FROM mini_order_run_sessions
         ) AS session ON session.session_id = batch.session_id
         WHERE batch.order_id = ANY($1)
         ORDER BY batch.order_id ASC, updated_at DESC, created_at DESC, batch_id DESC",
    )
    .bind(&order_ids)
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut batches = BTreeMap::new();
    for row in rows {
        let batch = progress_batch_from_row(row)?;
        batches
            .entry(batch.order_id.clone())
            .or_insert_with(Vec::new)
            .push(batch);
    }
    Ok(batches)
}

pub(super) async fn load_progress_batches_for_audit(
    pool: &PgPool,
) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
    let rows = sqlx::query_as::<_, ProgressBatchRow>(
        "SELECT batch.batch_id, batch.revision, batch.session_id,
                COALESCE(EXTRACT(EPOCH FROM session.started_at)::bigint,
                         EXTRACT(EPOCH FROM batch.created_at)::bigint) AS started_at_unix,
                COALESCE(EXTRACT(EPOCH FROM session.session_updated_at)::bigint,
                         EXTRACT(EPOCH FROM batch.updated_at)::bigint) AS completed_at_unix,
                batch.canonical_apparatus_id AS apparatus, batch.order_id, batch.action, batch.status,
                produced_qty::float8 AS produced_qty, uom, qr_payload,
                label_item_code, label_item_name, executor_name,
                worker_role, worker_ref, worker_display_name,
                wip_status, COALESCE(batch.canonical_current_apparatus_id, '') AS current_apparatus,
                current_location,
                COALESCE(batch.canonical_next_apparatus_id, '') AS next_apparatus,
                parent_batch_id, used_by_session_id,
                COALESCE(batch.canonical_used_by_apparatus_id, '') AS used_by_apparatus,
                processed_by_session_id,
                COALESCE(batch.canonical_processed_by_apparatus_id, '') AS processed_by_apparatus,
                return_ink_kg::float8 AS return_ink_kg,
                lamination_print_leftover_rolls::float8 AS lamination_print_leftover_rolls,
                lamination_film_leftover_rolls::float8 AS lamination_film_leftover_rolls,
                rezka_bosma_waste::float8 AS rezka_bosma_waste,
                rezka_lamination_waste::float8 AS rezka_lamination_waste,
                rezka_edge_waste::float8 AS rezka_edge_waste,
                total_waste::float8 AS total_waste,
                finished_goods_kg::float8 AS finished_goods_kg,
                bobina_kg::float8 AS bobina_kg,
                finished_goods_meter::float8 AS finished_goods_meter,
                diameter::float8 AS diameter,
                description,
                payload_json
         FROM mini_progress_batches AS batch
         LEFT JOIN (
             SELECT session_id, started_at, updated_at AS session_updated_at
             FROM mini_order_run_sessions
         ) AS session ON session.session_id = batch.session_id
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
        "SELECT batch.batch_id, batch.revision, batch.session_id,
                COALESCE(EXTRACT(EPOCH FROM session.started_at)::bigint,
                         EXTRACT(EPOCH FROM batch.created_at)::bigint) AS started_at_unix,
                COALESCE(EXTRACT(EPOCH FROM session.session_updated_at)::bigint,
                         EXTRACT(EPOCH FROM batch.updated_at)::bigint) AS completed_at_unix,
                batch.canonical_apparatus_id AS apparatus, batch.order_id, batch.action, batch.status,
                produced_qty::float8 AS produced_qty, uom, qr_payload,
                label_item_code, label_item_name, executor_name,
                worker_role, worker_ref, worker_display_name,
                wip_status, COALESCE(batch.canonical_current_apparatus_id, '') AS current_apparatus,
                current_location,
                COALESCE(batch.canonical_next_apparatus_id, '') AS next_apparatus,
                parent_batch_id, used_by_session_id,
                COALESCE(batch.canonical_used_by_apparatus_id, '') AS used_by_apparatus,
                processed_by_session_id,
                COALESCE(batch.canonical_processed_by_apparatus_id, '') AS processed_by_apparatus,
                return_ink_kg::float8 AS return_ink_kg,
                lamination_print_leftover_rolls::float8 AS lamination_print_leftover_rolls,
                lamination_film_leftover_rolls::float8 AS lamination_film_leftover_rolls,
                rezka_bosma_waste::float8 AS rezka_bosma_waste,
                rezka_lamination_waste::float8 AS rezka_lamination_waste,
                rezka_edge_waste::float8 AS rezka_edge_waste,
                total_waste::float8 AS total_waste,
                finished_goods_kg::float8 AS finished_goods_kg,
                bobina_kg::float8 AS bobina_kg,
                finished_goods_meter::float8 AS finished_goods_meter,
                diameter::float8 AS diameter,
                description,
                payload_json
         FROM mini_progress_batches AS batch
         LEFT JOIN (
             SELECT session_id, started_at, updated_at AS session_updated_at
             FROM mini_order_run_sessions
         ) AS session ON session.session_id = batch.session_id
         WHERE lower(batch.qr_payload) = lower($1)",
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
