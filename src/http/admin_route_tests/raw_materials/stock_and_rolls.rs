use super::*;

#[tokio::test]
async fn queue_start_rejects_raw_material_stock_reserved_for_other_order() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store.clone());
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-raw-reserved".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:bosma_7".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("aparatchi assignment");
    let token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-raw-reserved").await;
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json(
                "zakaz-raw-reserved",
                "Raw reserved",
                "8812",
                "apparatus:default:bosma_7",
            ),
        ))
        .await
        .expect("map save");
    assert_eq!(map.status(), StatusCode::OK);
    provision_test_qolip(&router, &token, "zakaz-raw-reserved").await;

    let rule = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/raw-material-rules",
            &token,
            &canonical_requirement_set_material_policy_body(
                "apparatus:default:bosma_7",
                1,
                &["Kraska"],
                true,
            ),
        ))
        .await
        .expect("rule save");
    let rule_status = rule.status();
    let rule_body = json_body(rule).await;
    assert_eq!(rule_status, StatusCode::OK, "{rule_body:?}");

    let assigned = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{
                "order_id":"zakaz-raw-reserved",
                "barcode":"30AA"
            }"#,
        ))
        .await
        .expect("assign");
    let assigned_status = assigned.status();
    let assigned_body = json_body(assigned).await;
    assert_eq!(assigned_status, StatusCode::OK, "{assigned_body:?}");
    material_store
        .set_stock_status("30AA", "in_use", "zakaz-other")
        .await;

    let start = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(
                r#"{
                "apparatus":"apparatus:default:bosma_7",
                "order_id":"zakaz-raw-reserved",
                "action":"start",
                "material_barcodes":["30AA"]
            }"#,
                "zakaz-raw-reserved",
            ),
        ))
        .await
        .expect("queue action with reserved stock");
    let start_status = start.status();
    let start_body = json_body(start).await;
    assert_eq!(start_status, StatusCode::BAD_REQUEST, "{start_body:?}");
    assert_eq!(start_body["error"], "raw_material_stock_unavailable");

    let stock = material_store
        .raw_material_stock_by_barcode("30AA")
        .await
        .expect("stock lookup")
        .expect("stock");
    assert_eq!(stock.status, "in_use");
    assert_eq!(stock.reserved_order_id, "zakaz-other");
}

#[tokio::test]
async fn queue_start_commit_failure_does_not_reserve_raw_material_stock() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    let production_store = Arc::new(MemoryProductionMapStore::new());
    let mut state = test_state();
    state.production_maps = production_map_service_with_store(&state, production_store.clone());
    state.gscale = GscaleService::new().with_receipt_store(material_store.clone());
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-raw-rollback".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:bosma_7".to_string()],
            assigned_item_groups: Vec::new(),
        })
        .await
        .expect("aparatchi assignment");
    let token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-raw-rollback").await;
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json(
                "zakaz-raw-rollback",
                "Raw rollback",
                "8813",
                "apparatus:default:bosma_7",
            ),
        ))
        .await
        .expect("map save");
    assert_eq!(map.status(), StatusCode::OK);
    provision_test_qolip(&router, &token, "zakaz-raw-rollback").await;

    let rule = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/raw-material-rules",
            &token,
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

    let assigned = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{
                "order_id":"zakaz-raw-rollback",
                "barcode":"30AA"
            }"#,
        ))
        .await
        .expect("assign");
    let assigned_status = assigned.status();
    let assigned_body = json_body(assigned).await;
    assert_eq!(assigned_status, StatusCode::OK, "{assigned_body:?}");

    production_store.fail_next_queue_progress_commit();
    let start = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(
                r#"{
                "apparatus":"apparatus:default:bosma_7",
                "order_id":"zakaz-raw-rollback",
                "action":"start",
                "material_barcodes":["30AA"]
            }"#,
                "zakaz-raw-rollback",
            ),
        ))
        .await
        .expect("queue action with failing commit");
    assert_eq!(start.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let stock = material_store
        .raw_material_stock_by_barcode("30AA")
        .await
        .expect("stock lookup")
        .expect("stock");
    assert_eq!(stock.status, "available");
    assert_eq!(stock.reserved_order_id, "");
}

