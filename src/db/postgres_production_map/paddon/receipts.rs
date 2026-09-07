use super::{
    order_query_helpers::load_progress_batch, progress_helpers::receive_finished_goods_batch_tx,
    transaction_locks::lock_orders_and_apparatuses_tx,
};
use crate::core::production_map::{
    PaddonReceipt, PaddonReceiveWrite, ProductionMapError, validate_receipt_retry,
};
use sqlx::{Executor, PgPool, Postgres};

pub(super) async fn load<'e, E: Executor<'e, Database = Postgres>>(
    db: E,
    code: &str,
) -> Result<Option<PaddonReceipt>, ProductionMapError> {
    let value = sqlx::query_scalar::<_, Option<serde_json::Value>>(
        "SELECT receipt_json FROM mini_paddons WHERE code = $1",
    )
    .bind(code.trim())
    .fetch_optional(db)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .flatten();
    value
        .map(|v| serde_json::from_value(v).map_err(|_| ProductionMapError::StoreFailed))
        .transpose()
}

pub(super) async fn commit(
    pool: &PgPool,
    write: PaddonReceiveWrite,
) -> Result<PaddonReceipt, ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let orders: Vec<_> = write
        .originals
        .iter()
        .map(|b| b.order_id.as_str())
        .collect();
    let apparatuses: Vec<_> = write
        .originals
        .iter()
        .flat_map(|b| {
            [
                &b.apparatus,
                &b.current_apparatus,
                &b.next_apparatus,
                &b.used_by_apparatus,
                &b.processed_by_apparatus,
            ]
        })
        .map(String::as_str)
        .filter(|s| !s.is_empty() && !s.starts_with("warehouse:"))
        .collect();
    lock_orders_and_apparatuses_tx(&mut tx, &orders, &apparatuses).await?;
    let code = &write.receipt.paddon.code;
    let id =
        sqlx::query_scalar::<_, String>("SELECT id FROM mini_paddons WHERE code = $1 FOR UPDATE")
            .bind(code)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?
            .ok_or(ProductionMapError::PaddonNotFound)?;
    let mut expected: Vec<_> = write.originals.iter().map(|b| b.batch_id.clone()).collect();
    expected.sort();
    if let Some(receipt) = load(&mut *tx, code).await? {
        validate_receipt_retry(&receipt, &write.receipt.warehouse, &expected)?;
        return Ok(receipt);
    }
    let ids = sqlx::query_scalar::<_, String>("SELECT progress_batch_id FROM mini_paddon_items WHERE paddon_id = $1 AND removed_at IS NULL ORDER BY progress_batch_id")
        .bind(&id).fetch_all(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    if ids != expected || ids.is_empty() {
        return Err(ProductionMapError::PaddonReceiptConflict);
    }
    // Lock and re-read before writing: a stale scan must never overwrite a correction/receipt.
    for batch_id in &ids {
        sqlx::query("SELECT batch_id FROM mini_progress_batches WHERE batch_id = $1 FOR UPDATE")
            .bind(batch_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let current = load_progress_batch(&mut *tx, batch_id)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        let original = write
            .originals
            .iter()
            .find(|b| &b.batch_id == batch_id)
            .ok_or(ProductionMapError::PaddonReceiptConflict)?;
        if &current != original {
            return Err(ProductionMapError::PaddonReceiptConflict);
        }
    }
    for (batch, stock) in write.receipt.items.iter().zip(&write.receipt.stocks) {
        receive_finished_goods_batch_tx(&mut tx, batch, stock).await?;
    }
    sqlx::query("UPDATE mini_paddons SET location = $2, updated_at = now(), receipt_json = $3 WHERE id = $1")
        .bind(&id).bind(&write.receipt.warehouse).bind(serde_json::to_value(&write.receipt).map_err(|_| ProductionMapError::StoreFailed)?)
        .execute(&mut *tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(write.receipt)
}
