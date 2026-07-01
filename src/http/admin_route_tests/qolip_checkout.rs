use super::*;
use crate::core::qolip::{
    QolipBlock, QolipCellQr, QolipCheckout, QolipError, QolipLocation, QolipProduct,
    QolipProductSpec, QolipService, QolipStorePort,
};

#[tokio::test]
async fn qolip_checkout_decrements_location_and_rejects_overdraw() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let worker = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"id":"checkout_worker_1","name":"Checkout worker","phone":"+998901112277","level":"Master"}"#,
        ))
        .await
        .expect("create worker");
    assert_eq!(worker.status(), StatusCode::OK);

    let location = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/locations",
            &token,
            r#"{
                "block":"A",
                "warehouse":"Qolip ombor",
                "item_code":"ITEM-1",
                "item_name":"Test qolip",
                "qolip_code":"Q-100",
                "size":42,
                "quantity":5,
                "row_letter":"B",
                "column_number":3
            }"#,
        ))
        .await
        .expect("create location");
    assert_eq!(location.status(), StatusCode::OK);
    let location_body = json_body(location).await;
    let location_id = location_body["location"]["id"]
        .as_str()
        .expect("location id")
        .to_string();

    let workers = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/qolip/workers", &token))
        .await
        .expect("list workers");
    assert_eq!(workers.status(), StatusCode::OK);
    let workers_body = json_body(workers).await;
    assert!(
        workers_body["workers"]
            .as_array()
            .expect("workers")
            .iter()
            .any(|worker| worker["id"] == "checkout_worker_1")
    );

    let checkout = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/checkouts",
            &token,
            &format!(
                r#"{{"location_id":"{location_id}","quantity":2,"worker_id":"checkout_worker_1"}}"#
            ),
        ))
        .await
        .expect("issue checkout");
    assert_eq!(checkout.status(), StatusCode::OK);
    let checkout_body = json_body(checkout).await;
    assert_eq!(checkout_body["checkout"]["quantity"], 2);
    assert_eq!(
        checkout_body["checkout"]["issued_to_name"],
        "Checkout worker"
    );

    let locations = build_router(state.clone())
        .oneshot(request("GET", "/v1/mobile/qolip/locations?block=A", &token))
        .await
        .expect("list locations");
    assert_eq!(locations.status(), StatusCode::OK);
    let locations_body = json_body(locations).await;
    let remaining = locations_body["locations"]
        .as_array()
        .expect("locations")
        .iter()
        .find(|entry| entry["id"] == location_id)
        .and_then(|entry| entry["quantity"].as_i64())
        .expect("remaining quantity");
    assert_eq!(remaining, 3);

    let overdraw = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/checkouts",
            &token,
            &format!(
                r#"{{"location_id":"{location_id}","quantity":4,"worker_id":"checkout_worker_1"}}"#
            ),
        ))
        .await
        .expect("overdraw checkout");
    assert_eq!(overdraw.status(), StatusCode::CONFLICT);
    let overdraw_body = json_body(overdraw).await;
    assert_eq!(overdraw_body["error"], "insufficient_stock");
}

