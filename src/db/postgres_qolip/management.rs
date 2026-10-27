use sqlx::{PgPool, Postgres, Transaction};
use std::collections::BTreeMap;

use super::product_transfer::{
    load_spec, lock_composition, reject_in_use, reject_product_in_use, write_override,
};
use super::rows::{QolipProductSpecRow, row_to_product_spec};
use crate::core::auth::models::Principal;
use crate::core::qolip::{
    QolipError, QolipManagedSet, QolipProductMoldEdits, QolipSetCreate, QolipSetEdit, QolipSetProduct,
    QolipSetRename, QolipSetSnapshot, role_code,
};

#[cfg(test)]
#[path = "set_names_tests.rs"]
mod set_names_tests;

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
        "SELECT sets.set_id, sets.warehouse_key,
             COALESCE(item.code, sets.item_code_key, ''), COALESCE(item.name, ''), COALESCE(item.item_group, ''),
             COALESCE(names.name, '')
         FROM mini_qolip_sets sets
         LEFT JOIN mini_items item ON lower(btrim(item.code)) = sets.item_code_key
         LEFT JOIN mini_qolip_set_names names ON names.set_id = sets.set_id
         WHERE NOT sets.is_product_local AND ($1::text[] IS NULL OR sets.warehouse_key = ANY($1))
           AND ($2::text IS NULL OR sets.set_id = $2)
         ORDER BY sets.set_id",
    ).bind(warehouses).bind(id).fetch_all(&mut **tx).await.map_err(super::reworks::storage_error)?;
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
    .map_err(super::reworks::storage_error)?;
    let mut by_set = BTreeMap::<String, Vec<_>>::new();
    for row in members {
        let spec = row_to_product_spec(row);
        by_set.entry(spec.set_id()).or_default().push(spec);
    }
    let products = sqlx::query_as::<_, (String, String, String)>(
        "SELECT link.set_id, item.code, item.name
         FROM mini_qolip_set_products link JOIN mini_items item ON item.code = link.item_code
         WHERE link.set_id = ANY($1)
         ORDER BY 1, 2",
    )
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(super::reworks::storage_error)?;
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
            |(id, warehouse, item_code, item_name, item_group, name)| {
                let members = by_set.remove(&id).unwrap_or_default();
                QolipManagedSet {
                    name,
                    warehouse: members
                        .first()
                        .map(|s| s.warehouse.clone())
                        .unwrap_or(warehouse),
                    snapshot: QolipSetSnapshot::new(
                        id.clone(),
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

pub(super) async fn rename_set(
    pool: &PgPool,
    input: &QolipSetRename,
    principal: &Principal,
    allowed: Option<&[String]>,
) -> Result<QolipManagedSet, QolipError> {
    let mut tx = pool.begin().await.map_err(super::reworks::storage_error)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("qolip:set-name:{}", input.set_id))
        .execute(&mut *tx).await.map_err(super::reworks::storage_error)?;
    let warehouses = allowed.map(|values| values.iter()
        .map(|value| value.trim().to_lowercase()).collect::<Vec<_>>());
    let id = sqlx::query_scalar::<_, String>(
        "SELECT set_id FROM mini_qolip_sets
         WHERE set_id = $1 AND NOT is_product_local
           AND ($2::text[] IS NULL OR warehouse_key = ANY($2))",
    ).bind(&input.set_id).bind(warehouses).fetch_optional(&mut *tx)
        .await.map_err(super::reworks::storage_error)?.ok_or(QolipError::SetChanged)?;
    let previous = read_sets(&mut tx, allowed, Some(&id)).await?
        .pop().ok_or(QolipError::SetChanged)?;
    sqlx::query(
        "INSERT INTO mini_qolip_set_names (set_id, name) VALUES ($1, $2)
         ON CONFLICT (set_id) DO UPDATE SET name = EXCLUDED.name, updated_at = now()",
    ).bind(&id).bind(&input.name).execute(&mut *tx).await.map_err(|error| {
        if error.as_database_error().and_then(|error| error.constraint())
            == Some("mini_qolip_set_names_name_unique") {
            QolipError::SetNameConflict
        } else {
            super::reworks::storage_error(error)
        }
    })?;
    audit(&mut tx, principal, "set_rename", &id, serde_json::json!({
        "before": previous.name, "after": input.name,
    })).await?;
    let updated = read_sets(&mut tx, allowed, Some(&id)).await?
        .pop().ok_or(QolipError::SetChanged)?;
    tx.commit().await.map_err(super::reworks::storage_error)?;
    Ok(updated)
}

pub(super) async fn list(
    pool: &PgPool,
    allowed: Option<&[String]>,
) -> Result<Vec<QolipManagedSet>, QolipError> {
    let mut tx = pool.begin().await.map_err(super::reworks::storage_error)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(super::reworks::storage_error)?;
    let result = read_sets(&mut tx, allowed, None)
        .await?
        .into_iter()
        .filter(|set| !set.snapshot.members.is_empty())
        .collect();
    tx.commit().await.map_err(super::reworks::storage_error)?;
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
            .map_err(super::reworks::storage_error)?;
    }
    for code in &codes {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1)::bigint)")
            .bind(code)
            .execute(&mut **tx)
            .await
            .map_err(super::reworks::storage_error)?;
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
    .map_err(super::reworks::storage_error)?;
    Ok(())
}

