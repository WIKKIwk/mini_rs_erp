use super::*;

#[tokio::test]
async fn raw_material_assignment_orders_only_return_active_orders() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-assignment-orders".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: vec!["Kraska".to_string()],
        })
        .await
        .expect("material assignment");
    let token = session(&state, PrincipalRole::Admin).await;
    let material_token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-assignment-orders",
    )
    .await;
    let router = build_router(state);

    for (id, title, code) in [
        ("zakaz-raw-orders", "Raw order", "8811"),
        (
            "template-zakaz-raw-orders",
            "Raw order template",
            "template-8811",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps",
                &token,
                &pechat_order_map_json(id, title, code, "apparatus:default:bosma_7"),
            ))
            .await
            .expect("production map save");
        assert_eq!(response.status(), StatusCode::OK);
    }

    let response = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments/orders",
            &material_token,
        ))
        .await
        .expect("raw material assignment orders");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body.as_array().map(Vec::len), Some(1));
    assert_eq!(body[0]["map"]["id"], "zakaz-raw-orders");
}

#[tokio::test]
async fn raw_material_assignment_candidates_only_return_assignable_stock() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    material_store
        .set_stock_status("30CC", "in_use", "zakaz-other")
        .await;
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store.clone());
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-candidates".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:bosma_7".to_string()],
            assigned_item_groups: vec!["Kraska".to_string()],
        })
        .await
        .expect("material assignment");
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-candidates",
        "Kalidor",
    )
    .await;
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let material_token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-candidates",
    )
    .await;
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-candidates",
                "Candidate order",
                "8813",
                "apparatus:default:bosma_7",
            ),
        ))
        .await
        .expect("map save");
    assert_eq!(map.status(), StatusCode::OK);
    let rule = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/raw-material-rules",
            &admin_token,
            &canonical_requirement_set_material_policy_body(
                "apparatus:default:bosma_7",
                1,
                &["Kraska"],
                true,
            ),
        ))
        .await
        .expect("rule save");
    assert_eq!(rule.status(), StatusCode::OK);

    let candidates = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments/candidates?order_id=zakaz-candidates",
            &material_token,
        ))
        .await
        .expect("assignment candidates");
    assert_eq!(candidates.status(), StatusCode::OK);
    let body = json_body(candidates).await;
    assert_eq!(body.as_array().map(Vec::len), Some(1));
    assert_eq!(body[0]["barcode"], "30AA");
    assert_eq!(body[0]["warehouse"], "Kalidor");
    assert_eq!(body[0]["item_group"], "Kraska");
    assert_eq!(body[0]["apparatus_options"][0], "apparatus:default:bosma_7");

    let forbidden_apparatus = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments/candidates?order_id=zakaz-candidates&apparatus=apparatus%3Adefault%3Aasset-007",
            &material_token,
        ))
        .await
        .expect("unassigned apparatus candidates");
    assert_eq!(forbidden_apparatus.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(forbidden_apparatus).await["error"],
        "apparatus_not_assigned"
    );

    material_store
        .set_stock_status("30AA", "in_use", "zakaz-other")
        .await;
    let unavailable = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &material_token,
            r#"{
                "order_id":"zakaz-candidates",
                "barcode":"30AA",
                "apparatus":"apparatus:default:bosma_7"
            }"#,
        ))
        .await
        .expect("assign stale candidate");
    assert_eq!(unavailable.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(unavailable).await["error"],
        "raw_material_stock_unavailable"
    );
    material_store
        .set_stock_status("30AA", "available", "")
        .await;

    let assigned = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &material_token,
            r#"{
                "order_id":"zakaz-candidates",
                "barcode":"30AA",
                "apparatus":"apparatus:default:bosma_7"
            }"#,
        ))
        .await
        .expect("assign candidate");
    assert_eq!(assigned.status(), StatusCode::OK);

    let candidates = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments/candidates?order_id=zakaz-candidates",
            &material_token,
        ))
        .await
        .expect("assignment candidates after assign");
    assert_eq!(candidates.status(), StatusCode::OK);
    assert_eq!(
        json_body(candidates).await.as_array().map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn raw_material_assignment_candidates_rank_rulons_by_smallest_leftover() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    for (barcode, code, name) in [
        ("30R985", "ROLL-1000", "CPP 985/35"),
        ("30R1000", "ROLL-1000", "CPP 1000/35"),
        ("30R1005", "ROLL-1000", "CPP 1005/35"),
    ] {
        material_store.insert_stock(barcode, code, name, 10.0).await;
    }
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store);
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-ranked-rulons".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:bosma_7".to_string()],
            assigned_item_groups: vec!["Rulon".to_string()],
        })
        .await
        .expect("material assignment");
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-ranked-rulons",
        "Kalidor",
    )
    .await;
    let token = session(&state, PrincipalRole::Admin).await;
    let material_token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-ranked-rulons",
    )
    .await;
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json_with_dims(
                "zakaz-ranked-rulons",
                "Ranked rulons",
                "8816",
                "apparatus:default:bosma_7",
                7,
                985.0,
            ),
        ))
        .await
        .expect("map save");
    assert_eq!(map.status(), StatusCode::OK);

    let rule = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/raw-material-rules",
            &token,
            &canonical_requirement_set_material_policy_body(
                "apparatus:default:bosma_7",
                1,
                &["Rulon"],
                true,
            ),
        ))
        .await
        .expect("rule save");
    assert_eq!(rule.status(), StatusCode::OK);

    let candidates = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments/candidates?order_id=zakaz-ranked-rulons",
            &material_token,
        ))
        .await
        .expect("assignment candidates");
    assert_eq!(candidates.status(), StatusCode::OK);
    let body = json_body(candidates).await;
    assert_eq!(body.as_array().map(Vec::len), Some(3));
    assert_eq!(body[0]["barcode"], "30R985");
    assert_eq!(body[0]["match_type"], "exact_width");
    assert_eq!(body[0]["leftover_width_mm"], 0.0);
    assert_eq!(body[1]["barcode"], "30R1000");
    assert_eq!(body[1]["match_type"], "closest_width");
    assert_eq!(body[1]["leftover_width_mm"], 15.0);
    assert_eq!(body[2]["barcode"], "30R1005");
    assert_eq!(body[2]["leftover_width_mm"], 20.0);
}

