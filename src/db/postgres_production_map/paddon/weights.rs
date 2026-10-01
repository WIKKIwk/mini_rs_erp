use super::progress_helpers::{ProgressBatchRow, progress_batch_from_row};
use crate::core::production_map::paddon_weights::{audited_kg_changed, kg_from_values, set_totals};
use crate::core::production_map::{OrderProgressBatch, PaddonReceipt, ProductionMapError};
use sqlx::{Executor, Postgres};
use std::collections::BTreeMap;

pub(super) async fn load_batches<'e, E: Executor<'e, Database = Postgres>>(
    db: E,
    ids: &[String],
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
         WHERE batch.batch_id = ANY($1)"
    ).bind(ids).fetch_all(db).await.map_err(|_| ProductionMapError::StoreFailed)?;
    rows.into_iter().map(progress_batch_from_row).collect()
}

pub(super) async fn correction_gross<'e, E: Executor<'e, Database = Postgres>>(
    db: E,
    items: &[OrderProgressBatch],
) -> Result<BTreeMap<String, Option<f64>>, ProductionMapError> {
    let revisions: BTreeMap<_, _> = items
        .iter()
        .map(|b| (b.batch_id.as_str(), b.revision))
        .collect();
    let ids: Vec<_> = revisions.keys().copied().collect();
    let rows = sqlx::query_as::<_, (String, i64, serde_json::Value, serde_json::Value)>(
        "SELECT batch_id, new_revision, old_values, new_values FROM mini_progress_batch_corrections
         WHERE batch_id = ANY($1) ORDER BY new_revision ASC, id ASC",
    )
    .bind(ids)
    .fetch_all(db)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut result = BTreeMap::new();
    for (id, revision, old, new) in rows {
        if revision > 0
            && revisions
                .get(id.as_str())
                .is_some_and(|bound| revision as u64 <= *bound)
            && audited_kg_changed(&old, &new)
        {
            result.insert(id, kg_from_values(&new));
        }
    }
    Ok(result)
}

pub(super) async fn enrich_receipt<'e, E: Executor<'e, Database = Postgres>>(
    db: E,
    value: serde_json::Value,
) -> Result<PaddonReceipt, ProductionMapError> {
    let legacy = value
        .get("paddon")
        .is_some_and(|p| p.get("total_gross_kg").is_none() || p.get("total_net_kg").is_none());
    let mut receipt: PaddonReceipt =
        serde_json::from_value(value).map_err(|_| ProductionMapError::StoreFailed)?;
    if legacy {
        let corrections = correction_gross(db, &receipt.items).await?;
        set_totals(
            &mut receipt.paddon,
            receipt
                .items
                .iter()
                .map(|b| (b, corrections.get(&b.batch_id).copied())),
        );
    }
    Ok(receipt)
}
