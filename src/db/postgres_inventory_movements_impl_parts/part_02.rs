impl PostgresInventoryMovementStore {

    async fn return_to_warehouses_batch(
        &self,
        actor: &InventoryActor,
        input: &InventoryReturnBatchCreate,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError> {
        let mut tx = self.pool.begin().await.map_err(store_error)?;
        advisory_idempotency_lock(&mut tx, &input.idempotency_key).await?;
        let event_key = |index: usize| format!("{}:return:{index}", input.idempotency_key);
        let existing_first = movement_event_identity_tx(&mut tx, &event_key(0)).await?;
        if existing_first.is_some() {
            for (index, selector) in input.assets.iter().enumerate() {
                let existing = movement_event_identity_tx(&mut tx, &event_key(index))
                    .await?
                    .ok_or(InventoryMovementError::IdempotencyConflict)?;
                if existing.event_type != "returned_to_warehouse"
                    || existing.asset_kind != selector.asset_kind.as_str()
                    || !existing.asset_ref.eq_ignore_ascii_case(&selector.asset_ref)
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

        for (index, selector) in input.assets.iter().enumerate() {
            let asset = lock_asset_tx(&mut tx, selector.asset_kind, &selector.asset_ref).await?;
            ensure_asset_available(&asset)?;
            if !actor.can_manage_warehouse(&asset.warehouse) {
                return Err(InventoryMovementError::WarehouseForbidden);
            }
            let source_kind = sqlx::query_scalar::<_, String>(
                "SELECT kind FROM mini_inventory_locations WHERE id = $1",
            )
            .bind(&asset.physical_location_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(store_error)?
            .ok_or(InventoryMovementError::LocationNotFound)?;
            if InventoryLocationKind::parse(&source_kind)? != InventoryLocationKind::State {
                return Err(InventoryMovementError::InvalidLocation);
            }
            let destination_location_id =
                warehouse_location_id_tx(&mut tx, &asset.warehouse_id).await?;
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
            .bind(&destination_location_id)
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
                    event_type: "returned_to_warehouse",
                    transfer_id: "",
                    asset_kind: selector.asset_kind,
                    asset_ref: selector.asset_ref.clone(),
                    from_warehouse_id: &asset.warehouse_id,
                    to_warehouse_id: &asset.warehouse_id,
                    from_location_id: &asset.physical_location_id,
                    to_location_id: &destination_location_id,
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

    async fn create_transfer(
        &self,
        actor: &InventoryActor,
        transfer_id: &str,
        input: &InventoryTransferCreate,
    ) -> Result<InventoryTransfer, InventoryMovementError> {
        let mut tx = self.pool.begin().await.map_err(store_error)?;
        advisory_idempotency_lock(&mut tx, &input.idempotency_key).await?;
        if let Some(existing) =
            transfer_id_by_idempotency_tx(&mut tx, &input.idempotency_key).await?
        {
            tx.commit().await.map_err(store_error)?;
            let transfer = load_transfer(&self.pool, &existing).await?;
            if !transfer_matches_create_request(&transfer, input) {
                return Err(InventoryMovementError::IdempotencyConflict);
            }
            return Ok(transfer);
        }

        let source = warehouse_by_id_tx(&mut tx, &input.source_warehouse_id).await?;
        let destination = warehouse_by_id_tx(&mut tx, &input.destination_warehouse_id).await?;
        ensure_transfer_warehouse(&source)?;
        ensure_transfer_warehouse(&destination)?;
        if source.id == destination.id {
            return Err(InventoryMovementError::SameWarehouse);
        }
        if !actor.can_manage_warehouse(&source.name) {
            return Err(InventoryMovementError::WarehouseForbidden);
        }
        if destination.assignment_count == 0 {
            return Err(InventoryMovementError::DestinationWarehouseUnassigned);
        }
        let internal_transfer = actor.manages_transfer_internally(&source.name, &destination.name);
        let source_location_id = warehouse_location_id_tx(&mut tx, &source.id).await?;

        sqlx::query(
            r#"
            INSERT INTO mini_inventory_transfers (
                id, idempotency_key,
                source_warehouse_id, source_warehouse,
                destination_warehouse_id, destination_warehouse,
                status, note,
                requested_by_role, requested_by_ref, requested_by_name
            )
            VALUES ($1, $2, $3, $4, $5, $6, 'requested', $7, $8, $9, $10)
            "#,
        )
        .bind(transfer_id)
        .bind(&input.idempotency_key)
        .bind(&source.id)
        .bind(&source.name)
        .bind(&destination.id)
        .bind(&destination.name)
        .bind(&input.note)
        .bind(inventory_role_code(&actor.principal.role))
        .bind(actor.principal.ref_.trim())
        .bind(actor.principal.display_name.trim())
        .execute(&mut *tx)
        .await
        .map_err(store_error)?;

        let mut selectors = input.assets.clone();
        selectors.sort_by(|left, right| {
            left.asset_kind.cmp(&right.asset_kind).then_with(|| {
                left.asset_ref
                    .to_lowercase()
                    .cmp(&right.asset_ref.to_lowercase())
            })
        });
        for selector in selectors {
            let asset = lock_asset_tx(&mut tx, selector.asset_kind, &selector.asset_ref).await?;
            ensure_asset_available(&asset)?;
            let asset_kind = InventoryAssetKind::parse(&asset.asset_kind)?;
            if asset.warehouse_id != source.id {
                return Err(InventoryMovementError::WarehouseForbidden);
            }
            if asset.physical_location_id != source_location_id {
                return Err(InventoryMovementError::AssetNotInSourceWarehouse);
            }
            reserve_asset_tx(&mut tx, &asset, transfer_id).await?;
            sqlx::query(
                r#"
                INSERT INTO mini_inventory_transfer_lines (
                    transfer_id, asset_kind, asset_ref,
                    item_code, item_name, identifier,
                    qty, uom, source_physical_location_id
                )
                VALUES (
                    $1, $2, $3, $4, $5, $6,
                    ($7::bigint::numeric / 1000000)::numeric(18,6), $8, $9
                )
                "#,
            )
            .bind(transfer_id)
            .bind(asset_kind.as_str())
            .bind(&asset.asset_ref)
            .bind(&asset.item_code)
            .bind(&asset.item_name)
            .bind(&asset.identifier)
            .bind(asset.qty_units)
            .bind(&asset.uom)
            .bind(&asset.physical_location_id)
            .execute(&mut *tx)
            .await
            .map_err(store_error)?;
            insert_movement_event_tx(
                &mut tx,
                MovementEventDraft {
                    idempotency_key: format!(
                        "{}:{}:{}",
                        input.idempotency_key,
                        asset_kind.as_str(),
                        asset.asset_ref.to_ascii_lowercase()
                    ),
                    event_type: "transfer_requested",
                    transfer_id,
                    asset_kind,
                    asset_ref: asset.asset_ref.clone(),
                    from_warehouse_id: &source.id,
                    to_warehouse_id: &destination.id,
                    from_location_id: &asset.physical_location_id,
                    to_location_id: &asset.physical_location_id,
                    qty_units: asset.qty_units,
                    uom: &asset.uom,
                    actor,
                    note: &input.note,
                },
            )
            .await?;
        }
        if internal_transfer {
            let transfer = transfer_for_update_tx(&mut tx, transfer_id).await?;
            let lines = transfer_lines_tx(&mut tx, transfer_id).await?;
            update_transfer_actor_tx(&mut tx, transfer_id, "approved", "approved", actor).await?;
            dispatch_transfer_assets_tx(&mut tx, transfer_id, &lines).await?;
            update_transfer_actor_tx(&mut tx, transfer_id, "dispatched", "in_transit", actor)
                .await?;
            receive_transfer_assets_tx(&mut tx, &transfer, &lines, actor).await?;
            update_transfer_actor_tx(&mut tx, transfer_id, "received", "received", actor).await?;
            insert_transfer_stage_events_tx(
                &mut tx,
                &transfer,
                &lines,
                actor,
                &input.note,
                &format!("{}:internal:approve", input.idempotency_key),
                "transfer_approved",
                false,
            )
            .await?;
            insert_transfer_stage_events_tx(
                &mut tx,
                &transfer,
                &lines,
                actor,
                &input.note,
                &format!("{}:internal:dispatch", input.idempotency_key),
                "transfer_dispatched",
                false,
            )
            .await?;
            insert_transfer_stage_events_tx(
                &mut tx,
                &transfer,
                &lines,
                actor,
                &input.note,
                &format!("{}:internal:receive", input.idempotency_key),
                "transfer_received",
                true,
            )
            .await?;
        } else {
            enqueue_transfer_chat_events_tx(&mut tx, transfer_id, "requested", &destination.name)
                .await?;
        }
        tx.commit().await.map_err(store_error)?;
        load_transfer(&self.pool, transfer_id).await
    }

    async fn transfers(
        &self,
        actor: &InventoryActor,
        query: &InventoryTransferQuery,
    ) -> Result<Vec<InventoryTransfer>, InventoryMovementError> {
        let scope = actor
            .assigned_warehouses
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        if !actor.is_admin && scope.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, InventoryTransferRow>(
            r#"
            SELECT
                id, source_warehouse_id, source_warehouse,
                destination_warehouse_id, destination_warehouse,
                status, note,
                requested_by_name, approved_by_name, dispatched_by_name,
                received_by_name, rejected_by_name, cancelled_by_name,
                EXTRACT(EPOCH FROM created_at)::bigint AS created_at_unix,
                EXTRACT(EPOCH FROM approved_at)::bigint AS approved_at_unix,
                EXTRACT(EPOCH FROM dispatched_at)::bigint AS dispatched_at_unix,
                EXTRACT(EPOCH FROM received_at)::bigint AS received_at_unix,
                EXTRACT(EPOCH FROM rejected_at)::bigint AS rejected_at_unix,
                EXTRACT(EPOCH FROM cancelled_at)::bigint AS cancelled_at_unix
            FROM mini_inventory_transfers
            WHERE (
                    $1 = true
                    OR (
                        ($3 IN ('', 'all', 'outgoing') AND lower(source_warehouse) = ANY($2))
                        OR
                        ($3 IN ('', 'all', 'incoming') AND lower(destination_warehouse) = ANY($2))
                    )
                  )
              AND (
                    $1 = true
                    OR $3 IN ('', 'all')
                    OR ($3 = 'outgoing' AND lower(source_warehouse) = ANY($2))
                    OR ($3 = 'incoming' AND lower(destination_warehouse) = ANY($2))
                  )
              AND ($4 = '' OR status = $4)
            ORDER BY created_at DESC, id DESC
            LIMIT $5 OFFSET $6
            "#,
        )
        .bind(actor.is_admin)
        .bind(scope)
        .bind(query.direction.as_str())
        .bind(query.status.as_str())
        .bind(query.limit as i64)
        .bind(query.offset as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(store_error)?;
        hydrate_transfers(&self.pool, rows).await
    }
}
