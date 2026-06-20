use super::*;

#[tokio::test]
async fn raw_material_routes_assign_and_require_scan_for_queue_start() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
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
            assigned_apparatus: vec!["7 ta rangli pechat - A".to_string()],
        })
        .await
        .expect("aparatchi assignment");
    let token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "worker-raw-route").await;
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
                "7 ta rangli pechat - A",
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
            r#"{"apparatus":"7 ta rangli pechat - A","requires_material":true,"item_groups":["Kraska"]}"#,
        ))
        .await
        .expect("rule save");
    assert_eq!(rule.status(), StatusCode::OK);
    let rule_body = json_body(rule).await;
    assert_eq!(rule_body["apparatus"], "7 ta rangli pechat - A");
    assert_eq!(rule_body["requires_material"], true);

    let missing_assignment = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat - A",
                "order_id":"zakaz-raw-route",
                "action":"start"
            }"#,
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
    assert_eq!(assigned_body["apparatus"], "7 ta rangli pechat - A");
    assert_eq!(assigned_body["item_code"], "INK-BLACK");
    assert_eq!(assigned_body["item_name"], "Black ink");
    assert_eq!(assigned_body["item_group"], "Kraska");
    let warehouse_event = warehouse_events.recv().await.expect("warehouse event");
    assert_eq!(warehouse_event.event, "warehouse.updated");
    assert_eq!(warehouse_event.warehouse, "Kalidor");
    assert_eq!(warehouse_event.reason, "raw_material_assignment");

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
    assert_eq!(second_body["apparatus"], "7 ta rangli pechat - A");
    assert_eq!(second_body["item_code"], "INK-WHITE");
    let _second_warehouse_event = warehouse_events
        .recv()
        .await
        .expect("second warehouse event");

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

    let missing_scan = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat - A",
                "order_id":"zakaz-raw-route",
                "action":"start"
            }"#,
        ))
        .await
        .expect("queue action");
    assert_eq!(missing_scan.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_scan).await["error"],
        "raw_material_scan_required"
    );

    let partial_scan = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat - A",
                "order_id":"zakaz-raw-route",
                "action":"start",
                "material_barcodes":["30AA"]
            }"#,
        ))
        .await
        .expect("queue action with partial scan");
    assert_eq!(partial_scan.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(partial_scan).await["error"],
        "raw_material_mismatch"
    );

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat - A",
                "order_id":"zakaz-raw-route",
                "action":"start",
                "material_barcodes":["30AA","30CC"]
            }"#,
        ))
        .await
        .expect("queue action with scan");
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
    assert_eq!(started_materials.len(), 2);
    assert!(started_materials.iter().all(|item| {
        item["stock_status"] == "in_use" && item["reserved_order_id"] == "zakaz-raw-route"
    }));

    let completed = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat - A",
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
    assert!(completed_materials.iter().all(|item| {
        item["stock_status"] == "consumed" && item["reserved_order_id"] == "zakaz-raw-route"
    }));
}

#[tokio::test]
async fn raw_material_assignment_can_be_unlinked_before_start() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store);
    let token = session(&state, PrincipalRole::Admin).await;
    let mut warehouse_events = state.warehouse_events.subscribe();
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json(
                "zakaz-raw-unlink",
                "Raw unlink",
                "8822",
                "7 ta rangli pechat - A",
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
            r#"{"apparatus":"7 ta rangli pechat - A","requires_material":true,"item_groups":["Kraska"]}"#,
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
            r#"{"order_id":"zakaz-raw-unlink","barcode":"30AA"}"#,
        ))
        .await
        .expect("assign");
    assert_eq!(assigned.status(), StatusCode::OK);
    let _assigned_event = warehouse_events.recv().await.expect("assigned event");

    let unlinked = router
        .clone()
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{"order_id":"zakaz-raw-unlink","barcode":"30AA"}"#,
        ))
        .await
        .expect("unlink assignment");
    let unlink_status = unlinked.status();
    let unlink_body = json_body(unlinked).await;
    assert_eq!(unlink_status, StatusCode::OK, "{unlink_body:?}");
    assert_eq!(unlink_body["ok"], true);
    assert_eq!(unlink_body["assignment"]["order_id"], "zakaz-raw-unlink");
    assert_eq!(unlink_body["assignment"]["barcode"], "30AA");
    let unlinked_event = warehouse_events.recv().await.expect("unlinked event");
    assert_eq!(unlinked_event.event, "warehouse.updated");
    assert_eq!(unlinked_event.warehouse, "Kalidor");
    assert_eq!(unlinked_event.reason, "raw_material_assignment_unlink");

    let assignments_after_unlink = router
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
        ))
        .await
        .expect("assignments after unlink");
    assert_eq!(assignments_after_unlink.status(), StatusCode::OK);
    let assignments_body = json_body(assignments_after_unlink).await;
    assert!(
        assignments_body
            .as_array()
            .expect("assignments array")
            .iter()
            .all(|item| item["order_id"] != "zakaz-raw-unlink")
    );

    let missing = router
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{"order_id":"zakaz-raw-unlink","barcode":"30AA"}"#,
        ))
        .await
        .expect("unlink missing assignment");
    assert_eq!(missing.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing).await["error"],
        "raw_material_assignment_not_found"
    );
}

