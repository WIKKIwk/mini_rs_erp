use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

use super::router::build_router;
use crate::app::AppState;
use crate::config::AppConfig;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::gscale::models::{ScaleDriverPrintRequest, ScaleDriverPrintResponse};
use crate::core::gscale::ports::{EpcSource, GscalePortError, ScaleDriverPort};
use crate::core::rezka::RezkaService;
use crate::core::rezka::models::{CreateRezkaRepackDraftInput, RezkaRepackDraft};
use crate::core::rezka::ports::{RezkaErpPort, RezkaPortError};
use crate::core::session::manager::SessionManager;
use crate::core::werka::models::StockEntryBarcodeEntry;
use crate::core::werka::ports::{WerkaHomeLookup, WerkaPortError};
use crate::core::werka::service::WerkaService;

#[tokio::test]
async fn rezka_source_returns_current_stock_entry_piece() {
    let state = test_state(Arc::new(FakeLookup));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/rezka/source?barcode=SRC-600",
            &token,
            "",
        ))
        .await
        .expect("response");
    let body = json_body(response).await;

    assert_eq!(body["ok"], true);
    assert_eq!(body["source"]["barcode"], "SRC-600");
    assert_eq!(body["source"]["item_code"], "FLEXO-RAW");
    assert_eq!(body["source"]["warehouse"], "Stores - A");
    assert_eq!(body["source"]["qty"], 600.0);
}

#[tokio::test]
async fn rezka_split_creates_repack_and_prints_output_qrs() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut state = test_state(Arc::new(FakeLookup));
    state.rezka = RezkaService::new()
        .with_erp(Arc::new(FakeRezkaErp {
            events: events.clone(),
        }))
        .with_driver(Arc::new(FakeDriver {
            events: events.clone(),
        }))
        .with_epc_source(Arc::new(SeqEpc::new(["EPC-400", "EPC-200"])));
    let token = session(&state, PrincipalRole::Admin).await;

    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/rezka/split",
            &token,
            r#"{
                "source_barcode":"SRC-600",
                "source_stock_entry":"MAT-STE-001",
                "source_line_index":1,
                "reason":"Zakaz uchun kesildi",
                "driver_url":"http://127.0.0.1:39117",
                "printer":"godex",
                "print_mode":"label",
                "outputs":[
                    {
                        "item_code":"FLEXO-400",
                        "item_name":"Flexo 400",
                        "qty":400,
                        "uom":"m",
                        "target_warehouse":"Work In Progress - A",
                        "reason":"400 metr zakazga"
                    },
                    {
                        "item_code":"FLEXO-200",
                        "item_name":"Flexo 200",
                        "qty":200,
                        "uom":"m",
                        "target_warehouse":"Stores - A",
                        "reason":"qoldiq qaytdi"
                    }
                ]
            }"#,
        ))
        .await
        .expect("response");
    let body = json_body(response).await;

    assert_eq!(body["ok"], true);
    assert_eq!(body["stock_entry_name"], "MAT-STE-REPACK-1");
    assert_eq!(body["outputs"][0]["epc"], "EPC-400");
    assert_eq!(body["outputs"][1]["epc"], "EPC-200");
    assert_eq!(body["outputs"][0]["reason"], "400 metr zakazga");
    assert_eq!(body["outputs"][1]["reason"], "qoldiq qaytdi");
    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            "draft:SRC-600:2:Zakaz uchun kesildi:400 metr zakazga,qoldiq qaytdi",
            "print:EPC-400:FLEXO-400:400.000:m",
            "print:EPC-200:FLEXO-200:200.000:m",
            "submit:MAT-STE-REPACK-1"
        ]
    );
}

