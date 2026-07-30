use std::collections::BTreeMap;

use async_trait::async_trait;
use data_encoding::HEXLOWER;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};

use crate::core::inventory_movements::{
    InventoryActor, InventoryAsset, InventoryAssetKind, InventoryAssetQuery, InventoryLocation,
    InventoryLocationApparatus, InventoryLocationKind, InventoryLocationRef,
    InventoryMovementError, InventoryMovementStorePort, InventoryRelocationBatchCreate,
    InventoryRelocationCreate, InventoryReturnBatchCreate, RawMaterialStatePlacement,
    InventoryTransfer, InventoryTransferAction, InventoryTransferActionKind,
    InventoryTransferCreate, InventoryTransferLine, InventoryTransferQuery,
    InventoryTransferStatus, inventory_role_code,
};
use crate::db::postgres_raw_material_events::{
    RawMaterialEventDraft, insert_raw_material_event_tx,
};

#[derive(Clone)]
pub struct PostgresInventoryMovementStore {
    pool: PgPool,
}

impl PostgresInventoryMovementStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl InventoryMovementStorePort for PostgresInventoryMovementStore {
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
                    jsonb_agg(apparatus.name ORDER BY lower(apparatus.name))
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
                let apparatus = serde_json::from_value::<Vec<String>>(row.apparatus_json)
                    .map_err(|_| InventoryMovementError::StoreFailed)?;
                Ok(RawMaterialStatePlacement {
                    barcode: row.barcode,
                    location_id: row.location_id,
                    location_name: row.location_name,
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
                qty: asset.qty,
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
                saved.push(
                    fetch_asset(&self.pool, selector.asset_kind, &selector.asset_ref).await?,
                );
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
            let asset =
                lock_asset_tx(&mut tx, selector.asset_kind, &selector.asset_ref).await?;
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
                    qty: asset.qty,
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
                saved.push(
                    fetch_asset(&self.pool, selector.asset_kind, &selector.asset_ref).await?,
                );
            }
            return Ok(saved);
        }