#[tokio::test]
async fn raw_material_assignment_unlink_rejects_started_stock() {
    let material_store = Arc::new(RawMaterialStockLookup::default());
    let mut state = test_state();
    state.gscale = GscaleService::new().with_receipt_store(material_store);
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "worker-raw-unlink-locked".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat - A".to_string()],
        })
        .await
        .expect("aparatchi assignment");
    let token = session(&state, PrincipalRole::Admin).await;
    let worker_token =
        session_for(&state, PrincipalRole::Aparatchi, "worker-raw-unlink-locked").await;
    let router = build_router(state);

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &pechat_order_map_json(
                "zakaz-raw-unlink-locked",
                "Raw unlink locked",
                "8833",
                "7 ta rangli pechat - A",
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
            r#"{"apparatus":"7 ta rangli pechat - A","requires_material":true,"item_groups":["Kraska"]}"#,
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
            r#"{"order_id":"zakaz-raw-unlink-locked","barcode":"30AA"}"#,
        ))
        .await
        .expect("assign");
    assert_eq!(assigned.status(), StatusCode::OK);

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat - A",
                "order_id":"zakaz-raw-unlink-locked",
                "action":"start",
                "material_barcodes":["30AA"]
            }"#,
        ))
        .await
        .expect("start with material");
    assert_eq!(started.status(), StatusCode::OK);

    let locked = router
        .clone()
        .oneshot(request_with_body(
            "DELETE",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
            r#"{"order_id":"zakaz-raw-unlink-locked","barcode":"30AA"}"#,
        ))
        .await
        .expect("unlink locked assignment");
    assert_eq!(locked.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(locked).await["error"],
        "raw_material_assignment_locked"
    );

    let assignments_after_reject = router
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/raw-material-assignments",
            &token,
        ))
        .await
        .expect("assignments after locked unlink");
    assert_eq!(assignments_after_reject.status(), StatusCode::OK);
    let assignments_body = json_body(assignments_after_reject).await;
    assert!(
        assignments_body
            .as_array()
            .expect("assignments array")
            .iter()
            .any(|item| {
                item["order_id"] == "zakaz-raw-unlink-locked"
                    && item["barcode"] == "30AA"
                    && item["stock_status"] == "in_use"
            })
    );
}

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
            assigned_apparatus: vec!["7 ta rangli pechat - A".to_string()],
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
                "7 ta rangli pechat - A",
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
            r#"{"apparatus":"7 ta rangli pechat - A","requires_material":true,"item_groups":["Kraska"]}"#,
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
                "order_id":"zakaz-raw-reserved",
                "barcode":"30AA"
            }"#,
        ))
        .await
        .expect("assign");
    assert_eq!(assigned.status(), StatusCode::OK);
    material_store
        .set_stock_status("30AA", "in_use", "zakaz-other")
        .await;

    let start = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat - A",
                "order_id":"zakaz-raw-reserved",
                "action":"start",
                "material_barcodes":["30AA"]
            }"#,
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
                "7 ta rangli pechat - A",
                7.0,
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
            r#"{"apparatus":"7 ta rangli pechat - A","requires_material":true,"item_groups":["Rulon"]}"#,
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
    assert_eq!(
        json_body(undersized).await["error"],
        "raw_material_roll_size_mismatch"
    );

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
    assert_eq!(
        json_body(oversized).await["error"],
        "raw_material_roll_size_mismatch"
    );
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
