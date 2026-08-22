
async fn ensure_transfer_assets_tx(
    tx: &mut Transaction<'_, Postgres>,
    transfer_id: &str,
    lines: &[InventoryTransferLineRow],
) -> Result<(), InventoryMovementError> {
    for line in lines {
        let kind = InventoryAssetKind::parse(&line.asset_kind)?;
        let asset = lock_asset_tx(tx, kind, &line.asset_ref).await?;
        if asset.transfer_id != transfer_id
            || asset.qty_units != line.qty_units
            || matches!(
                asset.status.as_str(),
                "available" | "consumed" | "dispatched"
            )
        {
            return Err(InventoryMovementError::AssetUnavailable);
        }
    }
    Ok(())
}

async fn reserve_asset_tx(
    tx: &mut Transaction<'_, Postgres>,
    asset: &AssetLockRow,
    transfer_id: &str,
) -> Result<(), InventoryMovementError> {
    let result = match InventoryAssetKind::parse(&asset.asset_kind)? {
        InventoryAssetKind::RawMaterial => {
            sqlx::query(
                r#"
                UPDATE mini_raw_material_stock
                SET status = 'reserved',
                    payload_json = jsonb_set(
                        COALESCE(payload_json, '{}'::jsonb),
                        '{inventory_transfer_id}',
                        to_jsonb($2::text),
                        true
                    ),
                    updated_at = now()
                WHERE id = $1
                  AND status = 'available'
                  AND btrim(COALESCE(payload_json->>'inventory_transfer_id', '')) = ''
                "#,
            )
            .bind(&asset.asset_ref)
            .bind(transfer_id)
            .execute(&mut **tx)
            .await
        }
        InventoryAssetKind::FinishedGoods => {
            sqlx::query(
                r#"
                UPDATE mini_finished_goods_stock
                SET status = 'transfer_reserved',
                    payload_json = jsonb_set(
                        COALESCE(payload_json, '{}'::jsonb),
                        '{inventory_transfer_id}',
                        to_jsonb($2::text),
                        true
                    ),
                    updated_at = now()
                WHERE id = $1
                  AND status = 'available'
                  AND btrim(COALESCE(payload_json->>'inventory_transfer_id', '')) = ''
                "#,
            )
            .bind(&asset.asset_ref)
            .bind(transfer_id)
            .execute(&mut **tx)
            .await
        }
        InventoryAssetKind::Qolip => {
            sqlx::query(
                r#"
                UPDATE mini_qolip_locations
                SET inventory_transfer_id = $2, updated_at = now()
                WHERE id = $1 AND btrim(inventory_transfer_id) = ''
                "#,
            )
            .bind(&asset.asset_ref)
            .bind(transfer_id)
            .execute(&mut **tx)
            .await
        }
    }
    .map_err(store_error)?;
    if result.rows_affected() != 1 {
        return Err(InventoryMovementError::AssetUnavailable);
    }
    Ok(())
}

async fn release_transfer_assets_tx(
    tx: &mut Transaction<'_, Postgres>,
    transfer_id: &str,
    lines: &[InventoryTransferLineRow],
) -> Result<(), InventoryMovementError> {
    for line in lines {
        let result = match InventoryAssetKind::parse(&line.asset_kind)? {
            InventoryAssetKind::RawMaterial => {
                sqlx::query(
                    r#"
                    UPDATE mini_raw_material_stock
                    SET status = 'available',
                        payload_json = COALESCE(payload_json, '{}'::jsonb)
                            - 'inventory_transfer_id',
                        updated_at = now()
                    WHERE id = $1
                      AND payload_json->>'inventory_transfer_id' = $2
                      AND status = 'reserved'
                    "#,
                )
                .bind(&line.asset_ref)
                .bind(transfer_id)
                .execute(&mut **tx)
                .await
            }
            InventoryAssetKind::FinishedGoods => {
                sqlx::query(
                    r#"
                    UPDATE mini_finished_goods_stock
                    SET status = 'available',
                        payload_json = COALESCE(payload_json, '{}'::jsonb)
                            - 'inventory_transfer_id',
                        updated_at = now()
                    WHERE id = $1
                      AND payload_json->>'inventory_transfer_id' = $2
                      AND status = 'transfer_reserved'
                    "#,
                )
                .bind(&line.asset_ref)
                .bind(transfer_id)
                .execute(&mut **tx)
                .await
            }
            InventoryAssetKind::Qolip => {
                sqlx::query(
                    r#"
                    UPDATE mini_qolip_locations
                    SET inventory_transfer_id = '', updated_at = now()
                    WHERE id = $1 AND inventory_transfer_id = $2
                    "#,
                )
                .bind(&line.asset_ref)
                .bind(transfer_id)
                .execute(&mut **tx)
                .await
            }
        }
        .map_err(store_error)?;
        if result.rows_affected() != 1 {
            return Err(InventoryMovementError::AssetUnavailable);
        }
    }
    Ok(())
}