#[tokio::test]
async fn raw_material_assignment_checks_rulon_size_for_pechat_orders() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    material_store
        .insert_stock("30R980", "ROLL-980", "CPP 980/35", 10.0)
        .await;
    material_store
        .insert_stock("30R1000", "ROLL-1000", "CPP 1000/35", 11.0)
        .await;
    material_store
        .insert_stock("30R1020", "ROLL-1020", "CPP 1020/35", 9.0)
        .await;
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store);
    let token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json_with_dims(
                "zakaz-rulon-size",
                "Rulon size",
                "8813",
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

    let diagnostics = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments/diagnostics?barcode=30R980&order_id=zakaz-rulon-size&apparatus=apparatus%3Adefault%3Abosma_7",
            &token,
        ))
        .await
        .expect("roll size diagnostics");
    assert_eq!(diagnostics.status(), StatusCode::OK);
    let diagnostics_body = json_body(diagnostics).await;
    assert_eq!(diagnostics_body["compatible"], false);
    assert_eq!(
        diagnostics_body["reason"],
        "raw_material_roll_size_mismatch"
    );
    assert_eq!(diagnostics_body["order_width_mm"], 985.0);
    assert_eq!(diagnostics_body["roll_width_mm"], 980.0);
    assert_eq!(diagnostics_body["minimum_width_mm"], 985.0);
    assert_eq!(diagnostics_body["maximum_width_mm"], 1005.0);

    let assigned = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{
                "order_id":"zakaz-rulon-size",
                "barcode":"30R1000",
                "item_code":"INK-BLACK",
                "item_group":"Kraska"
            }"#,
        ))
        .await
        .expect("assign matching rulon");
    let assigned_status = assigned.status();
    let assigned_body = json_body(assigned).await;
    assert_eq!(assigned_status, StatusCode::OK, "{assigned_body:?}");
    assert_eq!(assigned_body["item_code"], "ROLL-1000");
    assert_eq!(assigned_body["item_name"], "CPP 1000/35");
    assert_eq!(assigned_body["item_group"], "Rulon eni");

    let undersized = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{
                "order_id":"zakaz-rulon-size",
                "barcode":"30R980"
            }"#,
        ))
        .await
        .expect("assign undersized rulon");
    assert_eq!(undersized.status(), StatusCode::BAD_REQUEST);
    let undersized_body = json_body(undersized).await;
    assert_eq!(undersized_body["error"], "raw_material_roll_size_mismatch");
    assert_eq!(undersized_body["order_width_mm"], 985.0);
    assert_eq!(undersized_body["roll_width_mm"], 980.0);
    assert_eq!(undersized_body["minimum_width_mm"], 985.0);
    assert_eq!(undersized_body["maximum_width_mm"], 1005.0);

    let oversized = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{
                "order_id":"zakaz-rulon-size",
                "barcode":"30R1020"
            }"#,
        ))
        .await
        .expect("assign oversized rulon");
    assert_eq!(oversized.status(), StatusCode::BAD_REQUEST);
    let oversized_body = json_body(oversized).await;
    assert_eq!(oversized_body["error"], "raw_material_roll_size_mismatch");
    assert_eq!(oversized_body["order_width_mm"], 985.0);
    assert_eq!(oversized_body["roll_width_mm"], 1020.0);
    assert_eq!(oversized_body["minimum_width_mm"], 985.0);
    assert_eq!(oversized_body["maximum_width_mm"], 1005.0);
}

