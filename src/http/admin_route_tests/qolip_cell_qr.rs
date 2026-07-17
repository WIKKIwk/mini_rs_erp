use super::*;
use crate::core::qolip::{
    QolipBlock, QolipCellQr, QolipCheckout, QolipError, QolipLocation, QolipProduct,
    QolipProductSpec, QolipService, QolipStorePort,
};

#[tokio::test]
async fn qolip_cell_qr_print_reuses_same_payload_for_same_cell() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests.clone(),
        fail: false,
    }));
    let token = session(&state, PrincipalRole::Admin).await;

    let body = r#"{
        "warehouse":"Qolip ombor",
        "block":"A",
        "row_letter":"B",
        "column_number":7,
        "driver_url":"http://127.0.0.1:39117",
        "printer":"zebra",
        "print_mode":"rfid"
    }"#;

    let first = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/cell-qr/print",
            &token,
            body,
        ))
        .await
        .expect("first print");
    assert_eq!(first.status(), StatusCode::OK);
    let first_body = json_body(first).await;
    let first_qr = first_body["cell_qr"]["qr_payload"]
        .as_str()
        .expect("first qr")
        .to_string();
    assert!(first_qr.starts_with("4002"), "{first_qr}");

    let second = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/cell-qr/print",
            &token,
            body,
        ))
        .await
        .expect("second print");
    assert_eq!(second.status(), StatusCode::OK);
    let second_body = json_body(second).await;
    assert_eq!(second_body["cell_qr"]["qr_payload"], first_qr);

    let printed = print_requests.lock().await;
    assert_eq!(printed.len(), 2);
    assert_eq!(printed[0].epc, first_qr);
    assert_eq!(printed[1].epc, first_qr);
    assert_eq!(printed[0].label_kind, "qolip_cell");
    assert_eq!(printed[0].item_name, "B7");
}

#[tokio::test]
async fn qolip_cell_qr_offline_prepares_label_without_rps_driver() {
    let mut state = test_state();
    state.gscale = GscaleService::new();
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/cell-qr/print",
            &token,
            r#"{
                "warehouse":"Qolip ombor",
                "block":"A",
                "row_letter":"B",
                "column_number":7,
                "driver_url":"usb://local",
                "printer":"godex",
                "print_mode":"label",
                "print_transport":"offline"
            }"#,
        ))
        .await
        .expect("offline print prepare");

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;
    assert_eq!(body["print"]["status"], "prepared");
    assert_eq!(body["print"]["printer_status"], "client_usb_pending");
    assert_eq!(body["print"]["label_kind"], "qolip_cell");
    assert_eq!(body["print"]["qr_payload"], body["cell_qr"]["qr_payload"]);
}

#[tokio::test]
async fn qolip_cell_qr_lookup_forbidden_block_does_not_create_qr_row() {
    let store = Arc::new(ForbiddenQolipStore::new());
    let mut state = test_state();
    state.qolip = QolipService::new(store.clone());
    let token = session_for(&state, PrincipalRole::Qolipchi, "worker-no-block").await;
    let qr_payload = "4002DC80510E7F1F83DB83DB";

    let response = build_router(state)
        .oneshot(request(
            "GET",
            &format!("/v1/mobile/qolip/cell-qr?qr={qr_payload}"),
            &token,
        ))
        .await
        .expect("cell qr lookup");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(store.created_count().await, 0);
}

#[tokio::test]
async fn qolip_code_qr_print_uses_code_as_stable_payload() {
    let print_requests = Arc::new(Mutex::new(Vec::<ScaleDriverPrintRequest>::new()));
    let mut state = test_state();
    state.gscale = GscaleService::new().with_driver(Arc::new(FakeProgressDriver {
        requests: print_requests.clone(),
        fail: false,
    }));
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/product-specs",
            &token,
            r#"{
                "item_code":"ITEM-001",
                "item_name":"Kross qolip",
                "item_group":"Qolip",
                "qolip_code":"QOLIP-0007",
                "size":42
            }"#,
        ))
        .await
        .expect("save product spec");
    assert_eq!(save.status(), StatusCode::OK);

    let print = build_router(state)
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/code-qr/print",
            &token,
            r#"{
                "qolip_code":"QOLIP-0007",
                "driver_url":"http://127.0.0.1:39117",
                "printer":"zebra",
                "print_mode":"rfid"
            }"#,
        ))
        .await
        .expect("print qolip code qr");
    assert_eq!(print.status(), StatusCode::OK);
    let body = json_body(print).await;
    assert_eq!(body["qolip_qr"]["qolip_code"], "QOLIP-0007");
    assert_eq!(body["qolip_qr"]["qr_payload"], "QOLIP-0007");

    let printed = print_requests.lock().await;
    assert_eq!(printed.len(), 1);
    assert_eq!(printed[0].epc, "QOLIP-0007");
    assert_eq!(printed[0].item_code, "QOLIP-0007");
    assert_eq!(printed[0].item_name, "Kross qolip • 42");
    assert_eq!(printed[0].label_kind, "qolip_cell");
}

struct ForbiddenQolipStore {
    created: Mutex<Vec<QolipCellQr>>,
}

impl ForbiddenQolipStore {
    fn new() -> Self {
        Self {
            created: Mutex::new(Vec::new()),
        }
    }

    async fn created_count(&self) -> usize {
        self.created.lock().await.len()
    }
}

#[async_trait]
impl QolipStorePort for ForbiddenQolipStore {
    async fn assigned_warehouses(&self, _principal: &Principal) -> Result<Vec<String>, QolipError> {
        Ok(Vec::new())
    }

    async fn assigned_blocks(&self, _principal: &Principal) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(Vec::new())
    }

    async fn all_blocks(&self) -> Result<Vec<QolipBlock>, QolipError> {
        Ok(vec![QolipBlock {
            name: "A".to_string(),
            warehouse: "Qolip ombor".to_string(),
        }])
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
        self.created.lock().await.push(cell.clone());
        Ok(cell)
    }

    async fn location_by_id(
        &self,
        _location_id: &str,
    ) -> Result<Option<QolipLocation>, QolipError> {
        Ok(None)
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