async fn dispatch_transfer_assets_tx(
    tx: &mut Transaction<'_, Postgres>,
    transfer_id: &str,
    lines: &[InventoryTransferLineRow],
) -> Result<(), InventoryMovementError> {
    for line in lines {
        let kind = InventoryAssetKind::parse(&line.asset_kind)?;
        let asset = lock_asset_tx(tx, kind, &line.asset_ref).await?;
        if asset.transfer_id != transfer_id || asset.qty_units != line.qty_units {
            return Err(InventoryMovementError::AssetUnavailable);
        }
        if kind == InventoryAssetKind::FinishedGoods {
            let result = sqlx::query(
                r#"
                UPDATE mini_finished_goods_stock
                SET status = 'in_transit', updated_at = now()
                WHERE id = $1
                  AND payload_json->>'inventory_transfer_id' = $2
                  AND status = 'transfer_reserved'
                "#,
            )
            .bind(&line.asset_ref)
            .bind(transfer_id)
            .execute(&mut **tx)
            .await
            .map_err(store_error)?;
            if result.rows_affected() != 1 {
                return Err(InventoryMovementError::AssetUnavailable);
            }
        }
    }
    Ok(())
}

async fn receive_transfer_assets_tx(
    tx: &mut Transaction<'_, Postgres>,
    transfer: &InventoryTransferRow,
    lines: &[InventoryTransferLineRow],
    actor: &InventoryActor,
) -> Result<(), InventoryMovementError> {
    let destination_location =
        warehouse_location_id_tx(tx, &transfer.destination_warehouse_id).await?;
    let receive_block = format!("Qabul - {}", transfer.destination_warehouse.trim());
    for line in lines {
        let kind = InventoryAssetKind::parse(&line.asset_kind)?;
        let asset = lock_asset_tx(tx, kind, &line.asset_ref).await?;
        if asset.transfer_id != transfer.id
            || asset.warehouse_id != transfer.source_warehouse_id
            || asset.qty_units != line.qty_units
        {
            return Err(InventoryMovementError::AssetUnavailable);
        }
        let result = match kind {
            InventoryAssetKind::RawMaterial => {
                sqlx::query(
                    r#"
                    UPDATE mini_raw_material_stock
                    SET warehouse = $2,
                        status = 'available',
                        payload_json = COALESCE(payload_json, '{}'::jsonb)
                            - 'inventory_transfer_id',
                        updated_at = now()
                    WHERE id = $1
                      AND payload_json->>'inventory_transfer_id' = $3
                      AND status = 'reserved'
                    "#,
                )
                .bind(&line.asset_ref)
                .bind(&transfer.destination_warehouse)
                .bind(&transfer.id)
                .execute(&mut **tx)
                .await
            }
            InventoryAssetKind::FinishedGoods => {
                sqlx::query(
                    r#"
                    UPDATE mini_finished_goods_stock
                    SET warehouse = $2,
                        status = 'available',
                        payload_json = COALESCE(payload_json, '{}'::jsonb)
                            - 'inventory_transfer_id',
                        updated_at = now()
                    WHERE id = $1
                      AND payload_json->>'inventory_transfer_id' = $3
                      AND status = 'in_transit'
                    "#,
                )
                .bind(&line.asset_ref)
                .bind(&transfer.destination_warehouse)
                .bind(&transfer.id)
                .execute(&mut **tx)
                .await
            }
            InventoryAssetKind::Qolip => {
                ensure_qolip_receive_block_tx(tx, &receive_block, &transfer.destination_warehouse)
                    .await?;
                sqlx::query(
                    r#"
                    UPDATE mini_qolip_locations
                    SET warehouse = $2,
                        block = $3,
                        row_letter = '',
                        column_number = NULL,
                        location_label = $3,
                        inventory_transfer_id = '',
                        updated_at = now()
                    WHERE id = $1 AND inventory_transfer_id = $4
                    "#,
                )
                .bind(&line.asset_ref)
                .bind(&transfer.destination_warehouse)
                .bind(&receive_block)
                .bind(&transfer.id)
                .execute(&mut **tx)
                .await
            }
        }
        .map_err(store_error)?;
        if result.rows_affected() != 1 {
            return Err(InventoryMovementError::AssetUnavailable);
        }
        sqlx::query(
            r#"
            INSERT INTO mini_inventory_placements (
                asset_kind, asset_ref, physical_location_id, version,
                updated_by_role, updated_by_ref, updated_by_name
            )
            VALUES ($1, $2, $3, 1, $4, $5, $6)
            ON CONFLICT (asset_kind, asset_ref) DO UPDATE SET
                physical_location_id = excluded.physical_location_id,
                version = mini_inventory_placements.version + 1,
                updated_by_role = excluded.updated_by_role,
                updated_by_ref = excluded.updated_by_ref,
                updated_by_name = excluded.updated_by_name,
                updated_at = now()
            "#,
        )
        .bind(kind.as_str())
        .bind(&line.asset_ref)
        .bind(&destination_location)
        .bind(inventory_role_code(&actor.principal.role))
        .bind(actor.principal.ref_.trim())
        .bind(actor.principal.display_name.trim())
        .execute(&mut **tx)
        .await
        .map_err(store_error)?;
        if kind == InventoryAssetKind::RawMaterial {
            insert_raw_material_transfer_events_tx(tx, transfer, line, actor).await?;
        }
    }
    Ok(())
}

