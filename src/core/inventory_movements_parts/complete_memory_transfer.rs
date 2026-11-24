fn complete_memory_transfer(
    state: &mut MemoryInventoryState,
    transfer: &mut InventoryTransfer,
    actor: &InventoryActor,
    now: i64,
) -> Result<(), InventoryMovementError> {
    ensure_memory_transfer_assets(state, transfer)?;
    let destination = warehouse_location(state, &transfer.destination_warehouse_id)?;
    for line in &transfer.lines {
        let asset = state
            .assets
            .get_mut(&(line.asset_kind, line.asset_ref.to_ascii_lowercase()))
            .ok_or(InventoryMovementError::AssetNotFound)?;
        if asset.transfer_id != transfer.id {
            return Err(InventoryMovementError::AssetUnavailable);
        }
        asset.custody_warehouse_id = transfer.destination_warehouse_id.clone();
        asset.custody_warehouse = transfer.destination_warehouse.clone();
        asset.physical_location = InventoryLocationRef::from(&destination);
        asset.transfer_id.clear();
        asset.status = "available".to_string();
        asset.placement_version += 1;
    }
    let actor_name = actor.principal.display_name.clone();
    if transfer.approved_at_unix.is_none() {
        transfer.approved_by_name = actor_name.clone();
        transfer.approved_at_unix = Some(now);
    }
    if transfer.dispatched_at_unix.is_none() {
        transfer.dispatched_by_name = actor_name.clone();
        transfer.dispatched_at_unix = Some(now);
    }
    transfer.status = InventoryTransferStatus::Received;
    transfer.received_by_name = actor_name;
    transfer.received_at_unix = Some(now);
    Ok(())
}

fn location_kind_rank(kind: InventoryLocationKind) -> u8 {
    match kind {
        InventoryLocationKind::State => 0,
        InventoryLocationKind::Warehouse => 1,
        InventoryLocationKind::Transit => 2,
    }
}

include!("../inventory_movements_inline_tests.rs");