#[tokio::test]
async fn raw_material_assignment_candidate_orders_only_return_compatible_orders() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store);
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-candidate-orders".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:bosma_7".to_string()],
            assigned_item_groups: vec!["Kraska".to_string()],
        })
        .await
        .expect("material assignment");
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-candidate-orders",
        "Kalidor",
    )
    .await;
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let material_token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-candidate-orders",
    )
    .await;
    let router = build_router(state);

    for (id, title, code, apparatus) in [
        (
            "zakaz-compatible",
            "Compatible order",
            "8814",
            "apparatus:default:bosma_7",
        ),
        (
            "zakaz-incompatible",
            "Incompatible order",
            "8815",
            "apparatus:default:bosma_8",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/production-maps",
                &admin_token,
                &pechat_order_map_json(id, title, code, apparatus),
            ))
            .await
            .expect("production map save");
        assert_eq!(response.status(), StatusCode::OK);
    }
    for (apparatus, item_group) in [
        ("apparatus:default:bosma_7", "Kraska"),
        ("apparatus:default:bosma_8", "Rulon"),
    ] {
        let response = router
            .clone()
            .oneshot(request_with_body(
                "PUT",
                "/v1/mobile/admin/raw-material-rules",
                &admin_token,
                &canonical_requirement_set_material_policy_body(apparatus, 1, &[item_group], true),
            ))
            .await
            .expect("rule save");
        assert_eq!(response.status(), StatusCode::OK);
    }

    let candidates = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments/candidate-orders?barcode=30AA",
            &material_token,
        ))
        .await
        .expect("candidate orders");
    assert_eq!(candidates.status(), StatusCode::OK);
    let body = json_body(candidates).await;
    assert_eq!(body.as_array().map(Vec::len), Some(1));
    assert_eq!(body[0]["order"]["map"]["id"], "zakaz-compatible");
    assert_eq!(body[0]["apparatus_options"][0], "apparatus:default:bosma_7");

    let assigned = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &material_token,
            r#"{
                "order_id":"zakaz-compatible",
                "barcode":"30AA",
                "apparatus":"apparatus:default:bosma_7"
            }"#,
        ))
        .await
        .expect("assign candidate order");
    assert_eq!(assigned.status(), StatusCode::OK);

    let candidates = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments/candidate-orders?barcode=30AA",
            &material_token,
        ))
        .await
        .expect("candidate orders after assignment");
    assert_eq!(candidates.status(), StatusCode::OK);
    assert_eq!(
        json_body(candidates).await.as_array().map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn raw_material_routes_assign_and_require_scan_for_queue_start() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    material_store
        .insert_stock("30DD", "INK-BLACK", "Black ink", 5.0)
        .await;
    material_store
        .insert_stock("30EE", "INK-BLACK", "Black ink", 4.0)
        .await;
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    let inventory_store = Arc::new(MemoryInventoryMovementStore::new());
    let state_location = InventoryLocation {
        id: "location:state:pechat-a".to_string(),
        kind: InventoryLocationKind::State,
        name: "Pechat A oldi".to_string(),
        warehouse_id: String::new(),
        factory_location_id: "factory:pechat".to_string(),
        active: true,
        apparatus: vec![InventoryLocationApparatus {
            id: "apparatus:default:bosma_7".to_string(),
            name: "Bosma 7".to_string(),
        }],
    };
    inventory_store
        .seed_locations(vec![state_location.clone()])
        .await;
    inventory_store
        .seed_assets(
            ["30AA", "30CC", "30DD"]
                .into_iter()
                .map(|barcode| InventoryAsset {
                    kind: InventoryAssetKind::RawMaterial,
                    asset_ref: format!("raw:{barcode}"),
                    custody_warehouse_id: "warehouse:kalidor".to_string(),
                    custody_warehouse: "Kalidor".to_string(),
                    item_code: "INK-BLACK".to_string(),
                    item_name: "Black ink".to_string(),
                    identifier: barcode.to_string(),
                    qty: 5.0,
                    uom: "Kg".to_string(),
                    status: "available".to_string(),
                    physical_location: InventoryLocationRef::from(&state_location),
                    transfer_id: String::new(),
                    placement_version: 1,
                })
                .collect(),
        )
        .await;
    state.inventory_movements = InventoryMovementService::new(inventory_store);
    state.gscale = GscaleService::new()
        .with_receipt_store(material_store.clone())
        .with_driver(Arc::new(FakeProgressDriver {
            requests: print_requests,
            fail: false,
        }));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-raw-route".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:bosma_7".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("aparatchi assignment");
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-raw-route".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:bosma_7".to_string()],
            assigned_item_groups: vec!["Kraska".to_string()],
        })
        .await
        .expect("material assignment");
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-raw-route",
        "Kalidor",
    )
    .await;
    let token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-raw-route").await;
    let material_token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-raw-route",
    )
    .await;
    let mut warehouse_events = state.warehouse_events.subscribe();
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json(
                "zakaz-raw-route",
                "Raw route",
                "8811",
                "apparatus:default:bosma_7",
            ),
        ))
        .await
        .expect("map save");
    assert_eq!(map.status(), StatusCode::OK);
    provision_test_qolip(&router, &token, "zakaz-raw-route").await;

    let other_map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json(
                "zakaz-raw-other",
                "Tashoq",
                "8812",
                "apparatus:default:bosma_7",
            ),
        ))
        .await
        .expect("other map save");
    assert_eq!(other_map.status(), StatusCode::OK);

    let rule = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/raw-material-rules",
            &token,
            &canonical_material_policy_body(
                "apparatus:default:bosma_7",
                1,
                serde_json::json!({
                    "mode": "requirement_sets",
                    "sets": [{
                        "requirement_id": "Yopishtiruvchi",
                        "item_group_ids": ["Kraska", "Kley"],
                        "minimum_required_count": 1
                    }]
                }),
                true,
            ),
        ))
        .await
        .expect("rule save");
    assert_eq!(rule.status(), StatusCode::OK);
    let rule_body = json_body(rule).await;
    assert_eq!(
        rule_body["revision"]["apparatus_id"],
        "apparatus:default:bosma_7"
    );
    assert_eq!(
        rule_body["revision"]["policies"]["material"]["mode"],
        "requirement_sets"
    );
    assert_eq!(
        rule_body["revision"]["policies"]["material"]["sets"][0]["item_group_ids"],
        serde_json::json!(["Kley", "Kraska"])
    );

    let missing_assignment = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(
                r#"{
                "apparatus":"apparatus:default:bosma_7",
                "order_id":"zakaz-raw-route",
                "action":"start"
            }"#,
                "zakaz-raw-route",
            ),
        ))
        .await
        .expect("queue action without assignment");
    assert_eq!(missing_assignment.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_assignment).await["error"],
        "raw_material_assignment_not_found"
    );

    let assigned = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{
                "order_id":"zakaz-raw-route",
                "barcode":"30AA"
            }"#,
        ))
        .await
        .expect("assign");
    let assigned_status = assigned.status();
    let assigned_body = json_body(assigned).await;
    assert_eq!(assigned_status, StatusCode::OK, "{assigned_body:?}");
    assert_eq!(assigned_body["apparatus"], "apparatus:default:bosma_7");
    assert_eq!(assigned_body["item_code"], "INK-BLACK");
    assert_eq!(assigned_body["item_name"], "Black ink");
    assert_eq!(assigned_body["item_group"], "Kraska");
    let warehouse_event = warehouse_events.recv().await.expect("warehouse event");
    assert_eq!(warehouse_event.event, "warehouse.updated");
    assert_eq!(warehouse_event.warehouse, "Kalidor");
    assert_eq!(warehouse_event.reason, "raw_material_assignment");

    let same_order_diagnostic = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments/diagnostics?barcode=30AA&order_id=zakaz-raw-route",
            &token,
        ))
        .await
        .expect("same-order assignment diagnostic");
    assert_eq!(same_order_diagnostic.status(), StatusCode::OK);
    let same_order_diagnostic_body = json_body(same_order_diagnostic).await;
    assert_eq!(
        same_order_diagnostic_body["reason"],
        "raw_material_already_assigned_to_order"
    );
    assert_eq!(same_order_diagnostic_body["order_title"], "Raw route");

    let other_order_diagnostic = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments/diagnostics?barcode=30AA&order_id=zakaz-raw-other",
            &token,
        ))
        .await
        .expect("other-order assignment diagnostic");
    assert_eq!(other_order_diagnostic.status(), StatusCode::OK);
    let other_order_diagnostic_body = json_body(other_order_diagnostic).await;
    assert_eq!(
        other_order_diagnostic_body["reason"],
        "raw_material_already_assigned"
    );
    assert_eq!(other_order_diagnostic_body["order_title"], "Raw route");

    let assigned_to_other_order = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{
                "order_id":"zakaz-raw-other",
                "barcode":"30AA"
            }"#,
        ))
        .await
        .expect("assign material to occupied order");
    let assigned_to_other_order_status = assigned_to_other_order.status();
    let assigned_to_other_order_body = json_body(assigned_to_other_order).await;
    assert_eq!(
        assigned_to_other_order_status,
        StatusCode::BAD_REQUEST,
        "{assigned_to_other_order_body:?}"
    );
    assert_eq!(
        assigned_to_other_order_body["error"],
        "raw_material_already_assigned"
    );
    assert_eq!(assigned_to_other_order_body["order_title"], "Raw route");

    let material_assignments = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments?order_id=zakaz-raw-route&apparatus=apparatus%3Adefault%3Abosma_7",
            &material_token,
        ))
        .await
        .expect("material assignment list");
    assert_eq!(material_assignments.status(), StatusCode::OK);
    let material_assignments_body = json_body(material_assignments).await;
    assert_eq!(material_assignments_body.as_array().map(Vec::len), Some(1));
    assert_eq!(material_assignments_body[0]["barcode"], "30AA");

    let assigned_edit = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/raw-material-stock",
            &material_token,
            r#"{"barcode":"30AA","item_code":"INK-WHITE","qty":10}"#,
        ))
        .await
        .expect("assigned raw stock correction");
    assert_eq!(assigned_edit.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(assigned_edit).await["error"],
        "raw_material_stock_locked"
    );

    let assigned_delete = router
        .clone()
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/raw-material-stock",
            &material_token,
            r#"{"barcode":"30AA"}"#,
        ))
        .await
        .expect("assigned raw stock deletion");
    assert_eq!(assigned_delete.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(assigned_delete).await["error"],
        "raw_material_stock_locked"
    );

    let duplicate_same_order = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{
                "order_id":"zakaz-raw-route",
                "barcode":"30AA"
            }"#,
        ))
        .await
        .expect("assign same material again");
    assert_eq!(duplicate_same_order.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(duplicate_same_order).await["error"],
        "raw_material_already_assigned_to_order"
    );

    let second_assigned = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{
                "order_id":"zakaz-raw-route",
                "barcode":"30CC"
            }"#,
        ))
        .await
        .expect("assign second");
    let second_status = second_assigned.status();
    let second_body = json_body(second_assigned).await;
    assert_eq!(second_status, StatusCode::OK, "{second_body:?}");
    assert_eq!(second_body["apparatus"], "apparatus:default:bosma_7");
    assert_eq!(second_body["item_code"], "INK-WHITE");
    let _second_warehouse_event = warehouse_events
        .recv()
        .await
        .expect("second warehouse event");

    let not_staged_assigned = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{
                "order_id":"zakaz-raw-route",
                "barcode":"30EE"
            }"#,
        ))
        .await
        .expect("assign material outside apparatus state");
    assert_eq!(not_staged_assigned.status(), StatusCode::OK);
    let _third_warehouse_event = warehouse_events
        .recv()
        .await
        .expect("third warehouse event");

    let lookup = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments/lookup?barcode=30AA",
            &token,
        ))
        .await
        .expect("raw material lookup");
    let lookup_status = lookup.status();
    let lookup_body = json_body(lookup).await;
    assert_eq!(lookup_status, StatusCode::OK, "{lookup_body:?}");
    assert_eq!(lookup_body["barcode"], "30AA");
    assert_eq!(lookup_body["warehouse"], "Kalidor");
    assert_eq!(lookup_body["item_code"], "INK-BLACK");
    assert_eq!(lookup_body["item_name"], "Black ink");
    assert_eq!(lookup_body["item_group"], "Kraska");
    assert_eq!(lookup_body["qty"], 12.0);
    assert_eq!(lookup_body["uom"], "Kg");
    assert_eq!(lookup_body["status"], "available");
    assert_eq!(lookup_body["source_receipt_id"], "GSR-30AA");
    assert_eq!(lookup_body["assignment"]["order_id"], "zakaz-raw-route");
    assert_eq!(
        lookup_body["assignment"]["apparatus"],
        "apparatus:default:bosma_7"
    );
    assert_eq!(lookup_body["order"]["id"], "zakaz-raw-route");
    assert!(lookup_body["queue_states"].is_object());
    assert!(lookup_body["logs"].is_array());

    let scoped_assignments = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments?order_id=zakaz-raw-route&apparatus=apparatus%3Adefault%3Abosma_7",
            &worker_token,
        ))
        .await
        .expect("scoped raw material assignments");
    assert_eq!(scoped_assignments.status(), StatusCode::OK);
    let scoped_assignments_body = json_body(scoped_assignments).await;
    assert_eq!(scoped_assignments_body.as_array().map(Vec::len), Some(3));
    assert!(scoped_assignments_body.as_array().is_some_and(|items| {
        items.iter().all(|item| {
            item["order_id"] == "zakaz-raw-route"
                && item["apparatus"] == "apparatus:default:bosma_7"
        })
    }));

    let start_requirements = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-start-requirements?order_id=zakaz-raw-route&apparatus=apparatus%3Adefault%3Abosma_7&material_barcodes=30AA",
            &worker_token,
        ))
        .await
        .expect("raw material start requirements");
    assert_eq!(start_requirements.status(), StatusCode::OK);
    let start_requirements_body = json_body(start_requirements).await;
    assert_eq!(start_requirements_body["policy"], "requirement_groups");
    assert_eq!(start_requirements_body["required_scan_count"], 1);
    assert_eq!(start_requirements_body["matched_scan_count"], 1);
    assert_eq!(start_requirements_body["assignments_satisfied"], true);
    assert_eq!(start_requirements_body["scan_satisfied"], true);
    assert_eq!(
        start_requirements_body["assignments"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );
    assert_eq!(
        start_requirements_body["start_assignments"]
            .as_array()
            .map(Vec::len),
        Some(3)
    );

    let intake_before_start = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-intake",
            &worker_token,
            r#"{
                "order_id":"zakaz-raw-route",
                "apparatus":"apparatus:default:bosma_7",
                "barcode":"30CC"
            }"#,
        ))
        .await
        .expect("intake before start");
    let intake_before_start_status = intake_before_start.status();
    let intake_before_start_body = json_body(intake_before_start).await;
    assert_eq!(
        intake_before_start_status,
        StatusCode::CONFLICT,
        "{intake_before_start_body:?}"
    );
    assert_eq!(
        intake_before_start_body["error"],
        "raw_material_order_not_active"
    );

    let missing_scan = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(
                r#"{
                "apparatus":"apparatus:default:bosma_7",
                "order_id":"zakaz-raw-route",
                "action":"start"
            }"#,
                "zakaz-raw-route",
            ),
        ))
        .await
        .expect("queue action");
    assert_eq!(missing_scan.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_scan).await["error"],
        "raw_material_scan_required"
    );

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(
                r#"{
                "apparatus":"apparatus:default:bosma_7",
                "order_id":"zakaz-raw-route",
                "action":"start",
                "material_barcodes":["30AA"]
            }"#,
                "zakaz-raw-route",
            ),
        ))
        .await
        .expect("queue action with one material from the required group");
    assert_eq!(started.status(), StatusCode::OK);
    assert_eq!(
        json_body(started).await["states"]["zakaz-raw-route"],
        "in_progress"
    );

    let assignments_after_start = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
        ))
        .await
        .expect("assignments after start");
    assert_eq!(assignments_after_start.status(), StatusCode::OK);
    let assignments_body = json_body(assignments_after_start).await;
    let started_materials = assignments_body
        .as_array()
        .expect("assignments array")
        .iter()
        .filter(|item| item["order_id"] == "zakaz-raw-route")
        .collect::<Vec<_>>();
    assert_eq!(started_materials.len(), 3);
    assert_eq!(
        started_materials
            .iter()
            .filter(|item| item["stock_status"] == "in_use")
            .count(),
        1
    );
    assert_eq!(
        started_materials
            .iter()
            .filter(|item| item["stock_status"] == "available")
            .count(),
        2
    );
    assert_eq!(
        started_materials
            .iter()
            .filter_map(|item| item["received_qty"].as_f64())
            .sum::<f64>(),
        12.0
    );

    let intake_candidates = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-intake-candidates?order_id=zakaz-raw-route&apparatus=apparatus%3Adefault%3Abosma_7",
            &worker_token,
        ))
        .await
        .expect("additional material intake candidates");
    assert_eq!(intake_candidates.status(), StatusCode::OK);
    let intake_candidates_body = json_body(intake_candidates).await;
    assert_eq!(intake_candidates_body.as_array().map(Vec::len), Some(1));
    assert_eq!(intake_candidates_body[0]["barcode"], "30CC");
    assert_eq!(intake_candidates_body[0]["stock_status"], "available");

    let not_staged_intake = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-intake",
            &worker_token,
            r#"{
                "order_id":"zakaz-raw-route",
                "apparatus":"apparatus:default:bosma_7",
                "barcode":"30EE"
            }"#,
        ))
        .await
        .expect("not staged additional material intake");
    assert_eq!(not_staged_intake.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(not_staged_intake).await["error"],
        "raw_material_state_not_ready"
    );

    let unassigned_intake = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-intake",
            &worker_token,
            r#"{
                "order_id":"zakaz-raw-route",
                "apparatus":"apparatus:default:bosma_7",
                "barcode":"30DD"
            }"#,
        ))
        .await
        .expect("unassigned additional material intake");
    assert_eq!(unassigned_intake.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(unassigned_intake).await["error"],
        "raw_material_assignment_not_found"
    );

    let intake = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-intake",
            &worker_token,
            r#"{
                "order_id":"zakaz-raw-route",
                "apparatus":"apparatus:default:bosma_7",
                "barcode":"30CC"
            }"#,
        ))
        .await
        .expect("assigned additional material intake");
    let intake_status = intake.status();
    let intake_body = json_body(intake).await;
    assert_eq!(intake_status, StatusCode::OK, "{intake_body:?}");
    assert_eq!(intake_body["stock_status"], "in_use");
    assert_eq!(intake_body["reserved_order_id"], "zakaz-raw-route");
    assert_eq!(intake_body["stock_qty"], 8.0);
    assert_eq!(intake_body["stock_uom"], "Kg");
    assert_eq!(intake_body["received_qty"], 8.0);
    assert_eq!(intake_body["consumed_qty"], 0.0);
    assert_eq!(intake_body["remaining_qty"], 8.0);

    let candidates_after_intake = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-intake-candidates?order_id=zakaz-raw-route&apparatus=apparatus%3Adefault%3Abosma_7",
            &worker_token,
        ))
        .await
        .expect("candidates after additional material intake");
    assert_eq!(candidates_after_intake.status(), StatusCode::OK);
    assert_eq!(
        json_body(candidates_after_intake)
            .await
            .as_array()
            .map(Vec::len),
        Some(0)
    );

    let repeated_intake = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-intake",
            &worker_token,
            r#"{
                "order_id":"zakaz-raw-route",
                "apparatus":"apparatus:default:bosma_7",
                "barcode":"30CC"
            }"#,
        ))
        .await
        .expect("repeated additional material intake");
    assert_eq!(repeated_intake.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(repeated_intake).await["error"],
        "raw_material_stock_unavailable"
    );

    let detached = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:bosma_7",
                "order_id":"zakaz-raw-route",
                "action":"detach_roll",
                "produced_qty":48,
                "uom":"m",
                "finished_goods_kg":21,
                "finished_goods_meter":48,
                "bobina_kg":7,
                "print_transport":"offline"
            }"#,
        ))
        .await
        .expect("detach roll after raw material scan");
    let detached_status = detached.status();
    let detached_body = json_body(detached).await;
    assert_eq!(detached_status, StatusCode::OK, "{detached_body:?}");
    assert_eq!(detached_body["states"]["zakaz-raw-route"], "paused");

    let assignments_after_detach = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments?order_id=zakaz-raw-route&apparatus=apparatus%3Adefault%3Abosma_7",
            &worker_token,
        ))
        .await
        .expect("assignments after roll detach");
    assert_eq!(assignments_after_detach.status(), StatusCode::OK);
    assert_eq!(
        json_body(assignments_after_detach)
            .await
            .as_array()
            .expect("assignments array")
            .iter()
            .filter(|item| item["stock_status"] == "in_use")
            .count(),
        2
    );

    let resumed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"apparatus:default:bosma_7",
                "order_id":"zakaz-raw-route",
                "action":"resume"
            }"#,
        ))
        .await
        .expect("resume after roll detach");
    let resumed_status = resumed.status();
    let resumed_body = json_body(resumed).await;
    assert_eq!(resumed_status, StatusCode::OK, "{resumed_body:?}");
    assert_eq!(resumed_body["states"]["zakaz-raw-route"], "in_progress");

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_returned_paint(
                r#"{
                "apparatus":"apparatus:default:bosma_7",
                "order_id":"zakaz-raw-route",
                "action":"complete",
                "produced_qty":3,
                "gross_qty":3,
                "return_ink_kg":1,
                "total_waste":1,
                "finished_goods_kg":3,
                "finished_goods_meter":3,
                "uom":"kg",
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
            ),
        ))
        .await
        .expect("complete after raw material scan");
    let completed_status = completed.status();
    let completed_body = json_body(completed).await;
    assert_eq!(completed_status, StatusCode::OK, "{completed_body:?}");
    assert_eq!(completed_body["states"]["zakaz-raw-route"], "completed");

    let assignments_after_complete = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
        ))
        .await
        .expect("assignments after complete");
    assert_eq!(assignments_after_complete.status(), StatusCode::OK);
    let completed_assignments_body = json_body(assignments_after_complete).await;
    let completed_materials = completed_assignments_body
        .as_array()
        .expect("assignments array")
        .iter()
        .filter(|item| item["order_id"] == "zakaz-raw-route")
        .collect::<Vec<_>>();
    assert_eq!(completed_materials.len(), 2);
    assert_eq!(
        completed_materials
            .iter()
            .filter(|item| item["stock_status"] == "consumed")
            .count(),
        2
    );
    assert!(
        completed_materials
            .iter()
            .all(|item| item["stock_status"] == "consumed")
    );
    assert_eq!(
        completed_materials
            .iter()
            .filter_map(|item| item["received_qty"].as_f64())
            .sum::<f64>(),
        20.0
    );
    assert_eq!(
        completed_materials
            .iter()
            .filter_map(|item| item["consumed_qty"].as_f64())
            .sum::<f64>(),
        20.0
    );
    assert_eq!(
        completed_materials
            .iter()
            .filter_map(|item| item["remaining_qty"].as_f64())
            .sum::<f64>(),
        0.0
    );
    let unused_stock = material_store
        .raw_material_stock_by_barcode("30EE")
        .await
        .expect("unused stock lookup")
        .expect("unused stock remains in inventory");
    assert_eq!(unused_stock.status, "available");
    assert_eq!(unused_stock.reserved_order_id, "");
    assert_eq!(unused_stock.qty, 4.0);
    assert_eq!(unused_stock.warehouse, "Kalidor");
}

