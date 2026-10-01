use sqlx::{PgConnection, PgPool};

use super::{
    order_query_helpers::load_progress_batch, progress_helpers::receive_finished_goods_batch_tx,
    transaction_locks::lock_order_and_apparatuses_tx,
};
use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapError, WarehouseWipReceiveWrite, WarehouseWipSnapshot,
};

pub(super) async fn load(
    pool: &PgPool,
    qr: &str,
) -> Result<Option<WarehouseWipSnapshot>, ProductionMapError> {
    let mut connection = pool
        .acquire()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    load_tx(&mut connection, qr).await
}

async fn load_tx(
    connection: &mut PgConnection,
    qr: &str,
) -> Result<Option<WarehouseWipSnapshot>, ProductionMapError> {
    // Never select an arbitrary first row: legacy QR uniqueness is case-sensitive
    // while scanners and the existing resolver compare case-insensitively.
    let ids = sqlx::query_scalar::<_, String>(
        "SELECT batch_id FROM mini_progress_batches WHERE lower(btrim(qr_payload)) = lower($1) LIMIT 2",
    ).bind(qr.trim()).fetch_all(&mut *connection).await.map_err(|_| ProductionMapError::StoreFailed)?;
    if ids.len() > 1 {
        return Err(ProductionMapError::WarehouseQrAmbiguous);
    }
    let Some(id) = ids.first() else {
        return Ok(None);
    };
    let batch = load_progress_batch(&mut *connection, id)
        .await?
        .ok_or(ProductionMapError::ProgressBatchNotFound)?;
    let map = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT map_json FROM mini_production_maps WHERE id = $1",
    )
    .bind(&batch.order_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let order = map
        .map(serde_json::from_value::<ProductionMapDefinition>)
        .transpose()
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let order_receiving_allowed = sqlx::query_scalar::<_, bool>(
        "SELECT state = 'active' AND early_close IS NULL FROM mini_order_control_states WHERE order_id = $1",
    ).bind(&batch.order_id).fetch_optional(&mut *connection).await
        .map_err(|_| ProductionMapError::StoreFailed)?.unwrap_or(true);
    let apparatus_active = sqlx::query_scalar::<_, bool>(
        "SELECT lifecycle_state = 'active' FROM mini_apparatus WHERE id = $1",
    )
    .bind(&batch.apparatus)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .unwrap_or(false);
    let paddon_code = sqlx::query_scalar::<_, String>(
        "SELECT p.code FROM mini_paddon_items i JOIN mini_paddons p ON p.id = i.paddon_id
         WHERE i.progress_batch_id = $1 AND i.removed_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let qr_is_paddon =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM mini_paddons WHERE code = $1)")
            .bind(qr.trim())
            .fetch_one(&mut *connection)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(Some(WarehouseWipSnapshot {
        batch,
        order,
        order_receiving_allowed,
        apparatus_active,
        paddon_code,
        qr_is_paddon,
    }))
}

pub(super) async fn commit(
    pool: &PgPool,
    write: WarehouseWipReceiveWrite,
) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let original = &write.expected.batch;
    let apparatuses: Vec<_> = [
        &original.apparatus,
        &original.current_apparatus,
        &original.next_apparatus,
        &original.used_by_apparatus,
        &original.processed_by_apparatus,
    ]
    .into_iter()
    .map(String::as_str)
    .filter(|id| !id.trim().is_empty() && !id.starts_with("warehouse:"))
    .collect();
    lock_order_and_apparatuses_tx(&mut tx, &original.order_id, &apparatuses).await?;
    // Pallet attachment takes this same row lock and rechecks waiting status.
    // The winning action commits first; the losing action must reload and reject.
    sqlx::query("SELECT batch_id FROM mini_progress_batches WHERE batch_id = $1 FOR UPDATE")
        .bind(&original.batch_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    // Keep the active apparatus state stable, including administrative retirement.
    sqlx::query("SELECT id FROM mini_apparatus WHERE id = $1 FOR SHARE")
        .bind(&original.apparatus)
        .execute(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let current = load_tx(&mut tx, &original.qr_payload)
        .await?
        .ok_or(ProductionMapError::ProgressBatchNotFound)?;
    if current != write.expected || current.receive_blocked_reason().is_some() {
        return Err(ProductionMapError::WarehouseWipConflict);
    }
    receive_finished_goods_batch_tx(&mut tx, &write.batch, &write.stock).await?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}