#[tokio::test]
async fn rezka_split_requires_rezka_capability() {
    let state = test_state(Arc::new(FakeLookup));
    let token = session(&state, PrincipalRole::Supplier).await;

    let response = build_router(state)
        .oneshot(request(
            "POST",
            "/v1/mobile/rezka/split",
            &token,
            r#"{"source_barcode":"SRC-600"}"#,
        ))
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

fn test_state(lookup: Arc<dyn WerkaHomeLookup>) -> AppState {
    let mut state = AppState::new(AppConfig {
        bind_addr: "127.0.0.1:8081".parse().expect("addr"),
        default_target_warehouse: String::new(),
        http_timeout: Duration::from_secs(15),
        session_store_path: "data/mobile_sessions.json".into(),
        profile_store_path: "data/mobile_profile_prefs.json".into(),
        push_token_store_path: "data/mobile_push_tokens.json".into(),
        admin_supplier_store_path: "data/mobile_admin_suppliers.json".into(),
        session_ttl_seconds: Some(30 * 24 * 60 * 60),
        supplier_prefix: "10".to_string(),
        werka_prefix: "20".to_string(),
        werka_code: "20ABCDEF1234".to_string(),
        werka_name: "Werka".to_string(),
        werka_phone: "+99888862440".to_string(),
        admin_phone: "+998880000000".to_string(),
        admin_name: "Admin".to_string(),
        admin_code: "19621978".to_string(),
    });
    state.sessions = SessionManager::memory(Some(30 * 24 * 60 * 60));
    state.werka = WerkaService::new().with_lookup(lookup);
    state
}

async fn session(state: &AppState, role: PrincipalRole) -> String {
    state
        .sessions
        .create(Principal {
            role,
            display_name: "Admin".to_string(),
            legal_name: "Admin".to_string(),
            ref_: "admin".to_string(),
            phone: "+998880000000".to_string(),
            avatar_url: String::new(),
        })
        .await
        .expect("session")
}

fn request(method: &str, uri: &str, token: &str, body: &str) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if !token.trim().is_empty() {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

async fn json_body(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    serde_json::from_slice(&bytes).expect("json")
}

struct FakeLookup;

#[async_trait]
impl WerkaHomeLookup for FakeLookup {
    async fn stock_entries_by_barcode(
        &self,
        barcode: &str,
        _limit: usize,
    ) -> Result<Vec<StockEntryBarcodeEntry>, WerkaPortError> {
        if barcode != "SRC-600" {
            return Ok(Vec::new());
        }
        Ok(vec![StockEntryBarcodeEntry {
            stock_entry_name: "MAT-STE-001".to_string(),
            stock_entry_type: "Material Receipt".to_string(),
            doc_status: 1,
            status: "Submitted".to_string(),
            company: "Accord".to_string(),
            posting_date: "2026-06-04".to_string(),
            posting_time: "09:00:00".to_string(),
            creation: "2026-06-04 09:00:00".to_string(),
            modified: "2026-06-04 09:00:00".to_string(),
            remarks: String::new(),
            line_index: 1,
            item_code: "FLEXO-RAW".to_string(),
            item_name: "Flexo raw".to_string(),
            qty: 600.0,
            uom: "m".to_string(),
            stock_uom: "m".to_string(),
            barcode: "SRC-600".to_string(),
            source_warehouse: String::new(),
            target_warehouse: "Stores - A".to_string(),
        }])
    }
}

struct FakeRezkaErp {
    events: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl RezkaErpPort for FakeRezkaErp {
    async fn create_rezka_repack_draft(
        &self,
        input: CreateRezkaRepackDraftInput,
    ) -> Result<RezkaRepackDraft, RezkaPortError> {
        self.events.lock().unwrap().push(format!(
            "draft:{}:{}:{}:{}",
            input.source.barcode,
            input.outputs.len(),
            input.reason,
            input
                .outputs
                .iter()
                .map(|output| output.reason.as_str())
                .collect::<Vec<_>>()
                .join(",")
        ));
        Ok(RezkaRepackDraft {
            name: "MAT-STE-REPACK-1".to_string(),
        })
    }

    async fn submit_rezka_repack_draft(&self, name: &str) -> Result<(), RezkaPortError> {
        self.events.lock().unwrap().push(format!("submit:{name}"));
        Ok(())
    }

    async fn delete_rezka_repack_draft(&self, name: &str) -> Result<(), RezkaPortError> {
        self.events.lock().unwrap().push(format!("delete:{name}"));
        Ok(())
    }
}

struct FakeDriver {
    events: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ScaleDriverPort for FakeDriver {
    async fn print_material_receipt(
        &self,
        request: ScaleDriverPrintRequest,
    ) -> Result<ScaleDriverPrintResponse, GscalePortError> {
        self.events.lock().unwrap().push(format!(
            "print:{}:{}:{:.3}:{}",
            request.epc, request.item_code, request.gross_qty, request.unit
        ));
        Ok(ScaleDriverPrintResponse {
            ok: true,
            status: "done".to_string(),
            epc: request.epc,
            printer: request.printer,
            mode: request.print_mode,
            printer_status: "OK".to_string(),
            ..ScaleDriverPrintResponse::default()
        })
    }
}

struct SeqEpc {
    values: Mutex<Vec<String>>,
}

impl SeqEpc {
    fn new<const N: usize>(values: [&str; N]) -> Self {
        Self {
            values: Mutex::new(values.into_iter().rev().map(str::to_string).collect()),
        }
    }
}

impl EpcSource for SeqEpc {
    fn next_epc(&self) -> String {
        self.values.lock().unwrap().pop().unwrap_or_default()
    }
}