pub(super) async fn create_set(
    pool: &PgPool,
    input: &QolipSetCreate,
    principal: &Principal,
    allowed: &[String],
) -> Result<QolipManagedSet, QolipError> {
    owner(&input.warehouse, Some(allowed))?;
    let mut tx = pool.begin().await.map_err(super::reworks::storage_error)?;
    lock_composition(&mut tx).await?;
    let codes = input.members.iter().map(|draft| draft.qolip_code.clone()).collect::<Vec<_>>();
    lock_codes(&mut tx, &codes).await?;
    let existing_receipt: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM mini_qolip_locations WHERE lower(qolip_code) = ANY($1)
            UNION ALL SELECT 1 FROM mini_qolip_checkouts WHERE lower(qolip_code) = ANY($1)
        )"
    ).bind(codes.iter().map(|code| code.trim().to_lowercase()).collect::<Vec<_>>())
        .fetch_one(&mut *tx).await.map_err(super::reworks::storage_error)?;
    if existing_receipt {
        return Err(QolipError::QolipCodeConflict);
    }
    let set_id: String = loop {
        let candidate: String = sqlx::query_scalar("SELECT public.mini_qolip_next_set_id()")
            .fetch_one(&mut *tx).await.map_err(super::reworks::storage_error)?;
        if !codes.iter().any(|code| code.trim() == candidate) { break candidate; }
    };
    for draft in &input.members {
        let spec = draft.standalone_spec(&input.warehouse, &set_id, principal)?;
        super::catalog::save_product_spec_tx(&mut tx, spec).await?;
    }
    let created = read_sets(&mut tx, Some(allowed), Some(&set_id)).await?
        .pop().ok_or(QolipError::StoreFailed)?;
    audit(&mut tx, principal, "set_create", &set_id,
        serde_json::json!({"set": created, "actor_name": principal.display_name})).await?;
    tx.commit().await.map_err(super::reworks::storage_error)?;
    Ok(created)
}

pub(super) async fn edit_set(
    pool: &PgPool,
    input: &QolipSetEdit,
    principal: &Principal,
    allowed: Option<&[String]>,
) -> Result<QolipManagedSet, QolipError> {
    let mut tx = pool.begin().await.map_err(super::reworks::storage_error)?;
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
        let spec = if current.item_code.is_empty() {
            draft.standalone_spec(&current.warehouse, &input.set_id, principal)?
        } else {
            draft.spec(
                (&current.item_code, &current.item_name, &current.item_group),
                &current.warehouse,
                &input.set_id,
                principal,
            )?
        };
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
    tx.commit().await.map_err(super::reworks::storage_error)?;
    Ok(updated)
}

pub(super) async fn edit_products(
    pool: &PgPool,
    input: &QolipProductMoldEdits,
    principal: &Principal,
    allowed: Option<&[String]>,
) -> Result<(), QolipError> {
    let mut tx = pool.begin().await.map_err(super::reworks::storage_error)?;
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
        ).bind(&change.item_code).fetch_optional(&mut *tx).await.map_err(super::reworks::storage_error)?
            .ok_or(QolipError::MissingItem)?;
        reject_product_in_use(&mut tx, &item.0).await?;
        for draft in &change.add {
            let code = draft.qolip_code.trim().to_lowercase();
            let existing = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM mini_qolip_product_specs WHERE lower(qolip_code) = $1)"
            ).bind(&code).fetch_one(&mut *tx).await.map_err(super::reworks::storage_error)?;
            let spec = if existing {
                if !draft.existing_only {
                    return Err(QolipError::QolipCodeConflict);
                }
                load_spec(&mut tx, &code).await?
            } else {
                if draft.existing_only {
                    return Err(QolipError::QolipCodeNotFound);
                }
                owner(&change.warehouse, allowed)?;
                let set_id: String = sqlx::query_scalar("SELECT public.mini_qolip_next_set_id()")
                    .fetch_one(&mut *tx).await.map_err(super::reworks::storage_error)?;
                let spec = draft.spec(
                    (&item.0, &item.1, &item.2),
                    &change.warehouse,
                    &set_id,
                    principal,
                )?;
                sqlx::query(
                    "INSERT INTO mini_qolip_sets
                    (set_id, item_code_key, warehouse_key, is_product_local)
                    VALUES ($1, lower(btrim($2)), lower(btrim($3)), true)",
                )
                .bind(&set_id)
                .bind(&item.0)
                .bind(&change.warehouse)
                .execute(&mut *tx)
                .await
                .map_err(super::reworks::storage_error)?;
                super::catalog::save_product_spec_tx(&mut tx, spec).await?
            };
            owner(&spec.warehouse, allowed)?;
            if spec.item_code.eq_ignore_ascii_case(&item.0)
                && !spec.item_group.eq_ignore_ascii_case(&item.2)
            {
                return Err(QolipError::ProductTransferConflict);
            }
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
    tx.commit().await.map_err(super::reworks::storage_error)?;
    Ok(())
}