#[tokio::test]
async fn pechat_queue_start_requires_matching_qolip_code_scan() {
    let state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "pechat-worker-qolip".to_string(),
            role_id: "aparatchi".to_string(),
            assigned_apparatus: vec!["7 ta rangli pechat - A".to_string()],
        })
        .await
        .expect("aparatchi assignment");
    let token = session(&state, PrincipalRole::Admin).await;
    let worker_token = session_for(&state, PrincipalRole::Aparatchi, "pechat-worker-qolip").await;
    let router = build_router(state.clone());

    let map = router
        .clone()
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            &production_order_map_json_with_product(
                "zakaz-qolip-scan",
                "Test qolip order",
                "ITEM-QOLIP",
                "9901",
                "7 ta rangli pechat - A",
                7.0,
                1250.0,
            ),
        ))
        .await
        .expect("map save");
    assert_eq!(map.status(), StatusCode::OK);

    let spec = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/product-specs",
            &token,
            r#"{
                "item_code":"ITEM-QOLIP",
                "item_name":"Test qolip order",
                "item_group":"Tayyor mahsulot",
                "qolip_code":"QOLIP-SCAN-1",
                "size":42
            }"#,
        ))
        .await
        .expect("spec save");
    assert_eq!(spec.status(), StatusCode::OK);

    let location = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/locations",
            &token,
            r#"{
                "block":"A",
                "warehouse":"Qolip ombor",
                "item_code":"ITEM-QOLIP",
                "item_name":"Test qolip order",
                "qolip_code":"QOLIP-SCAN-1",
                "size":42,
                "quantity":1,
                "row_letter":"B",
                "column_number":2
            }"#,
        ))
        .await
        .expect("location save");
    assert_eq!(location.status(), StatusCode::OK);

    let missing_scan = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat - A",
                "order_id":"zakaz-qolip-scan",
                "action":"start"
            }"#,
        ))
        .await
        .expect("queue action without qolip");
    assert_eq!(missing_scan.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_scan).await["error"],
        "qolip_scan_required"
    );

    let wrong_scan = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat - A",
                "order_id":"zakaz-qolip-scan",
                "action":"start",
                "qolip_code":"QOLIP-WRONG"
            }"#,
        ))
        .await
        .expect("queue action with wrong qolip");
    assert_eq!(wrong_scan.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(wrong_scan).await["error"], "qolip_code_not_found");

    let started = router
        .clone()
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/production-maps/queue-action",
            &worker_token,
            r#"{
                "apparatus":"7 ta rangli pechat - A",
                "order_id":"zakaz-qolip-scan",
                "action":"start",
                "qolip_code":"QOLIP-SCAN-1"
            }"#,
        ))
        .await
        .expect("queue action with qolip");
    assert_eq!(started.status(), StatusCode::OK);
    let started_body = json_body(started).await;
    assert_eq!(
        started_body["states"]["zakaz-qolip-scan"],
        serde_json::json!("in_progress")
    );

    let checkouts = router
        .oneshot(request("GET", "/v1/mobile/qolip/checkouts", &token))
        .await
        .expect("qolip checkouts");
    assert_eq!(checkouts.status(), StatusCode::OK);
    let checkouts_body = json_body(checkouts).await;
    let checkout = checkouts_body["checkouts"]
        .as_array()
        .expect("checkouts")
        .iter()
        .find(|entry| entry["qolip_code"] == "QOLIP-SCAN-1")
        .expect("qolip checkout");
    assert_eq!(checkout["issued_to_ref"], "pechat-worker-qolip");
    assert_eq!(checkout["quantity"], 1);
}

#[tokio::test]
async fn qolip_locations_without_assignment_are_forbidden() {
    let state = test_state();
    let token = session_for(&state, PrincipalRole::Qolipchi, "qolipchi-no-block").await;

    let response = build_router(state)
        .oneshot(request("GET", "/v1/mobile/qolip/locations", &token))
        .await
        .expect("locations response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(response).await["error"], "forbidden");
}

#[tokio::test]
async fn qolip_checkout_uses_authorized_location_snapshot() {
    let store = Arc::new(FlippingCheckoutStore::new());
    let mut state = test_state();
    let role_store = Arc::new(MemoryRoleDefinitionStore::new());
    role_store
        .put_role_assignment(RoleAssignment {
            principal_role: PrincipalRole::Qolipchi,
            principal_ref: "qolipchi-a".to_string(),
            role_id: "qolipchi".to_string(),
            assigned_apparatus: Vec::new(),
        })
        .await
        .expect("put role assignment");
    state.admin = state.admin.with_role_store(role_store);
    state.qolip = QolipService::new(store);
    let token = session(&state, PrincipalRole::Admin).await;

    let worker = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/admin/workers",
            &token,
            r#"{"id":"checkout_snapshot_worker","name":"Snapshot worker","phone":"+998901112288","level":"Master"}"#,
        ))
        .await
        .expect("create worker");
    assert_eq!(worker.status(), StatusCode::OK);

    let qolipchi_token = session_for(&state, PrincipalRole::Qolipchi, "qolipchi-a").await;
    let checkout = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/checkouts",
            &qolipchi_token,
            r#"{"location_id":"LOC-SWAP","quantity":1,"worker_id":"checkout_snapshot_worker"}"#,
        ))
        .await
        .expect("checkout response");

    assert_eq!(checkout.status(), StatusCode::OK);
    let body = json_body(checkout).await;
    assert_eq!(body["checkout"]["block"], "A");
    assert_eq!(body["checkout"]["item_code"], "ITEM-A");
}

