use super::*;

impl PostgresPreparationStore {
    pub async fn create_child_warehouse(
        &self,
        actor: &Principal,
        input: PreparationWarehouseCreate,
    ) -> Result<Value, PreparationError> {
        let name = input.warehouse_name()?;
        let parent = input.parent_key()?;
        let mut tx = self.pool.begin().await?;
        lock_warehouses(&mut tx).await?;
        let parent = exclusive_warehouse(&mut tx, &actor.ref_, &parent).await?;
        ensure_unique_name(&mut tx, &name, "").await?;
        let id = format!("warehouse:preparation:{:032x}", rand::random::<u128>());
        sqlx::query(
            "INSERT INTO mini_warehouses(id, name, parent_warehouse, preparation_owner_ref, payload_json)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&id).bind(&name).bind(&parent).bind(&actor.ref_)
        .bind(json!({"warehouse":name, "parent_warehouse":parent, "is_group":false, "company":""}))
        .execute(&mut *tx).await.map_err(warehouse_write_error)?;
        sqlx::query(
            "INSERT INTO mini_warehouse_assignments
             (assignment_kind, warehouse, warehouse_name, principal_role, principal_ref, display_name, payload_json)
             VALUES ('warehouse', $1, $1, 'tayyorlov_masteri', $2, $3, $4)",
        )
        .bind(&name).bind(&actor.ref_).bind(&actor.display_name)
        .bind(json!({"assignment_kind":"warehouse", "warehouse":name, "warehouse_name":name,
            "principal_role":"tayyorlov_masteri", "principal_ref":actor.ref_, "display_name":actor.display_name}))
        .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(json!({"warehouse":name, "parent_warehouse":parent}))
    }

    pub async fn rename_child_warehouse(
        &self,
        owner: &str,
        input: PreparationWarehouseRename,
    ) -> Result<Value, PreparationError> {
        let name = PreparationWarehouseCreate::clean(&input.name)?;
        let mut tx = self.pool.begin().await?;
        lock_warehouse_contents(&mut tx).await?;
        let (id, old) = managed_warehouse(&mut tx, owner, &input.warehouse).await?;
        ensure_unique_name(&mut tx, &name, &id).await?;
        if name != old {
            // Qolip ownership is immutable and outside a master's warehouse
            // workflow. Fail closed if an administrator placed such data here.
            let has_qolip: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM mini_qolip_product_specs
                 WHERE lower(payload_json->>'warehouse') = lower($1))",
            )
            .bind(&old)
            .fetch_one(&mut *tx)
            .await?;
            if has_qolip {
                return Err(PreparationError::Conflict(
                    "Omborga Qolip biriktirilgan. Nomini o‘zgartirish uchun administratorga murojaat qiling",
                ));
            }
            // Stable IDs and stock/barcode identities never change. Name FKs
            // cascade to assignments/materials; update their compatibility mirrors too.
            sqlx::query("UPDATE mini_warehouses SET name=$2,
                payload_json=payload_json || jsonb_build_object('warehouse',$2::text), updated_at=now()
                WHERE id=$1")
                .bind(&id).bind(&name).execute(&mut *tx).await.map_err(warehouse_write_error)?;
            sqlx::query(
                "UPDATE mini_warehouses SET parent_warehouse=$2,
                payload_json=payload_json || jsonb_build_object('parent_warehouse',$2::text)
                WHERE lower(parent_warehouse)=lower($1)",
            )
            .bind(&old)
            .bind(&name)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE mini_warehouse_assignments SET warehouse=$1,
                payload_json=payload_json || jsonb_build_object('warehouse',$1::text,'warehouse_name',$1::text)
                WHERE assignment_kind='warehouse' AND warehouse_name=$1")
                .bind(&name).execute(&mut *tx).await?;
            for table in [
                "mini_raw_material_stock",
                "mini_finished_goods_stock",
                "mini_gscale_receipts",
                "mini_rps_batches",
                "mini_qolip_locations",
                "mini_qolip_cell_qrs",
                "mini_qolip_checkouts",
            ] {
                sqlx::query(&format!(
                    "UPDATE {table} SET warehouse=$2,
                    payload_json=payload_json || jsonb_build_object('warehouse',$2::text)
                    WHERE lower(warehouse)=lower($1)"
                ))
                .bind(&old)
                .bind(&name)
                .execute(&mut *tx)
                .await?;
            }
            for column in ["source_warehouse", "destination_warehouse"] {
                sqlx::query(&format!(
                    "UPDATE mini_inventory_transfers SET {column}=$2
                    WHERE lower({column})=lower($1)"
                ))
                .bind(&old)
                .bind(&name)
                .execute(&mut *tx)
                .await?;
            }
            for table in [
                "mini_qolip_locations",
                "mini_qolip_cell_qrs",
                "mini_qolip_checkouts",
            ] {
                sqlx::query(&format!(
                    "UPDATE {table} SET block=$2,
                    payload_json=payload_json || jsonb_build_object('block',$2::text)
                    WHERE lower(block)=lower($1)"
                ))
                .bind(&old)
                .bind(&name)
                .execute(&mut *tx)
                .await?;
            }
        }
        tx.commit().await?;
        Ok(json!({"warehouse":name}))
    }

