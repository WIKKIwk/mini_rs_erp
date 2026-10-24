use sqlx::{PgPool, Postgres, Transaction};

use crate::core::auth::models::Principal;
use crate::core::qolip::{QolipError, QolipRework, QolipReworkReturn, QolipReworkSend, role_code};

const COLUMNS: &str = "id, qolip_code, qolip_set_id, warehouse, item_code, item_name, color, size,
    to_char(sent_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS sent_at,
    to_char(returned_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS returned_at";

#[derive(sqlx::FromRow)]
struct ReworkRow {
    id: String,
    qolip_code: String,
    qolip_set_id: String,
    warehouse: String,
    item_code: String,
    item_name: String,
    color: String,
    size: i32,
    sent_at: String,
    returned_at: Option<String>,
}

impl From<ReworkRow> for QolipRework {
    fn from(row: ReworkRow) -> Self {
        Self {
            id: row.id,
            qolip_code: row.qolip_code,
            qolip_set_id: row.qolip_set_id,
            warehouse: row.warehouse,
            item_code: row.item_code,
            item_name: row.item_name,
            color: row.color,
            size: row.size,
            sent_at: row.sent_at,
            returned_at: row.returned_at,
        }
    }
}

fn owner(warehouse: &str, allowed: Option<&[String]>) -> Result<(), QolipError> {
    if warehouse.trim().is_empty()
        || allowed.is_some_and(|values| {
            !values
                .iter()
                .any(|value| value.trim().eq_ignore_ascii_case(warehouse.trim()))
        })
    {
        return Err(QolipError::AccessDenied);
    }
    Ok(())
}

pub(super) fn storage_error(error: sqlx::Error) -> QolipError {
    if error
        .as_database_error()
        .is_some_and(|error| error.message().contains("qolip_in_rework"))
    {
        QolipError::QolipInRework
    } else if error.as_database_error().is_some_and(|error| {
        error.message().contains("qolip_history_locked")
            || error.constraint() == Some("mini_qolip_mileage_identity_locked")
    }) {
        QolipError::QolipHistoryLocked
    } else {
        QolipError::StoreFailed
    }
}

pub(super) async fn list(
    pool: &PgPool,
    status: &str,
    allowed: Option<&[String]>,
) -> Result<Vec<QolipRework>, QolipError> {
    let allowed = allowed.map(|values| {
        values
            .iter()
            .map(|value| value.trim().to_lowercase())
            .collect::<Vec<_>>()
    });
    let rows = sqlx::query_as::<_, ReworkRow>(&format!("SELECT {COLUMNS} FROM mini_qolip_reworks
        WHERE ($1::text = 'all' OR ($1 = 'open' AND returned_at IS NULL) OR ($1 = 'returned' AND returned_at IS NOT NULL))
          AND ($2::text[] IS NULL OR lower(btrim(warehouse)) = ANY($2)) ORDER BY sent_at DESC, id"))
        .bind(status).bind(allowed).fetch_all(pool).await.map_err(storage_error)?;
    Ok(rows.into_iter().map(Into::into).collect())
}

pub(crate) async fn active_codes(
    pool: &PgPool,
    codes: &[String],
) -> Result<Vec<String>, QolipError> {
    let keys = codes
        .iter()
        .map(|code| code.trim().to_lowercase())
        .collect::<Vec<_>>();
    sqlx::query_scalar("SELECT qolip_code FROM mini_qolip_reworks WHERE returned_at IS NULL AND qolip_code_key = ANY($1) ORDER BY qolip_code_key")
        .bind(keys).fetch_all(pool).await.map_err(storage_error)
}

async fn lock_codes(
    tx: &mut Transaction<'_, Postgres>,
    codes: &[String],
) -> Result<(), QolipError> {
    let keys = codes
        .iter()
        .map(|code| code.trim().to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    for code in keys {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("qolip:{code}"))
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
    }
    Ok(())
}

async fn replay(
    tx: &mut Transaction<'_, Postgres>,
    actor: &str,
    request_id: &str,
    action: &str,
    request: &serde_json::Value,
    allowed: Option<&[String]>,
) -> Result<Option<Vec<QolipRework>>, QolipError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("qolip-rework-request:{actor}:{request_id}"))
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    let previous = sqlx::query_as::<_, (String, serde_json::Value, serde_json::Value)>(
        "SELECT action, request_json, response_json FROM mini_qolip_rework_requests WHERE actor_key = $1 AND request_id = $2")
        .bind(actor).bind(request_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((old_action, old_request, response)) = previous else {
        return Ok(None);
    };
    if old_action != action || old_request != *request {
        return Err(QolipError::ReworkRequestConflict);
    }
    let response: Vec<QolipRework> =
        serde_json::from_value(response).map_err(|_| QolipError::StoreFailed)?;
    for entry in &response {
        owner(&entry.warehouse, allowed)?;
    }
    Ok(Some(response))
}

async fn record_request(
    tx: &mut Transaction<'_, Postgres>,
    actor: &str,
    request_id: &str,
    action: &str,
    request: serde_json::Value,
    response: &[QolipRework],
) -> Result<(), QolipError> {
    sqlx::query("INSERT INTO mini_qolip_rework_requests(actor_key, request_id, action, request_json, response_json)
        VALUES ($1, $2, $3, $4, $5)")
        .bind(actor).bind(request_id).bind(action).bind(request)
        .bind(serde_json::to_value(response).map_err(|_| QolipError::StoreFailed)?)
        .execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}

pub(super) async fn send(
    pool: &PgPool,
    input: &QolipReworkSend,
    principal: &Principal,
    allowed: Option<&[String]>,
) -> Result<Vec<QolipRework>, QolipError> {
    let actor = format!("{}:{}", role_code(&principal.role), principal.ref_);
    let request = serde_json::to_value(input).map_err(|_| QolipError::StoreFailed)?;
    let mut tx = pool.begin().await.map_err(storage_error)?;
    if let Some(response) = replay(
        &mut tx,
        &actor,
        &input.request_id,
        "send",
        &request,
        allowed,
    )
    .await?
    {
        return Ok(response);
    }
    super::product_transfer::lock_composition(&mut tx).await?;
    lock_codes(&mut tx, &input.qolip_codes).await?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let mut response = Vec::with_capacity(input.qolip_codes.len());
    for code in &input.qolip_codes {
        let already_away: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_qolip_reworks WHERE qolip_code_key = $1 AND returned_at IS NULL)")
            .bind(code).fetch_one(&mut *tx).await.map_err(storage_error)?;
        if already_away {
            return Err(QolipError::QolipInRework);
        }
        let mut spec = super::product_transfer::load_spec(&mut tx, code).await?;
        owner(&spec.warehouse, allowed)?;
        super::product_transfer::reject_in_use(&mut tx, code).await?;
        let transferring: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_qolip_locations WHERE lower(btrim(qolip_code)) = $1 AND btrim(inventory_transfer_id) <> '')")
            .bind(code).fetch_one(&mut *tx).await.map_err(storage_error)?;
        if transferring {
            return Err(QolipError::QolipInUse);
        }
        let locations: serde_json::Value = sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(location)), '[]'::jsonb) FROM mini_qolip_locations location WHERE lower(btrim(qolip_code)) = $1")
            .bind(code).fetch_one(&mut *tx).await.map_err(storage_error)?;
        if spec.item_code.is_empty() {
            if let Some((item, name)) = sqlx::query_as::<_, (String, String)>("SELECT item.code, item.name
                FROM mini_qolip_effective_product_molds effective JOIN mini_items item ON item.code = effective.item_code
                WHERE effective.qolip_code_key = $1 ORDER BY item.code LIMIT 1")
                .bind(code).fetch_optional(&mut *tx).await.map_err(storage_error)? {
                spec.item_code = item; spec.item_name = name;
            }
        }
        let id = format!("qolip-rework:{:032x}", rand::random::<u128>());
        let row = sqlx::query_as::<_, ReworkRow>(&format!("INSERT INTO mini_qolip_reworks
            (id,qolip_code,qolip_set_id,warehouse,item_code,item_name,color,size,sent_at,location_json,created_by_role,created_by_ref)
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,to_timestamp($9::bigint::double precision),$10,$11,$12) RETURNING {COLUMNS}"))
            .bind(id).bind(&spec.qolip_code).bind(spec.set_id()).bind(&spec.warehouse)
            .bind(&spec.item_code).bind(&spec.item_name).bind(&spec.color).bind(spec.size)
            .bind(now).bind(locations).bind(role_code(&principal.role)).bind(&principal.ref_)
            .fetch_one(&mut *tx).await.map_err(storage_error)?;
        response.push(row.into());
    }
    record_request(
        &mut tx,
        &actor,
        &input.request_id,
        "send",
        request,
        &response,
    )
    .await?;
    tx.commit().await.map_err(storage_error)?;
    Ok(response)
}

pub(super) async fn receive(
    pool: &PgPool,
    input: &QolipReworkReturn,
    principal: &Principal,
    allowed: Option<&[String]>,
) -> Result<Vec<QolipRework>, QolipError> {
    let actor = format!("{}:{}", role_code(&principal.role), principal.ref_);
    let request = serde_json::to_value(input).map_err(|_| QolipError::StoreFailed)?;
    let mut tx = pool.begin().await.map_err(storage_error)?;
    if let Some(response) = replay(
        &mut tx,
        &actor,
        &input.request_id,
        "return",
        &request,
        allowed,
    )
    .await?
    {
        return Ok(response);
    }
    super::product_transfer::lock_composition(&mut tx).await?;
    let codes: Vec<String> = sqlx::query_scalar(
        "SELECT qolip_code FROM mini_qolip_reworks WHERE id = ANY($1) ORDER BY qolip_code_key",
    )
    .bind(&input.rework_ids)
    .fetch_all(&mut *tx)
    .await
    .map_err(storage_error)?;
    if codes.len() != input.rework_ids.len() {
        return Err(QolipError::ReworkConflict);
    }
    lock_codes(&mut tx, &codes).await?;
    let rows = sqlx::query_as::<_, ReworkRow>(&format!(
        "SELECT {COLUMNS} FROM mini_qolip_reworks WHERE id = ANY($1) ORDER BY id FOR UPDATE"
    ))
    .bind(&input.rework_ids)
    .fetch_all(&mut *tx)
    .await
    .map_err(storage_error)?;
    for row in &rows {
        owner(&row.warehouse, allowed)?;
        if row.returned_at.is_some() {
            return Err(QolipError::ReworkConflict);
        }
    }
    let now =
        time::OffsetDateTime::from_unix_timestamp(time::OffsetDateTime::now_utc().unix_timestamp())
            .map_err(|_| QolipError::StoreFailed)?;
    let mut response = Vec::with_capacity(rows.len());
    for row in rows {
        let updated = sqlx::query_as::<_, ReworkRow>(&format!("UPDATE mini_qolip_reworks
            SET returned_at = to_timestamp($2::bigint::double precision), returned_by_role = $3, returned_by_ref = $4
            WHERE id = $1 AND returned_at IS NULL RETURNING {COLUMNS}"))
            .bind(&row.id).bind(now.unix_timestamp()).bind(role_code(&principal.role)).bind(&principal.ref_)
            .fetch_optional(&mut *tx).await.map_err(storage_error)?.ok_or(QolipError::ReworkConflict)?;
        super::mileage::begin_cycle_tx(&mut tx, &row.id, &row.qolip_code, now).await?;
        response.push(updated.into());
    }
    record_request(
        &mut tx,
        &actor,
        &input.request_id,
        "return",
        request,
        &response,
    )
    .await?;
    tx.commit().await.map_err(storage_error)?;
    Ok(response)
}

#[cfg(test)]
#[path = "reworks_tests.rs"]
mod tests;
