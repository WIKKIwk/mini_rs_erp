pub(super) async fn remove_paddon_items(
    pool: &PgPool,
    code: &str,
    progress_batch_ids: &[String],
    actor: &QueueActionActor,
) -> Result<PaddonSnapshot, ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let allow_printed = super::paddon_management::lock_enabled(&mut tx).await?;
    let paddon = lock_paddon(&mut tx, code, false).await?;
    let mut item_ids = Vec::with_capacity(progress_batch_ids.len());
    let mut batch_ids = progress_batch_ids.to_vec();
    batch_ids.sort();
    for progress_batch_id in &batch_ids {
        if allow_printed {
            super::paddon_movement::lock_movable_roll(&mut tx, progress_batch_id, true).await?;
        }
        let item_id = sqlx::query_scalar::<_, String>(
            "SELECT id
             FROM mini_paddon_items
             WHERE paddon_id = $1 AND progress_batch_id = $2 AND removed_at IS NULL
             FOR UPDATE",
        )
        .bind(&paddon.id)
        .bind(progress_batch_id.trim())
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .ok_or(ProductionMapError::PaddonItemNotAssigned)?;
        item_ids.push(item_id);
    }
    for item_id in item_ids {
        sqlx::query(
            "UPDATE mini_paddon_items
             SET removed_at = now(), removed_by_ref = $2, removed_by_display_name = $3
             WHERE id = $1",
        )
        .bind(item_id)
        .bind(actor.ref_.trim())
        .bind(actor.display_name.trim())
        .execute(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    sqlx::query("UPDATE mini_paddons SET updated_at = now() WHERE id = $1")
        .bind(&paddon.id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    load_snapshot_by_code(pool, code, true, None)
        .await?
        .ok_or(ProductionMapError::PaddonNotFound)
}
