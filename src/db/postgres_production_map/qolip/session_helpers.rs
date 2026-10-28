use sqlx::{Postgres, Transaction};

use crate::core::production_map::{OrderRunSession, OrderRunStatus, ProductionMapError};

#[cfg(test)]
#[path = "automatic_start_tests.rs"]
mod automatic_start_tests;

// Downstream WIP sessions retain mold identity as production history. Only a
// session that claims the physical tooling participates in resource validation.
fn session_owns_qolip_lock(session: &OrderRunSession) -> bool {
    session
        .payload_json
        .get("qolip_lock_owner")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn automatic_resume_accepts_catalog_molds_and_requires_owned_physical_checkouts() {
        let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("test PostgreSQL");
        let mut tx = pool.begin().await.unwrap();
        // Temporary tables shadow production names on this connection only.
        sqlx::query(
            "CREATE TEMP TABLE mini_qolip_checkouts (
            qolip_code text, status text, issued_to_ref text) ON COMMIT DROP",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TEMP TABLE mini_qolip_locations (
            qolip_code text, quantity integer) ON COMMIT DROP",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        let mut session = OrderRunSession {
            session_id: "resume-validation".into(),
            apparatus: "apparatus:default:bosma_7".into(),
            order_id: "order-resume-validation".into(),
            stage_node_id: "print".into(),
            status: OrderRunStatus::Active,
            worker_role: "aparatchi".into(),
            worker_ref: "worker-1".into(),
            worker_display_name: "Worker".into(),
            started_at_unix: 1,
            updated_at_unix: 2,
            payload_json: serde_json::json!({"qolip_codes": ["CATALOG"]}),
        };
        validate_reacquired_qolip_checkouts_tx(&mut tx, &session)
            .await
            .unwrap();
        session.payload_json["qolip_codes"] = serde_json::json!(["PHYSICAL"]);
        sqlx::query("INSERT INTO mini_qolip_locations VALUES ('PHYSICAL', 1)")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            validate_reacquired_qolip_checkouts_tx(&mut tx, &session).await,
            Err(ProductionMapError::QolipCodeMismatch)
        );
        sqlx::query("DELETE FROM mini_qolip_locations")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO mini_qolip_checkouts VALUES ('PHYSICAL', 'open', 'worker-2')")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            validate_reacquired_qolip_checkouts_tx(&mut tx, &session).await,
            Err(ProductionMapError::QolipCodeMismatch)
        );
        sqlx::query("UPDATE mini_qolip_checkouts SET issued_to_ref = 'worker-1'")
            .execute(&mut *tx)
            .await
            .unwrap();
        session.payload_json["qolip_codes"] = serde_json::json!(["PHYSICAL", "CATALOG"]);
        validate_reacquired_qolip_checkouts_tx(&mut tx, &session)
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        pool.close().await;
    }
}

pub(super) async fn validate_reacquired_qolip_checkouts_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &OrderRunSession,
) -> Result<(), ProductionMapError> {
    let codes = session
        .payload_json
        .get("qolip_codes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str);
    for code in codes {
        // Start also accepts catalog-only molds. Resume must keep that policy
        // while rejecting physical stock or checkouts held by another worker.
        let issued_or_catalog_only: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM mini_qolip_checkouts
             WHERE lower(qolip_code) = lower($1) AND lower(status) = 'open'
               AND lower(issued_to_ref) = lower($2))
             OR (NOT EXISTS(SELECT 1 FROM mini_qolip_checkouts
                 WHERE lower(qolip_code) = lower($1) AND lower(status) = 'open')
                 AND NOT EXISTS(SELECT 1 FROM mini_qolip_locations
                 WHERE lower(qolip_code) = lower($1) AND quantity > 0))",
        )
        .bind(code.trim())
        .bind(session.worker_ref.trim())
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if !issued_or_catalog_only {
            return Err(ProductionMapError::QolipCodeMismatch);
        }
    }
    Ok(())
}

