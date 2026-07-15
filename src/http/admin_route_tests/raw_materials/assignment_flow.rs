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
            assigned_item_groups: Vec::new(),
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
    provision_test_qolip(&router, &token, "zakaz-raw-route").await;

    let rule = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/raw-material-rules",
            &token,
            r#"{
                "apparatus":"7 ta rangli pechat - A",
                "requires_material":true,
                "item_groups":["Kraska","Kley"],
                "requirement_groups":[
                    {
                        "name":"Yopishtiruvchi",
                        "item_groups":["Kraska","Kley"],
                        "min_required_count":1
                    }
                ]
            }"#,
        ))
        .await
        .expect("rule save");
    assert_eq!(rule.status(), StatusCode::OK);
    let rule_body = json_body(rule).await;
    assert_eq!(rule_body["apparatus"], "7 ta rangli pechat - A");
    assert_eq!(rule_body["requires_material"], true);
    assert_eq!(rule_body["requirement_groups"][0]["name"], "Yopishtiruvchi");
    assert_eq!(
        rule_body["requirement_groups"][0]["item_groups"][0],
        "Kraska"
    );

    let missing_assignment = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(r#"{
                "apparatus":"7 ta rangli pechat - A",
                "order_id":"zakaz-raw-route",
                "action":"start"
            }"#, "zakaz-raw-route"),
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
    assert_eq!(lookup_body["status"], "available");
    assert_eq!(lookup_body["source_receipt_id"], "GSR-30AA");
    assert_eq!(lookup_body["assignment"]["order_id"], "zakaz-raw-route");
    assert_eq!(
        lookup_body["assignment"]["apparatus"],
        "7 ta rangli pechat - A"
    );
    assert_eq!(lookup_body["order"]["id"], "zakaz-raw-route");
    assert!(lookup_body["queue_states"].is_object());
    assert!(lookup_body["logs"].is_array());

    let missing_scan = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            &with_test_qolip(r#"{
                "apparatus":"7 ta rangli pechat - A",
                "order_id":"zakaz-raw-route",
                "action":"start"
            }"#, "zakaz-raw-route"),
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
            &with_test_qolip(r#"{
                "apparatus":"7 ta rangli pechat - A",
                "order_id":"zakaz-raw-route",
                "action":"start",
                "material_barcodes":["30AA"]
            }"#, "zakaz-raw-route"),
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
            &with_test_qolip(r#"{
                "apparatus":"7 ta rangli pechat - A",
                "order_id":"zakaz-raw-route",
                "action":"start",
                "material_barcodes":["30AA","30CC"]
            }"#, "zakaz-raw-route"),
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
            &with_test_returned_paint(r#"{
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
            }"#),
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
            assigned_apparatus: Vec::new(),
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
            &admin_token,
            r#"{
                "apparatus":"7 ta rangli pechat - A",
                "requires_material":true,
                "item_groups":["Kraska"]
            }"#,
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
