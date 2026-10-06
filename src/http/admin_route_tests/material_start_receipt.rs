use super::*;
use crate::core::system_users::SystemUserUpsert;

const APPARATUS: &str = "apparatus:default:bosma_7";
const ORDER: &str = "zakaz-delivery-receipt";
const ENDPOINT: &str = "/v1/mobile/admin/raw-material-start-receipt";

async fn fixture() -> (AppState, String, Arc<MemoryInventoryMovementStore>) {
    let mut state = test_state();
    state.gscale =
        GscaleService::new().with_receipt_store(Arc::new(RawMaterialStockLookup::default()));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "delivery-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![APPARATUS.into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    for (id, warehouse) in [
        ("allowed-mover", "Kalidor"),
        ("other-mover", "Other warehouse"),
    ] {
        state
            .system_users
            .upsert_user(SystemUserUpsert {
                id: id.into(),
                role: PrincipalRole::Qolipchi,
                name: id.into(),
                phone: format!(
                    "+99890{}",
                    if id == "allowed-mover" {
                        "1111111"
                    } else {
                        "2222222"
                    }
                ),
            })
            .await
            .unwrap();
        assign_warehouse_to_principal(&state, PrincipalRole::Qolipchi, id, warehouse).await;
    }
    let store = Arc::new(MemoryInventoryMovementStore::new());
    let warehouse = InventoryLocation {
        id: "location:warehouse".into(),
        kind: InventoryLocationKind::Warehouse,
        name: "Kalidor".into(),
        warehouse_id: "warehouse:kalidor".into(),
        factory_location_id: String::new(),
        active: true,
        apparatus: vec![],
    };
    let location = InventoryLocation {
        id: "location:machine".into(),
        kind: InventoryLocationKind::State,
        name: "Bosma oldi".into(),
        warehouse_id: String::new(),
        factory_location_id: "state:bosma".into(),
        active: true,
        apparatus: vec![InventoryLocationApparatus {
            id: APPARATUS.into(),
            name: "Bosma".into(),
        }],
    };
    store
        .seed_locations(vec![warehouse.clone(), location])
        .await;
    store
        .seed_assets(vec![InventoryAsset {
            kind: InventoryAssetKind::RawMaterial,
            asset_ref: "raw:30AA".into(),
            custody_warehouse_id: warehouse.warehouse_id.clone(),
            custody_warehouse: warehouse.name.clone(),
            item_code: "INK-BLACK".into(),
            item_name: "Black ink".into(),
            identifier: "30AA".into(),
            qty: 12.0,
            uom: "Kg".into(),
            status: "available".into(),
            physical_location: InventoryLocationRef::from(&warehouse),
            transfer_id: String::new(),
            placement_version: 0,
        }])
        .await;
    state.inventory_movements = InventoryMovementService::new(store.clone());
    let admin = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state.clone());
    for (path, body) in [
        (
            "/v1/mobile/admin/production-maps",
            pechat_order_map_json(ORDER, "Delivery receipt", "0051", APPARATUS),
        ),
        (
            "/v1/mobile/admin/raw-material-rules",
            canonical_material_policy_body(
                APPARATUS,
                1,
                serde_json::json!({"mode":"all_required","item_group_ids":["Kraska"]}),
                false,
            ),
        ),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body("PUT", path, &admin, &body))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{:?}",
            json_body(response).await
        );
    }
    let response = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &admin,
            &serde_json::json!({"order_id":ORDER,"apparatus":APPARATUS,"barcode":"30AA"})
                .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{:?}",
        json_body(response).await
    );
    let worker = session_for(&state, PrincipalRole::Aparatchi, "delivery-worker").await;
    (state, worker, store)
}

fn receipt_body(deliverer: &str) -> String {
    serde_json::json!({"order_id":ORDER,"apparatus":APPARATUS,"barcode":"30AA",
        "delivered_by_role":"qolipchi","delivered_by_ref":deliverer,
        "idempotency_key":"delivery-receipt-1"})
    .to_string()
}