#[tokio::test]
async fn optional_rulon_policy_requires_scan_once_material_is_assigned() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    material_store
        .insert_stock("30R765", "ROLL-1000", "JEM 765/25", 244.5)
        .await;
    material_store
        .insert_stock("30R785", "ROLL-1000", "JEM 785/25", 244.5)
        .await;
    let mut state = test_state();
    let inventory_store = Arc::new(MemoryInventoryMovementStore::new());
    let state_location = InventoryLocation {
        id: "location:state:optional-rulon".to_string(),
        kind: InventoryLocationKind::State,
        name: "Bosma 7 oldi".to_string(),
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
            ["30R765", "30R785"]
                .into_iter()
                .map(|barcode| InventoryAsset {
                    kind: InventoryAssetKind::RawMaterial,
                    asset_ref: format!("raw:{barcode}"),
                    custody_warehouse_id: "warehouse:kalidor".to_string(),
                    custody_warehouse: "Kalidor".to_string(),
                    item_code: "ROLL-1000".to_string(),
                    item_name: barcode.to_string(),
                    identifier: barcode.to_string(),
                    qty: 244.5,
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
    state.gscale = GscaleService::new().with_receipt_store(material_store);
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-optional-rulon".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:bosma_7".to_string()],
            assigned_item_groups: vec!["Rulon eni".to_string()],
        })
        .await
        .expect("material scope");
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-optional-rulon",
        "Kalidor",
    )
    .await;
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let material_token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-optional-rulon",
    )
    .await;
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json_with_dims(
                "zakaz-optional-rulon",
                "Optional rulon",
                "0001",
                "apparatus:default:bosma_7",
                7,
                765.0,
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
            &canonical_material_policy_body(
                "apparatus:default:bosma_7",
                1,
                serde_json::json!({
                    "mode": "not_required",
                    "item_group_ids": ["Rulon"]
                }),
                true,
            ),
        ))
        .await
        .expect("optional material rule save");
    let rule_status = rule.status();
    let rule_body = json_body(rule).await;
    assert_eq!(rule_status, StatusCode::OK, "{rule_body:?}");
    assert_eq!(
        rule_body["revision"]["policies"]["material"],
        serde_json::json!({
            "mode": "not_required",
            "item_group_ids": ["Rulon"]
        })
    );

    let candidates = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments/candidates?order_id=zakaz-optional-rulon&apparatus=apparatus%3Adefault%3Abosma_7",
            &material_token,
        ))
        .await
        .expect("optional rulon candidates");
    let candidate_status = candidates.status();
    let candidate_body = json_body(candidates).await;
    assert_eq!(candidate_status, StatusCode::OK, "{candidate_body:?}");
    assert_eq!(candidate_body.as_array().map(Vec::len), Some(2));
    assert_eq!(candidate_body[0]["barcode"], "30R765");
    assert_eq!(candidate_body[0]["roll_width_mm"], 765.0);
    assert_eq!(candidate_body[1]["barcode"], "30R785");
    assert_eq!(candidate_body[1]["roll_width_mm"], 785.0);

    let assigned = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &material_token,
            r#"{
                "order_id":"zakaz-optional-rulon",
                "barcode":"30R765",
                "apparatus":"apparatus:default:bosma_7"
            }"#,
        ))
        .await
        .expect("assign optional material");
    let assigned_status = assigned.status();
    let assigned_body = json_body(assigned).await;
    assert_eq!(assigned_status, StatusCode::OK, "{assigned_body:?}");

    let requirements = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-start-requirements?order_id=zakaz-optional-rulon&apparatus=apparatus%3Adefault%3Abosma_7",
            &admin_token,
        ))
        .await
        .expect("optional start requirements");
    let requirements_status = requirements.status();
    let requirements_body = json_body(requirements).await;
    assert_eq!(requirements_status, StatusCode::OK, "{requirements_body:?}");
    assert_eq!(requirements_body["requires_material"], false);
    assert_eq!(requirements_body["material_scan_required"], true);
    assert_eq!(requirements_body["required_scan_count"], 1);
    assert_eq!(requirements_body["matched_scan_count"], 0);
    assert_eq!(requirements_body["assignments_satisfied"], true);
    assert_eq!(requirements_body["scan_satisfied"], false);

    let scanned_requirements = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-start-requirements?order_id=zakaz-optional-rulon&apparatus=apparatus%3Adefault%3Abosma_7&material_barcodes=30R765",
            &admin_token,
        ))
        .await
        .expect("optional scanned start requirements");
    let scanned_status = scanned_requirements.status();
    let scanned_body = json_body(scanned_requirements).await;
    assert_eq!(scanned_status, StatusCode::OK, "{scanned_body:?}");
    assert_eq!(scanned_body["required_scan_count"], 1);
    assert_eq!(scanned_body["matched_scan_count"], 1);
    assert_eq!(scanned_body["scan_satisfied"], true);
}

