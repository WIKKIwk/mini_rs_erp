impl MemoryInventoryMovementStore {
    async fn locations(&self) -> Result<Vec<InventoryLocation>, InventoryMovementError> {
        let mut locations = self
            .state
            .read()
            .await
            .locations
            .values()
            .filter(|location| location.active)
            .cloned()
            .collect::<Vec<_>>();
        locations.sort_by(|left, right| {
            location_kind_rank(left.kind)
                .cmp(&location_kind_rank(right.kind))
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        Ok(locations)
    }

    async fn raw_material_state_placements(
        &self,
        barcodes: &[String],
    ) -> Result<Vec<RawMaterialStatePlacement>, InventoryMovementError> {
        let requested = barcodes
            .iter()
            .map(|barcode| barcode.trim().to_ascii_uppercase())
            .collect::<BTreeSet<_>>();
        let state = self.state.read().await;
        let mut placements = Vec::new();
        for asset in state.assets.values().filter(|asset| {
            asset.kind == InventoryAssetKind::RawMaterial
                && asset.physical_location.kind == InventoryLocationKind::State
                && requested.contains(&asset.identifier.trim().to_ascii_uppercase())
                && asset.status != "consumed"
        }) {
            let Some(location) = state.locations.get(&asset.physical_location.id) else {
                continue;
            };
            let apparatus_ids = location
                .apparatus
                .iter()
                .map(|apparatus| {
                    ApparatusId::new(apparatus.id.trim().to_string())
                        .map(|id| id.to_string())
                        .map_err(|_| InventoryMovementError::StoreFailed)
                })
                .collect::<Result<Vec<_>, _>>()?;
            placements.push(RawMaterialStatePlacement {
                barcode: asset.identifier.trim().to_string(),
                location_id: location.id.clone(),
                location_name: location.name.clone(),
                apparatus_ids,
                apparatus: location
                    .apparatus
                    .iter()
                    .map(|apparatus| apparatus.name.clone())
                    .collect(),
            });
        }
        Ok(placements)
    }

    async fn assets(
        &self,
        actor: &InventoryActor,
        query: &InventoryAssetQuery,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError> {
        let state = self.state.read().await;
        let requested_warehouse = state
            .locations
            .values()
            .find(|location| {
                location.kind == InventoryLocationKind::Warehouse
                    && location
                        .warehouse_id
                        .eq_ignore_ascii_case(query.warehouse_id.trim())
            })
            .cloned();
        if !query.warehouse_id.trim().is_empty() && requested_warehouse.is_none() {
            return Err(InventoryMovementError::WarehouseNotFound);
        }
        if let Some(warehouse) = requested_warehouse.as_ref()
            && !actor.can_manage_warehouse(&warehouse.name)
        {
            return Err(InventoryMovementError::WarehouseForbidden);
        }
        let needle = query.query.to_ascii_lowercase();
        let mut assets = state
            .assets
            .values()
            .filter(|asset| actor.can_manage_warehouse(&asset.custody_warehouse))
            .filter(|asset| {
                !query.current_user_states_only
                    || (asset.physical_location.kind == InventoryLocationKind::State
                        && state
                            .placement_updated_by_ref
                            .get(&(asset.kind, asset.asset_ref.to_ascii_lowercase()))
                            .is_some_and(|owner_ref| {
                                owner_ref.trim() == actor.principal.ref_.trim()
                            }))
            })
            .filter(|asset| {
                requested_warehouse
                    .as_ref()
                    .map(|warehouse| {
                        asset.physical_location.kind == InventoryLocationKind::Warehouse
                            && asset.physical_location.id == warehouse.id
                    })
                    .unwrap_or(true)
            })
            .filter(|asset| {
                query
                    .asset_kind
                    .map(|kind| kind == asset.kind)
                    .unwrap_or(true)
            })
            .filter(|asset| {
                needle.is_empty()
                    || [
                        &asset.item_code,
                        &asset.item_name,
                        &asset.identifier,
                        &asset.asset_ref,
                    ]
                    .iter()
                    .any(|value| value.to_ascii_lowercase().contains(&needle))
            })
            .cloned()
            .collect::<Vec<_>>();
        assets.sort_by(|left, right| {
            left.item_name
                .to_lowercase()
                .cmp(&right.item_name.to_lowercase())
                .then_with(|| left.identifier.cmp(&right.identifier))
        });
        Ok(assets
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .collect())
    }

    async fn relocate(
        &self,
        actor: &InventoryActor,
        input: &InventoryRelocationCreate,
    ) -> Result<InventoryAsset, InventoryMovementError> {
        let mut state = self.state.write().await;
        if let Some((kind, asset_ref, location_id)) =
            state.relocation_idempotency.get(&input.idempotency_key)
        {
            if *kind != input.asset_kind
                || !asset_ref.eq_ignore_ascii_case(&input.asset_ref)
                || location_id != &input.physical_location_id
            {
                return Err(InventoryMovementError::IdempotencyConflict);
            }
            return state
                .assets
                .get(&(input.asset_kind, input.asset_ref.to_ascii_lowercase()))
                .cloned()
                .ok_or(InventoryMovementError::AssetNotFound);
        }
        let location = state
            .locations
            .get(input.physical_location_id.trim())
            .cloned()
            .ok_or(InventoryMovementError::LocationNotFound)?;
        if !location.active {
            return Err(InventoryMovementError::LocationInactive);
        }
        let key = (input.asset_kind, input.asset_ref.to_ascii_lowercase());
        let asset = state
            .assets
            .get_mut(&key)
            .ok_or(InventoryMovementError::AssetNotFound)?;
        if !actor.can_manage_warehouse(&asset.custody_warehouse) {
            return Err(InventoryMovementError::WarehouseForbidden);
        }
        if !asset.transfer_id.is_empty() || asset.status != "available" {
            return Err(InventoryMovementError::AssetUnavailable);
        }
        if location.kind == InventoryLocationKind::Warehouse
            && !location
                .warehouse_id
                .eq_ignore_ascii_case(&asset.custody_warehouse_id)
        {
            return Err(InventoryMovementError::CrossWarehouseRelocation);
        }
        asset.physical_location = InventoryLocationRef::from(&location);
        asset.placement_version += 1;
        let saved = asset.clone();
        state
            .placement_updated_by_ref
            .insert(key, actor.principal.ref_.trim().to_string());
        state.relocation_idempotency.insert(
            input.idempotency_key.clone(),
            (
                input.asset_kind,
                input.asset_ref.clone(),
                input.physical_location_id.clone(),
            ),
        );
        Ok(saved)
    }

    async fn relocate_batch(
        &self,
        actor: &InventoryActor,
        input: &InventoryRelocationBatchCreate,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError> {
        let mut state = self.state.write().await;
        let selectors = input
            .assets
            .iter()
            .map(|asset| (asset.asset_kind, asset.asset_ref.to_ascii_lowercase()))
            .collect::<Vec<_>>();
        if let Some((existing, location_id)) = state
            .relocation_batch_idempotency
            .get(&input.idempotency_key)
        {
            if existing != &selectors || location_id != &input.physical_location_id {
                return Err(InventoryMovementError::IdempotencyConflict);
            }
            return selectors
                .iter()
                .map(|key| {
                    state
                        .assets
                        .get(key)
                        .cloned()
                        .ok_or(InventoryMovementError::AssetNotFound)
                })
                .collect();
        }
        let location = state
            .locations
            .get(input.physical_location_id.trim())
            .cloned()
            .ok_or(InventoryMovementError::LocationNotFound)?;
        if !location.active {
            return Err(InventoryMovementError::LocationInactive);
        }
        for key in &selectors {
            let asset = state
                .assets
                .get(key)
                .ok_or(InventoryMovementError::AssetNotFound)?;
            if !actor.can_manage_warehouse(&asset.custody_warehouse) {
                return Err(InventoryMovementError::WarehouseForbidden);
            }
            if !asset.transfer_id.is_empty() || asset.status != "available" {
                return Err(InventoryMovementError::AssetUnavailable);
            }
            if location.kind == InventoryLocationKind::Warehouse
                && !location
                    .warehouse_id
                    .eq_ignore_ascii_case(&asset.custody_warehouse_id)
            {
                return Err(InventoryMovementError::CrossWarehouseRelocation);
            }
        }
        let mut saved = Vec::with_capacity(selectors.len());
        for key in &selectors {
            let asset = state
                .assets
                .get_mut(key)
                .ok_or(InventoryMovementError::AssetNotFound)?;
            asset.physical_location = InventoryLocationRef::from(&location);
            asset.placement_version += 1;
            saved.push(asset.clone());
        }
        for key in &selectors {
            state
                .placement_updated_by_ref
                .insert(key.clone(), actor.principal.ref_.trim().to_string());
        }
        state.relocation_batch_idempotency.insert(
            input.idempotency_key.clone(),
            (selectors, input.physical_location_id.clone()),
        );
        Ok(saved)
    }

    async fn return_to_warehouses_batch(
        &self,
        actor: &InventoryActor,
        input: &InventoryReturnBatchCreate,
    ) -> Result<Vec<InventoryAsset>, InventoryMovementError> {
        let mut state = self.state.write().await;
        let selectors = input
            .assets
            .iter()
            .map(|asset| (asset.asset_kind, asset.asset_ref.to_ascii_lowercase()))
            .collect::<Vec<_>>();
        if let Some(existing) = state.return_batch_idempotency.get(&input.idempotency_key) {
            if existing != &selectors {
                return Err(InventoryMovementError::IdempotencyConflict);
            }
            return selectors
                .iter()
                .map(|key| {
                    state
                        .assets
                        .get(key)
                        .cloned()
                        .ok_or(InventoryMovementError::AssetNotFound)
                })
                .collect();
        }
        let mut destinations = BTreeMap::new();
        for key in &selectors {
            let asset = state
                .assets
                .get(key)
                .ok_or(InventoryMovementError::AssetNotFound)?;
            if !actor.can_manage_warehouse(&asset.custody_warehouse) {
                return Err(InventoryMovementError::WarehouseForbidden);
            }
            if !asset.transfer_id.is_empty() || asset.status != "available" {
                return Err(InventoryMovementError::AssetUnavailable);
            }
            if asset.physical_location.kind != InventoryLocationKind::State {
                return Err(InventoryMovementError::InvalidLocation);
            }
            let destination = state
                .locations
                .values()
                .find(|location| {
                    location.active
                        && location.kind == InventoryLocationKind::Warehouse
                        && location
                            .warehouse_id
                            .eq_ignore_ascii_case(&asset.custody_warehouse_id)
                })
                .cloned()
                .ok_or(InventoryMovementError::LocationNotFound)?;
            destinations.insert(key.clone(), destination);
        }
        let mut saved = Vec::with_capacity(selectors.len());
        for key in &selectors {
            let destination = destinations
                .get(key)
                .ok_or(InventoryMovementError::LocationNotFound)?;
            let asset = state
                .assets
                .get_mut(key)
                .ok_or(InventoryMovementError::AssetNotFound)?;
            asset.physical_location = InventoryLocationRef::from(destination);
            asset.placement_version += 1;
            saved.push(asset.clone());
        }
        for key in &selectors {
            state
                .placement_updated_by_ref
                .insert(key.clone(), actor.principal.ref_.trim().to_string());
        }
        state
            .return_batch_idempotency
            .insert(input.idempotency_key.clone(), selectors);
        Ok(saved)
    }
}
