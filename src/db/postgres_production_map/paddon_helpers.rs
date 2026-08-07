use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::{FromRow, PgPool, Postgres, Transaction};

use crate::core::production_map::{
    PaddonCreateInput, PaddonSnapshot, PaddonSummary, ProductionMapError, QueueActionActor,
    WipProgressBatchQuery,
};

use super::order_query_helpers::load_progress_batch;
use super::wip_query_helpers::load_unassigned_wip_progress_batches;

#[derive(FromRow)]
struct PaddonRow {
    id: String,
    code: String,
    location: String,
    note: String,
    created_by_ref: String,
    created_by_display_name: String,
    created_at_unix: i64,
    updated_at_unix: i64,
    item_count: i64,
}

#[derive(FromRow)]
struct PaddonLockRow {
    id: String,
}

#[derive(FromRow)]
struct CreatedPaddonRow {
    code: String,
}

fn summary_from_row(row: PaddonRow) -> PaddonSummary {
    PaddonSummary {
        id: row.id,
        code: row.code,
        location: row.location,
        note: row.note,
        created_by_ref: row.created_by_ref,
        created_by_display_name: row.created_by_display_name,
        created_at_unix: row.created_at_unix,
        updated_at_unix: row.updated_at_unix,
        item_count: row.item_count,
    }
}

async fn load_summary_by_code(
    pool: &PgPool,
    code: &str,
) -> Result<Option<PaddonSummary>, ProductionMapError> {
    let row = sqlx::query_as::<_, PaddonRow>(
        "SELECT p.id, p.code, p.location, p.note,
                p.created_by_ref, p.created_by_display_name,
                EXTRACT(EPOCH FROM p.created_at)::bigint AS created_at_unix,
                EXTRACT(EPOCH FROM p.updated_at)::bigint AS updated_at_unix,
                COUNT(i.id) FILTER (WHERE i.removed_at IS NULL)::bigint AS item_count
         FROM mini_paddons AS p
         LEFT JOIN mini_paddon_items AS i ON i.paddon_id = p.id
         WHERE p.code = $1
         GROUP BY p.id
         LIMIT 1",
    )
    .bind(code.trim())
    .fetch_optional(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(row.map(summary_from_row))
}

pub(super) async fn load_paddon_summary(
    pool: &PgPool,
    code: &str,
) -> Result<Option<PaddonSummary>, ProductionMapError> {
    load_summary_by_code(pool, code).await
}

async fn load_snapshot_by_code(
    pool: &PgPool,
    code: &str,
    include_available_items: bool,
) -> Result<Option<PaddonSnapshot>, ProductionMapError> {
    let Some(paddon) = load_summary_by_code(pool, code).await? else {
        return Ok(None);
    };
    let batch_ids = sqlx::query_scalar::<_, String>(
        "SELECT progress_batch_id
         FROM mini_paddon_items
         WHERE paddon_id = $1 AND removed_at IS NULL
         ORDER BY added_at DESC, progress_batch_id DESC",
    )
    .bind(&paddon.id)
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut items = Vec::with_capacity(batch_ids.len());
    for batch_id in batch_ids {
        let batch = load_progress_batch(pool, &batch_id)
            .await?
            .ok_or(ProductionMapError::ProgressBatchNotFound)?;
        items.push(batch);
    }
    let available_items = if include_available_items {
        load_unassigned_wip_progress_batches(
            pool,
            WipProgressBatchQuery::new("", "", "", None, false, "", 500),
        )
        .await?
    } else {
        Vec::new()
    };
    Ok(Some(PaddonSnapshot {
        paddon,
        items,
        available_items,
    }))
}

pub(super) async fn load_paddons(
    pool: &PgPool,
    limit: usize,
) -> Result<Vec<PaddonSummary>, ProductionMapError> {
    let limit = i64::try_from(limit.clamp(1, 200)).unwrap_or(200);
    let rows = sqlx::query_as::<_, PaddonRow>(
        "SELECT p.id, p.code, p.location, p.note,
                p.created_by_ref, p.created_by_display_name,
                EXTRACT(EPOCH FROM p.created_at)::bigint AS created_at_unix,
                EXTRACT(EPOCH FROM p.updated_at)::bigint AS updated_at_unix,
                COUNT(i.id) FILTER (WHERE i.removed_at IS NULL)::bigint AS item_count
         FROM mini_paddons AS p
         LEFT JOIN mini_paddon_items AS i ON i.paddon_id = p.id
         GROUP BY p.id
         ORDER BY p.updated_at DESC, p.code ASC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(rows.into_iter().map(summary_from_row).collect())
}

pub(super) async fn create_paddon(
    pool: &PgPool,
    input: PaddonCreateInput,
) -> Result<PaddonSummary, ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let created = sqlx::query_as::<_, CreatedPaddonRow>(
        "WITH allocated AS (
             UPDATE mini_paddon_sequence
             SET next_number = next_number + 1
             WHERE id = 1 AND next_number <= 99999
             RETURNING next_number - 1 AS allocated_number
         )
         INSERT INTO mini_paddons (
             id, code, location, note, created_by_ref, created_by_display_name
         )
         SELECT
             'paddon-' || LPAD(allocated_number::text, 5, '0'),
             LPAD(allocated_number::text, 5, '0'),
             $1, $2, $3, $4
         FROM allocated
         RETURNING code",
    )
    .bind(input.location)
    .bind(input.note)
    .bind(input.actor_ref)
    .bind(input.actor_display_name)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    let Some(created) = created else {
        return Err(ProductionMapError::PaddonCodeExhausted);
    };
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    load_summary_by_code(pool, &created.code)
        .await?
        .ok_or(ProductionMapError::StoreFailed)
}