#[tokio::test]
async fn raw_material_assignment_limits_laminatsiya_roll_width_to_thirty_mm() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    material_store
        .insert_stock("30L660", "ROLL-1000", "CPP 660/35", 10.0)
        .await;
    material_store
        .insert_stock("30L690", "ROLL-1000", "CPP 690/35", 11.0)
        .await;
    material_store
        .insert_stock("30L691", "ROLL-1000", "CPP 691/35", 9.0)
        .await;
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store);
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-laminatsiya-width".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:asset-007".to_string()],
            assigned_item_groups: vec!["Rulon".to_string()],
        })
        .await
        .expect("material laminatsiya assignment");
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-laminatsiya-width",
        "Kalidor",
    )
    .await;
    let token = session(&state, PrincipalRole::Admin).await;
    let material_token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-laminatsiya-width",
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
                "zakaz-laminatsiya-rulon-size",
                "Laminatsiya rulon size",
                "8817",
                "apparatus:default:asset-007",
                7,
                660.0,
            ),
        ))
        .await
        .expect("laminatsiya map save");
    assert_eq!(map.status(), StatusCode::OK);

    let rule = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/raw-material-rules",
            &token,
            &canonical_requirement_set_material_policy_body(
                "apparatus:default:asset-007",
                1,
                &["Rulon"],
                false,
            ),
        ))
        .await
        .expect("laminatsiya rule save");
    assert_eq!(rule.status(), StatusCode::OK);

    let candidates = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments/candidates?order_id=zakaz-laminatsiya-rulon-size&apparatus=apparatus%3Adefault%3Aasset-007",
            &material_token,
        ))
        .await
        .expect("laminatsiya candidates");
    assert_eq!(candidates.status(), StatusCode::OK);
    let candidates_body = json_body(candidates).await;
    assert_eq!(candidates_body.as_array().map(Vec::len), Some(2));
    assert_eq!(candidates_body[0]["barcode"], "30L660");
    assert_eq!(candidates_body[1]["barcode"], "30L690");
    assert_eq!(candidates_body[1]["leftover_width_mm"], 30.0);

    let maximum_allowed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &material_token,
            r#"{
                "order_id":"zakaz-laminatsiya-rulon-size",
                "barcode":"30L690",
                "apparatus":"apparatus:default:asset-007"
            }"#,
        ))
        .await
        .expect("assign maximum laminatsiya width");
    let maximum_status = maximum_allowed.status();
    let maximum_body = json_body(maximum_allowed).await;
    assert_eq!(maximum_status, StatusCode::OK, "{maximum_body:?}");

    let oversized = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &material_token,
            r#"{
                "order_id":"zakaz-laminatsiya-rulon-size",
                "barcode":"30L691",
                "apparatus":"apparatus:default:asset-007"
            }"#,
        ))
        .await
        .expect("assign oversized laminatsiya width");
    assert_eq!(oversized.status(), StatusCode::BAD_REQUEST);
    let oversized_body = json_body(oversized).await;
    assert_eq!(oversized_body["error"], "raw_material_roll_size_mismatch");
    assert_eq!(oversized_body["order_width_mm"], 660.0);
    assert_eq!(oversized_body["roll_width_mm"], 691.0);
}

#[tokio::test]
async fn material_taminotchi_raw_material_assignment_allows_child_group_from_assigned_parent() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    material_store
        .insert_stock("30R1000", "ROLL-1000", "CPP 1000/35", 44.0)
        .await;
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store);
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-rulon-parent".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: vec!["apparatus:default:bosma_7".to_string()],
            assigned_item_groups: vec!["Rulon".to_string()],
        })
        .await
        .expect("material scope");
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-rulon-parent",
        "Kalidor",
    )
    .await;
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let material_token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-rulon-parent",
    )
    .await;
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json_with_dims(
                "zakaz-rulon-parent-scope",
                "Rulon parent scope",
                "8821",
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
            &admin_token,
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

    let assigned = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &material_token,
            r#"{
                "order_id":"zakaz-rulon-parent-scope",
                "barcode":"30R1000"
            }"#,
        ))
        .await
        .expect("assign child group material");
    let status = assigned.status();
    let body = json_body(assigned).await;

    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["item_code"], "ROLL-1000");
    assert_eq!(body["item_group"], "Rulon eni");
}

