use sqlx::PgPool;

use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{OrderProgressBatch, ProductionMapError, WipProgressBatchQuery};

use super::progress_helpers::{ProgressBatchRow, progress_batch_from_row};

pub(super) async fn load_wip_progress_batches(
    pool: &PgPool,
    query: WipProgressBatchQuery,
) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {

    let WipProgressBatchQuery {
        producer_apparatus,
        worker_ref,
        before_batch_id,
        production_day,
        apparatus,
        next_apparatus,
        current_location,
        status,
        include_processed,
        order_id,
        limit,
    } = query;
    if limit == 0 {
        return Ok(Vec::new());
    }
    let producer_apparatus = producer_apparatus.trim();
    if !producer_apparatus.is_empty() {
        ApparatusId::new(producer_apparatus.to_string())
            .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    let apparatus_id = if apparatus.trim().is_empty() {
        None
    } else {
        Some(
            ApparatusId::new(apparatus.trim().to_string())
                .map_err(|_| ProductionMapError::StoreFailed)?,
        )
    };
    let apparatus = apparatus_id.as_ref().map(ApparatusId::as_str).unwrap_or("");
    let apparatus_key = apparatus.to_string();
    let query_apparatus_key = if include_processed && !next_apparatus.trim().is_empty() {
        ""
    } else {
        apparatus_key.as_str()
    };
    let next_apparatus_key = if next_apparatus.trim().is_empty() {
        String::new()
    } else {
        ApparatusId::new(next_apparatus.trim().to_string())
            .map_err(|_| ProductionMapError::StoreFailed)?
            .to_string()
    };
    let current_location = current_location.trim();
    let status = status.map(|value| value.as_str()).unwrap_or_default();
    let limit = i64::try_from(limit.min(500)).unwrap_or(500);
    let sql =
        "SELECT batch.batch_id, batch.revision, batch.session_id,
                COALESCE(EXTRACT(EPOCH FROM batch.production_started_at)::bigint, 0) AS started_at_unix,
                EXTRACT(EPOCH FROM batch.production_completed_at)::bigint AS completed_at_unix,
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
         WHERE ($1 = '' OR COALESCE(batch.canonical_current_apparatus_id, '') = $1)
           AND ($2 = '' OR order_id = $2)
           AND ($7 OR (($3 = '' AND wip_status IN ('waiting', 'in_use')) OR ($3 <> '' AND wip_status = $3)))
           AND ($4 = '' OR current_location = $4)
           AND (
             $5 = ''
             OR COALESCE(batch.canonical_next_apparatus_id, '') = $5
           )
           AND ($8 = '' OR batch.canonical_apparatus_id = $8)
           AND ($9 = '' OR worker_ref = $9)
           AND ($10 = '' OR batch.batch_id < $10)
           AND ($11::text[] IS NULL OR (
                batch.canonical_apparatus_id = ANY($11)
                AND batch.wip_status <> 'peremotka'
                AND COALESCE(batch.payload_json->>'origin', '') <> 'layer_remainder'
                AND LEAST(batch.production_started_at, batch.production_completed_at) < to_timestamp($13)
                AND GREATEST(batch.production_started_at, batch.production_completed_at) >= to_timestamp($12)
           ))
         ORDER BY CASE WHEN $8 <> '' OR $11 IS NOT NULL THEN batch.batch_id END DESC,
                  updated_at DESC, created_at DESC, batch.batch_id DESC
         LIMIT $6";
    let rows = sqlx::query_as::<_, ProgressBatchRow>(sql)
        .bind(query_apparatus_key)
        .bind(order_id.trim())
        .bind(status)
        .bind(current_location)
        .bind(&next_apparatus_key)
        .bind(limit)
        .bind(include_processed)
        .bind(producer_apparatus)
        .bind(worker_ref.trim())
        .bind(before_batch_id.trim())
        .bind(production_day.as_ref().map(|day| day.producer_apparatus_ids.as_slice()))
        .bind(production_day.as_ref().map(|day| day.from_unix as f64))
        .bind(production_day.as_ref().map(|day| day.until_unix as f64))
        .fetch_all(pool)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let batches = rows
        .into_iter()
        .map(progress_batch_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(batches
        .into_iter()
        .filter(|batch| {
            (apparatus.is_empty()
                || batch.current_apparatus == apparatus
                || batch.apparatus == apparatus)
                && (next_apparatus_key.is_empty() || batch.next_apparatus == next_apparatus_key)
        })
        .collect())
}
