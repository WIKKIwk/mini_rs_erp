impl MemoryInventoryMovementStore {

    async fn create_transfer(
        &self,
        actor: &InventoryActor,
        transfer_id: &str,
        input: &InventoryTransferCreate,
    ) -> Result<InventoryTransfer, InventoryMovementError> {
        let mut state = self.state.write().await;
        if let Some(existing_id) = state.idempotency.get(&input.idempotency_key) {
            let existing = state
                .transfers
                .get(existing_id)
                .cloned()
                .ok_or(InventoryMovementError::IdempotencyConflict)?;
            if !transfer_matches_create(&existing, input) {
                return Err(InventoryMovementError::IdempotencyConflict);
            }
            return Ok(existing);
        }
        let source = warehouse_location(&state, &input.source_warehouse_id)?;
        let destination = warehouse_location(&state, &input.destination_warehouse_id)?;
        if source
            .warehouse_id
            .eq_ignore_ascii_case(&destination.warehouse_id)
        {
            return Err(InventoryMovementError::SameWarehouse);
        }
        if !actor.can_manage_warehouse(&source.name) {
            return Err(InventoryMovementError::WarehouseForbidden);
        }
        let mut lines = Vec::new();
        for selector in &input.assets {
            let key = (
                selector.asset_kind,
                selector.asset_ref.trim().to_ascii_lowercase(),
            );
            let asset = state
                .assets
                .get(&key)
                .ok_or(InventoryMovementError::AssetNotFound)?;
            if !asset
                .custody_warehouse_id
                .eq_ignore_ascii_case(&source.warehouse_id)
            {
                return Err(InventoryMovementError::WarehouseForbidden);
            }
            if !asset.transfer_id.is_empty() || asset.status != "available" {
                return Err(InventoryMovementError::AssetUnavailable);
            }
            if asset.physical_location.kind != InventoryLocationKind::Warehouse
                || asset.physical_location.id != source.id
            {
                return Err(InventoryMovementError::AssetNotInSourceWarehouse);
            }
            lines.push(InventoryTransferLine {
                asset_kind: asset.kind,
                asset_ref: asset.asset_ref.clone(),
                item_code: asset.item_code.clone(),
                item_name: asset.item_name.clone(),
                identifier: asset.identifier.clone(),
                qty: asset.qty,
                uom: asset.uom.clone(),
                source_physical_location_id: asset.physical_location.id.clone(),
            });
        }
        for selector in &input.assets {
            let key = (
                selector.asset_kind,
                selector.asset_ref.trim().to_ascii_lowercase(),
            );
            let asset = state
                .assets
                .get_mut(&key)
                .ok_or(InventoryMovementError::AssetNotFound)?;
            asset.transfer_id = transfer_id.to_string();
            asset.status = "transfer_reserved".to_string();
        }
        let internal_transfer = actor.manages_transfer_internally(&source.name, &destination.name);
        let mut transfer = InventoryTransfer {
            id: transfer_id.to_string(),
            source_warehouse_id: source.warehouse_id,
            source_warehouse: source.name,
            destination_warehouse_id: destination.warehouse_id,
            destination_warehouse: destination.name,
            status: InventoryTransferStatus::Requested,
            note: input.note.clone(),
            requested_by_name: actor.principal.display_name.clone(),
            approved_by_name: String::new(),
            dispatched_by_name: String::new(),
            received_by_name: String::new(),
            rejected_by_name: String::new(),
            cancelled_by_name: String::new(),
            created_at_unix: now_unix(),
            approved_at_unix: None,
            dispatched_at_unix: None,
            received_at_unix: None,
            rejected_at_unix: None,
            cancelled_at_unix: None,
            lines,
        };
        if internal_transfer {
            complete_memory_transfer(&mut state, &mut transfer, actor, now_unix())?;
        }
        state
            .idempotency
            .insert(input.idempotency_key.clone(), transfer.id.clone());
        state
            .transfers
            .insert(transfer.id.clone(), transfer.clone());
        Ok(transfer)
    }

    async fn transfers(
        &self,
        actor: &InventoryActor,
        query: &InventoryTransferQuery,
    ) -> Result<Vec<InventoryTransfer>, InventoryMovementError> {
        let state = self.state.read().await;
        let mut transfers = state
            .transfers
            .values()
            .filter(|transfer| match query.direction.as_str() {
                "incoming" => actor.can_manage_warehouse(&transfer.destination_warehouse),
                "outgoing" => actor.can_manage_warehouse(&transfer.source_warehouse),
                _ => {
                    actor.can_manage_warehouse(&transfer.source_warehouse)
                        || actor.can_manage_warehouse(&transfer.destination_warehouse)
                }
            })
            .filter(|transfer| query.status.is_empty() || transfer.status.as_str() == query.status)
            .cloned()
            .collect::<Vec<_>>();
        transfers.sort_by(|left, right| {
            right
                .created_at_unix
                .cmp(&left.created_at_unix)
                .then_with(|| right.id.cmp(&left.id))
        });
        Ok(transfers
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .collect())
    }

    async fn transfer_action(
        &self,
        actor: &InventoryActor,
        transfer_id: &str,
        action: InventoryTransferActionKind,
        _input: &InventoryTransferAction,
    ) -> Result<InventoryTransfer, InventoryMovementError> {
        let mut state = self.state.write().await;
        let transfer = state
            .transfers
            .get(transfer_id)
            .cloned()
            .ok_or(InventoryMovementError::TransferNotFound)?;
        let source_access = actor.can_manage_warehouse(&transfer.source_warehouse);
        let destination_access = actor.can_manage_warehouse(&transfer.destination_warehouse);
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
        if let Some((existing_transfer_id, existing_action)) =
            state.action_idempotency.get(&_input.idempotency_key)
        {
            if existing_transfer_id != transfer_id || *existing_action != action {
                return Err(InventoryMovementError::IdempotencyConflict);
            }
            return Ok(transfer);
        }
        let now = now_unix();
        let internal_transfer = actor.manages_transfer_internally(
            &transfer.source_warehouse,
            &transfer.destination_warehouse,
        );

        let mut updated = transfer.clone();
        if transfer_action_already_applied(updated.status, action) {
            state.action_idempotency.insert(
                _input.idempotency_key.clone(),
                (transfer_id.to_string(), action),
            );
            return Ok(updated);
        }
        match action {
            InventoryTransferActionKind::Approve => {
                if updated.status != InventoryTransferStatus::Requested {
                    return Err(InventoryMovementError::InvalidTransition);
                }
                ensure_memory_transfer_assets(&state, &updated)?;
                updated.status = InventoryTransferStatus::Approved;
                updated.approved_by_name = actor.principal.display_name.clone();
                updated.approved_at_unix = Some(now);
                if internal_transfer {
                    complete_memory_transfer(&mut state, &mut updated, actor, now)?;
                }
            }
            InventoryTransferActionKind::Reject => {
                if updated.status != InventoryTransferStatus::Requested {
                    return Err(InventoryMovementError::InvalidTransition);
                }
                ensure_memory_transfer_assets(&state, &updated)?;
                updated.status = InventoryTransferStatus::Rejected;
                updated.rejected_by_name = actor.principal.display_name.clone();
                updated.rejected_at_unix = Some(now);
                release_memory_assets(&mut state, &updated);
            }
            InventoryTransferActionKind::Dispatch => {
                if updated.status != InventoryTransferStatus::Approved {
                    return Err(InventoryMovementError::InvalidTransition);
                }
                updated.status = InventoryTransferStatus::InTransit;
                updated.dispatched_by_name = actor.principal.display_name.clone();
                updated.dispatched_at_unix = Some(now);
                if internal_transfer {
                    complete_memory_transfer(&mut state, &mut updated, actor, now)?;
                } else {
                    ensure_memory_transfer_assets(&state, &updated)?;
                    for line in &updated.lines {
                        if let Some(asset) = state
                            .assets
                            .get_mut(&(line.asset_kind, line.asset_ref.to_ascii_lowercase()))
                        {
                            asset.status = "in_transit".to_string();
                        }
                    }
                }
            }
            InventoryTransferActionKind::Receive => {
                if updated.status != InventoryTransferStatus::InTransit {
                    return Err(InventoryMovementError::InvalidTransition);
                }
                ensure_memory_transfer_assets(&state, &updated)?;
                let destination = warehouse_location(&state, &updated.destination_warehouse_id)?;
                for line in &updated.lines {
                    let asset = state
                        .assets
                        .get_mut(&(line.asset_kind, line.asset_ref.to_ascii_lowercase()))
                        .ok_or(InventoryMovementError::AssetNotFound)?;
                    if asset.transfer_id != updated.id {
                        return Err(InventoryMovementError::AssetUnavailable);
                    }
                    asset.custody_warehouse_id = updated.destination_warehouse_id.clone();
                    asset.custody_warehouse = updated.destination_warehouse.clone();
                    asset.physical_location = InventoryLocationRef::from(&destination);
                    asset.transfer_id.clear();
                    asset.status = "available".to_string();
                    asset.placement_version += 1;
                }
                updated.status = InventoryTransferStatus::Received;
                updated.received_by_name = actor.principal.display_name.clone();
                updated.received_at_unix = Some(now);
            }
            InventoryTransferActionKind::Cancel => {
                if !matches!(
                    updated.status,
                    InventoryTransferStatus::Requested | InventoryTransferStatus::Approved
                ) {
                    return Err(InventoryMovementError::InvalidTransition);
                }
                ensure_memory_transfer_assets(&state, &updated)?;
                updated.status = InventoryTransferStatus::Cancelled;
                updated.cancelled_by_name = actor.principal.display_name.clone();
                updated.cancelled_at_unix = Some(now);
                release_memory_assets(&mut state, &updated);
            }
        }
        state.transfers.insert(updated.id.clone(), updated.clone());
        state.action_idempotency.insert(
            _input.idempotency_key.clone(),
            (transfer_id.to_string(), action),
        );
        Ok(updated)
    }
}
