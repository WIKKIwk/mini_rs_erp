async fn store_finished_goods_batch_tx(
    tx: &mut Transaction<'_, Postgres>,
    batch: &OrderProgressBatch,
    stock: &FinishedGoodsStockEntry,
) -> Result<(), ProductionMapError> {
    // Individual-roll receiving must not overwrite a concurrent pallet receipt.
    // Acquire the same advisory locks before row locks as all progress writes.
    let apparatuses: Vec<_> = [&batch.apparatus, &batch.current_apparatus, &batch.next_apparatus,
        &batch.used_by_apparatus, &batch.processed_by_apparatus].into_iter()
        .map(String::as_str).filter(|a| !a.trim().is_empty() && !is_warehouse_processing_marker(a)).collect();
    lock_order_and_apparatuses_tx(tx, &batch.order_id, &apparatuses).await?;
    let current_status = sqlx::query_scalar::<_, String>(
        "SELECT wip_status FROM mini_progress_batches WHERE batch_id = $1 FOR UPDATE",
    ).bind(&batch.batch_id).fetch_optional(&mut **tx).await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or(ProductionMapError::ProgressBatchNotFound)?;
    if current_status != "waiting" {
        return Err(ProductionMapError::ProgressBatchNotAccepted);
    }
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

fn require_live_apparatus_id(value: &str) -> Result<(), ProductionMapError> {
    if crate::core::production_map::canonical_apparatus_id(value).is_some() {
        Ok(())
    } else {
        Err(ProductionMapError::StoreFailed)
    }
}

include!("../progress_rows.rs");