pub(super) async fn load_paddon_snapshot(
    pool: &PgPool,
    code: &str,
) -> Result<Option<PaddonSnapshot>, ProductionMapError> {
    load_snapshot_by_code(pool, code, true).await
}

pub(super) async fn load_paddon_scan_snapshot(
    pool: &PgPool,
    code: &str,
) -> Result<Option<PaddonSnapshot>, ProductionMapError> {
    load_snapshot_by_code(pool, code, false).await
}

async fn lock_paddon(
    tx: &mut Transaction<'_, Postgres>,
    code: &str,
) -> Result<PaddonLockRow, ProductionMapError> {
    sqlx::query_as::<_, PaddonLockRow>(
        "SELECT id
         FROM mini_paddons
         WHERE code = $1
         FOR UPDATE",
    )
    .bind(code.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?
    .ok_or(ProductionMapError::PaddonNotFound)
}

fn new_item_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default();
    format!("paddon-item-{now}-{:08x}", rand::random::<u32>())
}

pub(super) async fn add_paddon_item(
    pool: &PgPool,
    code: &str,
    progress_batch_id: &str,
    actor: &QueueActionActor,
) -> Result<PaddonSnapshot, ProductionMapError> {
    let progress_batch_ids = [progress_batch_id.trim().to_string()];
    add_paddon_items(pool, code, &progress_batch_ids, actor).await
}

pub(super) async fn add_paddon_items(
    pool: &PgPool,
    code: &str,
    progress_batch_ids: &[String],
    actor: &QueueActionActor,
) -> Result<PaddonSnapshot, ProductionMapError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let paddon = lock_paddon(&mut tx, code).await?;
    let mut changed = false;
    for progress_batch_id in progress_batch_ids {
        let progress_batch_id = progress_batch_id.trim();
        let batch_exists = sqlx::query_scalar::<_, String>(
            "SELECT batch_id FROM mini_progress_batches WHERE batch_id = $1",
        )
        .bind(progress_batch_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?
        .is_some();
        if !batch_exists {
            return Err(ProductionMapError::ProgressBatchNotFound);
        }
        let active_paddon = sqlx::query_scalar::<_, String>(
            "SELECT paddon_id
             FROM mini_paddon_items
             WHERE progress_batch_id = $1 AND removed_at IS NULL
             FOR UPDATE",
        )
        .bind(progress_batch_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if let Some(active_paddon) = active_paddon {
            if active_paddon != paddon.id {
                return Err(ProductionMapError::PaddonItemAlreadyAssigned);
            }
            continue;
        }
        sqlx::query(
            "INSERT INTO mini_paddon_items (
                 id, paddon_id, progress_batch_id, added_by_ref, added_by_display_name
             ) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(new_item_id())
        .bind(&paddon.id)
        .bind(progress_batch_id)
        .bind(actor.ref_.trim())
        .bind(actor.display_name.trim())
        .execute(&mut *tx)
        .await
        .map_err(|error| match error {
            sqlx::Error::Database(database_error)
                if database_error.constraint() == Some("idx_mini_paddon_items_active_batch") =>
            {
                ProductionMapError::PaddonItemAlreadyAssigned
            }
            _ => ProductionMapError::StoreFailed,
        })?;
        changed = true;
    }
    if changed {
        sqlx::query("UPDATE mini_paddons SET updated_at = now() WHERE id = $1")
            .bind(&paddon.id)
            .execute(&mut *tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
    }
    tx.commit()
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    load_snapshot_by_code(pool, code, true)
        .await?
        .ok_or(ProductionMapError::PaddonNotFound)
}

pub(super) async fn remove_paddon_item(
    pool: &PgPool,
    code: &str,
    progress_batch_id: &str,
    actor: &QueueActionActor,
) -> Result<PaddonSnapshot, ProductionMapError> {
    let progress_batch_ids = [progress_batch_id.trim().to_string()];
    remove_paddon_items(pool, code, &progress_batch_ids, actor).await
}

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
    let paddon = lock_paddon(&mut tx, code).await?;
    let mut item_ids = Vec::with_capacity(progress_batch_ids.len());
    for progress_batch_id in progress_batch_ids {
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
    load_snapshot_by_code(pool, code, true)
        .await?
        .ok_or(ProductionMapError::PaddonNotFound)
}
