async fn restore_checkout_location_tx(
    tx: &mut Transaction<'_, Postgres>,
    restore: &crate::core::qolip::QolipLocation,
) -> Result<(), QolipError> {
    let existing_row = sqlx::query_as::<_, QolipLocationRow>(
        "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                size, quantity, row_letter, column_number, location_label,
                created_by_role, created_by_ref, created_by_name
         FROM mini_qolip_locations
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(restore.id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(super::reworks::storage_error)?;

    if let Some(existing_row) = existing_row {
        let existing = row_to_location(existing_row);
        if !location_identity_matches(&existing, restore) {
            return Err(QolipError::LocationIdentityMismatch);
        }
        sqlx::query(
            "UPDATE mini_qolip_locations
             SET quantity = $2, updated_at = now()
             WHERE id = $1",
        )
        .bind(restore.id.trim())
        .bind(existing.quantity + restore.quantity)
        .execute(&mut **tx)
        .await
        .map_err(super::reworks::storage_error)?;
    } else {
        sqlx::query(
            "INSERT INTO mini_qolip_locations (
                 id, block, warehouse, item_code, item_name, qolip_code,
                 size, quantity, row_letter, column_number, location_label,
                 created_by_role, created_by_ref, created_by_name, payload_json
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
        )
        .bind(restore.id.trim())
        .bind(restore.block.trim())
        .bind(restore.warehouse.trim())
        .bind(restore.item_code.trim())
        .bind(restore.item_name.trim())
        .bind(restore.qolip_code.trim())
        .bind(restore.size)
        .bind(restore.quantity)
        .bind(restore.row_letter.trim())
        .bind(restore.column_number)
        .bind(restore.location_label.trim())
        .bind(restore.created_by_role.trim())
        .bind(restore.created_by_ref.trim())
        .bind(restore.created_by_name.trim())
        .bind(serde_json::to_value(restore).map_err(|_| QolipError::StoreFailed)?)
        .execute(&mut **tx)
        .await
        .map_err(super::reworks::storage_error)?;
    }
    Ok(())
}

pub(super) async fn return_checkout_to_location(
    pool: &PgPool,
    checkout_id: &str,
    row_letter: &str,
    column_number: Option<i32>,
) -> Result<QolipCheckout, QolipError> {
    for _ in 0..3 {
        if let Some(checkout) =
            return_checkout_once(pool, checkout_id, row_letter, column_number).await?
        {
            return Ok(checkout);
        }
    }
    Err(QolipError::StoreFailed)
}

async fn return_checkout_once(
    pool: &PgPool,
    checkout_id: &str,
    row_letter: &str,
    column_number: Option<i32>,
) -> Result<Option<QolipCheckout>, QolipError> {
    let checkout_id = checkout_id.trim();
    let mut tx = pool.begin().await.map_err(super::reworks::storage_error)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await
        .map_err(super::reworks::storage_error)?;
    let (code, status) = sqlx::query_as::<_, (String, String)>(
        "SELECT qolip_code,status FROM mini_qolip_checkouts WHERE id = $1",
    )
    .bind(checkout_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(super::reworks::storage_error)?
    .ok_or(QolipError::CheckoutNotFound)?;
    if !status.eq_ignore_ascii_case("open") {
        return Err(QolipError::CheckoutNotReturnable);
    }
    let Some(sessions) =
        crate::db::postgres_production_map::prepare_qolip_return_tx(&mut tx, &code).await?
    else {
        tx.rollback()
            .await
            .map_err(super::reworks::storage_error)?;
        return Ok(None);
    };

    let row = sqlx::query_as::<_, QolipCheckoutRow>(
        "UPDATE mini_qolip_checkouts
         SET status = 'returned', updated_at = now()
         WHERE id = $1 AND lower(status) = 'open'
         RETURNING id, location_id, block, warehouse, item_code, item_name, qolip_code,
             size, quantity, row_letter, column_number, location_label,
             issued_to_ref, issued_to_name, status,
             issued_by_role, issued_by_ref, issued_by_name,
             to_char(issued_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS issued_at",
    )
    .bind(checkout_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(super::reworks::storage_error)?;

    let Some(row) = row else {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM mini_qolip_checkouts WHERE id = $1)",
        )
        .bind(checkout_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(super::reworks::storage_error)?;
        return Err(if exists {
            QolipError::CheckoutNotReturnable
        } else {
            QolipError::CheckoutNotFound
        });
    };

    let checkout = row_to_checkout(row);
    let restore = location_from_checkout_target(&checkout, row_letter, column_number)?;
    restore_checkout_location_tx(&mut tx, &restore).await?;
    crate::db::postgres_production_map::release_returned_qolips_tx(&mut tx, sessions, checkout_id)
        .await?;

    tx.commit().await.map_err(super::reworks::storage_error)?;
    Ok(Some(checkout))
}