pub(super) async fn reject_qolip_in_use_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &OrderRunSession,
) -> Result<(), ProductionMapError> {
    if !session_owns_qolip_lock(session) {
        return Ok(());
    }
    if !matches!(
        session.status,
        OrderRunStatus::Active
            | OrderRunStatus::Paused
            | OrderRunStatus::Frozen
            | OrderRunStatus::RollDetached
    ) {
        return Ok(());
    }
    let mut qolip_codes = session
        .payload_json
        .get("qolip_codes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if let Some(qolip_code) = session
        .payload_json
        .get("qolip_code")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && !qolip_codes
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(qolip_code))
    {
        qolip_codes.push(qolip_code.to_string());
    }
    qolip_codes.sort_by_key(|code| code.to_ascii_lowercase());
    qolip_codes.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    if qolip_codes.is_empty() {
        return Ok(());
    }
    for qolip_code in qolip_codes {
        let lock_key = format!("qolip:{}", qolip_code.to_ascii_lowercase());
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(lock_key)
            .execute(&mut **tx)
            .await
            .map_err(|_| ProductionMapError::StoreFailed)?;
        let in_rework: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM mini_qolip_reworks
             WHERE qolip_code_key = lower(btrim($1)) AND returned_at IS NULL)",
        )
        .bind(&qolip_code)
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if in_rework {
            return Err(ProductionMapError::QolipInRework);
        }
        let already_in_use = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1
                FROM mini_order_run_sessions AS session
                WHERE session.status IN ('active', 'paused', 'frozen', 'roll_detached')
                  AND session.session_id <> $2
                  AND session.payload_json->>'qolip_lock_owner' = 'true'
                  AND (
                    lower(session.payload_json->>'qolip_code') = lower($1)
                    OR EXISTS (
                        SELECT 1
                        FROM jsonb_array_elements_text(
                            CASE
                                WHEN jsonb_typeof(session.payload_json->'qolip_codes') = 'array'
                                THEN session.payload_json->'qolip_codes'
                                ELSE '[]'::jsonb
                            END
                        ) AS code(value)
                        WHERE lower(code.value) = lower($1)
                    )
                  )
             )",
        )
        .bind(&qolip_code)
        .bind(session.session_id.trim())
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| ProductionMapError::StoreFailed)?;
        if already_in_use {
            return Err(ProductionMapError::QolipAlreadyInUse);
        }
    }
    Ok(())
}

pub(super) async fn validate_qolip_set_tx(
    tx: &mut Transaction<'_, Postgres>,
    session: &OrderRunSession,
) -> Result<(), ProductionMapError> {
    validate_qolip_set_with_confirmation_tx(tx, session, false).await
}

pub(super) async fn validate_automatic_qolip_set_tx(
    tx: &mut Transaction<'_, Postgres>, session: &OrderRunSession,
) -> Result<(), ProductionMapError> {
    validate_qolip_set_with_confirmation_tx(tx, session, true).await
}