struct FlippingCheckoutStore {
    calls: AtomicUsize,
}

impl FlippingCheckoutStore {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }

    fn location(block: &str, item_code: &str) -> QolipLocation {
        QolipLocation {
            id: "LOC-SWAP".to_string(),
            block: block.to_string(),
            warehouse: "Qolip ombor".to_string(),
            item_code: item_code.to_string(),
            item_name: item_code.to_string(),
            qolip_code: format!("Q-{item_code}"),
            size: 42,
            quantity: 3,
            row_letter: "B".to_string(),
            column_number: Some(1),
            location_label: "B1".to_string(),
            created_by_role: "admin".to_string(),
            created_by_ref: "admin".to_string(),
            created_by_name: "Admin".to_string(),
        }
    }
}

#[async_trait]
impl QolipStorePort for FlippingCheckoutStore {
    async fn assigned_warehouses(&self, _principal: &Principal) -> Result<Vec<String>, QolipError> {
        Ok(vec!["Qolip ombor".to_string()])
    }

    async fn assigned_blocks(&self, _principal: &Principal) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(vec![QolipBlock {
            name: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
        }])
    }

    async fn all_blocks(&self) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(vec![
            QolipBlock {
                name: "A".to_string(),
                warehouse: "Qolip ombor".to_string(),
            },
            QolipBlock {
                name: "B".to_string(),
                warehouse: "Qolip ombor".to_string(),
            },
        ])
    }

    async fn products(
        &self,
        _query: &str,
        _limit: usize,
        _with_qolip_only: bool,
    ) -> Result<Vec<QolipProduct>, QolipError> {
        Ok(Vec::new())
    }

    async fn product_spec(&self, _item_code: &str) -> Result<Option<QolipProductSpec>, QolipError> {
        Ok(None)
    }

    async fn put_product_spec(
        &self,
        spec: QolipProductSpec,
    ) -> Result<QolipProductSpec, QolipError> {
        Ok(spec)
    }

    async fn locations(&self, _block: &str) -> Result<Vec<QolipLocation>, QolipError> {
        Ok(Vec::new())
    }

    async fn put_location(&self, location: QolipLocation) -> Result<QolipLocation, QolipError> {
        Ok(location)
    }

    async fn get_or_create_cell_qr(&self, cell: QolipCellQr) -> Result<QolipCellQr, QolipError> {
        Ok(cell)
    }

    async fn location_by_id(
        &self,
        _location_id: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            Ok(Some(Self::location("A", "ITEM-A")))
        } else {
            Ok(Some(Self::location("B", "ITEM-B")))
        }
    }

    async fn issue_checkout(&self, checkout: QolipCheckout) -> Result<QolipCheckout, QolipError> {
        Ok(checkout)
    }

    async fn checkouts(
        &self,
        _block: Option<&str>,
        _allowed_blocks: Option<&[String]>,
        _status: &str,
        _limit: usize,
    ) -> Result<Vec<QolipCheckout>, QolipError> {
        Ok(Vec::new())
    }

    async fn checkout_by_id(
        &self,
        _checkout_id: &str,
    ) -> Result<Option<QolipCheckout>, QolipError> {
        Ok(None)
    }

    async fn return_checkout(
        &self,
        _checkout_id: &str,
        _row_letter: &str,
        _column_number: Option<i32>,
    ) -> Result<QolipCheckout, QolipError> {
        Err(QolipError::CheckoutNotFound)
    }

    async fn move_location(
        &self,
        _location_id: &str,
        _row_letter: &str,
        _column_number: i32,
        _quantity: i32,
    ) -> Result<QolipLocation, QolipError> {
        Err(QolipError::LocationNotFound)
    }

    async fn cell_qr_by_payload(
        &self,
        _qr_payload: &str,
    ) -> Result<Option<QolipCellQr>, QolipError> {
        Ok(None)
    }
}