#[tokio::test]
async fn admin_raw_material_stock_lists_new_stock_model() {
    let mut state = test_state();
    state.gscale =
        GscaleService::new().with_receipt_store(Arc::new(RawMaterialStockLookup::default()));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-stock?warehouse=Kalidor",
            &token,
        ))
        .await
        .expect("raw stock list");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body[0]["warehouse"], "Kalidor");
    assert_eq!(body[0]["item_code"], "INK-BLACK");
    assert_eq!(body[0]["barcode"], "30AA");
    assert_eq!(body[0]["qty"], 12.0);
    assert_eq!(body[0]["status"], "available");
}

#[tokio::test]
async fn material_taminotchi_can_list_raw_material_stock_for_assignment() {
    let mut state = test_state();
    state.gscale =
        GscaleService::new().with_receipt_store(Arc::new(RawMaterialStockLookup::default()));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-stock".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: vec!["Kraska".to_string()],
        })
        .await
        .expect("material stock role");
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-stock",
        "Kalidor",
    )
    .await;
    let token = session_for(&state, PrincipalRole::MaterialTaminotchi, "material-stock").await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-stock?warehouse=Kalidor",
            &token,
        ))
        .await
        .expect("material raw stock list");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body[0]["warehouse"], "Kalidor");
    assert_eq!(body[0]["item_code"], "INK-BLACK");
    assert_eq!(body[0]["barcode"], "30AA");
}

#[tokio::test]
async fn material_taminotchi_can_correct_available_raw_material_without_changing_identity() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store.clone());
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-stock-edit".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: vec!["Kraska".to_string()],
        })
        .await
        .expect("material stock edit role");
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-stock-edit",
        "Kalidor",
    )
    .await;
    let token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-stock-edit",
    )
    .await;
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);

    let admin_edit = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/raw-material-stock",
            &admin_token,
            r#"{"barcode":"30AA","item_code":"INK-WHITE","qty":10.5}"#,
        ))
        .await
        .expect("admin raw stock correction");
    assert_eq!(admin_edit.status(), StatusCode::FORBIDDEN);

    let response = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/raw-material-stock",
            &token,
            r#"{"barcode":"30AA","item_code":"INK-WHITE","qty":10.5}"#,
        ))
        .await
        .expect("raw stock correction");

    let status = response.status();
    let body = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["item_code"], "INK-WHITE");
    assert_eq!(body["item_name"], "White ink");
    assert_eq!(body["qty"], 10.5);
    assert_eq!(body["barcode"], "30AA");
    assert_eq!(body["source_receipt_id"], "GSR-30AA");

    material_store
        .set_stock_status("30AA", "reserved", "zakaz-locked")
        .await;
    let locked = router
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/raw-material-stock",
            &token,
            r#"{"barcode":"30AA","item_code":"INK-BLACK","qty":9}"#,
        ))
        .await
        .expect("locked raw stock correction");
    assert_eq!(locked.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(locked).await["error"],
        "raw_material_stock_locked"
    );
}

#[tokio::test]
async fn material_taminotchi_safely_deletes_only_available_raw_material_in_own_scope() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store.clone());
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-stock-delete".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: vec!["Kraska".to_string()],
        })
        .await
        .expect("material stock delete role");
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-stock-delete",
        "Kalidor",
    )
    .await;
    let token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-stock-delete",
    )
    .await;
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);

    let admin_delete = router
        .clone()
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/raw-material-stock",
            &admin_token,
            r#"{"barcode":"30AA"}"#,
        ))
        .await
        .expect("admin raw stock delete");
    assert_eq!(admin_delete.status(), StatusCode::FORBIDDEN);

    let deleted = router
        .clone()
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/raw-material-stock",
            &token,
            r#"{"barcode":"30AA"}"#,
        ))
        .await
        .expect("raw stock delete");
    let status = deleted.status();
    let body = json_body(deleted).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["barcode"], "30AA");
    assert!(
        material_store
            .raw_material_stock_by_barcode("30AA")
            .await
            .expect("deleted stock lookup")
            .is_none()
    );

    material_store
        .set_stock_status("30CC", "reserved", "zakaz-locked")
        .await;
    let locked = router
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/raw-material-stock",
            &token,
            r#"{"barcode":"30CC"}"#,
        ))
        .await
        .expect("locked raw stock delete");
    assert_eq!(locked.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(locked).await["error"],
        "raw_material_stock_locked"
    );
}