async fn validate_qolip_set_with_confirmation_tx(
    tx: &mut Transaction<'_, Postgres>, session: &OrderRunSession, automatic_start: bool,
) -> Result<(), ProductionMapError> {
    if !session_owns_qolip_lock(session) {
        return Ok(());
    }
    let Some(set_id) = session.payload_json.get("qolip_set_id")
        .and_then(serde_json::Value::as_str).filter(|id| !id.is_empty()) else {
        return Ok(());
    };
    let item_code = sqlx::query_scalar::<_, String>(
        "SELECT product_code FROM mini_production_maps WHERE id = $1",
    ).bind(&session.order_id).fetch_one(&mut **tx).await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let mut rows = sqlx::query_as::<_, (String, String, String, i32, String, bool)>(
        "SELECT spec.qolip_code, COALESCE(spec.qolip_set_id, ''),
                COALESCE(spec.payload_json->>'warehouse', ''), spec.size,
                COALESCE(spec.payload_json->>'color', ''), effective.is_extension
         FROM mini_qolip_product_specs spec
         JOIN mini_qolip_effective_product_molds effective ON effective.qolip_code = spec.qolip_code
         WHERE effective.item_code_key = lower(btrim($1))
         ORDER BY lower(spec.qolip_code) FOR SHARE OF spec",
    ).bind(&item_code).fetch_all(&mut **tx).await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let legacy = sqlx::query_as::<_, (String, String, String, i32, String, bool)>(
        "SELECT l.qolip_code, ''::text,
                COALESCE(mini_qolip_assigned_warehouse(l.created_by_role, l.created_by_ref), ''), l.size, COALESCE(l.payload_json->>'color', ''), false
         FROM mini_qolip_locations l WHERE lower(l.item_code) = lower($1)
           AND NOT EXISTS (SELECT 1 FROM mini_qolip_product_specs s WHERE lower(s.qolip_code) = lower(l.qolip_code))
         UNION ALL
         SELECT c.qolip_code, ''::text,
                COALESCE(mini_qolip_assigned_warehouse(c.issued_by_role, c.issued_by_ref), ''), c.size, COALESCE(c.payload_json->>'color', ''), false
         FROM mini_qolip_checkouts c WHERE lower(c.item_code) = lower($1) AND c.status = 'open'
           AND NOT EXISTS (SELECT 1 FROM mini_qolip_product_specs s WHERE lower(s.qolip_code) = lower(c.qolip_code))",
    ).bind(&item_code).fetch_all(&mut **tx).await
        .map_err(|_| ProductionMapError::StoreFailed)?;
    rows.extend(legacy);
    if automatic_start {
        let raw = session.payload_json.get("qolip_confirmation").ok_or(ProductionMapError::QolipSetChanged)?;
        let confirmation: crate::core::production_map::AutomaticQolipConfirmation = serde_json::from_value(raw.clone())
            .map_err(|_| ProductionMapError::QolipSetChanged)?;
        let selected = rows.iter().map(|(code, id, warehouse, size, color, is_extension)| {
            crate::core::qolip::QolipProductSpec {
                qolip_code: code.clone(), qolip_set_id:id.clone(), item_code:item_code.clone(),
                warehouse:warehouse.clone(), size:*size, color:color.clone(), is_extension:*is_extension,
                ..Default::default()
            }
        }).filter(|spec| spec.is_extension || spec.set_id() == set_id).collect::<Vec<_>>();
        if confirmation.set_id != set_id
            || confirmation.fingerprint != crate::core::production_map::automatic_qolip_set_fingerprint(set_id, &selected) {
            return Err(ProductionMapError::QolipSetChanged);
        }
        validate_automatic_common_set_tx(tx, session, &item_code, set_id).await?;
    }
    let mut has_base = false;
    let mut selected_base = false;
    let required = rows.into_iter().filter_map(|(code, id, warehouse, size, color, is_extension)| {
        let spec = crate::core::qolip::QolipProductSpec {
            qolip_set_id: id, item_code: item_code.clone(), warehouse,
            ..Default::default()
        };
        has_base |= !is_extension;
        selected_base |= !is_extension && spec.set_id() == set_id;
        (is_extension || spec.set_id() == set_id).then(|| (code.trim().to_lowercase(), (size, color)))
    }).collect::<std::collections::BTreeMap<_, _>>();
    let scanned = session.payload_json.get("qolip_codes")
        .and_then(serde_json::Value::as_array).into_iter().flatten()
        .filter_map(serde_json::Value::as_str)
        .map(|code| code.trim().to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(members) = session.payload_json.get("qolip_set_snapshot").and_then(|value| value.get("members")).and_then(serde_json::Value::as_array) {
        let expected = members.iter().filter_map(|member| Some((
            member.get("qolip_code")?.as_str()?.trim().to_lowercase(),
            (i32::try_from(member.get("size")?.as_i64()?).ok()?, member.get("color")?.as_str()?.to_string())
        ))).collect::<std::collections::BTreeMap<_, _>>();
        if expected.len() != members.len() || expected != required { return Err(ProductionMapError::QolipCodeMismatch); }
    }
    if required.is_empty() || (has_base && !selected_base)
        || required.keys().cloned().collect::<std::collections::BTreeSet<_>>() != scanned {
        return Err(ProductionMapError::QolipCodeMismatch);
    }
    Ok(())
}

/// Product associations can change between opening a sheet and committing a
/// start. Validate every product while the caller holds the composition lock.
async fn validate_automatic_common_set_tx(
    tx: &mut Transaction<'_, Postgres>, session: &OrderRunSession, primary: &str, selected_set: &str,
) -> Result<(), ProductionMapError> {
    let map: serde_json::Value = sqlx::query_scalar("SELECT map_json FROM mini_production_maps WHERE id=$1")
        .bind(&session.order_id).fetch_one(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    if combo_primary_revision_tx(tx, &map, primary).await? {
        return Ok(());
    }
    let products = map.get("products").and_then(serde_json::Value::as_array).into_iter().flatten()
        .filter_map(|product| product.get("item_code").and_then(serde_json::Value::as_str))
        .map(|code| code.trim().to_lowercase()).filter(|code| !code.is_empty() && !code.eq_ignore_ascii_case(primary))
        .collect::<std::collections::BTreeSet<_>>();
    let expected = session.payload_json.get("qolip_codes").and_then(serde_json::Value::as_array).into_iter().flatten()
        .filter_map(serde_json::Value::as_str).map(|code| code.trim().to_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    for product in products {
        let rows = sqlx::query_as::<_, (String,String,bool)>(
            "SELECT spec.qolip_code, COALESCE(spec.qolip_set_id,''), effective.is_extension
             FROM mini_qolip_product_specs spec JOIN mini_qolip_effective_product_molds effective ON effective.qolip_code=spec.qolip_code
             WHERE effective.item_code_key=$1 ORDER BY lower(spec.qolip_code) FOR SHARE OF spec",
        ).bind(&product).fetch_all(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
        let has_base = rows.iter().any(|(_,_,extension)| !extension);
        let selected_base = rows.iter().any(|(_,set,extension)| !extension && set == selected_set);
        let codes = rows.into_iter().filter(|(_,set,extension)| *extension || set == selected_set)
            .map(|(code,_,_)| code.trim().to_lowercase()).collect::<std::collections::BTreeSet<_>>();
        if codes != expected || (has_base && !selected_base) { return Err(ProductionMapError::QolipSetChanged); }
    }
    Ok(())
}

/// Only a canonical immutable combo revision may replace the legacy common-set
/// policy. A JSON flag or a forged parent SKU is never enough to skip children.
pub(super) async fn combo_primary_revision_tx(
    tx: &mut Transaction<'_, Postgres>, map: &serde_json::Value, primary: &str,
) -> Result<bool, ProductionMapError> {
    let Some(value) = map.get("combo").filter(|value| !value.is_null()) else { return Ok(false); };
    let reference: crate::core::combo::ComboReference = serde_json::from_value(value.clone())
        .map_err(|_| ProductionMapError::QolipCodeMismatch)?;
    if reference.item_code != primary || reference.revision < 1
        || map.get("products").and_then(serde_json::Value::as_array).is_none_or(Vec::is_empty) {
        return Err(ProductionMapError::QolipCodeMismatch);
    }
    let canonical = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT reference_json FROM mini_combo_revisions WHERE item_code=$1",
    ).bind(primary).fetch_optional(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)?;
    if canonical != Some(serde_json::to_value(reference).map_err(|_| ProductionMapError::QolipCodeMismatch)?) {
        return Err(ProductionMapError::QolipCodeMismatch);
    }
    Ok(true)
}