async fn insert_raw_material_transfer_events_tx(
    tx: &mut Transaction<'_, Postgres>,
    transfer: &InventoryTransferRow,
    line: &InventoryTransferLineRow,
    actor: &InventoryActor,
) -> Result<(), InventoryMovementError> {
    let qty = erp_quantity_from_units(line.qty_units);
    for (suffix, event_type, warehouse, qty_delta) in [
        (
            "out",
            "transfer_out",
            transfer.source_warehouse.as_str(),
            -qty,
        ),
        (
            "in",
            "transfer_in",
            transfer.destination_warehouse.as_str(),
            qty,
        ),
    ] {
        insert_raw_material_event_tx(
            tx,
            RawMaterialEventDraft {
                idempotency_key: format!(
                    "inventory_transfer:{}:{}:{}",
                    transfer.id,
                    line.asset_ref.to_ascii_lowercase(),
                    suffix
                ),
                event_type: event_type.to_string(),
                warehouse: warehouse.to_string(),
                barcode: line.identifier.clone(),
                item_code: line.item_code.clone(),
                item_name: line.item_name.clone(),
                qty_delta,
                uom: line.uom.clone(),
                stock_status_before: Some("reserved".to_string()),
                stock_status_after: Some("available".to_string()),
                order_id: None,
                apparatus: None,
                actor_role: inventory_role_code(&actor.principal.role).to_string(),
                actor_ref: actor.principal.ref_.clone(),
                actor_display_name: actor.principal.display_name.clone(),
                owner_role: String::new(),
                owner_ref: String::new(),
                owner_display_name: String::new(),
                source_type: "warehouse_transfer".to_string(),
                source_id: transfer.id.clone(),
                source_line_ref: Some(line.asset_ref.clone()),
                correlation_id: Some(transfer.id.clone()),
                payload_json: serde_json::json!({
                    "source_warehouse_id": transfer.source_warehouse_id,
                    "destination_warehouse_id": transfer.destination_warehouse_id,
                    "qty": qty,
                    "uom": line.uom,
                }),
            },
        )
        .await
        .map_err(store_error)?;
    }
    Ok(())
}

async fn ensure_qolip_receive_block_tx(
    tx: &mut Transaction<'_, Postgres>,
    block: &str,
    warehouse: &str,
) -> Result<(), InventoryMovementError> {
    sqlx::query(
        r#"
        INSERT INTO mini_warehouses (
            id, name, company, is_group, parent_warehouse, payload_json
        )
        VALUES (
            'warehouse:' || lower($1),
            $1,
            '',
            false,
            $2,
            jsonb_build_object('source', 'inventory_transfer_receiving_block')
        )
        ON CONFLICT ((lower(name))) DO UPDATE SET
            parent_warehouse = excluded.parent_warehouse,
            is_group = false,
            payload_json = mini_warehouses.payload_json
                || excluded.payload_json,
            updated_at = now()
        "#,
    )
    .bind(block.trim())
    .bind(warehouse.trim())
    .execute(&mut **tx)
    .await
    .map_err(store_error)?;
    Ok(())
}

async fn inventory_location_for_update_tx(
    tx: &mut Transaction<'_, Postgres>,
    location_id: &str,
) -> Result<InventoryLocationRow, InventoryMovementError> {
    sqlx::query_as::<_, InventoryLocationRow>(
        r#"
        SELECT
            id, kind, name,
            COALESCE(warehouse_id, '') AS warehouse_id,
            COALESCE(factory_location_id, '') AS factory_location_id,
            active,
            '[]'::jsonb AS apparatus_json
        FROM mini_inventory_locations
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(location_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(store_error)?
    .ok_or(InventoryMovementError::LocationNotFound)
}

async fn warehouse_by_id(
    pool: &PgPool,
    warehouse_id: &str,
) -> Result<WarehouseLookupRow, InventoryMovementError> {
    warehouse_lookup_query()
        .bind(warehouse_id.trim())
        .fetch_optional(pool)
        .await
        .map_err(store_error)?
        .ok_or(InventoryMovementError::WarehouseNotFound)
}
