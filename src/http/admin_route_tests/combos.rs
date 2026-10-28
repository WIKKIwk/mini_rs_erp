use super::*;
use serde_json::json;

#[tokio::test]
async fn combo_routes_reject_unauthorized_and_unassigned_tooling_readers() {
    let state = test_state();
    let supplier = session(&state, PrincipalRole::Supplier).await;
    let qolip = session_for(&state, PrincipalRole::Qolipchi, "unassigned-combo-reader").await;
    let router = build_router(state);
    for (method, token, status) in [
        ("GET", "", StatusCode::UNAUTHORIZED),
        ("POST", supplier.as_str(), StatusCode::FORBIDDEN),
        ("GET", qolip.as_str(), StatusCode::FORBIDDEN),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                method,
                "/v1/mobile/admin/combos?item_code=COMBO-R1",
                token,
                "{}",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), status, "{}", json_body(response).await);
    }
}

#[tokio::test]
async fn combo_order_rejects_forged_primary_identity_before_hydration_without_writes() {
    let state = test_state();
    let admin = session(&state, PrincipalRole::Admin).await;
    let map: serde_json::Value = serde_json::from_str(&pechat_order_map_json(
        "zakaz-combo-forged",
        "Named combo",
        "7789",
        "apparatus:default:bosma_8",
    ))
    .unwrap();
    let mut template = crate::core::calculate_orders::CalculateOrderTemplate {
        products: crate::core::order_layout::tests::products(),
        customer_ref: "CUST-001".into(),
        customer: "Customer".into(),
        kg: 300.0,
        edge_allowance_mm: 15.0,
        layers: vec![crate::core::formula::LayerInput::new("pet", "12")],
        name: "Named combo".into(),
        product: "Named combo".into(),
        item_code: "COMBO-R1".into(),
        ..Default::default()
    };
    let fingerprint = crate::core::combo::layout_fingerprint(&template).unwrap();
    template.combo = Some(crate::core::combo::ComboReference {
        id: "combo-master".into(),
        item_code: "COMBO-R1".into(),
        name: "Named combo".into(),
        revision: 1,
        layout_fingerprint: fingerprint,
    });
    template.item_code = "SPOOFED".into();
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps/with-order",
            &admin,
            &json!({"map":map,"template":template}).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(response).await["error"],
        "combo_reference_mismatch"
    );
    assert!(
        state
            .production_maps
            .raw_map("zakaz-combo-forged")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn combo_cannot_be_installed_through_map_only_update() {
    let state = test_state();
    let admin = session(&state, PrincipalRole::Admin).await;
    let mut map: serde_json::Value = serde_json::from_str(&pechat_order_map_json(
        "combo-map-only",
        "Named combo",
        "7789",
        "apparatus:default:bosma_8",
    ))
    .unwrap();
    map["combo"] = json!({"id":"combo-master","item_code":"COMBO-R1","name":"Named combo","revision":1,"layout_fingerprint":"forged"});
    map["products"] = json!(crate::core::order_layout::tests::products());
    map["product_code"] = json!("COMBO-R1");
    let response = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin,
            &map.to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(response).await["error"],
        "combo_template_required"
    );
    assert!(
        state
            .production_maps
            .raw_map("combo-map-only")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn combo_order_waits_for_its_own_tooling_and_does_not_use_child_molds() {
    use crate::core::qolip::{
        MemoryQolipStore, QolipProduct, QolipProductSpec, QolipService, QolipStorePort,
    };
    let store = Arc::new(MemoryQolipStore::new());
    let mut products = crate::core::order_layout::tests::products()[..2].to_vec();
    for product in &mut products {
        product.frame_count = 1;
    }
    let mut items = products
        .iter()
        .map(|p| QolipProduct {
            code: p.item_code.clone(),
            name: p.product.clone(),
            item_group: "Tayyor mahsulot".into(),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    items.push(QolipProduct {
        code: "COMBO-TOOLING".into(),
        name: "Combo tooling".into(),
        item_group: "Kombo".into(),
        ..Default::default()
    });
    store.seed_products(items).await;
    store
        .put_product_spec(QolipProductSpec {
            item_code: products[0].item_code.clone(),
            item_name: products[0].product.clone(),
            item_group: "Tayyor mahsulot".into(),
            warehouse: "Molds".into(),
            qolip_code: "CHILD-MOLD".into(),
            size: 650,
            ..Default::default()
        })
        .await
        .unwrap();
    let mut state = test_state();
    state.qolip = QolipService::new(store.clone());
    let admin = session(&state, PrincipalRole::Admin).await;
    let mut map: serde_json::Value = serde_json::from_str(&pechat_order_map_json(
        "zakaz-combo-tooling",
        "Combo tooling",
        "7790",
        "apparatus:default:bosma_8",
    ))
    .unwrap();
    map["product_code"] = json!("COMBO-TOOLING");
    map["products"] = json!(products);
    map["combo"] = json!({"id":"combo-tooling","item_code":"COMBO-TOOLING","name":"Combo tooling","revision":1,"layout_fingerprint":"test-layout"});
    // Tooling is intentionally allowed to be prepared after the production map.
    state
        .production_maps
        .upsert_map(serde_json::from_value(map).unwrap())
        .await
        .unwrap();
    let router = build_router(state.clone());
    let body = json!({"order_id":"zakaz-combo-tooling","apparatus":"apparatus:default:bosma_8","qolip_code":""}).to_string();
    let response = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/qolip-validate",
            &admin,
            &body,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(response).await["error"], "combo_qolip_required");
    let response = router.clone().oneshot(request_with_body("POST",
        "/v1/mobile/admin/production-maps/queue-action", &admin,
        &json!({"order_id":"zakaz-combo-tooling","apparatus":"apparatus:default:bosma_8","action":"start"}).to_string())).await.unwrap();
    let status = response.status();
    let blocked = json_body(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{blocked}");
    assert_eq!(blocked["error"], "combo_qolip_required");

    store
        .put_product_spec(QolipProductSpec {
            item_code: "COMBO-TOOLING".into(),
            item_name: "Combo tooling".into(),
            item_group: "Kombo".into(),
            warehouse: "Molds".into(),
            qolip_code: "COMBO-MOLD".into(),
            size: 650,
            ..Default::default()
        })
        .await
        .unwrap();
    let response = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/qolip-validate",
            &admin,
            &body,
        ))
        .await
        .unwrap();
    let status = response.status();
    let body = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["qolip"]["required_qolip_codes"], json!(["COMBO-MOLD"]));
}

#[tokio::test]
async fn combo_create_requires_catalog_creation_as_well_as_production_permission() {
    let state = test_state();
    state
        .admin
        .upsert_role_definition(crate::core::authz::RoleDefinitionUpsert {
            id: "combo-planner-readonly-catalog".into(),
            label: "Planner".into(),
            base_role: None,
            capability_codes: vec!["production.map.manage".into()],
        })
        .await
        .unwrap();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "combo-planner".into(),
            role_id: "combo-planner-readonly-catalog".into(),
            assigned_apparatus: vec![],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let token = session_for(&state, PrincipalRole::Aparatchi, "combo-planner").await;
    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/combos",
            &token,
            "{}",
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn combo_quick_template_rejects_forged_or_stale_reference_without_writes() {
    let state = test_state();
    let admin = session(&state, PrincipalRole::Admin).await;
    let mut original = crate::core::calculate_orders::CalculateOrderTemplate {
        products: crate::core::order_layout::tests::products(),
        customer_ref: "CUST-001".into(),
        customer: "Customer".into(),
        kg: 300.0,
        edge_allowance_mm: 15.0,
        layers: vec![crate::core::formula::LayerInput::new("pet", "12")],
        name: "Combo draft".into(),
        product: "Combo draft".into(),
        item_code: "COMBO-R1".into(),
        ..Default::default()
    };
    let fingerprint = crate::core::combo::layout_fingerprint(&original).unwrap();
    original.combo = Some(crate::core::combo::ComboReference {
        id: "combo-master".into(),
        item_code: "COMBO-R1".into(),
        name: "Combo draft".into(),
        revision: 1,
        layout_fingerprint: fingerprint,
    });
    for stale in [false, true] {
        let mut submitted = original.clone();
        if stale {
            submitted.products[0].frame_product_size_mm += 10.0;
        } else {
            submitted.item_code = "SPOOFED".into();
        }
        let response = build_router(state.clone())
            .oneshot(request_with_body(
                "POST",
                "/v1/mobile/calculate/orders",
                &admin,
                &serde_json::to_string(&submitted).unwrap(),
            ))
            .await
            .unwrap();
        let status = response.status();
        let payload = json_body(response).await;
        assert_eq!(status, StatusCode::CONFLICT, "{payload}");
        assert_eq!(payload["error"], "combo_reference_mismatch");
        assert!(
            state
                .calculate_orders
                .list("admin:19621978")
                .await
                .unwrap()
                .is_empty()
        );
    }
}