        for (index, selector) in input.assets.iter().enumerate() {
            let asset =
                lock_asset_tx(&mut tx, selector.asset_kind, &selector.asset_ref).await?;
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
                    qty: asset.qty,
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
        let internal_transfer =
            actor.manages_transfer_internally(&source.name, &destination.name);
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
                    ($7::double precision)::numeric(18,3), $8, $9
                )
                "#,
            )
            .bind(transfer_id)
            .bind(asset_kind.as_str())
            .bind(&asset.asset_ref)
            .bind(&asset.item_code)
            .bind(&asset.item_name)
            .bind(&asset.identifier)
            .bind(asset.qty)
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
                    qty: asset.qty,
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
            update_transfer_actor_tx(&mut tx, transfer_id, "approved", "approved", actor)
                .await?;
            dispatch_transfer_assets_tx(&mut tx, transfer_id, &lines).await?;
            update_transfer_actor_tx(&mut tx, transfer_id, "dispatched", "in_transit", actor)
                .await?;
            receive_transfer_assets_tx(&mut tx, &transfer, &lines, actor).await?;
            update_transfer_actor_tx(&mut tx, transfer_id, "received", "received", actor)
                .await?;
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
            enqueue_transfer_chat_events_tx(
                &mut tx,
                transfer_id,
                "requested",
                &destination.name,
            )
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

    async fn transfer_action(
        &self,
        actor: &InventoryActor,
        transfer_id: &str,
        action: InventoryTransferActionKind,
        input: &InventoryTransferAction,
    ) -> Result<InventoryTransfer, InventoryMovementError> {
        let mut tx = self.pool.begin().await.map_err(store_error)?;
        advisory_idempotency_lock(&mut tx, &input.idempotency_key).await?;
        let transfer = transfer_for_update_tx(&mut tx, transfer_id).await?;
        let status = InventoryTransferStatus::parse(&transfer.status)?;
        let source_access = actor.can_manage_warehouse(&transfer.source_warehouse);
        let destination_access = actor.can_manage_warehouse(&transfer.destination_warehouse);
        let internal_transfer = actor.manages_transfer_internally(
            &transfer.source_warehouse,
            &transfer.destination_warehouse,
        );
        let lines = transfer_lines_tx(&mut tx, transfer_id).await?;

        let authorized = match action {
            InventoryTransferActionKind::Approve
            | InventoryTransferActionKind::Reject
            | InventoryTransferActionKind::Receive => destination_access,
            InventoryTransferActionKind::Dispatch | InventoryTransferActionKind::Cancel => {
                source_access
            }
        };
        if !authorized {
            return Err(InventoryMovementError::WarehouseForbidden);
        }
        if let Some(existing) = transfer_action_identity_tx(&mut tx, &input.idempotency_key).await?
        {
            if existing.transfer_id != transfer_id || existing.action != action.as_str() {
                return Err(InventoryMovementError::IdempotencyConflict);
            }
            tx.commit().await.map_err(store_error)?;
            return load_transfer(&self.pool, transfer_id).await;
        }
        if action_already_applied(status, action) {
            insert_transfer_action_identity_tx(
                &mut tx,
                &input.idempotency_key,
                transfer_id,
                action,
                actor,
            )
            .await?;
            tx.commit().await.map_err(store_error)?;
            return load_transfer(&self.pool, transfer_id).await;
        }
        match action {
            InventoryTransferActionKind::Approve | InventoryTransferActionKind::Reject => {
                if status != InventoryTransferStatus::Requested {
                    return Err(InventoryMovementError::InvalidTransition);
                }
            }
            InventoryTransferActionKind::Dispatch => {
                if status != InventoryTransferStatus::Approved {
                    return Err(InventoryMovementError::InvalidTransition);
                }
            }
            InventoryTransferActionKind::Receive => {
                if status != InventoryTransferStatus::InTransit {
                    return Err(InventoryMovementError::InvalidTransition);
                }
            }
            InventoryTransferActionKind::Cancel => {
                if !matches!(
                    status,
                    InventoryTransferStatus::Requested | InventoryTransferStatus::Approved
                ) {
                    return Err(InventoryMovementError::InvalidTransition);
                }
            }
        }
        insert_transfer_action_identity_tx(
            &mut tx,
            &input.idempotency_key,
            transfer_id,
            action,
            actor,
        )
        .await?;

        match action {
            InventoryTransferActionKind::Approve => {
                update_transfer_actor_tx(&mut tx, transfer_id, "approved", "approved", actor)
                    .await?;
                if internal_transfer {
                    dispatch_transfer_assets_tx(&mut tx, transfer_id, &lines).await?;
                    update_transfer_actor_tx(
                        &mut tx,
                        transfer_id,
                        "dispatched",
                        "in_transit",
                        actor,
                    )
                    .await?;
                    receive_transfer_assets_tx(&mut tx, &transfer, &lines, actor).await?;
                    update_transfer_actor_tx(
                        &mut tx,
                        transfer_id,
                        "received",
                        "received",
                        actor,
                    )
                    .await?;
                }
            }
            InventoryTransferActionKind::Reject => {
                release_transfer_assets_tx(&mut tx, transfer_id, &lines).await?;
                update_transfer_actor_tx(&mut tx, transfer_id, "rejected", "rejected", actor)
                    .await?;
            }
            InventoryTransferActionKind::Dispatch => {
                dispatch_transfer_assets_tx(&mut tx, transfer_id, &lines).await?;
                update_transfer_actor_tx(&mut tx, transfer_id, "dispatched", "in_transit", actor)
                    .await?;
                if internal_transfer {
                    receive_transfer_assets_tx(&mut tx, &transfer, &lines, actor).await?;
                    update_transfer_actor_tx(
                        &mut tx,
                        transfer_id,
                        "received",
                        "received",
                        actor,
                    )
                    .await?;
                }
            }
            InventoryTransferActionKind::Receive => {
                receive_transfer_assets_tx(&mut tx, &transfer, &lines, actor).await?;
                update_transfer_actor_tx(&mut tx, transfer_id, "received", "received", actor)
                    .await?;
            }
            InventoryTransferActionKind::Cancel => {
                release_transfer_assets_tx(&mut tx, transfer_id, &lines).await?;
                update_transfer_actor_tx(&mut tx, transfer_id, "cancelled", "cancelled", actor)
                    .await?;
            }
        }

        let event_type = match action {
            InventoryTransferActionKind::Approve => "transfer_approved",
            InventoryTransferActionKind::Reject => "transfer_rejected",
            InventoryTransferActionKind::Dispatch => "transfer_dispatched",
            InventoryTransferActionKind::Receive => "transfer_received",
            InventoryTransferActionKind::Cancel => "transfer_cancelled",
        };
        insert_transfer_stage_events_tx(
            &mut tx,
            &transfer,
            &lines,
            actor,
            &input.note,
            &input.idempotency_key,
            event_type,
            action == InventoryTransferActionKind::Receive,
        )
        .await?;
        if internal_transfer && action == InventoryTransferActionKind::Approve {
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
        } else if internal_transfer && action == InventoryTransferActionKind::Dispatch {
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
        }
        if !internal_transfer {
            enqueue_transfer_chat_events_tx(
                &mut tx,
                transfer_id,
                transfer_chat_status(action),
                &transfer.destination_warehouse,
            )
            .await?;
        }
        tx.commit().await.map_err(store_error)?;
        load_transfer(&self.pool, transfer_id).await
    }
}