#[tokio::test]
async fn material_start_receipt_filters_deliverers_receives_once_and_enables_start_scan() {
    let (state, worker, store) = fixture().await;
    let router = build_router(state.clone());
    let preview = router
        .clone()
        .oneshot(request_with_body(
            "GET",
            &format!("{ENDPOINT}?order_id={ORDER}&apparatus={APPARATUS}&barcode=30AA"),
            &worker,
            "",
        ))
        .await
        .unwrap();
    assert_eq!(
        preview.status(),
        StatusCode::OK,
        "{:?}",
        json_body(preview).await
    );
    let preview = router
        .clone()
        .oneshot(request_with_body(
            "GET",
            &format!("{ENDPOINT}?order_id={ORDER}&apparatus={APPARATUS}&barcode=30AA"),
            &worker,
            "",
        ))
        .await
        .unwrap();
    let preview = json_body(preview).await;
    assert_eq!(preview["already_at_apparatus"], false);
    assert_eq!(preview["deliverers"].as_array().unwrap().len(), 1);
    assert_eq!(preview["deliverers"][0]["ref"], "allowed-mover");
    let rejected = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            ENDPOINT,
            &worker,
            &receipt_body("other-mover"),
        ))
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(rejected).await["error"],
        "material_deliverer_not_allowed"
    );
    for _ in 0..2 {
        let received = router
            .clone()
            .oneshot(request_with_body(
                "POST",
                ENDPOINT,
                &worker,
                &receipt_body("allowed-mover"),
            ))
            .await
            .unwrap();
        assert_eq!(
            received.status(),
            StatusCode::OK,
            "{:?}",
            json_body(received).await
        );
    }
    let actor = crate::core::inventory_movements::InventoryActor::new(
        Principal {
            role: PrincipalRole::Admin,
            display_name: "Admin".into(),
            ref_: "admin".into(),
            legal_name: String::new(),
            phone: String::new(),
            avatar_url: String::new(),
        },
        true,
        [],
    );
    let assets = state
        .inventory_movements
        .assets(
            &actor,
            crate::core::inventory_movements::InventoryAssetQuery::default(),
        )
        .await
        .unwrap();
    assert_eq!(assets[0].physical_location.id, "location:machine");
    assert_eq!(assets[0].placement_version, 1);
    let audit = store
        .delivery_receipt_audit("delivery-receipt-1")
        .await
        .unwrap();
    assert_eq!(audit["received_by"]["ref"], "delivery-worker");
    assert_eq!(audit["delivered_by"]["ref"], "allowed-mover");
    let requirements = router.clone().oneshot(request_with_body("GET",
        &format!("/v1/mobile/admin/raw-material-start-requirements?order_id={ORDER}&apparatus={APPARATUS}&material_barcodes=30AA"),
        &worker, "")).await.unwrap();
    assert_eq!(requirements.status(), StatusCode::OK);
    assert_eq!(json_body(requirements).await["scan_satisfied"], true);
    let started = router.oneshot(request_with_body("POST", "/v1/mobile/admin/production-maps/queue-action", &worker,
        &serde_json::json!({"order_id":ORDER,"apparatus":APPARATUS,"action":"start","material_barcodes":["30AA"]}).to_string())).await.unwrap();
    assert_eq!(
        started.status(),
        StatusCode::OK,
        "{:?}",
        json_body(started).await
    );
    assert_eq!(
        state
            .gscale
            .raw_material_stock_by_barcode("30AA")
            .await
            .unwrap()
            .unwrap()
            .status,
        "in_use"
    );
}

#[tokio::test]
async fn material_start_receipt_rejects_unassigned_operator_and_unassigned_barcode() {
    let (state, worker, _) = fixture().await;
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "unassigned-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec!["apparatus:default:bosma_8".into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let other = session_for(&state, PrincipalRole::Aparatchi, "unassigned-worker").await;
    let router = build_router(state);
    let response = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            ENDPOINT,
            &other,
            &receipt_body("allowed-mover"),
        ))
        .await
        .unwrap();
    assert!(!response.status().is_success());
    assert_eq!(json_body(response).await["error"], "apparatus_not_assigned");
    let response = router
        .oneshot(request_with_body(
            "GET",
            &format!("{ENDPOINT}?order_id={ORDER}&apparatus={APPARATUS}&barcode=30CC"),
            &worker,
            "",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(response).await["error"], "raw_material_mismatch");
}

#[tokio::test]
async fn material_start_receipt_rechecks_order_freeze_before_moving_material() {
    let (state, worker, store) = fixture().await;
    state
        .production_maps
        .apply_apparatus_queue_action_with_material_scan_and_progress(
            crate::core::production_map::MaterialScanProgressAction {
                apparatus: APPARATUS,
                order_id: ORDER,
                action: crate::core::production_map::queue_state::ApparatusQueueAction::Start,
                assigned_apparatus: &[APPARATUS.into()],
                actor: crate::core::production_map::QueueActionActor {
                    role: "aparatchi".into(),
                    ref_: "delivery-worker".into(),
                    display_name: "Operator".into(),
                },
                material_barcodes: &["30AA".into()],
                state_material_barcodes: &["30AA".into()],
                progress: crate::core::production_map::QueueProgressInput::default(),
                qolip_validation: None,
            },
        )
        .await
        .unwrap();
    let control = state
        .production_maps
        .request_order_freeze(
            ORDER,
            crate::core::production_map::QueueActionActor {
                role: "admin".into(),
                ref_: "admin".into(),
                display_name: "Admin".into(),
            },
        )
        .await
        .unwrap();
    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            ENDPOINT,
            &worker,
            &receipt_body("allowed-mover"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let error = match control.state {
        crate::core::production_map::OrderControlState::Frozen => "order_frozen",
        crate::core::production_map::OrderControlState::FreezeRequested => "order_freeze_requested",
        _ => panic!("expected a frozen order or an active freeze request"),
    };
    assert_eq!(json_body(response).await["error"], error);
    assert!(
        store
            .delivery_receipt_audit("delivery-receipt-1")
            .await
            .is_none()
    );
}

#[tokio::test]
async fn material_start_receipt_rechecks_deliverer_permission_after_selection() {
    let (state, worker, store) = fixture().await;
    let router = build_router(state.clone());
    let response = router
        .clone()
        .oneshot(request_with_body(
            "GET",
            &format!("{ENDPOINT}?order_id={ORDER}&apparatus={APPARATUS}&barcode=30AA"),
            &worker,
            "",
        ))
        .await
        .unwrap();
    assert_eq!(
        json_body(response).await["deliverers"][0]["ref"],
        "allowed-mover"
    );
    state
        .warehouses
        .unassign_warehouse(crate::core::warehouses::WarehouseAssignmentDeleteRequest {
            assignment_kind: "warehouse".into(),
            warehouse: "Kalidor".into(),
            warehouse_name: None,
            apparatus_id: None,
            principal_role: PrincipalRole::Qolipchi,
            principal_ref: "allowed-mover".into(),
        })
        .await
        .unwrap();
    let response = router
        .oneshot(request_with_body(
            "POST",
            ENDPOINT,
            &worker,
            &receipt_body("allowed-mover"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(response).await["error"],
        "material_deliverer_not_allowed"
    );
    assert!(
        store
            .delivery_receipt_audit("delivery-receipt-1")
            .await
            .is_none()
    );
}
