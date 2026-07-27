use super::*;

fn warehouse_location(id: &str, name: &str) -> InventoryLocation {
    InventoryLocation {
        id: format!("inventory_location:{id}"),
        kind: InventoryLocationKind::Warehouse,
        name: name.to_string(),
        warehouse_id: id.to_string(),
        factory_location_id: String::new(),
        active: true,
        apparatus: Vec::new(),
    }
}

#[tokio::test]
async fn assigned_warehouse_users_complete_bilateral_inventory_transfer() {
    let mut state = test_state();
    let store = Arc::new(MemoryInventoryMovementStore::new());
    let source = warehouse_location("warehouse:material", "Material ombor");
    let destination = warehouse_location("warehouse:qolip", "Qolip ombor");
    store
        .seed_locations(vec![source.clone(), destination.clone()])
        .await;
    store
        .seed_assets(vec![InventoryAsset {
            kind: InventoryAssetKind::RawMaterial,
            asset_ref: "raw:qr-001".to_string(),
            custody_warehouse_id: source.warehouse_id.clone(),
            custody_warehouse: source.name.clone(),
            item_code: "PE-001".to_string(),
            item_name: "Polietilen".to_string(),
            identifier: "QR-001".to_string(),
            qty: 25.0,
            uom: "kg".to_string(),
            status: "available".to_string(),
            physical_location: InventoryLocationRef::from(&source),
            transfer_id: String::new(),
            placement_version: 1,
        }])
        .await;
    state.inventory_movements = InventoryMovementService::new(store);
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-1",
        "Material ombor",
    )
    .await;
    assign_warehouse_to_principal(&state, PrincipalRole::Qolipchi, "qolip-1", "Qolip ombor").await;
    let source_token = session_for(&state, PrincipalRole::MaterialTaminotchi, "material-1").await;
    let destination_token = session_for(&state, PrincipalRole::Qolipchi, "qolip-1").await;

    let requested = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/inventory/transfers",
            &source_token,
            r#"{
                "source_warehouse_id":"warehouse:material",
                "destination_warehouse_id":"warehouse:qolip",
                "assets":[{"asset_kind":"raw_material","asset_ref":"raw:qr-001"}],
                "note":"Kelishilgan transfer",
                "idempotency_key":"route-transfer-1"
            }"#,
        ))
        .await
        .expect("request transfer");
    assert_eq!(requested.status(), StatusCode::OK);
    let requested = json_body(requested).await;
    let transfer_id = requested["id"].as_str().expect("transfer id").to_string();
    assert_eq!(requested["status"], "requested");
    assert_eq!(requested["lines"][0]["qty"], 25.0);

    let source_cannot_approve = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            &format!("/v1/mobile/admin/inventory/transfers/{transfer_id}/approve"),
            &source_token,
            r#"{"idempotency_key":"route-approve-wrong"}"#,
        ))
        .await
        .expect("source approval");
    assert_eq!(source_cannot_approve.status(), StatusCode::FORBIDDEN);

    for (token, action, key, expected) in [
        (
            destination_token.as_str(),
            "approve",
            "route-approve-1",
            "approved",
        ),
        (
            source_token.as_str(),
            "dispatch",
            "route-dispatch-1",
            "in_transit",
        ),
        (
            destination_token.as_str(),
            "receive",
            "route-receive-1",
            "received",
        ),
    ] {
        let response = build_router(state.clone())
            .oneshot(request_with_body(
                "POST",
                &format!("/v1/mobile/admin/inventory/transfers/{transfer_id}/{action}"),
                token,
                &format!(r#"{{"idempotency_key":"{key}"}}"#),
            ))
            .await
            .expect("transfer action");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_body(response).await["status"], expected);
    }

    let destination_assets = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/inventory/assets?warehouse_id=warehouse%3Aqolip",
            &destination_token,
        ))
        .await
        .expect("destination assets");
    assert_eq!(destination_assets.status(), StatusCode::OK);
    let destination_assets = json_body(destination_assets).await;
    assert_eq!(destination_assets[0]["qty"], 25.0);
    assert_eq!(destination_assets[0]["custody_warehouse"], "Qolip ombor");
}

#[tokio::test]
async fn state_relocation_preserves_custody_and_quantity() {
    let mut state = test_state();
    let store = Arc::new(MemoryInventoryMovementStore::new());
    let warehouse = warehouse_location("warehouse:material", "Material ombor");
    let factory_state = InventoryLocation {
        id: "inventory_location:state:bosma".to_string(),
        kind: InventoryLocationKind::State,
        name: "Bosma oldi".to_string(),
        warehouse_id: String::new(),
        factory_location_id: "state_bosma".to_string(),
        active: true,
        apparatus: Vec::new(),
    };
    store
        .seed_locations(vec![warehouse.clone(), factory_state.clone()])
        .await;
    store
        .seed_assets(vec![InventoryAsset {
            kind: InventoryAssetKind::FinishedGoods,
            asset_ref: "fg:1".to_string(),
            custody_warehouse_id: warehouse.warehouse_id.clone(),
            custody_warehouse: warehouse.name.clone(),
            item_code: "FG-1".to_string(),
            item_name: "Tayyor mahsulot".to_string(),
            identifier: "FG-1".to_string(),
            qty: 8.0,
            uom: "dona".to_string(),
            status: "available".to_string(),
            physical_location: InventoryLocationRef::from(&warehouse),
            transfer_id: String::new(),
            placement_version: 1,
        }])
        .await;
    state.inventory_movements = InventoryMovementService::new(store);
    assign_warehouse_to_principal(&state, PrincipalRole::Werka, "werka-1", "Material ombor").await;
    let token = session_for(&state, PrincipalRole::Werka, "werka-1").await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/inventory/relocations",
            &token,
            r#"{
                "asset_kind":"finished_goods",
                "asset_ref":"fg:1",
                "physical_location_id":"inventory_location:state:bosma",
                "idempotency_key":"route-relocate-1"
            }"#,
        ))
        .await
        .expect("relocate");
    assert_eq!(response.status(), StatusCode::OK);
    let moved = json_body(response).await;
    assert_eq!(moved["qty"], 8.0);
    assert_eq!(moved["custody_warehouse"], "Material ombor");
    assert_eq!(moved["physical_location"]["name"], "Bosma oldi");
}