const ASSET_LIST_SQL: &str = r#"
WITH assets AS (
    SELECT
        'raw_material'::text AS asset_kind,
        stock.id AS asset_ref,
        stock.warehouse,
        stock.item_code,
        COALESCE(NULLIF(btrim(stock.item_name), ''), stock.item_code) AS item_name,
        stock.barcode AS identifier,
        stock.qty::float8 AS qty,
        stock.uom,
        CASE
            WHEN btrim(COALESCE(stock.payload_json->>'inventory_transfer_id', '')) <> ''
                THEN COALESCE(transfer.status, 'transfer_reserved')
            ELSE stock.status
        END AS status,
        COALESCE(stock.payload_json->>'inventory_transfer_id', '') AS transfer_id
    FROM mini_raw_material_stock stock
    LEFT JOIN mini_inventory_transfers transfer
      ON transfer.id = stock.payload_json->>'inventory_transfer_id'
    WHERE stock.qty > 0 AND stock.status <> 'consumed'

    UNION ALL

    SELECT
        'finished_goods'::text,
        stock.id,
        stock.warehouse,
        stock.item_code,
        COALESCE(NULLIF(btrim(stock.item_name), ''), stock.item_code),
        stock.id,
        stock.qty::float8,
        stock.uom,
        stock.status,
        COALESCE(stock.payload_json->>'inventory_transfer_id', '')
    FROM mini_finished_goods_stock stock
    WHERE stock.qty > 0 AND stock.status <> 'dispatched'

    UNION ALL

    SELECT
        'qolip'::text,
        stock.id,
        stock.warehouse,
        stock.item_code,
        stock.item_name,
        stock.qolip_code,
        stock.quantity::float8,
        'dona'::text,
        CASE
            WHEN btrim(stock.inventory_transfer_id) = '' THEN 'available'
            ELSE COALESCE(transfer.status, 'transfer_reserved')
        END,
        stock.inventory_transfer_id
    FROM mini_qolip_locations stock
    LEFT JOIN mini_inventory_transfers transfer
      ON transfer.id = stock.inventory_transfer_id
    WHERE stock.quantity > 0
)
SELECT
    assets.asset_kind,
    assets.asset_ref,
    warehouse.id AS custody_warehouse_id,
    assets.warehouse AS custody_warehouse,
    assets.item_code,
    assets.item_name,
    assets.identifier,
    assets.qty,
    assets.uom,
    assets.status,
    location.id AS physical_location_id,
    location.kind AS physical_location_kind,
    location.name AS physical_location_name,
    assets.transfer_id,
    COALESCE(placement.version, 1)::bigint AS placement_version
FROM assets
JOIN mini_warehouses warehouse
  ON lower(warehouse.name) = lower(assets.warehouse)
JOIN mini_inventory_locations warehouse_location
  ON warehouse_location.warehouse_id = warehouse.id
LEFT JOIN mini_inventory_placements placement
  ON placement.asset_kind = assets.asset_kind
 AND lower(placement.asset_ref) = lower(assets.asset_ref)
JOIN mini_inventory_locations location
  ON location.id = COALESCE(placement.physical_location_id, warehouse_location.id)
WHERE ($1 = true OR lower(assets.warehouse) = ANY($2))
  AND (
        $3 = ''
        OR (
            location.kind = 'warehouse'
            AND location.warehouse_id = $3
        )
  )
  AND ($4 = '' OR assets.asset_kind = $4)
  AND (
        $5 = ''
        OR lower(assets.item_code) LIKE $6
        OR lower(assets.item_name) LIKE $6
        OR lower(assets.identifier) LIKE $6
        OR lower(assets.asset_ref) LIKE $6
  )
  AND (
        $9 = false
        OR (
            location.kind = 'state'
            AND placement.updated_by_ref = $10
        )
  )
ORDER BY lower(assets.item_name), lower(assets.identifier), assets.asset_ref
LIMIT $7 OFFSET $8
"#;

#[derive(sqlx::FromRow)]
struct InventoryLocationRow {
    id: String,
    kind: String,
    name: String,
    warehouse_id: String,
    factory_location_id: String,
    active: bool,
    apparatus_json: Value,
}

#[derive(sqlx::FromRow)]
struct RawMaterialStatePlacementRow {
    barcode: String,
    location_id: String,
    location_name: String,
    apparatus_json: Value,
}

#[derive(sqlx::FromRow)]
struct InventoryAssetRow {
    asset_kind: String,
    asset_ref: String,
    custody_warehouse_id: String,
    custody_warehouse: String,
    item_code: String,
    item_name: String,
    identifier: String,
    qty: f64,
    uom: String,
    status: String,
    physical_location_id: String,
    physical_location_kind: String,
    physical_location_name: String,
    transfer_id: String,
    placement_version: i64,
}

#[derive(sqlx::FromRow)]
struct AssetLockRow {
    asset_kind: String,
    asset_ref: String,
    warehouse_id: String,
    warehouse: String,
    item_code: String,
    item_name: String,
    identifier: String,
    qty: f64,
    uom: String,
    status: String,
    transfer_id: String,
    physical_location_id: String,
}

#[derive(sqlx::FromRow)]
struct WarehouseLookupRow {
    id: String,
    name: String,
    is_group: bool,
    parent_warehouse: String,
    assignment_count: i64,
}

#[derive(sqlx::FromRow, Clone)]
struct InventoryTransferRow {
    id: String,
    source_warehouse_id: String,
    source_warehouse: String,
    destination_warehouse_id: String,
    destination_warehouse: String,
    status: String,
    note: String,
    requested_by_name: String,
    approved_by_name: String,
    dispatched_by_name: String,
    received_by_name: String,
    rejected_by_name: String,
    cancelled_by_name: String,
    created_at_unix: i64,
    approved_at_unix: Option<i64>,
    dispatched_at_unix: Option<i64>,
    received_at_unix: Option<i64>,
    rejected_at_unix: Option<i64>,
    cancelled_at_unix: Option<i64>,
}

