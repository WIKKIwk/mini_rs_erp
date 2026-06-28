use sqlx::PgPool;

use crate::core::production_map::{
    OrderProgressBatch, ProductionMapError, WipProgressBatchQuery, queue_state,
};

use super::progress_helpers::{ProgressBatchRow, progress_batch_from_row};

pub(super) async fn load_wip_progress_batches(
    pool: &PgPool,
    query: WipProgressBatchQuery,
) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
    let WipProgressBatchQuery {
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
    let apparatus = apparatus.trim();
    let apparatus_key = queue_state::apparatus_search_key(apparatus);
    let query_apparatus_key = if include_processed && !next_apparatus.trim().is_empty() {
        ""
    } else {
        apparatus_key.as_str()
    };
    let next_apparatus_key = queue_state::apparatus_search_key(&next_apparatus);
    let current_location = current_location.trim();
    let status = status.map(|value| value.as_str()).unwrap_or_default();
    let limit = i64::try_from(limit.min(500)).unwrap_or(500);
    let rows = sqlx::query_as::<_, ProgressBatchRow>(
        "SELECT batch_id, session_id, apparatus, order_id, action, status,
                produced_qty::float8 AS produced_qty, uom, qr_payload,
                label_item_code, label_item_name, executor_name,
                worker_role, worker_ref, worker_display_name,
                wip_status, current_apparatus, current_apparatus_key, current_location,
                next_apparatus, parent_batch_id, used_by_session_id,
                used_by_apparatus, processed_by_session_id,
                processed_by_apparatus,
                return_ink_kg::float8 AS return_ink_kg,
                lamination_print_leftover_rolls::float8 AS lamination_print_leftover_rolls,
                lamination_film_leftover_rolls::float8 AS lamination_film_leftover_rolls,
                rezka_bosma_waste::float8 AS rezka_bosma_waste,
                rezka_lamination_waste::float8 AS rezka_lamination_waste,
                rezka_edge_waste::float8 AS rezka_edge_waste,
                total_waste::float8 AS total_waste,
                finished_goods_kg::float8 AS finished_goods_kg,
                finished_goods_meter::float8 AS finished_goods_meter,
                description,
                payload_json
         FROM mini_progress_batches
         WHERE ($1 = '' OR current_apparatus_key = $1)
           AND ($2 = '' OR order_id = $2)
           AND ($7 OR (($3 = '' AND wip_status <> 'processed') OR ($3 <> '' AND wip_status = $3)))
           AND ($4 = '' OR current_location = $4)
           AND ($5 = '' OR lower(regexp_replace(trim(next_apparatus), '\\s+-\\s+[A-Za-z0-9_-]{1,16}$', '')) = $5)
         ORDER BY updated_at DESC, created_at DESC, batch_id DESC
         LIMIT $6",
    )
    .bind(query_apparatus_key)
    .bind(order_id.trim())
    .bind(status)
    .bind(current_location)
    .bind(next_apparatus_key)
    .bind(limit)
    .bind(include_processed)
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
            apparatus.is_empty()
                || queue_state::apparatus_titles_match(&batch.current_apparatus, apparatus)
                || queue_state::apparatus_titles_match(&batch.apparatus, apparatus)
        })
        .collect())
}
