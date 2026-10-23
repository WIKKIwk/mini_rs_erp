use std::collections::BTreeSet;

use crate::core::production_map::{OrderProgressBatch, ProductionMapError, QueueActionActor};
use sqlx::{PgPool, Postgres, Transaction};

pub(super) async fn load_available(
    pool: &PgPool,
    target_id: &str,
    target_apparatus: &str,
) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
    let ids = sqlx::query_scalar::<_, String>(
        "SELECT b.batch_id FROM mini_progress_batches b
         WHERE b.wip_status='waiting'
           AND ($2='' OR b.canonical_apparatus_id=$2)
           AND COALESCE(b.payload_json->>'finished_goods_stock_id', '')=''
           AND COALESCE(b.payload_json->>'received_warehouse', '')=''
           AND NOT EXISTS (SELECT 1 FROM mini_paddon_items i JOIN mini_paddons p ON p.id=i.paddon_id
               WHERE i.progress_batch_id=b.batch_id AND i.removed_at IS NULL
                 AND (p.id=$1 OR p.receipt_json IS NOT NULL OR p.locked_at IS NOT NULL))
         ORDER BY b.updated_at DESC, b.batch_id LIMIT 500",
    )
    .bind(target_id)
    .bind(target_apparatus)
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    super::paddon_weights::load_batches(pool, &ids).await
}

pub(super) async fn move_items(
    tx: &mut Transaction<'_, Postgres>,
    code: &str,
    batch_ids: &[String],
    actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    let target = sqlx::query_scalar::<_, String>("SELECT id FROM mini_paddons WHERE code=$1")
        .bind(code)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or(ProductionMapError::PaddonNotFound)?;
    let sources = sqlx::query_scalar::<_, String>("SELECT DISTINCT paddon_id FROM mini_paddon_items WHERE progress_batch_id=ANY($1) AND removed_at IS NULL")
        .bind(batch_ids).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    let mut lock_ids: BTreeSet<_> = sources.into_iter().collect();
    lock_ids.insert(target.clone());
    // Lock source and destination in one stable order, then the rolls in order.
    // Receipt takes the same pallet and roll locks and remains authoritative.
    let locked = sqlx::query_as::<_, (String, Option<serde_json::Value>, bool, String)>(
        "SELECT id, receipt_json, locked_at IS NOT NULL, apparatus_id FROM mini_paddons WHERE id=ANY($1) ORDER BY id FOR UPDATE",
    )
    .bind(lock_ids.iter().cloned().collect::<Vec<_>>())
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if !locked.iter().any(|(id, _, _, _)| id == &target) {
        return Err(ProductionMapError::PaddonNotFound);
    }
    if locked.iter().any(|(_, receipt, _, _)| receipt.is_some()) {
        return Err(ProductionMapError::PaddonAlreadyReceived);
    }
    if locked.iter().any(|(_, _, printed, _)| *printed) {
        return Err(ProductionMapError::PaddonLocked);
    }
    let target_apparatus = &locked.iter().find(|(id, _, _, _)| id == &target)
        .ok_or(ProductionMapError::PaddonNotFound)?.3;
    if actor.role == "aparatchi" && locked.iter().any(|(_, _, _, id)| id != target_apparatus) {
        return Err(ProductionMapError::PaddonTransferForbidden);
    }
    let mut changed = BTreeSet::new();
    let mut batch_ids = batch_ids.to_vec();
    batch_ids.sort();
    batch_ids.dedup();
    for batch in batch_ids {
        lock_movable_roll(tx, &batch).await?;
        if actor.role == "aparatchi" && !target_apparatus.is_empty() {
            let origin = sqlx::query_scalar::<_, Option<String>>("SELECT canonical_apparatus_id FROM mini_progress_batches WHERE batch_id=$1")
                .bind(&batch).fetch_one(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
            if origin.as_deref() != Some(target_apparatus.as_str()) { return Err(ProductionMapError::PaddonInvalidInput); }
        }
        let source = sqlx::query_as::<_, (String, String)>("SELECT id, paddon_id FROM mini_paddon_items WHERE progress_batch_id=$1 AND removed_at IS NULL FOR UPDATE")
            .bind(&batch).fetch_optional(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        if let Some((item_id, source)) = source {
            if source == target {
                continue;
            }
            // Another output transaction may have attached an initially free roll
            // after our source lookup. Roll back rather than touch an unlocked pallet.
            if !lock_ids.contains(&source) {
                return Err(ProductionMapError::PaddonItemAlreadyAssigned);
            }
            sqlx::query("UPDATE mini_paddon_items SET removed_at=now(), removed_by_ref=$2, removed_by_display_name=$3 WHERE id=$1")
                .bind(item_id).bind(actor.ref_.trim()).bind(actor.display_name.trim())
                .execute(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
            changed.insert(source);
        }
        sqlx::query("INSERT INTO mini_paddon_items(id, paddon_id, progress_batch_id, added_by_ref, added_by_display_name) VALUES($1,$2,$3,$4,$5)")
            .bind(super::paddon_helpers::new_item_id()).bind(&target).bind(batch)
            .bind(actor.ref_.trim()).bind(actor.display_name.trim())
            .execute(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        changed.insert(target.clone());
    }
    sqlx::query("UPDATE mini_paddons SET updated_at=now() WHERE id=ANY($1)")
        .bind(changed.into_iter().collect::<Vec<_>>())
        .execute(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn lock_movable_roll(
    tx: &mut Transaction<'_, Postgres>,
    batch: &str,
) -> Result<(), ProductionMapError> {
    let movable = sqlx::query_scalar::<_, bool>(
        "SELECT wip_status='waiting'
            AND COALESCE(payload_json->>'finished_goods_stock_id', '')=''
            AND COALESCE(payload_json->>'received_warehouse', '')=''
         FROM mini_progress_batches WHERE batch_id=$1 FOR UPDATE",
    )
    .bind(batch)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::ProgressBatchNotFound)?;
    if !movable {
        return Err(ProductionMapError::ProgressBatchNotAccepted);
    }
    Ok(())
}