    pub async fn delete_child_warehouse(
        &self,
        owner: &str,
        warehouse: &str,
    ) -> Result<Value, PreparationError> {
        let mut tx = self.pool.begin().await?;
        lock_warehouse_contents(&mut tx).await?;
        let (id, name) = managed_warehouse(&mut tx, owner, warehouse).await?;
        let children: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM mini_warehouses
            WHERE lower(parent_warehouse)=lower($1))",
        )
        .bind(&name)
        .fetch_one(&mut *tx)
        .await?;
        if children {
            return Err(PreparationError::WarehouseHasChildren);
        }
        let stock: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM mini_raw_material_stock WHERE lower(warehouse)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_finished_goods_stock WHERE lower(warehouse)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_qolip_locations WHERE lower(warehouse)=lower($1) OR lower(block)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_inventory_placements p
                JOIN mini_inventory_locations l ON l.id=p.physical_location_id WHERE l.warehouse_id=$2)",
        ).bind(&name).bind(&id).fetch_one(&mut *tx).await?;
        if stock {
            return Err(PreparationError::WarehouseNotEmpty);
        }
        let materials: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM mini_preparation_material_warehouse_scopes WHERE warehouse_id=$2)
             OR EXISTS(SELECT 1 FROM mini_preparation_materials WHERE lower(warehouse_name)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_qolip_product_specs WHERE lower(payload_json->>'warehouse')=lower($1))",
        ).bind(&name).bind(&id).fetch_one(&mut *tx).await?;
        if materials {
            return Err(PreparationError::WarehouseHasMaterials);
        }
        let used: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM mini_inventory_transfers
                WHERE source_warehouse_id=$2 OR destination_warehouse_id=$2
                   OR lower(source_warehouse)=lower($1) OR lower(destination_warehouse)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_rps_batches WHERE lower(warehouse)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_gscale_receipts WHERE lower(warehouse)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_qolip_checkouts WHERE lower(warehouse)=lower($1) OR lower(block)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_qolip_cell_qrs WHERE lower(warehouse)=lower($1) OR lower(block)=lower($1))
             OR EXISTS(SELECT 1 FROM mini_preparation_warehouse_history_names WHERE warehouse_id=$2)",
        ).bind(&name).bind(&id).fetch_one(&mut *tx).await?;
        if used {
            return Err(PreparationError::WarehouseInUse);
        }
        sqlx::query("DELETE FROM mini_warehouse_assignments WHERE assignment_kind='warehouse' AND warehouse_name=$1")
            .bind(&name).execute(&mut *tx).await?;
        // No stock, catalog or history is ever deleted by this endpoint.
        sqlx::query("DELETE FROM mini_warehouses WHERE id=$1")
            .bind(&id)
            .execute(&mut *tx)
            .await
            .map_err(warehouse_write_error)?;
        tx.commit().await?;
        Ok(json!({"warehouse":name, "deleted":true}))
    }
}

async fn lock_warehouses(tx: &mut Transaction<'_, Postgres>) -> Result<(), PreparationError> {
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut **tx)
        .await?;
    // Name references without FKs and absent assignments need predicate-level
    // protection too. All lifecycle operations take locks in this order.
    sqlx::query(
        "LOCK TABLE mini_warehouses, mini_warehouse_assignments IN SHARE ROW EXCLUSIVE MODE",
    )
    .execute(&mut **tx)
    .await
    .map_err(warehouse_write_error)?;
    Ok(())
}

async fn lock_warehouse_contents(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), PreparationError> {
    lock_warehouses(tx).await?;
    sqlx::query(
        "LOCK TABLE mini_items, mini_raw_material_stock, mini_finished_goods_stock,
        mini_inventory_locations, mini_inventory_placements, mini_inventory_transfers,
        mini_preparation_materials, mini_preparation_material_warehouse_scopes,
        mini_preparation_warehouse_history_names, mini_gscale_receipts, mini_rps_batches,
        mini_qolip_locations, mini_qolip_cell_qrs, mini_qolip_checkouts, mini_qolip_product_specs
        IN SHARE ROW EXCLUSIVE MODE",
    )
    .execute(&mut **tx)
    .await
    .map_err(warehouse_write_error)?;
    Ok(())
}

async fn managed_warehouse(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    name: &str,
) -> Result<(String, String), PreparationError> {
    let row: (String, String) = sqlx::query_as("SELECT id, name FROM mini_warehouses
        WHERE lower(name)=lower($1) AND preparation_owner_ref=$2 AND parent_warehouse<>'' AND NOT is_group
        FOR UPDATE")
        .bind(name.trim()).bind(owner).fetch_optional(&mut **tx).await?
        .ok_or(PreparationError::WarehouseNotOwned)?;
    exclusive_warehouse(tx, owner, &row.1)
        .await
        .map_err(|e| match e {
            PreparationError::WarehouseNotExclusive => PreparationError::WarehouseNotOwned,
            e => e,
        })?;
    Ok(row)
}

async fn ensure_unique_name(
    tx: &mut Transaction<'_, Postgres>,
    name: &str,
    own_id: &str,
) -> Result<(), PreparationError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM mini_warehouses
        WHERE lower(name)=lower($1) AND id<>$2)",
    )
    .bind(name)
    .bind(own_id)
    .fetch_one(&mut **tx)
    .await?;
    if exists {
        return Err(PreparationError::WarehouseNameTaken);
    }
    Ok(())
}

fn warehouse_write_error(error: sqlx::Error) -> PreparationError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("23505") => PreparationError::WarehouseNameTaken,
        Some("23503") => PreparationError::WarehouseInUse,
        Some("55P03" | "40P01") => PreparationError::Conflict(
            "Omborda boshqa amal bajarilmoqda. Birozdan keyin qayta urinib ko‘ring",
        ),
        _ => error.into(),
    }
}
