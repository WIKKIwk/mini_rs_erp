use sqlx::{PgPool, Postgres, Transaction};

use crate::core::qolip::normalize::{
    location_from_checkout, location_from_checkout_target, location_identity_matches,
};
use crate::core::qolip::{QolipCheckout, QolipError};

use super::rows::{QolipCheckoutRow, QolipLocationRow, row_to_checkout, row_to_location};

pub(super) async fn save_checkout(
    pool: &PgPool,
    checkout: QolipCheckout,
) -> Result<QolipCheckout, QolipError> {
    let mut tx = pool.begin().await.map_err(|_| QolipError::StoreFailed)?;
    let saved = save_checkout_tx(&mut tx, &checkout).await?;
    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
    Ok(saved)
}

pub(crate) async fn save_checkout_tx(
    tx: &mut Transaction<'_, Postgres>,
    checkout: &QolipCheckout,
) -> Result<QolipCheckout, QolipError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext(lower($1))::bigint)")
        .bind(checkout.qolip_code.trim())
        .execute(&mut **tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;

    if !checkout.item_group.trim().is_empty() {
        let product_group = sqlx::query_scalar::<_, String>(
            "SELECT item_group
             FROM mini_items
             WHERE lower(code) = lower($1)
             FOR SHARE",
        )
        .bind(checkout.item_code.trim())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?
        .ok_or(QolipError::QolipCodeMismatch)?;
        if !product_group
            .trim()
            .eq_ignore_ascii_case(checkout.item_group.trim())
        {
            return Err(QolipError::QolipCodeMismatch);
        }
        let spec = sqlx::query_as::<_, (String, String, String, String, i32)>(
            "SELECT item_code, item_name, item_group, qolip_code, size
             FROM mini_qolip_product_specs
             WHERE lower(qolip_code) = lower($1)
             FOR SHARE",
        )
        .bind(checkout.qolip_code.trim())
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
        let Some((item_code, item_name, item_group, qolip_code, size)) = spec else {
            return Err(QolipError::QolipCodeNotFound);
        };
        if !item_code
            .trim()
            .eq_ignore_ascii_case(checkout.item_code.trim())
            || !item_name
                .trim()
                .eq_ignore_ascii_case(checkout.item_name.trim())
            || !item_group
                .trim()
                .eq_ignore_ascii_case(checkout.item_group.trim())
            || !qolip_code
                .trim()
                .eq_ignore_ascii_case(checkout.qolip_code.trim())
            || size != checkout.size
        {
            return Err(QolipError::QolipCodeMismatch);
        }
    }

    let current_row = sqlx::query_as::<_, QolipLocationRow>(
        "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                size, quantity, row_letter, column_number, location_label,
                created_by_role, created_by_ref, created_by_name
         FROM mini_qolip_locations
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(checkout.location_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    let Some(current_row) = current_row else {
        return Err(QolipError::LocationNotFound);
    };
    let current = row_to_location(current_row);
    let expected = location_from_checkout(checkout);
    if !location_identity_matches(&current, &expected) {
        return Err(QolipError::LocationIdentityMismatch);
    }
    let current_qty = current.quantity;
    if checkout.quantity > current_qty {
        return Err(QolipError::InsufficientStock);
    }

    let remaining = current_qty - checkout.quantity;
    if remaining > 0 {
        sqlx::query(
            "UPDATE mini_qolip_locations
             SET quantity = $2, updated_at = now()
             WHERE id = $1",
        )
        .bind(checkout.location_id.trim())
        .bind(remaining)
        .execute(&mut **tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    } else {
        sqlx::query("DELETE FROM mini_qolip_locations WHERE id = $1")
            .bind(checkout.location_id.trim())
            .execute(&mut **tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
    }

    let row = sqlx::query_as::<_, QolipCheckoutRow>(
        "INSERT INTO mini_qolip_checkouts (
             id, location_id, block, warehouse, item_code, item_name, qolip_code,
             size, quantity, row_letter, column_number, location_label,
             issued_to_ref, issued_to_name, status,
             issued_by_role, issued_by_ref, issued_by_name, payload_json
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
         RETURNING id, location_id, block, warehouse, item_code, item_name, qolip_code,
             size, quantity, row_letter, column_number, location_label,
             issued_to_ref, issued_to_name, status,
             issued_by_role, issued_by_ref, issued_by_name,
             to_char(issued_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS issued_at",
    )
    .bind(checkout.id.trim())
    .bind(checkout.location_id.trim())
    .bind(checkout.block.trim())
    .bind(checkout.warehouse.trim())
    .bind(checkout.item_code.trim())
    .bind(checkout.item_name.trim())
    .bind(checkout.qolip_code.trim())
    .bind(checkout.size)
    .bind(checkout.quantity)
    .bind(checkout.row_letter.trim())
    .bind(checkout.column_number)
    .bind(checkout.location_label.trim())
    .bind(checkout.issued_to_ref.trim())
    .bind(checkout.issued_to_name.trim())
    .bind(checkout.status.trim())
    .bind(checkout.issued_by_role.trim())
    .bind(checkout.issued_by_ref.trim())
    .bind(checkout.issued_by_name.trim())
    .bind(serde_json::to_value(checkout).map_err(|_| QolipError::StoreFailed)?)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    Ok(row_to_checkout(row))
}

pub(super) async fn load_checkouts(
    pool: &PgPool,
    block: Option<&str>,
    allowed_blocks: Option<&[String]>,
    status: &str,
    limit: usize,
) -> Result<Vec<QolipCheckout>, QolipError> {
    let block = block.map(str::trim).filter(|value| !value.is_empty());
    let rows = if let Some(block) = block {
        sqlx::query_as::<_, QolipCheckoutRow>(
            "SELECT id, location_id, block, warehouse, item_code, item_name, qolip_code,
                    size, quantity, row_letter, column_number, location_label,
                    issued_to_ref, issued_to_name, status,
                    issued_by_role, issued_by_ref, issued_by_name,
                    to_char(issued_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS issued_at
             FROM mini_qolip_checkouts
             WHERE lower(status) = lower($1)
               AND lower(block) = lower($2)
             ORDER BY issued_at DESC
             LIMIT $3",
        )
        .bind(status.trim())
        .bind(block)
        .bind(limit as i64)
        .fetch_all(pool)
        .await
    } else if let Some(allowed_blocks) = allowed_blocks {
        if allowed_blocks.is_empty() {
            return Ok(Vec::new());
        }
        let allowed: Vec<String> = allowed_blocks
            .iter()
            .map(|block| block.trim().to_lowercase())
            .filter(|block| !block.is_empty())
            .collect();
        if allowed.is_empty() {
            return Ok(Vec::new());
        }
        sqlx::query_as::<_, QolipCheckoutRow>(
            "SELECT id, location_id, block, warehouse, item_code, item_name, qolip_code,
                    size, quantity, row_letter, column_number, location_label,
                    issued_to_ref, issued_to_name, status,
                    issued_by_role, issued_by_ref, issued_by_name,
                    to_char(issued_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS issued_at
             FROM mini_qolip_checkouts
             WHERE lower(status) = lower($1)
               AND lower(block) = ANY($2)
             ORDER BY issued_at DESC
             LIMIT $3",
        )
        .bind(status.trim())
        .bind(allowed)
        .bind(limit as i64)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query_as::<_, QolipCheckoutRow>(
            "SELECT id, location_id, block, warehouse, item_code, item_name, qolip_code,
                    size, quantity, row_letter, column_number, location_label,
                    issued_to_ref, issued_to_name, status,
                    issued_by_role, issued_by_ref, issued_by_name,
                    to_char(issued_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS issued_at
             FROM mini_qolip_checkouts
             WHERE lower(status) = lower($1)
             ORDER BY issued_at DESC
             LIMIT $2",
        )
        .bind(status.trim())
        .bind(limit as i64)
        .fetch_all(pool)
        .await
    }
    .map_err(|_| QolipError::StoreFailed)?;

    Ok(rows.into_iter().map(row_to_checkout).collect())
}

pub(super) async fn load_open_checkouts_for_worker(
    pool: &PgPool,
    worker_refs: &[String],
    _worker_name: &str,
    limit: usize,
) -> Result<Vec<QolipCheckout>, QolipError> {
    let worker_refs = worker_refs
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if worker_refs.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, QolipCheckoutRow>(
        "SELECT id, location_id, block, warehouse, item_code, item_name, qolip_code,
                size, quantity, row_letter, column_number, location_label,
                issued_to_ref, issued_to_name, status,
                issued_by_role, issued_by_ref, issued_by_name,
                to_char(issued_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS issued_at
         FROM mini_qolip_checkouts AS checkout
         WHERE lower(checkout.status) = 'open'
           AND (
               lower(checkout.issued_to_ref) = ANY($1)
               OR EXISTS (
                   SELECT 1
                   FROM mini_worker_identity_aliases AS alias
                   WHERE lower(alias.worker_id) = ANY($1)
                     AND alias.alias_type = 'phone'
                     AND checkout.issued_to_ref ~ '^[+0-9() .-]+$'
                     AND alias.alias_key = regexp_replace(checkout.issued_to_ref, '[^0-9]', '', 'g')
                     AND checkout.issued_at >= alias.valid_from
                     AND (alias.valid_to IS NULL OR checkout.issued_at < alias.valid_to)
               )
           )
         ORDER BY issued_at DESC
         LIMIT $2",
    )
    .bind(&worker_refs)
    .bind(limit.max(1) as i64)
    .fetch_all(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    Ok(rows.into_iter().map(row_to_checkout).collect())
}

pub(super) async fn load_checkout_by_id(
    pool: &PgPool,
    checkout_id: &str,
) -> Result<Option<QolipCheckout>, QolipError> {
    let row = sqlx::query_as::<_, QolipCheckoutRow>(
        "SELECT id, location_id, block, warehouse, item_code, item_name, qolip_code,
                size, quantity, row_letter, column_number, location_label,
                issued_to_ref, issued_to_name, status,
                issued_by_role, issued_by_ref, issued_by_name,
                to_char(issued_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS issued_at
         FROM mini_qolip_checkouts
         WHERE id = $1",
    )
    .bind(checkout_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    Ok(row.map(row_to_checkout))
}

pub(super) async fn return_checkout_to_location(
    pool: &PgPool,
    checkout_id: &str,
    row_letter: &str,
    column_number: Option<i32>,
) -> Result<QolipCheckout, QolipError> {
    let checkout_id = checkout_id.trim();
    let mut tx = pool.begin().await.map_err(|_| QolipError::StoreFailed)?;

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
    .map_err(|_| QolipError::StoreFailed)?;

    let Some(row) = row else {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM mini_qolip_checkouts WHERE id = $1)",
        )
        .bind(checkout_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
        return Err(if exists {
            QolipError::CheckoutNotReturnable
        } else {
            QolipError::CheckoutNotFound
        });
    };

    let checkout = row_to_checkout(row);
    let restore = location_from_checkout_target(&checkout, row_letter, column_number)?;
    let existing_row = sqlx::query_as::<_, QolipLocationRow>(
        "SELECT id, block, warehouse, item_code, item_name, qolip_code,
                size, quantity, row_letter, column_number, location_label,
                created_by_role, created_by_ref, created_by_name
         FROM mini_qolip_locations
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(restore.id.trim())
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;

    if let Some(existing_row) = existing_row {
        let existing = row_to_location(existing_row);
        if !location_identity_matches(&existing, &restore) {
            return Err(QolipError::LocationIdentityMismatch);
        }
        sqlx::query(
            "UPDATE mini_qolip_locations
             SET quantity = $2, updated_at = now()
             WHERE id = $1",
        )
        .bind(restore.id.trim())
        .bind(existing.quantity + restore.quantity)
        .execute(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
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
        .bind(serde_json::to_value(&restore).map_err(|_| QolipError::StoreFailed)?)
        .execute(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    }

    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
    Ok(checkout)
}
