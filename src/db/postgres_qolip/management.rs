use sqlx::{PgPool, Postgres, Transaction};
use std::collections::BTreeMap;

use super::product_transfer::{
    load_spec, lock_composition, reject_in_use, reject_product_in_use, write_override,
};
use super::rows::{QolipProductSpecRow, row_to_product_spec};
use crate::core::auth::models::Principal;
use crate::core::qolip::{
    QolipError, QolipManagedSet, QolipProductMoldEdits, QolipSetEdit, QolipSetProduct,
    QolipSetSnapshot, role_code,
};

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

async fn read_sets(
    tx: &mut Transaction<'_, Postgres>,
    allowed: Option<&[String]>,
    id: Option<&str>,
) -> Result<Vec<QolipManagedSet>, QolipError> {
    let warehouses = allowed.map(|values| {
        values
            .iter()
            .map(|s| s.trim().to_lowercase())
            .collect::<Vec<_>>()
    });
    let rows = sqlx::query_as::<_, (String, String, String, String, String, String)>(
        "SELECT sets.set_id, sets.display_code, sets.warehouse_key,
             COALESCE(item.code, sets.item_code_key), COALESCE(item.name, ''), COALESCE(item.item_group, '')
         FROM mini_qolip_sets sets
         LEFT JOIN mini_items item ON lower(btrim(item.code)) = sets.item_code_key
         WHERE NOT sets.is_product_local AND ($1::text[] IS NULL OR sets.warehouse_key = ANY($1))
           AND ($2::text IS NULL OR sets.set_id = $2)
         ORDER BY lower(sets.display_code), sets.set_id",
    ).bind(warehouses).bind(id).fetch_all(&mut **tx).await.map_err(|_| QolipError::StoreFailed)?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let ids = rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>();
    let members = sqlx::query_as::<_, QolipProductSpecRow>(&format!(
        "SELECT {} FROM mini_qolip_product_specs spec WHERE spec.qolip_set_id = ANY($1)
         ORDER BY lower(spec.qolip_code)",
        super::shared_sets::COLUMNS,
    ))
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    let mut by_set = BTreeMap::<String, Vec<_>>::new();
    for row in members {
        let spec = row_to_product_spec(row);
        by_set.entry(spec.set_id()).or_default().push(spec);
    }
    let products = sqlx::query_as::<_, (String, String, String)>(
        "SELECT link.set_id, item.code, item.name
         FROM mini_qolip_set_products link JOIN mini_items item ON item.code = link.item_code
         WHERE link.set_id = ANY($1)
         UNION
         SELECT spec.qolip_set_id, item.code, item.name
         FROM mini_qolip_effective_product_molds effective
         JOIN mini_qolip_product_specs spec ON spec.qolip_code = effective.qolip_code
         JOIN mini_items item ON item.code = effective.item_code
         WHERE spec.qolip_set_id = ANY($1)
         ORDER BY 1, 2",
    )
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    let mut links = BTreeMap::<String, Vec<_>>::new();
    for (id, code, name) in products {
        links
            .entry(id)
            .or_default()
            .push(QolipSetProduct { code, name });
    }
    Ok(rows
        .into_iter()
        .map(
            |(id, display, warehouse, item_code, item_name, item_group)| {
                let members = by_set.remove(&id).unwrap_or_default();
                QolipManagedSet {
                    warehouse: members
                        .first()
                        .map(|s| s.warehouse.clone())
                        .unwrap_or(warehouse),
                    snapshot: QolipSetSnapshot::new(
                        id.clone(),
                        display,
                        members,
                        links.remove(&id).unwrap_or_default(),
                    ),
                    item_code,
                    item_name,
                    item_group,
                }
            },
        )
        .collect())
}

pub(super) async fn list(
    pool: &PgPool,
    allowed: Option<&[String]>,
) -> Result<Vec<QolipManagedSet>, QolipError> {
    let mut tx = pool.begin().await.map_err(|_| QolipError::StoreFailed)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(|_| QolipError::StoreFailed)?;
    let result = read_sets(&mut tx, allowed, None).await?;
    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
    Ok(result)
}

async fn lock_codes(
    tx: &mut Transaction<'_, Postgres>,
    codes: &[String],
) -> Result<(), QolipError> {
    let mut codes = codes
        .iter()
        .map(|code| code.trim().to_lowercase())
        .collect::<Vec<_>>();
    codes.sort();
    codes.dedup();
    for code in &codes {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("qolip:{code}"))
            .execute(&mut **tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
    }
    for code in &codes {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1)::bigint)")
            .bind(code)
            .execute(&mut **tx)
            .await
            .map_err(|_| QolipError::StoreFailed)?;
        reject_in_use(tx, code).await?;
    }
    Ok(())
}

async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    principal: &Principal,
    action: &str,
    entity: &str,
    payload: serde_json::Value,
) -> Result<(), QolipError> {
    let id = format!("qolip-management:{:032x}", rand::random::<u128>());
    sqlx::query(
        "INSERT INTO mini_engine_events
        (event_id, domain, action, entity_id, actor_key, idempotency_key, payload_json)
        VALUES ($1, 'qolip', $2, $3, $4, $1, $5)",
    )
    .bind(id)
    .bind(action)
    .bind(entity)
    .bind(format!("{}:{}", role_code(&principal.role), principal.ref_))
    .bind(payload)
    .execute(&mut **tx)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    Ok(())
}

