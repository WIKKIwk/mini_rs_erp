use sqlx::{PgConnection, PgPool};

use super::rows::{QolipProductSpecRow, row_to_product_spec};
use crate::core::auth::models::Principal;
use crate::core::qolip::{
    QolipError, QolipProductSpec, QolipSetProduct, QolipSetSnapshot, role_code,
};

pub(super) const COLUMNS: &str = "spec.item_code, spec.item_name, spec.item_group, spec.qolip_code, spec.size,
    COALESCE(spec.payload_json->>'warehouse', '') AS warehouse,
    COALESCE(spec.payload_json->>'color', '') AS color,
    COALESCE(spec.qolip_set_id, '') AS qolip_set_id,
    spec.created_by_role, spec.created_by_ref, spec.created_by_name";

async fn snapshot_identifiers(
    conn: &mut PgConnection,
    identifiers: &[String],
) -> Result<Option<QolipSetSnapshot>, QolipError> {
    let sets = sqlx::query_scalar::<_, String>(
        "SELECT sets.set_id FROM mini_qolip_sets sets
         WHERE (sets.set_id = ANY($1)
           OR EXISTS (SELECT 1 FROM mini_qolip_product_specs spec
                      WHERE spec.qolip_set_id = sets.set_id AND lower(spec.qolip_code) = ANY($2)))
           AND NOT sets.is_product_local
           AND EXISTS (SELECT 1 FROM mini_qolip_product_specs member WHERE member.qolip_set_id = sets.set_id)
         ORDER BY sets.set_id LIMIT 2",
    ).bind(identifiers).bind(identifiers.iter().map(|value| value.trim().to_lowercase()).collect::<Vec<_>>()).fetch_all(&mut *conn).await.map_err(|_| QolipError::StoreFailed)?;
    if sets.len() > 1 {
        return Err(QolipError::SetAmbiguous);
    }
    let Some(id) = sets.into_iter().next() else {
        return Ok(None);
    };
    let rows = sqlx::query_as::<_, QolipProductSpecRow>(&format!(
        "SELECT {COLUMNS} FROM mini_qolip_product_specs spec WHERE spec.qolip_set_id = $1 ORDER BY lower(spec.qolip_code)"
    )).bind(&id).fetch_all(&mut *conn).await.map_err(|_| QolipError::StoreFailed)?;
    let products = sqlx::query_as::<_, (String, String)>(
        "SELECT DISTINCT item.code, item.name FROM mini_items item
         WHERE EXISTS (SELECT 1 FROM mini_qolip_set_products link WHERE link.set_id = $1 AND link.item_code = item.code)
            OR EXISTS (SELECT 1 FROM mini_qolip_effective_product_molds effective
                JOIN mini_qolip_product_specs spec ON spec.qolip_code = effective.qolip_code
                WHERE effective.item_code = item.code AND spec.qolip_set_id = $1)
         ORDER BY item.code",
    ).bind(&id).fetch_all(&mut *conn).await.map_err(|_| QolipError::StoreFailed)?
        .into_iter().map(|(code, name)| QolipSetProduct { code, name }).collect();
    Ok(Some(QolipSetSnapshot::new(
        id,
        rows.into_iter().map(row_to_product_spec).collect(),
        products,
    )))
}

async fn snapshot(
    conn: &mut PgConnection,
    identifier: &str,
) -> Result<Option<QolipSetSnapshot>, QolipError> {
    snapshot_identifiers(conn, &[identifier.trim().to_string()]).await
}

pub(super) async fn resolve(
    pool: &PgPool,
    identifier: &str,
) -> Result<Option<QolipSetSnapshot>, QolipError> {
    let mut conn = pool.acquire().await.map_err(|_| QolipError::StoreFailed)?;
    if let Some(set) = snapshot(&mut conn, identifier).await? {
        return Ok(Some(set));
    }
    let candidates = crate::core::qolip::candidate_codes(identifier);
    if candidates.is_empty() {
        return Ok(None);
    }
    snapshot_identifiers(&mut conn, &candidates).await
}