#[tokio::test]
async fn material_taminotchi_raw_material_assignment_rejects_unassigned_item_group() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store);
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-raw-route".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:bosma_7".to_string()],
            assigned_item_groups: vec!["Kley".to_string()],
        })
        .await
        .expect("material scope");
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let material_token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-raw-route",
    )
    .await;
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-material-scope",
                "Raw material scope",
                "8812",
                "apparatus:default:bosma_7",
            ),
        ))
        .await
        .expect("map save");
    assert_eq!(map.status(), StatusCode::OK);

    let rule = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/raw-material-rules",
            &admin_token,
            &canonical_requirement_set_material_policy_body(
                "apparatus:default:bosma_7",
                1,
                &["Kraska"],
                true,
            ),
        ))
        .await
        .expect("rule save");
    assert_eq!(rule.status(), StatusCode::OK);

    let lookup = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments/lookup?barcode=30AA",
            &material_token,
        ))
        .await
        .expect("lookup");
    assert_eq!(lookup.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(lookup).await["error"],
        "item group is not assigned to material taminotchi"
    );

    let assigned = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &material_token,
            r#"{
                "order_id":"zakaz-material-scope",
                "barcode":"30AA"
            }"#,
        ))
        .await
        .expect("assign");
    assert_eq!(assigned.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(assigned).await["error"],
        "item group is not assigned to material taminotchi"
    );
}