#[tokio::test]
async fn material_taminotchi_reprints_existing_assigned_raw_material_identity() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store.clone());
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-stock-reprint".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: vec!["Kraska".to_string()],
        })
        .await
        .expect("material stock reprint role");
    assign_warehouse_to_principal(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-stock-reprint",
        "Kalidor",
    )
    .await;
    let token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-stock-reprint",
    )
    .await;
    let admin_token = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &admin_token,
            &pechat_order_map_json(
                "zakaz-raw-reprint",
                "Raw reprint",
                "8814",
                "apparatus:default:bosma_7",
            ),
        ))
        .await
        .expect("raw reprint map");
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
        .expect("raw reprint rule");
    assert_eq!(rule.status(), StatusCode::OK);
    let assigned = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-assignments",
            &admin_token,
            r#"{
                "order_id":"zakaz-raw-reprint",
                "barcode":"30AA",
                "apparatus":"apparatus:default:bosma_7"
            }"#,
        ))
        .await
        .expect("assign raw stock before reprint");
    let assigned_status = assigned.status();
    let assigned_body = json_body(assigned).await;
    assert_eq!(assigned_status, StatusCode::OK, "{assigned_body:?}");

    let response = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-stock/reprint/prepare",
            &token,
            r#"{"barcode":"30AA"}"#,
        ))
        .await
        .expect("prepare raw stock reprint");
    let status = response.status();
    let body = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["stock"]["barcode"], "30AA");
    assert_eq!(body["stock"]["source_receipt_id"], "GSR-30AA");
    assert_eq!(body["print"]["epc"], "30AA");
    assert_eq!(body["print"]["item_code"], "INK-BLACK");
    assert_eq!(body["print"]["gross_qty"], 12.0);
    assert_eq!(body["print"]["print_count"], 1);
    assert_eq!(body["print"]["label_kind"], "material_product");
    let reprint_id = body["reprint_id"].as_str().expect("reprint id");

    let confirm_body = serde_json::json!({
        "barcode": "30AA",
        "reprint_id": reprint_id,
    })
    .to_string();
    let confirmed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-stock/reprint/confirm",
            &token,
            &confirm_body,
        ))
        .await
        .expect("confirm raw stock reprint");
    assert_eq!(confirmed.status(), StatusCode::OK);
    assert_eq!(json_body(confirmed).await["barcode"], "30AA");

    let stored = material_store
        .raw_material_stock_by_barcode("30AA")
        .await
        .expect("stock lookup")
        .expect("stock");
    assert_eq!(stored.barcode, "30AA");
    assert_eq!(stored.source_receipt_id, "GSR-30AA");
    assert_eq!(stored.item_code, "INK-BLACK");
    assert_eq!(stored.qty, 12.0);

    let identity_override = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-stock/reprint/prepare",
            &token,
            r#"{"barcode":"30AA","epc":"30NEW","qty":999}"#,
        ))
        .await
        .expect("reject identity override");
    assert_eq!(identity_override.status(), StatusCode::BAD_REQUEST);

    material_store
        .set_stock_status("30AA", "reserved", "zakaz-locked")
        .await;
    let locked = router
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/raw-material-stock/reprint/prepare",
            &token,
            r#"{"barcode":"30AA"}"#,
        ))
        .await
        .expect("locked raw stock reprint");
    assert_eq!(locked.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(locked).await["error"],
        "raw_material_stock_locked"
    );
}

#[tokio::test]
async fn material_taminotchi_raw_material_stock_hides_unassigned_warehouse() {
    let mut state = test_state();
    state.gscale =
        GscaleService::new().with_receipt_store(Arc::new(RawMaterialStockLookup::default()));
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "material-stock-no-warehouse".to_string(),
            role_id: "material_taminotchi".to_string(),
            assigned_apparatus: Vec::new(),
            assigned_item_groups: vec!["Kraska".to_string()],
        })
        .await
        .expect("material stock role");
    let token = session_for(
        &state,
        PrincipalRole::MaterialTaminotchi,
        "material-stock-no-warehouse",
    )
    .await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-stock?warehouse=Kalidor",
            &token,
        ))
        .await
        .expect("material raw stock list");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert!(body.as_array().expect("array").is_empty(), "{body}");
}
