use sqlx::PgPool;

use crate::core::qolip::normalize::{location_identity_matches, normalize_move_target};
use crate::core::qolip::{QolipError, QolipLocation};

use super::rows::{QolipLocationRow, row_to_location};

pub(super) async fn load_locations(
    pool: &PgPool,
    block: &str,
) -> Result<Vec<QolipLocation>, QolipError> {
    let rows = sqlx::query_as::<_, QolipLocationRow>(
        "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                size, quantity, row_letter, column_number, location_label,
                created_by_role, created_by_ref, created_by_name
         FROM mini_qolip_locations
         WHERE lower(block) = lower($1)
         ORDER BY lower(row_letter), column_number NULLS LAST, lower(item_name), lower(qolip_code)",
    )
    .bind(block.trim())
    .fetch_all(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    Ok(rows.into_iter().map(row_to_location).collect())
}

pub(super) async fn save_location(
    pool: &PgPool,
    location: QolipLocation,
) -> Result<QolipLocation, QolipError> {
    let mut tx = pool.begin().await.map_err(|_| QolipError::StoreFailed)?;

    sqlx::query("SELECT pg_advisory_xact_lock(hashtext(lower($1))::bigint)")
        .bind(location.qolip_code.trim())
        .execute(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

    sqlx::query(
        "DELETE FROM mini_qolip_locations
         WHERE lower(qolip_code) = lower($1)
           AND id <> $2",
    )
    .bind(location.qolip_code.trim())
    .bind(location.id.trim())
    .execute(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    let row = sqlx::query_as::<_, QolipLocationRow>(
        "INSERT INTO mini_qolip_locations (
             id, block, warehouse, item_code, item_name, qolip_code,
             size, quantity, row_letter, column_number, location_label,
             created_by_role, created_by_ref, created_by_name, payload_json
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
         ON CONFLICT (id) DO UPDATE SET
             block = excluded.block,
             warehouse = excluded.warehouse,
             item_code = excluded.item_code,
             item_name = excluded.item_name,
             qolip_code = excluded.qolip_code,
             size = excluded.size,
             quantity = excluded.quantity,
             row_letter = excluded.row_letter,
             column_number = excluded.column_number,
             location_label = excluded.location_label,
             created_by_role = excluded.created_by_role,
             created_by_ref = excluded.created_by_ref,
             created_by_name = excluded.created_by_name,
             payload_json = excluded.payload_json,
             updated_at = now()
         RETURNING id, block, warehouse, item_code, item_name, qolip_code,
             size, quantity, row_letter, column_number, location_label,
             created_by_role, created_by_ref, created_by_name",
    )
    .bind(location.id.trim())
    .bind(location.block.trim())
    .bind(location.warehouse.trim())
    .bind(location.item_code.trim())
    .bind(location.item_name.trim())
    .bind(location.qolip_code.trim())
    .bind(location.size)
    .bind(location.quantity)
    .bind(location.row_letter.trim())
    .bind(location.column_number)
    .bind(location.location_label.trim())
    .bind(location.created_by_role.trim())
    .bind(location.created_by_ref.trim())
    .bind(location.created_by_name.trim())
    .bind(serde_json::to_value(&location).map_err(|_| QolipError::StoreFailed)?)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
    Ok(row_to_location(row))
}

pub(super) async fn load_location_by_id(
    pool: &PgPool,
    location_id: &str,
) -> Result<Option<QolipLocation>, QolipError> {
    let row = sqlx::query_as::<_, QolipLocationRow>(
        "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                size, quantity, row_letter, column_number, location_label,
                created_by_role, created_by_ref, created_by_name
         FROM mini_qolip_locations
         WHERE id = $1",
    )
    .bind(location_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    Ok(row.map(row_to_location))
}

pub(super) async fn load_location_by_qolip_code(
    pool: &PgPool,
    qolip_code: &str,
) -> Result<Option<QolipLocation>, QolipError> {
    let row = sqlx::query_as::<_, QolipLocationRow>(
        "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                size, quantity, row_letter, column_number, location_label,
                created_by_role, created_by_ref, created_by_name
         FROM mini_qolip_locations
         WHERE lower(qolip_code) = lower($1)
         ORDER BY updated_at DESC, created_at DESC
         LIMIT 1",
    )
    .bind(qolip_code.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    Ok(row.map(row_to_location))
}

pub(super) async fn move_location_to_cell(
    pool: &PgPool,
    location_id: &str,
    row_letter: &str,
    column_number: i32,
    quantity: i32,
) -> Result<QolipLocation, QolipError> {
    let location_id = location_id.trim();
    let mut tx = pool.begin().await.map_err(|_| QolipError::StoreFailed)?;

    let source_row = sqlx::query_as::<_, QolipLocationRow>(
        "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                size, quantity, row_letter, column_number, location_label,
                created_by_role, created_by_ref, created_by_name
         FROM mini_qolip_locations
         WHERE id = $1",
    )
    .bind(location_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    let Some(source_row) = source_row else {
        return Err(QolipError::LocationNotFound);
    };
    let source = row_to_location(source_row);
    let target = normalize_move_target(&source, row_letter, column_number, quantity)?;

    let mut lock_ids = vec![source.id.clone(), target.id.clone()];
    lock_ids.sort();
    lock_ids.dedup();
    for lock_id in &lock_ids {
        sqlx::query("SELECT id FROM mini_qolip_locations WHERE id = $1 FOR UPDATE")
            .bind(lock_id.trim())
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
    }

    let source_row = sqlx::query_as::<_, QolipLocationRow>(
        "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                size, quantity, row_letter, column_number, location_label,
                created_by_role, created_by_ref, created_by_name
         FROM mini_qolip_locations
         WHERE id = $1",
    )
    .bind(location_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    let Some(source_row) = source_row else {
        return Err(QolipError::LocationNotFound);
    };
    let source = row_to_location(source_row);
    let target = normalize_move_target(&source, row_letter, column_number, quantity)?;

    let target_row = sqlx::query_as::<_, QolipLocationRow>(
        "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                size, quantity, row_letter, column_number, location_label,
                created_by_role, created_by_ref, created_by_name
         FROM mini_qolip_locations
         WHERE id = $1",
    )
    .bind(target.id.trim())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    if let Some(existing_row) = &target_row {
        let existing = row_to_location(existing_row.clone());
        if !location_identity_matches(&existing, &target) {
            return Err(QolipError::LocationIdentityMismatch);
        }
    }

    let remaining = source.quantity - quantity;
    if remaining > 0 {
        sqlx::query(
            "UPDATE mini_qolip_locations
             SET quantity = $2, updated_at = now()
             WHERE id = $1",
        )
        .bind(source.id.trim())
        .bind(remaining)
        .execute(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    } else {
        sqlx::query("DELETE FROM mini_qolip_locations WHERE id = $1")
            .bind(source.id.trim())
            .execute(&mut *tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
    }

    let saved = if let Some(existing_row) = target_row {
        let merged_qty = existing_row.quantity + target.quantity;
        let row = sqlx::query_as::<_, QolipLocationRow>(
            "UPDATE mini_qolip_locations
             SET quantity = $2, updated_at = now()
             WHERE id = $1
             RETURNING id, block, warehouse, item_code, item_name, qolip_code,
                 size, quantity, row_letter, column_number, location_label,
                 created_by_role, created_by_ref, created_by_name",
        )
        .bind(target.id.trim())
        .bind(merged_qty)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
        row_to_location(row)
    } else {
        let row = sqlx::query_as::<_, QolipLocationRow>(
            "INSERT INTO mini_qolip_locations (
                 id, block, warehouse, item_code, item_name, qolip_code,
                 size, quantity, row_letter, column_number, location_label,
                 created_by_role, created_by_ref, created_by_name, payload_json
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
             RETURNING id, block, warehouse, item_code, item_name, qolip_code,
                 size, quantity, row_letter, column_number, location_label,
                 created_by_role, created_by_ref, created_by_name",
        )
        .bind(target.id.trim())
        .bind(target.block.trim())
        .bind(target.warehouse.trim())
        .bind(target.item_code.trim())
        .bind(target.item_name.trim())
        .bind(target.qolip_code.trim())
        .bind(target.size)
        .bind(target.quantity)
        .bind(target.row_letter.trim())
        .bind(target.column_number)
        .bind(target.location_label.trim())
        .bind(target.created_by_role.trim())
        .bind(target.created_by_ref.trim())
        .bind(target.created_by_name.trim())
        .bind(serde_json::to_value(&target).map_err(|_| QolipError::StoreFailed)?)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
        row_to_location(row)
    };

    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
    Ok(saved)
}
