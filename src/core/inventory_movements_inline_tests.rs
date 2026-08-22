#[cfg(test)]
mod tests {
    use super::*;

    fn principal(role: PrincipalRole, ref_: &str, name: &str) -> Principal {
        Principal {
            role,
            display_name: name.to_string(),
            legal_name: name.to_string(),
            ref_: ref_.to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        }
    }

    fn warehouse_location_fixture(id: &str, name: &str) -> InventoryLocation {
        InventoryLocation {
            id: format!("location:{id}"),
            kind: InventoryLocationKind::Warehouse,
            name: name.to_string(),
            warehouse_id: id.to_string(),
            factory_location_id: String::new(),
            active: true,
            apparatus: Vec::new(),
        }
    }

    #[tokio::test]
    async fn raw_material_state_placements_return_only_requested_state_assets() {
        let store = Arc::new(MemoryInventoryMovementStore::new());
        let warehouse = warehouse_location_fixture("warehouse:a", "A ombor");
        let state_location = InventoryLocation {
            id: "location:state:pechat".to_string(),
            kind: InventoryLocationKind::State,
            name: "Pechat oldi".to_string(),
            warehouse_id: String::new(),
            factory_location_id: "factory:pechat".to_string(),
            active: true,
            apparatus: vec![InventoryLocationApparatus {
                id: "apparatus:catalog:pechat-001".to_string(),
                name: "7 ta rangli pechat - A".to_string(),
            }],
        };
        store
            .seed_locations(vec![warehouse.clone(), state_location.clone()])
            .await;
        store
            .seed_assets(vec![
                InventoryAsset {
                    kind: InventoryAssetKind::RawMaterial,
                    asset_ref: "raw:state".to_string(),
                    custody_warehouse_id: warehouse.warehouse_id.clone(),
                    custody_warehouse: warehouse.name.clone(),
                    item_code: "PE".to_string(),
                    item_name: "Polietilen".to_string(),
                    identifier: "RM-STATE".to_string(),
                    qty: 10.0,
                    uom: "kg".to_string(),
                    status: "available".to_string(),
                    physical_location: InventoryLocationRef::from(&state_location),
                    transfer_id: String::new(),
                    placement_version: 2,
                },
                InventoryAsset {
                    kind: InventoryAssetKind::RawMaterial,
                    asset_ref: "raw:warehouse".to_string(),
                    custody_warehouse_id: warehouse.warehouse_id.clone(),
                    custody_warehouse: warehouse.name.clone(),
                    item_code: "PE".to_string(),
                    item_name: "Polietilen".to_string(),
                    identifier: "RM-WAREHOUSE".to_string(),
                    qty: 10.0,
                    uom: "kg".to_string(),
                    status: "available".to_string(),
                    physical_location: InventoryLocationRef::from(&warehouse),
                    transfer_id: String::new(),
                    placement_version: 1,
                },
            ])
            .await;
        let service = InventoryMovementService::new(store);

        let placements = service
            .raw_material_state_placements(&[
                "rm-state".to_string(),
                "RM-WAREHOUSE".to_string(),
                "RM-NOT-REQUESTED".to_string(),
            ])
            .await
            .expect("state placements");

        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].barcode, "RM-STATE");
        assert_eq!(placements[0].location_id, state_location.id);
        assert_eq!(
            placements[0].apparatus_ids,
            vec!["apparatus:catalog:pechat-001".to_string()]
        );
        assert_eq!(
            placements[0].apparatus,
            vec!["7 ta rangli pechat - A".to_string()]
        );
    }

    #[tokio::test]
    async fn state_relocation_blocks_transfer_until_asset_returns_to_warehouse() {
        let store = Arc::new(MemoryInventoryMovementStore::new());
        let source = warehouse_location_fixture("warehouse:a", "A ombor");
        let destination = warehouse_location_fixture("warehouse:b", "B ombor");
        let state_location = InventoryLocation {
            id: "location:state:bosma".to_string(),
            kind: InventoryLocationKind::State,
            name: "Bosma oldi".to_string(),
            warehouse_id: String::new(),
            factory_location_id: "state_bosma".to_string(),
            active: true,
            apparatus: Vec::new(),
        };
        store
            .seed_locations(vec![
                source.clone(),
                destination.clone(),
                state_location.clone(),
            ])
            .await;
        store
            .seed_assets(vec![InventoryAsset {
                kind: InventoryAssetKind::RawMaterial,
                asset_ref: "raw:1".to_string(),
                custody_warehouse_id: source.warehouse_id.clone(),
                custody_warehouse: source.name.clone(),
                item_code: "PE".to_string(),
                item_name: "Polietilen".to_string(),
                identifier: "QR-1".to_string(),
                qty: 10.0,
                uom: "kg".to_string(),
                status: "available".to_string(),
                physical_location: InventoryLocationRef::from(&source),
                transfer_id: String::new(),
                placement_version: 1,
            }])
            .await;
        let service = InventoryMovementService::new(store);
        let actor = InventoryActor::new(
            principal(PrincipalRole::MaterialTaminotchi, "m1", "Materialchi"),
            false,
            ["A ombor".to_string()],
        );

        let moved = service
            .relocate(
                &actor,
                InventoryRelocationCreate {
                    asset_kind: InventoryAssetKind::RawMaterial,
                    asset_ref: "raw:1".to_string(),
                    physical_location_id: state_location.id.clone(),
                    note: String::new(),
                    idempotency_key: "relocate-1".to_string(),
                },
            )
            .await
            .expect("relocate");

        assert_eq!(moved.qty, 10.0);
        assert_eq!(moved.custody_warehouse, "A ombor");
        assert_eq!(moved.physical_location.name, "Bosma oldi");

        let state_assets = service
            .assets(
                &actor,
                InventoryAssetQuery {
                    current_user_states_only: true,
                    ..InventoryAssetQuery::default()
                },
            )
            .await
            .expect("current user state assets");
        assert_eq!(state_assets.len(), 1);

        let other_actor = InventoryActor::new(
            principal(
                PrincipalRole::MaterialTaminotchi,
                "m2",
                "Boshqa materialchi",
            ),
            false,
            ["A ombor".to_string()],
        );
        let other_state_assets = service
            .assets(
                &other_actor,
                InventoryAssetQuery {
                    current_user_states_only: true,
                    ..InventoryAssetQuery::default()
                },
            )
            .await
            .expect("other user state assets");
        assert!(other_state_assets.is_empty());

        let source_assets = service
            .assets(
                &actor,
                InventoryAssetQuery {
                    warehouse_id: source.warehouse_id.clone(),
                    ..InventoryAssetQuery::default()
                },
            )
            .await
            .expect("source warehouse assets");
        assert!(source_assets.is_empty());

        let error = service
            .create_transfer(
                &actor,
                InventoryTransferCreate {
                    source_warehouse_id: source.warehouse_id,
                    destination_warehouse_id: destination.warehouse_id,
                    assets: vec![InventoryAssetSelector {
                        asset_kind: InventoryAssetKind::RawMaterial,
                        asset_ref: "raw:1".to_string(),
                    }],
                    note: String::new(),
                    idempotency_key: "state-transfer-1".to_string(),
                },
            )
            .await
            .expect_err("state asset must return to a warehouse before transfer");

        assert_eq!(error, InventoryMovementError::AssetNotInSourceWarehouse);
    }

    #[tokio::test]
    async fn transfer_rejects_same_warehouse_id_case_insensitively() {
        let store = Arc::new(MemoryInventoryMovementStore::new());
        store
            .seed_locations(vec![warehouse_location_fixture("warehouse:a", "A ombor")])
            .await;
        let service = InventoryMovementService::new(store);
        let actor = InventoryActor::new(
            principal(PrincipalRole::MaterialTaminotchi, "m1", "Materialchi"),
            false,
            ["A ombor".to_string()],
        );

        let error = service
            .create_transfer(
                &actor,
                InventoryTransferCreate {
                    source_warehouse_id: "warehouse:a".to_string(),
                    destination_warehouse_id: "WAREHOUSE:A".to_string(),
                    assets: Vec::new(),
                    note: String::new(),
                    idempotency_key: "same-warehouse-case".to_string(),
                },
            )
            .await
            .expect_err("same warehouse must not become an internal transfer");

        assert_eq!(error, InventoryMovementError::SameWarehouse);
    }

    #[tokio::test]
    async fn batch_relocation_is_atomic_when_an_asset_is_unavailable() {
        let store = Arc::new(MemoryInventoryMovementStore::new());
        let source = warehouse_location_fixture("warehouse:a", "A ombor");
        let state_location = InventoryLocation {
            id: "location:state:bosma".to_string(),
            kind: InventoryLocationKind::State,
            name: "Bosma oldi".to_string(),
            warehouse_id: String::new(),
            factory_location_id: "state_bosma".to_string(),
            active: true,
            apparatus: Vec::new(),
        };
        store
            .seed_locations(vec![source.clone(), state_location.clone()])
            .await;
        store
            .seed_assets(vec![
                InventoryAsset {
                    kind: InventoryAssetKind::RawMaterial,
                    asset_ref: "raw:1".to_string(),
                    custody_warehouse_id: source.warehouse_id.clone(),
                    custody_warehouse: source.name.clone(),
                    item_code: "PE-1".to_string(),
                    item_name: "Polietilen 1".to_string(),
                    identifier: "QR-1".to_string(),
                    qty: 10.0,
                    uom: "kg".to_string(),
                    status: "available".to_string(),
                    physical_location: InventoryLocationRef::from(&source),
                    transfer_id: String::new(),
                    placement_version: 1,
                },
                InventoryAsset {
                    kind: InventoryAssetKind::RawMaterial,
                    asset_ref: "raw:2".to_string(),
                    custody_warehouse_id: source.warehouse_id.clone(),
                    custody_warehouse: source.name.clone(),
                    item_code: "PE-2".to_string(),
                    item_name: "Polietilen 2".to_string(),
                    identifier: "QR-2".to_string(),
                    qty: 8.0,
                    uom: "kg".to_string(),
                    status: "reserved".to_string(),
                    physical_location: InventoryLocationRef::from(&source),
                    transfer_id: "transfer:active".to_string(),
                    placement_version: 1,
                },
            ])
            .await;
        let service = InventoryMovementService::new(store);
        let actor = InventoryActor::new(
            principal(PrincipalRole::MaterialTaminotchi, "m1", "Materialchi"),
            false,
            ["A ombor".to_string()],
        );

        let error = service
            .relocate_batch(
                &actor,
                InventoryRelocationBatchCreate {
                    assets: vec![
                        InventoryAssetSelector {
                            asset_kind: InventoryAssetKind::RawMaterial,
                            asset_ref: "raw:1".to_string(),
                        },
                        InventoryAssetSelector {
                            asset_kind: InventoryAssetKind::RawMaterial,
                            asset_ref: "raw:2".to_string(),
                        },
                    ],
                    physical_location_id: state_location.id,
                    note: String::new(),
                    idempotency_key: "relocate-batch-atomic".to_string(),
                },
            )
            .await
            .expect_err("the whole batch must fail");
        assert_eq!(error, InventoryMovementError::AssetUnavailable);

        let assets = service
            .assets(
                &actor,
                InventoryAssetQuery {
                    warehouse_id: source.warehouse_id,
                    ..InventoryAssetQuery::default()
                },
            )
            .await
            .expect("source assets");
        let first = assets
            .iter()
            .find(|asset| asset.asset_ref == "raw:1")
            .expect("first asset");
        assert_eq!(first.physical_location.id, source.id);
        assert_eq!(first.placement_version, 1);
    }

    #[tokio::test]
    async fn batch_return_sends_each_state_asset_to_its_custody_warehouse() {
        let store = Arc::new(MemoryInventoryMovementStore::new());
        let warehouse_a = warehouse_location_fixture("warehouse:a", "A ombor");
        let warehouse_b = warehouse_location_fixture("warehouse:b", "B ombor");
        let state_location = InventoryLocation {
            id: "location:state:bosma".to_string(),
            kind: InventoryLocationKind::State,
            name: "Bosma oldi".to_string(),
            warehouse_id: String::new(),
            factory_location_id: "state_bosma".to_string(),
            active: true,
            apparatus: Vec::new(),
        };
        store
            .seed_locations(vec![
                warehouse_a.clone(),
                warehouse_b.clone(),
                state_location.clone(),
            ])
            .await;
        store
            .seed_assets(vec![
                InventoryAsset {
                    kind: InventoryAssetKind::RawMaterial,
                    asset_ref: "raw:a".to_string(),
                    custody_warehouse_id: warehouse_a.warehouse_id.clone(),
                    custody_warehouse: warehouse_a.name.clone(),
                    item_code: "PE-A".to_string(),
                    item_name: "Polietilen A".to_string(),
                    identifier: "QR-A".to_string(),
                    qty: 10.0,
                    uom: "kg".to_string(),
                    status: "available".to_string(),
                    physical_location: InventoryLocationRef::from(&state_location),
                    transfer_id: String::new(),
                    placement_version: 2,
                },
                InventoryAsset {
                    kind: InventoryAssetKind::RawMaterial,
                    asset_ref: "raw:b".to_string(),
                    custody_warehouse_id: warehouse_b.warehouse_id.clone(),
                    custody_warehouse: warehouse_b.name.clone(),
                    item_code: "PE-B".to_string(),
                    item_name: "Polietilen B".to_string(),
                    identifier: "QR-B".to_string(),
                    qty: 8.0,
                    uom: "kg".to_string(),
                    status: "available".to_string(),
                    physical_location: InventoryLocationRef::from(&state_location),
                    transfer_id: String::new(),
                    placement_version: 3,
                },
            ])
            .await;
        let service = InventoryMovementService::new(store);
        let actor = InventoryActor::new(
            principal(PrincipalRole::MaterialTaminotchi, "m1", "Materialchi"),
            false,
            ["A ombor".to_string(), "B ombor".to_string()],
        );

        let returned = service
            .return_to_warehouses_batch(
                &actor,
                InventoryReturnBatchCreate {
                    assets: vec![
                        InventoryAssetSelector {
                            asset_kind: InventoryAssetKind::RawMaterial,
                            asset_ref: "raw:a".to_string(),
                        },
                        InventoryAssetSelector {
                            asset_kind: InventoryAssetKind::RawMaterial,
                            asset_ref: "raw:b".to_string(),
                        },
                    ],
                    note: String::new(),
                    idempotency_key: "state-return-own-warehouses".to_string(),
                },
            )
            .await
            .expect("return batch");

        assert_eq!(returned.len(), 2);
        assert_eq!(returned[0].physical_location.id, warehouse_a.id);
        assert_eq!(returned[1].physical_location.id, warehouse_b.id);
        assert_eq!(returned[0].placement_version, 3);
        assert_eq!(returned[1].placement_version, 4);
    }

    #[tokio::test]
    async fn relocation_cannot_change_custody_warehouse() {
        let store = Arc::new(MemoryInventoryMovementStore::new());
        let source = warehouse_location_fixture("warehouse:a", "A ombor");
        let destination = warehouse_location_fixture("warehouse:b", "B ombor");
        store
            .seed_locations(vec![source.clone(), destination.clone()])
            .await;
        store
            .seed_assets(vec![InventoryAsset {
                kind: InventoryAssetKind::RawMaterial,
                asset_ref: "raw:1".to_string(),
                custody_warehouse_id: source.warehouse_id.clone(),
                custody_warehouse: source.name.clone(),
                item_code: "PE".to_string(),
                item_name: "Polietilen".to_string(),
                identifier: "QR-1".to_string(),
                qty: 10.0,
                uom: "kg".to_string(),
                status: "available".to_string(),
                physical_location: InventoryLocationRef::from(&source),
                transfer_id: String::new(),
                placement_version: 1,
            }])
            .await;
        let service = InventoryMovementService::new(store);
        let actor = InventoryActor::new(
            principal(PrincipalRole::MaterialTaminotchi, "m1", "Materialchi"),
            false,
            ["A ombor".to_string()],
        );

        let error = service
            .relocate(
                &actor,
                InventoryRelocationCreate {
                    asset_kind: InventoryAssetKind::RawMaterial,
                    asset_ref: "raw:1".to_string(),
                    physical_location_id: destination.id,
                    note: String::new(),
                    idempotency_key: "relocate-cross-warehouse".to_string(),
                },
            )
            .await
            .expect_err("warehouse change must use bilateral transfer");

        assert_eq!(error, InventoryMovementError::CrossWarehouseRelocation);
    }

    #[tokio::test]
    async fn bilateral_transfer_preserves_total_quantity_and_requires_both_sides() {
        let store = Arc::new(MemoryInventoryMovementStore::new());
        let source = warehouse_location_fixture("warehouse:a", "A ombor");
        let destination = warehouse_location_fixture("warehouse:b", "B ombor");
        store
            .seed_locations(vec![source.clone(), destination.clone()])
            .await;
        store
            .seed_assets(vec![InventoryAsset {
                kind: InventoryAssetKind::Qolip,
                asset_ref: "qolip:1".to_string(),
                custody_warehouse_id: source.warehouse_id.clone(),
                custody_warehouse: source.name.clone(),
                item_code: "Q-1".to_string(),
                item_name: "Qolip".to_string(),
                identifier: "QOLIP-1".to_string(),
                qty: 4.0,
                uom: "dona".to_string(),
                status: "available".to_string(),
                physical_location: InventoryLocationRef::from(&source),
                transfer_id: String::new(),
                placement_version: 1,
            }])
            .await;
        let service = InventoryMovementService::new(store);
        let source_actor = InventoryActor::new(
            principal(PrincipalRole::Qolipchi, "q1", "Qolipchi"),
            false,
            ["A ombor".to_string()],
        );
        let destination_actor = InventoryActor::new(
            principal(PrincipalRole::MaterialTaminotchi, "m1", "Materialchi"),
            false,
            ["B ombor".to_string()],
        );

        let transfer = service
            .create_transfer(
                &source_actor,
                InventoryTransferCreate {
                    source_warehouse_id: source.warehouse_id.clone(),
                    destination_warehouse_id: destination.warehouse_id.clone(),
                    assets: vec![InventoryAssetSelector {
                        asset_kind: InventoryAssetKind::Qolip,
                        asset_ref: "qolip:1".to_string(),
                    }],
                    note: "Kelishildi".to_string(),
                    idempotency_key: "transfer-1".to_string(),
                },
            )
            .await
            .expect("request");
        assert_eq!(transfer.status, InventoryTransferStatus::Requested);

        let approved = service
            .transfer_action(
                &destination_actor,
                &transfer.id,
                InventoryTransferActionKind::Approve,
                InventoryTransferAction {
                    note: String::new(),
                    idempotency_key: "approve-1".to_string(),
                },
            )
            .await
            .expect("approve");
        assert_eq!(approved.status, InventoryTransferStatus::Approved);

        let dispatched = service
            .transfer_action(
                &source_actor,
                &transfer.id,
                InventoryTransferActionKind::Dispatch,
                InventoryTransferAction {
                    note: String::new(),
                    idempotency_key: "dispatch-1".to_string(),
                },
            )
            .await
            .expect("dispatch");
        assert_eq!(dispatched.status, InventoryTransferStatus::InTransit);

        let received = service
            .transfer_action(
                &destination_actor,
                &transfer.id,
                InventoryTransferActionKind::Receive,
                InventoryTransferAction {
                    note: String::new(),
                    idempotency_key: "receive-1".to_string(),
                },
            )
            .await
            .expect("receive");
        assert_eq!(received.status, InventoryTransferStatus::Received);

        let destination_assets = service
            .assets(
                &destination_actor,
                InventoryAssetQuery {
                    warehouse_id: destination.warehouse_id.clone(),
                    ..InventoryAssetQuery::default()
                },
            )
            .await
            .expect("destination assets");
        assert_eq!(destination_assets.len(), 1);
        assert_eq!(destination_assets[0].qty, 4.0);
        assert_eq!(destination_assets[0].custody_warehouse, "B ombor");
    }

    #[tokio::test]
    async fn transfer_approval_is_atomic_when_a_reserved_asset_identity_is_lost() {
        let store = Arc::new(MemoryInventoryMovementStore::new());
        let source = warehouse_location_fixture("warehouse:a", "A ombor");
        let destination = warehouse_location_fixture("warehouse:b", "B ombor");
        store
            .seed_locations(vec![source.clone(), destination.clone()])
            .await;
        store
            .seed_assets(vec![
                InventoryAsset {
                    kind: InventoryAssetKind::RawMaterial,
                    asset_ref: "raw:1".to_string(),
                    custody_warehouse_id: source.warehouse_id.clone(),
                    custody_warehouse: source.name.clone(),
                    item_code: "M-1".to_string(),
                    item_name: "Material 1".to_string(),
                    identifier: "RAW-1".to_string(),
                    qty: 8.0,
                    uom: "kg".to_string(),
                    status: "available".to_string(),
                    physical_location: InventoryLocationRef::from(&source),
                    transfer_id: String::new(),
                    placement_version: 1,
                },
                InventoryAsset {
                    kind: InventoryAssetKind::RawMaterial,
                    asset_ref: "raw:2".to_string(),
                    custody_warehouse_id: source.warehouse_id.clone(),
                    custody_warehouse: source.name.clone(),
                    item_code: "M-2".to_string(),
                    item_name: "Material 2".to_string(),
                    identifier: "RAW-2".to_string(),
                    qty: 9.0,
                    uom: "kg".to_string(),
                    status: "available".to_string(),
                    physical_location: InventoryLocationRef::from(&source),
                    transfer_id: String::new(),
                    placement_version: 1,
                },
            ])
            .await;
        let service = InventoryMovementService::new(store.clone());
        let source_actor = InventoryActor::new(
            principal(PrincipalRole::Werka, "w1", "Werka"),
            false,
            [source.name.clone()],
        );
        let destination_actor = InventoryActor::new(
            principal(PrincipalRole::Qolipchi, "q1", "Qolipchi"),
            false,
            [destination.name.clone()],
        );

        let transfer = service
            .create_transfer(
                &source_actor,
                InventoryTransferCreate {
                    source_warehouse_id: source.warehouse_id.clone(),
                    destination_warehouse_id: destination.warehouse_id.clone(),
                    assets: vec![
                        InventoryAssetSelector {
                            asset_kind: InventoryAssetKind::RawMaterial,
                            asset_ref: "raw:1".to_string(),
                        },
                        InventoryAssetSelector {
                            asset_kind: InventoryAssetKind::RawMaterial,
                            asset_ref: "raw:2".to_string(),
                        },
                    ],
                    note: String::new(),
                    idempotency_key: "transfer-approval-atomic".to_string(),
                },
            )
            .await
            .expect("request");

        store
            .seed_assets(vec![InventoryAsset {
                kind: InventoryAssetKind::RawMaterial,
                asset_ref: "raw:2".to_string(),
                custody_warehouse_id: source.warehouse_id.clone(),
                custody_warehouse: source.name.clone(),
                item_code: "M-2".to_string(),
                item_name: "Material 2".to_string(),
                identifier: "RAW-2".to_string(),
                qty: 9.0,
                uom: "kg".to_string(),
                status: "reserved".to_string(),
                physical_location: InventoryLocationRef::from(&source),
                transfer_id: "other-transfer".to_string(),
                placement_version: 1,
            }])
            .await;

        let error = service
            .transfer_action(
                &destination_actor,
                &transfer.id,
                InventoryTransferActionKind::Approve,
                InventoryTransferAction {
                    note: String::new(),
                    idempotency_key: "approve-atomic".to_string(),
                },
            )
            .await
            .expect_err("approval must reject a torn reservation");
        assert_eq!(error, InventoryMovementError::AssetUnavailable);

        let transfers = service
            .transfers(
                &destination_actor,
                InventoryTransferQuery {
                    direction: "incoming".to_string(),
                    ..InventoryTransferQuery::default()
                },
            )
            .await
            .expect("transfers");
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].status, InventoryTransferStatus::Requested);

        let source_assets = service
            .assets(
                &source_actor,
                InventoryAssetQuery {
                    warehouse_id: source.warehouse_id,
                    ..InventoryAssetQuery::default()
                },
            )
            .await
            .expect("source assets");
        let first = source_assets
            .iter()
            .find(|asset| asset.asset_ref == "raw:1")
            .expect("first reserved asset");
        assert_eq!(first.status, "transfer_reserved");
        assert_eq!(first.transfer_id, transfer.id);
    }

    #[tokio::test]
    async fn transfer_between_warehouses_assigned_to_same_actor_completes_immediately() {
        let store = Arc::new(MemoryInventoryMovementStore::new());
        let source = warehouse_location_fixture("warehouse:a", "A ombor");
        let destination = warehouse_location_fixture("warehouse:b", "B ombor");
        store
            .seed_locations(vec![source.clone(), destination.clone()])
            .await;
        store
            .seed_assets(vec![InventoryAsset {
                kind: InventoryAssetKind::RawMaterial,
                asset_ref: "raw:1".to_string(),
                custody_warehouse_id: source.warehouse_id.clone(),
                custody_warehouse: source.name.clone(),
                item_code: "M-1".to_string(),
                item_name: "Material".to_string(),
                identifier: "RAW-1".to_string(),
                qty: 8.0,
                uom: "kg".to_string(),
                status: "available".to_string(),
                physical_location: InventoryLocationRef::from(&source),
                transfer_id: String::new(),
                placement_version: 1,
            }])
            .await;
        let service = InventoryMovementService::new(store);
        let actor = InventoryActor::new(
            principal(PrincipalRole::MaterialTaminotchi, "m1", "Materialchi"),
            false,
            [source.name.clone(), destination.name.clone()],
        );

        let transfer = service
            .create_transfer(
                &actor,
                InventoryTransferCreate {
                    source_warehouse_id: source.warehouse_id.clone(),
                    destination_warehouse_id: destination.warehouse_id.clone(),
                    assets: vec![InventoryAssetSelector {
                        asset_kind: InventoryAssetKind::RawMaterial,
                        asset_ref: "raw:1".to_string(),
                    }],
                    note: String::new(),
                    idempotency_key: "internal-transfer-1".to_string(),
                },
            )
            .await
            .expect("internal transfer");

        assert_eq!(transfer.status, InventoryTransferStatus::Received);
        assert_eq!(transfer.approved_by_name, "Materialchi");
        assert_eq!(transfer.dispatched_by_name, "Materialchi");
        assert_eq!(transfer.received_by_name, "Materialchi");

        let source_assets = service
            .assets(
                &actor,
                InventoryAssetQuery {
                    warehouse_id: source.warehouse_id,
                    ..InventoryAssetQuery::default()
                },
            )
            .await
            .expect("source assets");
        assert!(source_assets.is_empty());

        let destination_assets = service
            .assets(
                &actor,
                InventoryAssetQuery {
                    warehouse_id: destination.warehouse_id,
                    ..InventoryAssetQuery::default()
                },
            )
            .await
            .expect("destination assets");
        assert_eq!(destination_assets.len(), 1);
        assert_eq!(destination_assets[0].status, "available");
        assert!(destination_assets[0].transfer_id.is_empty());
        assert_eq!(destination_assets[0].physical_location.id, destination.id);
    }

    #[tokio::test]
    async fn rejected_transfer_releases_asset_and_action_keys_are_operation_scoped() {
        let store = Arc::new(MemoryInventoryMovementStore::new());
        let source = warehouse_location_fixture("warehouse:a", "A ombor");
        let destination = warehouse_location_fixture("warehouse:b", "B ombor");
        store
            .seed_locations(vec![source.clone(), destination.clone()])
            .await;
        store
            .seed_assets(vec![InventoryAsset {
                kind: InventoryAssetKind::FinishedGoods,
                asset_ref: "finished:1".to_string(),
                custody_warehouse_id: source.warehouse_id.clone(),
                custody_warehouse: source.name.clone(),
                item_code: "T-1".to_string(),
                item_name: "Tayyor mahsulot".to_string(),
                identifier: "FG-1".to_string(),
                qty: 12.0,
                uom: "dona".to_string(),
                status: "available".to_string(),
                physical_location: InventoryLocationRef::from(&source),
                transfer_id: String::new(),
                placement_version: 1,
            }])
            .await;
        let service = InventoryMovementService::new(store);
        let source_actor = InventoryActor::new(
            principal(PrincipalRole::Werka, "w1", "Werka"),
            false,
            ["A ombor".to_string()],
        );
        let destination_actor = InventoryActor::new(
            principal(PrincipalRole::Qolipchi, "q1", "Qolipchi"),
            false,
            ["B ombor".to_string()],
        );

        let transfer = service
            .create_transfer(
                &source_actor,
                InventoryTransferCreate {
                    source_warehouse_id: source.warehouse_id.clone(),
                    destination_warehouse_id: destination.warehouse_id,
                    assets: vec![InventoryAssetSelector {
                        asset_kind: InventoryAssetKind::FinishedGoods,
                        asset_ref: "finished:1".to_string(),
                    }],
                    note: String::new(),
                    idempotency_key: "transfer-reject-1".to_string(),
                },
            )
            .await
            .expect("request");
        service
            .transfer_action(
                &destination_actor,
                &transfer.id,
                InventoryTransferActionKind::Reject,
                InventoryTransferAction {
                    note: String::new(),
                    idempotency_key: "reject-1".to_string(),
                },
            )
            .await
            .expect("reject");

        let assets = service
            .assets(
                &source_actor,
                InventoryAssetQuery {
                    warehouse_id: source.warehouse_id,
                    ..InventoryAssetQuery::default()
                },
            )
            .await
            .expect("source assets");
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].status, "available");
        assert!(assets[0].transfer_id.is_empty());

        let conflict = service
            .transfer_action(
                &source_actor,
                &transfer.id,
                InventoryTransferActionKind::Cancel,
                InventoryTransferAction {
                    note: String::new(),
                    idempotency_key: "reject-1".to_string(),
                },
            )
            .await
            .expect_err("same action key cannot identify another operation");
        assert_eq!(conflict, InventoryMovementError::IdempotencyConflict);
    }
}
