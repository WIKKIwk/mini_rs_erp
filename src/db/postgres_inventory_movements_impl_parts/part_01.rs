impl PostgresInventoryMovementStore {
    async fn locations(&self) -> Result<Vec<InventoryLocation>, InventoryMovementError> {
        let rows = sqlx::query_as::<_, InventoryLocationRow>(
            r#"
            SELECT
                location.id,
                location.kind,
                location.name,
                COALESCE(location.warehouse_id, '') AS warehouse_id,
                COALESCE(location.factory_location_id, '') AS factory_location_id,
                location.active,
                COALESCE(
                    jsonb_agg(
                        jsonb_build_object('id', apparatus.id, 'name', apparatus.name)
                        ORDER BY
                            COALESCE((apparatus.payload_json->>'sort_order')::integer, 2147483647),
                            lower(apparatus.name)
                    ) FILTER (WHERE apparatus.id IS NOT NULL),
                    '[]'::jsonb
                ) AS apparatus_json
            FROM mini_inventory_locations location
            LEFT JOIN mini_factory_location_apparatus_links links
              ON links.location_id = location.factory_location_id
            LEFT JOIN mini_apparatus apparatus
              ON apparatus.id = links.apparatus_id
            WHERE location.active = true
              AND (
                    location.kind <> 'warehouse'
                    OR EXISTS (
                        SELECT 1
                        FROM mini_warehouses warehouse
                        WHERE warehouse.id = location.warehouse_id
                          AND btrim(warehouse.parent_warehouse) = ''
                    )
              )
            GROUP BY
                location.id, location.kind, location.name, location.warehouse_id,
                location.factory_location_id, location.active
            ORDER BY
                CASE location.kind WHEN 'state' THEN 0 WHEN 'warehouse' THEN 1 ELSE 2 END,
                lower(location.name)
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        rows.into_iter().map(location_from_row).collect()
    }

    async fn raw_material_state_placements(
        &self,
        barcodes: &[String],
    ) -> Result<Vec<RawMaterialStatePlacement>, InventoryMovementError> {
        let normalized = barcodes
            .iter()
            .map(|barcode| barcode.trim().to_ascii_lowercase())
            .filter(|barcode| !barcode.is_empty())
            .collect::<Vec<_>>();
        if normalized.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, RawMaterialStatePlacementRow>(
            r#"
            SELECT
                stock.barcode,
                location.id AS location_id,
                location.name AS location_name,
                COALESCE(
                    jsonb_agg(
                        jsonb_build_object('id', apparatus.id, 'name', apparatus.name)
                        ORDER BY lower(apparatus.name)
                    )
                        FILTER (WHERE apparatus.id IS NOT NULL),
                    '[]'::jsonb
                ) AS apparatus_json
            FROM mini_raw_material_stock stock
            JOIN mini_inventory_placements placement
              ON placement.asset_kind = 'raw_material'
             AND lower(placement.asset_ref) = lower(stock.id)
            JOIN mini_inventory_locations location
              ON location.id = placement.physical_location_id
             AND location.kind = 'state'
             AND location.active = true
            LEFT JOIN mini_factory_location_apparatus_links links
              ON links.location_id = location.factory_location_id
            LEFT JOIN mini_apparatus apparatus
              ON apparatus.id = links.apparatus_id
            WHERE lower(stock.barcode) = ANY($1)
              AND stock.qty > 0
              AND stock.status <> 'consumed'
            GROUP BY stock.barcode, location.id, location.name
            ORDER BY lower(stock.barcode)
            "#,
        )
        .bind(normalized)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        rows.into_iter()
            .map(|row| {
                let apparatus =
                    serde_json::from_value::<Vec<InventoryLocationApparatus>>(row.apparatus_json)
                        .map_err(|_| InventoryMovementError::StoreFailed)?;
                let apparatus_ids = apparatus
                    .iter()
                    .map(|apparatus| {
                        ApparatusId::new(apparatus.id.trim().to_string())
                            .map(|id| id.to_string())
                            .map_err(|_| InventoryMovementError::StoreFailed)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let apparatus = apparatus
                    .into_iter()
                    .map(|apparatus| apparatus.name)
                    .collect();
                Ok(RawMaterialStatePlacement {
                    barcode: row.barcode,
                    location_id: row.location_id,
                    location_name: row.location_name,
                    apparatus_ids,
                    apparatus,
                })
            })
            .collect()
    }

    async fn assets(
        &self,
        actor: &InventoryActor,
        query: &InventoryAssetQuery,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError> {
        if !query.warehouse_id.is_empty() {
            let warehouse = warehouse_by_id(&self.pool, &query.warehouse_id).await?;
            if !actor.can_manage_warehouse(&warehouse.name) {
                return Err(InventoryMovementError::WarehouseForbidden);
            }
        }
        let scope = actor
            .assigned_warehouses
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let kind = query
            .asset_kind
            .map(InventoryAssetKind::as_str)
            .unwrap_or("");
        let needle = format!("%{}%", query.query.trim().to_ascii_lowercase());
        let rows = sqlx::query_as::<_, InventoryAssetRow>(ASSET_LIST_SQL)
            .bind(actor.is_admin)
            .bind(scope)
            .bind(query.warehouse_id.trim())
            .bind(kind)
            .bind(query.query.trim().to_ascii_lowercase())
            .bind(needle)
            .bind(query.limit as i64)
            .bind(query.offset as i64)
            .bind(query.current_user_states_only)
            .bind(actor.principal.ref_.trim())
            .fetch_all(&self.pool)
            .await
            .map_err(store_error)?;
        rows.into_iter().map(asset_from_row).collect()
    }

    async fn relocate(
        &self,
        actor: &InventoryActor,
        input: &InventoryRelocationCreate,
    ) -> Result<InventoryAsset, InventoryMovementError> {
        let mut tx = self.pool.begin().await.map_err(store_error)?;
        advisory_idempotency_lock(&mut tx, &input.idempotency_key).await?;
        if let Some(existing) = movement_event_identity_tx(&mut tx, &input.idempotency_key).await? {
            if existing.event_type != "relocated"
                || existing.asset_kind != input.asset_kind.as_str()
                || !existing.asset_ref.eq_ignore_ascii_case(&input.asset_ref)
                || existing.to_location_id != input.physical_location_id
            {
                return Err(InventoryMovementError::IdempotencyConflict);
            }
            tx.commit().await.map_err(store_error)?;
            return fetch_asset(&self.pool, input.asset_kind, &input.asset_ref).await;
        }

        let asset = lock_asset_tx(&mut tx, input.asset_kind, &input.asset_ref).await?;
        ensure_asset_available(&asset)?;
        if !actor.can_manage_warehouse(&asset.warehouse) {
            return Err(InventoryMovementError::WarehouseForbidden);
        }
        let location =
            inventory_location_for_update_tx(&mut tx, &input.physical_location_id).await?;
        if !location.active {
            return Err(InventoryMovementError::LocationInactive);
        }
        let location_kind = InventoryLocationKind::parse(&location.kind)?;
        if location_kind == InventoryLocationKind::Warehouse
            && location.warehouse_id != asset.warehouse_id
        {
            return Err(InventoryMovementError::CrossWarehouseRelocation);
        }
        let version = sqlx::query_scalar::<_, i64>(
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
            RETURNING version
            "#,
        )
        .bind(input.asset_kind.as_str())
        .bind(input.asset_ref.trim())
        .bind(&location.id)
        .bind(inventory_role_code(&actor.principal.role))
        .bind(actor.principal.ref_.trim())
        .bind(actor.principal.display_name.trim())
        .fetch_one(&mut *tx)
        .await
        .map_err(store_error)?;
        insert_movement_event_tx(
            &mut tx,
            MovementEventDraft {
                idempotency_key: input.idempotency_key.clone(),
                event_type: "relocated",
                transfer_id: "",
                asset_kind: input.asset_kind,
                asset_ref: input.asset_ref.clone(),
                from_warehouse_id: &asset.warehouse_id,
                to_warehouse_id: &asset.warehouse_id,
                from_location_id: &asset.physical_location_id,
                to_location_id: &location.id,
                qty_units: asset.qty_units,
                uom: &asset.uom,
                actor,
                note: &input.note,
            },
        )
        .await?;
        tx.commit().await.map_err(store_error)?;

        let mut saved = fetch_asset(&self.pool, input.asset_kind, &input.asset_ref).await?;
        saved.placement_version = version;
        Ok(saved)
    }

    async fn relocate_batch(
        &self,
        actor: &InventoryActor,
        input: &InventoryRelocationBatchCreate,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError> {
        let mut tx = self.pool.begin().await.map_err(store_error)?;
        advisory_idempotency_lock(&mut tx, &input.idempotency_key).await?;
        let event_key = |index: usize| format!("{}:batch:{index}", input.idempotency_key);
        let existing_first = movement_event_identity_tx(&mut tx, &event_key(0)).await?;
        if existing_first.is_some() {
            for (index, selector) in input.assets.iter().enumerate() {
                let existing = movement_event_identity_tx(&mut tx, &event_key(index))
                    .await?
                    .ok_or(InventoryMovementError::IdempotencyConflict)?;
                if existing.event_type != "relocated"
                    || existing.asset_kind != selector.asset_kind.as_str()
                    || !existing.asset_ref.eq_ignore_ascii_case(&selector.asset_ref)
                    || existing.to_location_id != input.physical_location_id
                {
                    return Err(InventoryMovementError::IdempotencyConflict);
                }
            }
            if movement_event_identity_tx(&mut tx, &event_key(input.assets.len()))
                .await?
                .is_some()
            {
                return Err(InventoryMovementError::IdempotencyConflict);
            }
            tx.commit().await.map_err(store_error)?;
            let mut saved = Vec::with_capacity(input.assets.len());
            for selector in &input.assets {
                saved
                    .push(fetch_asset(&self.pool, selector.asset_kind, &selector.asset_ref).await?);
            }
            return Ok(saved);
        }

        let location =
            inventory_location_for_update_tx(&mut tx, &input.physical_location_id).await?;
        if !location.active {
            return Err(InventoryMovementError::LocationInactive);
        }
        let location_kind = InventoryLocationKind::parse(&location.kind)?;
        for (index, selector) in input.assets.iter().enumerate() {
            let asset = lock_asset_tx(&mut tx, selector.asset_kind, &selector.asset_ref).await?;
            ensure_asset_available(&asset)?;
            if !actor.can_manage_warehouse(&asset.warehouse) {
                return Err(InventoryMovementError::WarehouseForbidden);
            }
            if location_kind == InventoryLocationKind::Warehouse
                && location.warehouse_id != asset.warehouse_id
            {
                return Err(InventoryMovementError::CrossWarehouseRelocation);
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
            .bind(selector.asset_kind.as_str())
            .bind(selector.asset_ref.trim())
            .bind(&location.id)
            .bind(inventory_role_code(&actor.principal.role))
            .bind(actor.principal.ref_.trim())
            .bind(actor.principal.display_name.trim())
            .execute(&mut *tx)
            .await
            .map_err(store_error)?;
            let key = event_key(index);
            insert_movement_event_tx(
                &mut tx,
                MovementEventDraft {
                    idempotency_key: key,
                    event_type: "relocated",
                    transfer_id: "",
                    asset_kind: selector.asset_kind,
                    asset_ref: selector.asset_ref.clone(),
                    from_warehouse_id: &asset.warehouse_id,
                    to_warehouse_id: &asset.warehouse_id,
                    from_location_id: &asset.physical_location_id,
                    to_location_id: &location.id,
                    qty_units: asset.qty_units,
                    uom: &asset.uom,
                    actor,
                    note: &input.note,
                },
            )
            .await?;
        }
        tx.commit().await.map_err(store_error)?;

        let mut saved = Vec::with_capacity(input.assets.len());
        for selector in &input.assets {
            saved.push(fetch_asset(&self.pool, selector.asset_kind, &selector.asset_ref).await?);
        }
        Ok(saved)
    }
}