pub(super) async fn linked_specs(
    pool: &PgPool,
    item_code: &str,
) -> Result<Vec<QolipProductSpec>, QolipError> {
    let rows = sqlx::query_as::<_, QolipProductSpecRow>(&format!(
        "SELECT {COLUMNS}, link.is_extension FROM mini_qolip_product_specs spec
         JOIN mini_qolip_effective_product_molds link ON link.qolip_code = spec.qolip_code
         JOIN mini_items item ON item.code = link.item_code
         WHERE link.item_code_key = lower(btrim($1))
           AND (lower(btrim(spec.item_code)) <> link.item_code_key OR lower(btrim(spec.item_group)) = lower(btrim(item.item_group)))
         ORDER BY lower(spec.qolip_code)"
    )).bind(item_code.trim()).fetch_all(pool).await.map_err(|_| QolipError::StoreFailed)?;
    Ok(rows.into_iter().map(row_to_product_spec).collect())
}

pub(super) async fn reuse(
    pool: &PgPool,
    set_id: &str,
    item_code: &str,
    fingerprint: &str,
    principal: &Principal,
    allowed: &[String],
) -> Result<QolipSetSnapshot, QolipError> {
    let mut tx = pool.begin().await.map_err(|_| QolipError::StoreFailed)?;
    super::product_transfer::lock_composition(&mut tx).await?;
    let item_exists = sqlx::query_scalar::<_, String>(
        "SELECT code FROM mini_items WHERE lower(btrim(code)) = lower(btrim($1)) FOR KEY SHARE"
    ).bind(item_code).fetch_optional(&mut *tx).await.map_err(|_| QolipError::StoreFailed)?;
    if item_exists.is_none() { return Err(QolipError::MissingItem); }
    // A short catalog operation: freeze membership and metadata until the confirmed link is saved.
    // This also prevents new members appearing between the fingerprint check and commit.
    sqlx::query("LOCK TABLE mini_qolip_product_specs IN SHARE MODE")
        .execute(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    let current = snapshot(&mut tx, set_id)
        .await?
        .ok_or(QolipError::SetChanged)?;
    if current.set_id != set_id || current.fingerprint != fingerprint {
        return Err(QolipError::SetChanged);
    }
    if current.members.is_empty()
        || current.members.iter().any(|spec| {
            spec.warehouse.trim().is_empty()
                || !allowed
                    .iter()
                    .any(|warehouse| warehouse.trim().eq_ignore_ascii_case(spec.warehouse.trim()))
        })
    {
        return Err(QolipError::AccessDenied);
    }
    super::product_transfer::reject_product_in_use(&mut tx, item_code).await?;
    let written = sqlx::query(
        "INSERT INTO mini_qolip_set_products(set_id, item_code, created_by_role, created_by_ref, created_by_name)
         SELECT $1, code, $3, $4, $5 FROM mini_items WHERE lower(btrim(code)) = lower(btrim($2))
         ON CONFLICT DO NOTHING",
    ).bind(set_id).bind(item_code).bind(role_code(&principal.role)).bind(&principal.ref_).bind(&principal.display_name)
        .execute(&mut *tx).await.map_err(|_| QolipError::StoreFailed)?;
    let linked = sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM mini_qolip_set_products WHERE set_id = $1 AND item_code_key = lower(btrim($2)))")
        .bind(set_id).bind(item_code).fetch_one(&mut *tx).await.map_err(|_| QolipError::StoreFailed)?;
    if !linked {
        return Err(QolipError::MissingItem);
    }
    // The user confirmed the complete physical set, including any members
    // previously excluded for this product. Other products keep their overrides.
    let restored = sqlx::query("DELETE FROM mini_qolip_product_overrides override
        USING mini_qolip_product_specs spec
        WHERE override.item_code_key = lower(btrim($1))
          AND override.qolip_code = spec.qolip_code AND spec.qolip_set_id = $2")
        .bind(item_code).bind(set_id).execute(&mut *tx).await.map_err(|_| QolipError::StoreFailed)?;
    if written.rows_affected() > 0 || restored.rows_affected() > 0 {
        sqlx::query("INSERT INTO mini_engine_events(event_id, domain, action, entity_id, actor_key, idempotency_key, payload_json)
            VALUES ($1, 'qolip', 'set_reuse', $2, $3, $1, $4)")
            .bind(format!("qolip-set-reuse:{:032x}", rand::random::<u128>())).bind(item_code)
            .bind(format!("{}:{}", role_code(&principal.role), principal.ref_))
            .bind(serde_json::json!({"set_id": set_id, "fingerprint": fingerprint, "members": current.members, "actor_name": principal.display_name}))
            .execute(&mut *tx).await.map_err(|_| QolipError::StoreFailed)?;
    }
    let result = snapshot(&mut tx, set_id)
        .await?
        .ok_or(QolipError::SetChanged)?;
    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
    Ok(result)
}