#[derive(sqlx::FromRow, Clone)]
struct InventoryTransferLineRow {
    transfer_id: String,
    asset_kind: String,
    asset_ref: String,
    item_code: String,
    item_name: String,
    identifier: String,
    qty: f64,
    uom: String,
    source_physical_location_id: String,
}

#[derive(sqlx::FromRow)]
struct MovementIdentityRow {
    event_type: String,
    asset_kind: String,
    asset_ref: String,
    to_location_id: String,
}

#[derive(sqlx::FromRow)]
struct TransferActionIdentityRow {
    transfer_id: String,
    action: String,
}

fn location_from_row(
    row: InventoryLocationRow,
) -> Result<InventoryLocation, InventoryMovementError> {
    let apparatus = serde_json::from_value::<Vec<InventoryLocationApparatus>>(row.apparatus_json)
        .map_err(|_| InventoryMovementError::StoreFailed)?;
    Ok(InventoryLocation {
        id: row.id,
        kind: InventoryLocationKind::parse(&row.kind)?,
        name: row.name,
        warehouse_id: row.warehouse_id,
        factory_location_id: row.factory_location_id,
        active: row.active,
        apparatus,
    })
}

fn asset_from_row(row: InventoryAssetRow) -> Result<InventoryAsset, InventoryMovementError> {
    Ok(InventoryAsset {
        kind: InventoryAssetKind::parse(&row.asset_kind)?,
        asset_ref: row.asset_ref,
        custody_warehouse_id: row.custody_warehouse_id,
        custody_warehouse: row.custody_warehouse,
        item_code: row.item_code,
        item_name: row.item_name,
        identifier: row.identifier,
        qty: row.qty,
        uom: row.uom,
        status: row.status,
        physical_location: InventoryLocationRef {
            id: row.physical_location_id,
            kind: InventoryLocationKind::parse(&row.physical_location_kind)?,
            name: row.physical_location_name,
        },
        transfer_id: row.transfer_id,
        placement_version: row.placement_version,
    })
}

async fn fetch_asset(
    pool: &PgPool,
    kind: InventoryAssetKind,
    asset_ref: &str,
) -> Result<InventoryAsset, InventoryMovementError> {
    let rows = sqlx::query_as::<_, InventoryAssetRow>(ASSET_LIST_SQL)
        .bind(true)
        .bind(Vec::<String>::new())
        .bind("")
        .bind(kind.as_str())
        .bind(asset_ref.trim().to_ascii_lowercase())
        .bind(format!("%{}%", asset_ref.trim().to_ascii_lowercase()))
        .bind(50_i64)
        .bind(0_i64)
        .bind(false)
        .bind("")
        .fetch_all(pool)
        .await
        .map_err(store_error)?;
    rows.into_iter()
        .find(|row| row.asset_ref.eq_ignore_ascii_case(asset_ref))
        .ok_or(InventoryMovementError::AssetNotFound)
        .and_then(asset_from_row)
}

