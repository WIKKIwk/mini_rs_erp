use super::*;
use crate::db::postgres_admin_catalog::{
    helpers::update_operational_item_projections, item_delete_safety::item_delete_blocker,
};

impl PostgresPreparationStore {
    pub async fn list_owned_materials(&self, owner: &str) -> Result<Value, PreparationError> {
        let rows: Vec<Value> = sqlx::query_scalar(
            "SELECT jsonb_build_object('item_code', i.code, 'name', i.name,
                'warehouses', COALESCE((SELECT jsonb_agg(w.name ORDER BY w.name)
                    FROM mini_preparation_material_warehouse_scopes s
                    JOIN mini_warehouses w ON w.id=s.warehouse_id
                    WHERE s.item_code=i.code AND s.active),
                    CASE WHEN m.warehouse_name IS NULL THEN '[]'::jsonb
                         ELSE jsonb_build_array(m.warehouse_name) END))
             FROM mini_preparation_materials m JOIN mini_items i ON i.code=m.item_code
             WHERE m.owner_ref=$1 ORDER BY lower(i.name), i.code",
        )
        .bind(owner)
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({"materials": rows}))
    }

    pub async fn rename_owned_material(
        &self,
        owner: &str,
        input: MaterialRename,
    ) -> Result<Value, PreparationError> {
        let name = input.name.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() || name.chars().count() > 160 {
            return Err(PreparationError::Invalid(
                "Homashyo nomi 1–160 ta belgidan iborat bo‘lishi kerak",
            ));
        }
        let mut tx = self.pool.begin().await?;
        lock_material_catalog(&mut tx).await?;
        let (code, _) = owned_material(&mut tx, owner, &input.item_code).await?;
        let duplicate: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM mini_preparation_materials
            WHERE owner_ref=$1 AND name_key=lower($2) AND item_code<>$3)",
        )
        .bind(owner)
        .bind(&name)
        .bind(&code)
        .fetch_one(&mut *tx)
        .await?;
        if duplicate {
            return Err(PreparationError::MaterialNameTaken);
        }
        sqlx::query("UPDATE mini_preparation_materials SET name_key=lower($2) WHERE item_code=$1")
            .bind(&code)
            .bind(&name)
            .execute(&mut *tx)
            .await
            .map_err(material_write_error)?;
        sqlx::query("UPDATE mini_items SET name=$2,
            payload_json=payload_json || jsonb_build_object('name',$2::text), updated_at=now() WHERE code=$1")
            .bind(&code).bind(&name).execute(&mut *tx).await?;
        update_operational_item_projections(&mut tx, &code, &code, &name)
            .await
            .map_err(|error| {
                tracing::error!(?error, "preparation material rename projections failed");
                PreparationError::StoreFailed
            })?;
        // Saved formula names are a mutable projection. Receipt/event history stays intact.
        sqlx::query("UPDATE mini_preparation_formulas f SET lines=(SELECT jsonb_agg(
                CASE WHEN line->>'item_code'=$1 THEN line || jsonb_build_object('name',$2::text)
                     ELSE line END ORDER BY ordinal)
                FROM jsonb_array_elements(f.lines) WITH ORDINALITY AS entries(line,ordinal)), updated_at=now()
            WHERE EXISTS(SELECT 1 FROM jsonb_array_elements(f.lines) line WHERE line->>'item_code'=$1)")
            .bind(&code).bind(&name).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(json!({"item_code":code, "name":name}))
    }

    pub async fn update_owned_material_warehouses(
        &self,
        owner: &str,
        input: MaterialWarehousesUpdate,
    ) -> Result<Value, PreparationError> {
        if input.warehouses.len() > 100 || input.warehouses.iter().any(|w| w.trim().is_empty()) {
            return Err(PreparationError::Invalid("Omborlar ro‘yxati noto‘g‘ri"));
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout='5s'")
            .execute(&mut *tx)
            .await?;
        // Same warehouse-before-material ordering as warehouse management.
        // Freeze assignments and legacy stock writers while changing visibility.
        sqlx::query("LOCK TABLE mini_warehouses, mini_warehouse_assignments IN SHARE MODE")
            .execute(&mut *tx)
            .await
            .map_err(material_write_error)?;
        // Drain FOR SHARE readers before they take their scope-check snapshot.
        // A row lock alone can let a waiting receipt retain an old scope result.
        sqlx::query("LOCK TABLE mini_items, mini_preparation_materials IN EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await
            .map_err(material_write_error)?;
        let (code, name) = owned_material(&mut tx, owner, &input.item_code).await?;
        sqlx::query(
            "LOCK TABLE mini_preparation_material_warehouse_scopes IN SHARE ROW EXCLUSIVE MODE",
        )
        .execute(&mut *tx)
        .await
        .map_err(material_write_error)?;
        sqlx::query("LOCK TABLE mini_raw_material_stock, mini_finished_goods_stock,
            mini_gscale_receipts, mini_inventory_transfers, mini_inventory_transfer_lines IN SHARE MODE")
            .execute(&mut *tx).await.map_err(material_write_error)?;
        let current: Vec<(String, String)> = sqlx::query_as(
            "SELECT w.id,w.name FROM mini_warehouses w
             WHERE EXISTS(SELECT 1 FROM mini_preparation_material_warehouse_scopes s
                 WHERE s.item_code=$1 AND s.warehouse_id=w.id AND s.active)
                OR (NOT EXISTS(SELECT 1 FROM mini_preparation_material_warehouse_scopes s
                        WHERE s.item_code=$1)
                    AND EXISTS(SELECT 1 FROM mini_preparation_materials m
                        WHERE m.item_code=$1 AND lower(m.warehouse_name)=lower(w.name)))",
        )
        .bind(&code)
        .fetch_all(&mut *tx)
        .await?;
        let mut selected = std::collections::BTreeMap::new();
        for requested in &input.warehouses {
            let warehouse = if let Some((_, name)) = current
                .iter()
                .find(|(_, name)| name.eq_ignore_ascii_case(requested.trim()))
            {
                name.clone()
            } else {
                exclusive_warehouse(&mut tx, owner, requested).await?
            };
            let id: String = sqlx::query_scalar("SELECT id FROM mini_warehouses WHERE name=$1")
                .bind(&warehouse)
                .fetch_one(&mut *tx)
                .await?;
            selected.insert(id, warehouse);
        }
        for (id, warehouse) in &current {
            if selected.contains_key(id) {
                continue;
            }
            // Existing administrator/shared bindings may be retained, never changed.
            exclusive_warehouse(&mut tx, owner, warehouse).await?;
        }
        let selected_ids: Vec<String> = selected.keys().cloned().collect();
        let current_ids: Vec<String> = current.iter().map(|(id, _)| id.clone()).collect();
        let busy: bool = sqlx::query_scalar(
            "WITH removed AS (
                SELECT w.id,w.name FROM mini_warehouses w WHERE NOT(w.id=ANY($2))
                AND (w.id=ANY($3) OR EXISTS(SELECT 1 FROM mini_preparation_materials m
                    WHERE m.item_code=$1 AND m.warehouse_name IS NULL
                    AND NOT EXISTS(SELECT 1 FROM mini_preparation_material_warehouse_scopes s WHERE s.item_code=$1)))
             )
             SELECT EXISTS(SELECT 1 FROM mini_raw_material_stock s JOIN removed w ON lower(s.warehouse)=lower(w.name)
                WHERE lower(s.item_code)=lower($1) AND s.qty>0 AND s.status NOT IN ('deleted','consumed'))
             OR EXISTS(SELECT 1 FROM mini_finished_goods_stock s JOIN removed w ON lower(s.warehouse)=lower(w.name)
                WHERE lower(s.item_code)=lower($1) AND s.qty>0 AND s.status<>'dispatched')
             OR EXISTS(SELECT 1 FROM mini_gscale_receipts r JOIN removed w ON lower(r.warehouse)=lower(w.name)
                WHERE lower(r.item_code)=lower($1) AND r.status='draft')
             OR EXISTS(SELECT 1 FROM mini_inventory_transfers t
                JOIN mini_inventory_transfer_lines l ON l.transfer_id=t.id
                JOIN removed w ON t.source_warehouse_id=w.id OR t.destination_warehouse_id=w.id
                WHERE lower(l.item_code)=lower($1) AND t.status IN ('requested','approved','in_transit'))",
        ).bind(&code).bind(&selected_ids).bind(&current_ids).fetch_one(&mut *tx).await?;
        if busy {
            return Err(PreparationError::MaterialWarehouseInUse);
        }
        // Retain inactive scope rows: deleting the last one would reactivate the
        // legacy unscoped/global fallback. No stock, formulas or history are moved.
        sqlx::query(
            "UPDATE mini_preparation_material_warehouse_scopes
            SET active=false,updated_at=now() WHERE item_code=$1 AND active",
        )
        .bind(&code)
        .execute(&mut *tx)
        .await?;
        for id in selected.keys() {
            sqlx::query(
                "INSERT INTO mini_preparation_material_warehouse_scopes
                (item_code,warehouse_id,scope_kind,created_by_role,created_by_ref)
                VALUES($1,$2,'exclusive','tayyorlov_masteri',$3)
                ON CONFLICT(item_code,warehouse_id) DO UPDATE SET active=true,updated_at=now()",
            )
            .bind(&code)
            .bind(id)
            .bind(owner)
            .execute(&mut *tx)
            .await?;
        }
        let mut warehouses: Vec<String> = selected.into_values().collect();
        warehouses.sort();
        sqlx::query("UPDATE mini_preparation_materials SET warehouse_name=$2 WHERE item_code=$1")
            .bind(&code)
            .bind(warehouses.first())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(json!({"item_code":code,"name":name,"warehouses":warehouses}))
    }

    pub async fn delete_owned_material(
        &self,
        owner: &str,
        item_code: &str,
    ) -> Result<Value, PreparationError> {
        let mut tx = self.pool.begin().await?;
        lock_material_catalog(&mut tx).await?;
        // Protect legacy text/JSON references without FKs from concurrent writes.
        let (code, name) = owned_material(&mut tx, owner, item_code).await?;
        sqlx::query(
            "LOCK TABLE mini_raw_material_stock, mini_finished_goods_stock,
            mini_gscale_receipts, mini_rps_batches, mini_qolip_locations, mini_qolip_checkouts,
            mini_qolip_product_specs, mini_raw_material_assignments,
            mini_inventory_transfer_lines, mini_orders, mini_order_products, mini_production_maps,
            mini_customer_items, mini_quick_order_templates, mini_preparation_formulas IN SHARE MODE",
        )
        .execute(&mut *tx)
        .await
        .map_err(material_write_error)?;
        let used: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM mini_raw_material_stock WHERE lower(item_code)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_finished_goods_stock WHERE lower(item_code)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_gscale_receipts WHERE lower(item_code)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_rps_batches WHERE lower(item_code)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_raw_material_events WHERE lower(item_code)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_inventory_transfer_lines WHERE lower(item_code)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_customer_items WHERE lower(item_code)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_qolip_locations WHERE lower(item_code)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_qolip_checkouts WHERE lower(item_code)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_raw_material_assignments WHERE lower(item_code)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_orders WHERE lower(product_code)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_order_products WHERE lower(item_code)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_production_maps WHERE lower(product_code)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_preparation_formulas f, jsonb_array_elements(f.lines) line
                WHERE lower(line->>'item_code')=lower($1))
             OR EXISTS(SELECT 1 FROM mini_preparation_operations WHERE kind<>'material'
                AND (lower(response_json->>'item_code')=lower($1)
                     OR response_json->'lines' @> jsonb_build_array(jsonb_build_object('item_code',$1::text))))",
        ).bind(&code).fetch_one(&mut *tx).await?;
        if used {
            return Err(PreparationError::MaterialInUse);
        }
        let blocker = item_delete_blocker(&mut tx, &code, &name)
            .await
            .map_err(|error| {
                tracing::error!(?error, "preparation material delete safety lookup failed");
                PreparationError::StoreFailed
            })?;
        if !blocker.is_empty() {
            return Err(PreparationError::MaterialInUse);
        }
        // A creation-only command is retained for idempotent retries, but its
        // warehouse display link must not prevent deleting an otherwise empty warehouse.
        sqlx::query(
            "DELETE FROM mini_preparation_warehouse_history_names link
            USING mini_preparation_operations operation
            WHERE operation.id=link.operation_id AND operation.kind='material'
              AND operation.owner_ref=$1 AND operation.response_json->>'item_code'=$2",
        )
        .bind(owner)
        .bind(&code)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM mini_preparation_materials WHERE item_code=$1 AND owner_ref=$2")
            .bind(&code)
            .bind(owner)
            .execute(&mut *tx)
            .await
            .map_err(material_write_error)?;
        sqlx::query("DELETE FROM mini_items WHERE code=$1")
            .bind(&code)
            .execute(&mut *tx)
            .await
            .map_err(material_write_error)?;
        tx.commit().await?;
        Ok(json!({"item_code":code, "deleted":true}))
    }
}

async fn lock_material_catalog(tx: &mut Transaction<'_, Postgres>) -> Result<(), PreparationError> {
    sqlx::query("SET LOCAL lock_timeout='5s'")
        .execute(&mut **tx)
        .await?;
    sqlx::query("LOCK TABLE mini_items, mini_preparation_materials IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut **tx)
        .await
        .map_err(material_write_error)?;
    Ok(())
}

async fn owned_material(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    code: &str,
) -> Result<(String, String), PreparationError> {
    sqlx::query_as("SELECT i.code,i.name FROM mini_preparation_materials m
        JOIN mini_items i ON i.code=m.item_code WHERE m.owner_ref=$1 AND m.item_code=$2 FOR UPDATE OF m,i")
        .bind(owner).bind(code.trim()).fetch_optional(&mut **tx).await?
        .ok_or(PreparationError::MaterialNotOwned)
}

fn material_write_error(error: sqlx::Error) -> PreparationError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("23505") => PreparationError::MaterialNameTaken,
        Some("23503") => PreparationError::MaterialInUse,
        Some("55P03" | "40P01") => {
            PreparationError::Conflict("Homashyo hozir band. Birozdan keyin qayta urinib ko‘ring")
        }
        _ => error.into(),
    }
}
