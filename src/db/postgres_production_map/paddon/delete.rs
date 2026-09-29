use sqlx::PgPool;

use crate::core::production_map::ProductionMapError;

pub(super) async fn delete_paddon(pool: &PgPool, code: &str) -> Result<(), ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    // Item assignment/removal, output recording and receiving take this same
    // lock. Check history after acquiring it so a concurrent write cannot be lost.
    let (id, used) = sqlx::query_as::<_, (String, bool)>(
        "SELECT id, receipt_json IS NOT NULL OR updated_at <> created_at
         FROM mini_paddons WHERE code = $1 FOR UPDATE",
    )
    .bind(code)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::PaddonNotFound)?;
    // Membership/receipt writes update this timestamp, including history that
    // may later be removed by an order reset.
    if used {
        return Err(ProductionMapError::PaddonDeleteLocked);
    }
    let has_history = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM mini_paddon_items WHERE paddon_id = $1)
             OR EXISTS (SELECT 1 FROM mini_paddon_receipt_lines WHERE paddon_id = $1)
             OR EXISTS (
                 SELECT 1 FROM mini_inventory_movement_events
                 WHERE (source_document_type = 'paddon_receipt' AND source_document_id = $1)
                    OR payload_json->>'paddon_id' = $1
                    OR payload_json->>'paddon_code' = $2
             )",
    )
    .bind(&id)
    .bind(code)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    // Include removed memberships: a previously used paddon is not an unused one.
    if has_history {
        return Err(ProductionMapError::PaddonDeleteLocked);
    }
    sqlx::query("DELETE FROM mini_paddons WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    // The active-selection FK clears any selection of this empty paddon.
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)
}