pub(super) async fn edit_set(
    pool: &PgPool,
    input: &QolipSetEdit,
    principal: &Principal,
    allowed: Option<&[String]>,
) -> Result<QolipManagedSet, QolipError> {
    let mut tx = pool.begin().await.map_err(|_| QolipError::StoreFailed)?;
    lock_composition(&mut tx).await?;
    let current = read_sets(&mut tx, allowed, Some(&input.set_id))
        .await?
        .pop()
        .ok_or(QolipError::SetChanged)?;
    if current.snapshot.fingerprint != input.fingerprint {
        return Err(QolipError::SetChanged);
    }
    owner(&current.warehouse, allowed)?;
    for product in &current.snapshot.products {
        reject_product_in_use(&mut tx, &product.code).await?;
    }
    // Lock every current member as an active session may use another member of this set.
    let codes = current
        .snapshot
        .members
        .iter()
        .map(|s| s.qolip_code.clone())
        .chain(input.add.iter().map(|s| s.qolip_code.clone()))
        .collect::<Vec<_>>();
    lock_codes(&mut tx, &codes).await?;
    for code in &input.remove {
        if !current
            .snapshot
            .members
            .iter()
            .any(|s| s.qolip_code.eq_ignore_ascii_case(code.trim()))
        {
            return Err(QolipError::SetChanged);
        }
    }
    for draft in &input.add {
        let spec = draft.spec(
            (&current.item_code, &current.item_name, &current.item_group),
            &current.warehouse,
            &input.set_id,
            principal,
        )?;
        super::catalog::save_product_spec_tx(&mut tx, spec).await?;
    }
    if !input.remove.is_empty() {
        super::catalog::delete_product_specs_tx(&mut tx, &input.remove).await?;
    }
    let updated = read_sets(&mut tx, allowed, Some(&input.set_id))
        .await?
        .pop()
        .ok_or(QolipError::SetChanged)?;
    audit(
        &mut tx,
        principal,
        "set_edit",
        &input.set_id,
        serde_json::json!({
            "before": current, "after": updated, "actor_name": principal.display_name,
        }),
    )
    .await?;
    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
    Ok(updated)
}

pub(super) async fn edit_products(
    pool: &PgPool,
    input: &QolipProductMoldEdits,
    principal: &Principal,
    allowed: Option<&[String]>,
) -> Result<(), QolipError> {
    let mut tx = pool.begin().await.map_err(|_| QolipError::StoreFailed)?;
    lock_composition(&mut tx).await?;
    let codes = input
        .changes
        .iter()
        .flat_map(|c| {
            c.add
                .iter()
                .map(|s| s.qolip_code.clone())
                .chain(c.remove.iter().cloned())
        })
        .collect::<Vec<_>>();
    lock_codes(&mut tx, &codes).await?;
    for change in &input.changes {
        let item = sqlx::query_as::<_, (String, String, String)>(
            "SELECT code, name, item_group FROM mini_items WHERE lower(btrim(code)) = lower(btrim($1)) FOR UPDATE",
        ).bind(&change.item_code).fetch_optional(&mut *tx).await.map_err(|_| QolipError::StoreFailed)?
            .ok_or(QolipError::MissingItem)?;
        reject_product_in_use(&mut tx, &item.0).await?;
        for draft in &change.add {
            let code = draft.qolip_code.trim().to_lowercase();
            let existing = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM mini_qolip_product_specs WHERE lower(qolip_code) = $1)"
            ).bind(&code).fetch_one(&mut *tx).await.map_err(|_| QolipError::StoreFailed)?;
            let spec = if existing {
                load_spec(&mut tx, &code).await?
            } else {
                owner(&change.warehouse, allowed)?;
                let set_id = format!("qolip-local:{:032x}", rand::random::<u128>());
                let spec = draft.spec(
                    (&item.0, &item.1, &item.2),
                    &change.warehouse,
                    &set_id,
                    principal,
                )?;
                sqlx::query(
                    "INSERT INTO mini_qolip_sets
                    (set_id, item_code_key, warehouse_key, display_code, is_product_local)
                    VALUES ($1, lower(btrim($2)), lower(btrim($3)), $4, true)",
                )
                .bind(&set_id)
                .bind(&item.0)
                .bind(&change.warehouse)
                .bind(&spec.qolip_code)
                .execute(&mut *tx)
                .await
                .map_err(|_| QolipError::StoreFailed)?;
                super::catalog::save_product_spec_tx(&mut tx, spec).await?
            };
            owner(&spec.warehouse, allowed)?;
            write_override(&mut tx, &item.0, &spec.qolip_code, "include", principal).await?;
        }
        for code in &change.remove {
            let spec = load_spec(&mut tx, &code.trim().to_lowercase()).await?;
            owner(&spec.warehouse, allowed)?;
            // An exclusion is idempotent and never deletes the physical mold.
            write_override(&mut tx, &item.0, &spec.qolip_code, "exclude", principal).await?;
        }
        audit(
            &mut tx,
            principal,
            "product_molds_edit",
            &item.0,
            serde_json::json!({
                "added": change.add.iter().map(|d| &d.qolip_code).collect::<Vec<_>>(),
                "removed": change.remove, "actor_name": principal.display_name,
            }),
        )
        .await?;
    }
    tx.commit().await.map_err(|_| QolipError::StoreFailed)?;
    Ok(())
}