async fn lock_asset_tx(
    tx: &mut Transaction<'_, Postgres>,
    kind: InventoryAssetKind,
    asset_ref: &str,
) -> Result<AssetLockRow, InventoryMovementError> {
    let row = match kind {
        InventoryAssetKind::RawMaterial => {
            sqlx::query_as::<_, AssetLockRow>(
                r#"
                SELECT
                    'raw_material'::text AS asset_kind,
                    stock.id AS asset_ref,
                    warehouse.id AS warehouse_id,
                    stock.warehouse,
                    stock.item_code,
                    COALESCE(NULLIF(btrim(stock.item_name), ''), stock.item_code) AS item_name,
                    stock.barcode AS identifier,
                    stock.qty::float8 AS qty,
                    stock.uom,
                    stock.status,
                    COALESCE(stock.payload_json->>'inventory_transfer_id', '') AS transfer_id,
                    COALESCE(
                        placement.physical_location_id,
                        warehouse_location.id
                    ) AS physical_location_id
                FROM mini_raw_material_stock stock
                JOIN mini_warehouses warehouse
                  ON lower(warehouse.name) = lower(stock.warehouse)
                JOIN mini_inventory_locations warehouse_location
                  ON warehouse_location.warehouse_id = warehouse.id
                LEFT JOIN mini_inventory_placements placement
                  ON placement.asset_kind = 'raw_material'
                 AND lower(placement.asset_ref) = lower(stock.id)
                WHERE lower(stock.id) = lower($1)
                FOR UPDATE OF stock
                "#,
            )
            .bind(asset_ref.trim())
            .fetch_optional(&mut **tx)
            .await
        }
        InventoryAssetKind::FinishedGoods => {
            sqlx::query_as::<_, AssetLockRow>(
                r#"
                SELECT
                    'finished_goods'::text AS asset_kind,
                    stock.id AS asset_ref,
                    warehouse.id AS warehouse_id,
                    stock.warehouse,
                    stock.item_code,
                    COALESCE(NULLIF(btrim(stock.item_name), ''), stock.item_code) AS item_name,
                    stock.id AS identifier,
                    stock.qty::float8 AS qty,
                    stock.uom,
                    stock.status,
                    COALESCE(stock.payload_json->>'inventory_transfer_id', '') AS transfer_id,
                    COALESCE(
                        placement.physical_location_id,
                        warehouse_location.id
                    ) AS physical_location_id
                FROM mini_finished_goods_stock stock
                JOIN mini_warehouses warehouse
                  ON lower(warehouse.name) = lower(stock.warehouse)
                JOIN mini_inventory_locations warehouse_location
                  ON warehouse_location.warehouse_id = warehouse.id
                LEFT JOIN mini_inventory_placements placement
                  ON placement.asset_kind = 'finished_goods'
                 AND lower(placement.asset_ref) = lower(stock.id)
                WHERE lower(stock.id) = lower($1)
                FOR UPDATE OF stock
                "#,
            )
            .bind(asset_ref.trim())
            .fetch_optional(&mut **tx)
            .await
        }
        InventoryAssetKind::Qolip => {
            sqlx::query_as::<_, AssetLockRow>(
                r#"
                SELECT
                    'qolip'::text AS asset_kind,
                    stock.id AS asset_ref,
                    warehouse.id AS warehouse_id,
                    stock.warehouse,
                    stock.item_code,
                    stock.item_name,
                    stock.qolip_code AS identifier,
                    stock.quantity::float8 AS qty,
                    'dona'::text AS uom,
                    CASE
                        WHEN btrim(stock.inventory_transfer_id) = '' THEN 'available'
                        ELSE 'transfer_reserved'
                    END AS status,
                    stock.inventory_transfer_id AS transfer_id,
                    COALESCE(
                        placement.physical_location_id,
                        warehouse_location.id
                    ) AS physical_location_id
                FROM mini_qolip_locations stock
                JOIN mini_warehouses warehouse
                  ON lower(warehouse.name) = lower(stock.warehouse)
                JOIN mini_inventory_locations warehouse_location
                  ON warehouse_location.warehouse_id = warehouse.id
                LEFT JOIN mini_inventory_placements placement
                  ON placement.asset_kind = 'qolip'
                 AND lower(placement.asset_ref) = lower(stock.id)
                WHERE lower(stock.id) = lower($1)
                FOR UPDATE OF stock
                "#,
            )
            .bind(asset_ref.trim())
            .fetch_optional(&mut **tx)
            .await
        }
    }
    .map_err(store_error)?;
    row.ok_or(InventoryMovementError::AssetNotFound)
}

fn ensure_asset_available(asset: &AssetLockRow) -> Result<(), InventoryMovementError> {
    if !asset.transfer_id.trim().is_empty() || asset.status != "available" || asset.qty <= 0.0 {
        Err(InventoryMovementError::AssetUnavailable)
    } else {
        Ok(())
    }
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
        if asset.transfer_id != transfer_id || (asset.qty - line.qty).abs() > 0.000_001 {
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
            || (asset.qty - line.qty).abs() > 0.000_001
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
    for (suffix, event_type, warehouse, qty_delta) in [
        (
            "out",
            "transfer_out",
            transfer.source_warehouse.as_str(),
            -line.qty,
        ),
        (
            "in",
            "transfer_in",
            transfer.destination_warehouse.as_str(),
            line.qty,
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
                    "qty": line.qty,
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

async fn warehouse_by_id_tx(
    tx: &mut Transaction<'_, Postgres>,
    warehouse_id: &str,
) -> Result<WarehouseLookupRow, InventoryMovementError> {
    warehouse_lookup_query()
        .bind(warehouse_id.trim())
        .fetch_optional(&mut **tx)
        .await
        .map_err(store_error)?
        .ok_or(InventoryMovementError::WarehouseNotFound)
}

fn warehouse_lookup_query<'q>()
-> sqlx::query::QueryAs<'q, Postgres, WarehouseLookupRow, sqlx::postgres::PgArguments> {
    sqlx::query_as::<_, WarehouseLookupRow>(
        r#"
        SELECT
            warehouse.id,
            warehouse.name,
            warehouse.is_group,
            warehouse.parent_warehouse,
            (
                SELECT count(*)::bigint
                FROM mini_warehouse_assignments assignment
                WHERE lower(assignment.warehouse) = lower(warehouse.name)
            ) AS assignment_count
        FROM mini_warehouses warehouse
        WHERE warehouse.id = $1
        "#,
    )
}

fn ensure_transfer_warehouse(warehouse: &WarehouseLookupRow) -> Result<(), InventoryMovementError> {
    if warehouse.is_group || !warehouse.parent_warehouse.trim().is_empty() {
        Err(InventoryMovementError::WarehouseNotFound)
    } else {
        Ok(())
    }
}

async fn warehouse_location_id_tx(
    tx: &mut Transaction<'_, Postgres>,
    warehouse_id: &str,
) -> Result<String, InventoryMovementError> {
    sqlx::query_scalar::<_, String>(
        r#"
        SELECT id
        FROM mini_inventory_locations
        WHERE kind = 'warehouse' AND warehouse_id = $1 AND active = true
        "#,
    )
    .bind(warehouse_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(store_error)?
    .ok_or(InventoryMovementError::LocationNotFound)
}

async fn enqueue_transfer_chat_events_tx(
    tx: &mut Transaction<'_, Postgres>,
    transfer_id: &str,
    status: &str,
    destination_warehouse: &str,
) -> Result<(), InventoryMovementError> {
    let existing_targets = sqlx::query_as::<_, (String, String, String)>(
        r#"
        SELECT DISTINCT target_role, target_ref, target_display_name
        FROM mini_inventory_transfer_chat_outbox
        WHERE transfer_id = $1
        ORDER BY target_role, target_ref
        "#,
    )
    .bind(transfer_id.trim())
    .fetch_all(&mut **tx)
    .await
    .map_err(store_error)?;
    let targets = if existing_targets.is_empty() {
        sqlx::query_as::<_, (String, String, String)>(
        r#"
        SELECT principal_role, principal_ref, display_name
        FROM mini_warehouse_assignments
        WHERE lower(warehouse) = lower($1)
          AND principal_role <> 'customer'
        ORDER BY lower(display_name), lower(principal_ref)
        "#,
        )
        .bind(destination_warehouse.trim())
        .fetch_all(&mut **tx)
        .await
        .map_err(store_error)?
    } else {
        existing_targets
    };
    if targets.is_empty() {
        return Err(InventoryMovementError::DestinationWarehouseUnassigned);
    }
    for (target_role, target_ref, target_display_name) in targets {
        sqlx::query(
            r#"
            INSERT INTO mini_inventory_transfer_chat_outbox (
                event_id, transfer_id, status,
                target_role, target_ref, target_display_name
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (transfer_id, target_role, target_ref, status) DO NOTHING
            "#,
        )
        .bind(random_id("inventory_transfer_chat"))
        .bind(transfer_id.trim())
        .bind(status)
        .bind(target_role)
        .bind(target_ref)
        .bind(target_display_name)
        .execute(&mut **tx)
        .await
        .map_err(store_error)?;
    }
    Ok(())
}

fn transfer_chat_status(action: InventoryTransferActionKind) -> &'static str {
    match action {
        InventoryTransferActionKind::Approve => "approved",
        InventoryTransferActionKind::Reject => "rejected",
        InventoryTransferActionKind::Dispatch => "in_transit",
        InventoryTransferActionKind::Receive => "received",
        InventoryTransferActionKind::Cancel => "cancelled",
    }
}

async fn advisory_idempotency_lock(
    tx: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
) -> Result<(), InventoryMovementError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1)::bigint)")
        .bind(idempotency_key.trim())
        .execute(&mut **tx)
        .await
        .map_err(store_error)?;
    Ok(())
}

async fn transfer_id_by_idempotency_tx(
    tx: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
) -> Result<Option<String>, InventoryMovementError> {
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM mini_inventory_transfers WHERE idempotency_key = $1",
    )
    .bind(idempotency_key.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(store_error)
}

async fn movement_event_identity_tx(
    tx: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
) -> Result<Option<MovementIdentityRow>, InventoryMovementError> {
    sqlx::query_as::<_, MovementIdentityRow>(
        r#"
        SELECT event_type, asset_kind, asset_ref, to_location_id
        FROM mini_inventory_movement_events
        WHERE idempotency_key = $1
        "#,
    )
    .bind(idempotency_key.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(store_error)
}

async fn transfer_action_identity_tx(
    tx: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
) -> Result<Option<TransferActionIdentityRow>, InventoryMovementError> {
    sqlx::query_as::<_, TransferActionIdentityRow>(
        r#"
        SELECT transfer_id, action
        FROM mini_inventory_transfer_actions
        WHERE idempotency_key = $1
        "#,
    )
    .bind(idempotency_key.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(store_error)
}

async fn insert_transfer_action_identity_tx(
    tx: &mut Transaction<'_, Postgres>,
    idempotency_key: &str,
    transfer_id: &str,
    action: InventoryTransferActionKind,
    actor: &InventoryActor,
) -> Result<(), InventoryMovementError> {
    sqlx::query(
        r#"
        INSERT INTO mini_inventory_transfer_actions (
            idempotency_key, transfer_id, action,
            actor_role, actor_ref, actor_name
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(idempotency_key.trim())
    .bind(transfer_id.trim())
    .bind(action.as_str())
    .bind(inventory_role_code(&actor.principal.role))
    .bind(actor.principal.ref_.trim())
    .bind(actor.principal.display_name.trim())
    .execute(&mut **tx)
    .await
    .map_err(store_error)?;
    Ok(())
}

async fn transfer_for_update_tx(
    tx: &mut Transaction<'_, Postgres>,
    transfer_id: &str,
) -> Result<InventoryTransferRow, InventoryMovementError> {
    sqlx::query_as::<_, InventoryTransferRow>(
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
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(transfer_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(store_error)?
    .ok_or(InventoryMovementError::TransferNotFound)
}

async fn transfer_lines_tx(
    tx: &mut Transaction<'_, Postgres>,
    transfer_id: &str,
) -> Result<Vec<InventoryTransferLineRow>, InventoryMovementError> {
    sqlx::query_as::<_, InventoryTransferLineRow>(
        r#"
        SELECT
            transfer_id, asset_kind, asset_ref,
            item_code, item_name, identifier,
            qty::float8 AS qty, uom, source_physical_location_id
        FROM mini_inventory_transfer_lines
        WHERE transfer_id = $1
        ORDER BY asset_kind, asset_ref
        "#,
    )
    .bind(transfer_id.trim())
    .fetch_all(&mut **tx)
    .await
    .map_err(store_error)
}

async fn update_transfer_actor_tx(
    tx: &mut Transaction<'_, Postgres>,
    transfer_id: &str,
    actor_column: &str,
    status: &str,
    actor: &InventoryActor,
) -> Result<(), InventoryMovementError> {
    let allowed = [
        "approved",
        "rejected",
        "dispatched",
        "received",
        "cancelled",
    ];
    if !allowed.contains(&actor_column) {
        return Err(InventoryMovementError::StoreFailed);
    }
    let query = format!(
        "UPDATE mini_inventory_transfers
         SET status = $2,
             {actor_column}_by_role = $3,
             {actor_column}_by_ref = $4,
             {actor_column}_by_name = $5,
             {actor_column}_at = now(),
             updated_at = now()
         WHERE id = $1"
    );
    sqlx::query(&query)
        .bind(transfer_id)
        .bind(status)
        .bind(inventory_role_code(&actor.principal.role))
        .bind(actor.principal.ref_.trim())
        .bind(actor.principal.display_name.trim())
        .execute(&mut **tx)
        .await
        .map_err(store_error)?;
    Ok(())
}

async fn insert_transfer_stage_events_tx(
    tx: &mut Transaction<'_, Postgres>,
    transfer: &InventoryTransferRow,
    lines: &[InventoryTransferLineRow],
    actor: &InventoryActor,
    note: &str,
    idempotency_prefix: &str,
    event_type: &str,
    moved_to_destination: bool,
) -> Result<(), InventoryMovementError> {
    let destination_location = if moved_to_destination {
        Some(warehouse_location_id_tx(tx, &transfer.destination_warehouse_id).await?)
    } else {
        None
    };
    for line in lines {
        insert_movement_event_tx(
            tx,
            MovementEventDraft {
                idempotency_key: format!(
                    "{}:{}:{}",
                    idempotency_prefix,
                    line.asset_kind,
                    line.asset_ref.to_ascii_lowercase()
                ),
                event_type,
                transfer_id: &transfer.id,
                asset_kind: InventoryAssetKind::parse(&line.asset_kind)?,
                asset_ref: line.asset_ref.clone(),
                from_warehouse_id: &transfer.source_warehouse_id,
                to_warehouse_id: &transfer.destination_warehouse_id,
                from_location_id: &line.source_physical_location_id,
                to_location_id: destination_location
                    .as_deref()
                    .unwrap_or(&line.source_physical_location_id),
                qty: line.qty,
                uom: &line.uom,
                actor,
                note,
            },
        )
        .await?;
    }
    Ok(())
}

fn action_already_applied(
    status: InventoryTransferStatus,
    action: InventoryTransferActionKind,
) -> bool {
    matches!(
        (status, action),
        (
            InventoryTransferStatus::Approved
                | InventoryTransferStatus::InTransit
                | InventoryTransferStatus::Received,
            InventoryTransferActionKind::Approve
        ) | (
            InventoryTransferStatus::Rejected,
            InventoryTransferActionKind::Reject
        ) | (
            InventoryTransferStatus::InTransit | InventoryTransferStatus::Received,
            InventoryTransferActionKind::Dispatch
        ) | (
            InventoryTransferStatus::Received,
            InventoryTransferActionKind::Receive
        ) | (
            InventoryTransferStatus::Cancelled,
            InventoryTransferActionKind::Cancel
        )
    )
}

fn transfer_matches_create_request(
    transfer: &InventoryTransfer,
    input: &InventoryTransferCreate,
) -> bool {
    if !transfer
        .source_warehouse_id
        .eq_ignore_ascii_case(&input.source_warehouse_id)
        || !transfer
            .destination_warehouse_id
            .eq_ignore_ascii_case(&input.destination_warehouse_id)
        || transfer.note != input.note
    {
        return false;
    }
    let existing = transfer
        .lines
        .iter()
        .map(|line| {
            (
                line.asset_kind.as_str().to_string(),
                line.asset_ref.to_ascii_lowercase(),
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    let requested = input
        .assets
        .iter()
        .map(|asset| {
            (
                asset.asset_kind.as_str().to_string(),
                asset.asset_ref.to_ascii_lowercase(),
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    existing == requested && existing.len() == transfer.lines.len()
}

async fn load_transfer(
    pool: &PgPool,
    transfer_id: &str,
) -> Result<InventoryTransfer, InventoryMovementError> {
    let row = sqlx::query_as::<_, InventoryTransferRow>(
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
        WHERE id = $1
        "#,
    )
    .bind(transfer_id.trim())
    .fetch_optional(pool)
    .await
    .map_err(store_error)?
    .ok_or(InventoryMovementError::TransferNotFound)?;
    let mut transfers = hydrate_transfers(pool, vec![row]).await?;
    transfers
        .pop()
        .ok_or(InventoryMovementError::TransferNotFound)
}

async fn hydrate_transfers(
    pool: &PgPool,
    rows: Vec<InventoryTransferRow>,
) -> Result<Vec<InventoryTransfer>, InventoryMovementError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let ids = rows.iter().map(|row| row.id.clone()).collect::<Vec<_>>();
    let line_rows = sqlx::query_as::<_, InventoryTransferLineRow>(
        r#"
        SELECT
            transfer_id, asset_kind, asset_ref,
            item_code, item_name, identifier,
            qty::float8 AS qty, uom, source_physical_location_id
        FROM mini_inventory_transfer_lines
        WHERE transfer_id = ANY($1)
        ORDER BY transfer_id, asset_kind, asset_ref
        "#,
    )
    .bind(ids)
    .fetch_all(pool)
    .await
    .map_err(store_error)?;
    let mut lines_by_transfer = BTreeMap::<String, Vec<InventoryTransferLine>>::new();
    for line in line_rows {
        lines_by_transfer
            .entry(line.transfer_id)
            .or_default()
            .push(InventoryTransferLine {
                asset_kind: InventoryAssetKind::parse(&line.asset_kind)?,
                asset_ref: line.asset_ref,
                item_code: line.item_code,
                item_name: line.item_name,
                identifier: line.identifier,
                qty: line.qty,
                uom: line.uom,
                source_physical_location_id: line.source_physical_location_id,
            });
    }
    rows.into_iter()
        .map(|row| {
            let lines = lines_by_transfer.remove(&row.id).unwrap_or_default();
            Ok(InventoryTransfer {
                id: row.id,
                source_warehouse_id: row.source_warehouse_id,
                source_warehouse: row.source_warehouse,
                destination_warehouse_id: row.destination_warehouse_id,
                destination_warehouse: row.destination_warehouse,
                status: InventoryTransferStatus::parse(&row.status)?,
                note: row.note,
                requested_by_name: row.requested_by_name,
                approved_by_name: row.approved_by_name,
                dispatched_by_name: row.dispatched_by_name,
                received_by_name: row.received_by_name,
                rejected_by_name: row.rejected_by_name,
                cancelled_by_name: row.cancelled_by_name,
                created_at_unix: row.created_at_unix,
                approved_at_unix: row.approved_at_unix,
                dispatched_at_unix: row.dispatched_at_unix,
                received_at_unix: row.received_at_unix,
                rejected_at_unix: row.rejected_at_unix,
                cancelled_at_unix: row.cancelled_at_unix,
                lines,
            })
        })
        .collect()
}

struct MovementEventDraft<'a> {
    idempotency_key: String,
    event_type: &'a str,
    transfer_id: &'a str,
    asset_kind: InventoryAssetKind,
    asset_ref: String,
    from_warehouse_id: &'a str,
    to_warehouse_id: &'a str,
    from_location_id: &'a str,
    to_location_id: &'a str,
    qty: f64,
    uom: &'a str,
    actor: &'a InventoryActor,
    note: &'a str,
}

async fn insert_movement_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    draft: MovementEventDraft<'_>,
) -> Result<(), InventoryMovementError> {
    sqlx::query(
        r#"
        INSERT INTO mini_inventory_movement_events (
            id, idempotency_key, event_type, transfer_id,
            asset_kind, asset_ref,
            from_warehouse_id, to_warehouse_id,
            from_location_id, to_location_id,
            qty, uom,
            actor_role, actor_ref, actor_name,
            note, payload_json
        )
        VALUES (
            $1, $2, $3, NULLIF($4, ''),
            $5, $6, $7, $8, $9, $10,
            ($11::double precision)::numeric(18,3), $12,
            $13, $14, $15, $16, '{}'::jsonb
        )
        ON CONFLICT (idempotency_key) DO NOTHING
        "#,
    )
    .bind(random_id("inventory_event"))
    .bind(draft.idempotency_key)
    .bind(draft.event_type)
    .bind(draft.transfer_id)
    .bind(draft.asset_kind.as_str())
    .bind(draft.asset_ref)
    .bind(draft.from_warehouse_id)
    .bind(draft.to_warehouse_id)
    .bind(draft.from_location_id)
    .bind(draft.to_location_id)
    .bind(draft.qty)
    .bind(draft.uom)
    .bind(inventory_role_code(&draft.actor.principal.role))
    .bind(draft.actor.principal.ref_.trim())
    .bind(draft.actor.principal.display_name.trim())
    .bind(draft.note)
    .execute(&mut **tx)
    .await
    .map_err(store_error)?;
    Ok(())
}

fn random_id(prefix: &str) -> String {
    format!("{prefix}_{}", HEXLOWER.encode(&rand::random::<[u8; 16]>()))
}

fn store_error(_error: sqlx::Error) -> InventoryMovementError {
    InventoryMovementError::StoreFailed
}
