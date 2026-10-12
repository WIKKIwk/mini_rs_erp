#![cfg(feature = "verification")]
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use mini_rs_erp::core::{
    apparatus_standard::{ApparatusId, RuntimeApparatusConfiguration},
    auth::models::{Principal, PrincipalRole},
    production_map::*,
};
use mini_rs_erp::{app::AppState, config::AppConfig, http::router::build_router};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};
use tower::ServiceExt;
type R<T> = Result<T, ProductionMapError>;
struct EmptyCatalog;
#[async_trait::async_trait]
impl CanonicalApparatusResolver for EmptyCatalog {
    async fn resolve(&self, _: &ApparatusId) -> R<Option<Arc<RuntimeApparatusConfiguration>>> {
        Ok(None)
    }
    async fn list(&self) -> R<Vec<Arc<RuntimeApparatusConfiguration>>> {
        Ok(vec![])
    }
}
struct ReprintStore {
    record: LayerRemainderRecord,
}
#[async_trait::async_trait]
impl ProductionMapStorePort for ReprintStore {
    async fn maps(&self) -> R<Vec<ProductionMapDefinition>> {
        Ok(vec![])
    }
    async fn put_map(&self, _: ProductionMapDefinition) -> R<()> {
        panic!("reprint wrote map")
    }
    async fn put_maps_batch(&self, _: &[ProductionMapDefinition]) -> R<()> {
        panic!("reprint wrote maps")
    }
    async fn delete_map(&self, _: &str) -> R<()> {
        panic!("reprint deleted map")
    }
    async fn apparatus_sequences(&self) -> R<BTreeMap<String, Vec<String>>> {
        Ok(BTreeMap::new())
    }
    async fn put_apparatus_sequence(&self, _: &str, _: Vec<String>) -> R<()> {
        panic!("reprint wrote sequence")
    }
    async fn apparatus_queue_states(&self) -> R<BTreeMap<String, BTreeMap<String, String>>> {
        Ok(BTreeMap::new())
    }
    async fn put_apparatus_queue_states(&self, _: &str, _: BTreeMap<String, String>) -> R<()> {
        panic!("reprint wrote queue")
    }
    async fn raw_material_assignments(&self) -> R<Vec<RawMaterialAssignment>> {
        Ok(vec![])
    }
    async fn put_raw_material_assignment(&self, _: RawMaterialAssignment) -> R<()> {
        panic!("reprint wrote stock")
    }
    async fn delete_raw_material_assignment(
        &self,
        _: &str,
        _: &str,
    ) -> R<Option<RawMaterialAssignment>> {
        panic!("reprint deleted stock")
    }
    async fn layer_remainder_by_id(&self, id: &str) -> R<Option<LayerRemainderRecord>> {
        Ok((id == self.record.operation_id).then(|| self.record.clone()))
    }
    async fn record_layer_remainder(
        &self,
        _: LayerRemainderInput,
        _: QueueActionActor,
    ) -> R<LayerRemainderRecord> {
        panic!("reprint created remainder")
    }
}
fn test_config(path: &Path) -> AppConfig {
    AppConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        default_target_warehouse: String::new(),
        http_timeout: Duration::from_secs(2),
        session_store_path: path.join("sessions.json"),
        profile_store_path: path.join("profiles.json"),
        push_token_store_path: path.join("push.json"),
        session_ttl_seconds: Some(3600),
        supplier_prefix: "10".into(),
        werka_prefix: "20".into(),
        werka_code: String::new(),
        werka_name: "Warehouse".into(),
        werka_phone: String::new(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: String::new(),
        material_taminotchi_phone: String::new(),
        admin_phone: String::new(),
        admin_name: String::new(),
        admin_code: String::new(),
    }
}

fn record() -> LayerRemainderRecord {
    serde_json::from_value(json!({"operation_id":"operation-20","apparatus":"apparatus:default:laminatsiya_1","order_id":"order-1","session_id":"session-1","source_kind":"raw_material","source_id":"raw-50","qr_payload":"2001REMAINDER20","source_net_kg":50,"source_meter":1500,"tare_kg":2,"producer_name":"Original supplier","remaining_net_kg":20,"remaining_meter":600,"recorded_by":{"role":"aparatchi","ref_":"laminator","display_name":"Laminator"},"created_at_unix":10,"progress_batch":null,"raw_material_stock":{"id":"raw-20","warehouse":"Warehouse","item_code":"CPP-1","item_name":"CPP","barcode":"2001REMAINDER20","qty":20,"uom":"kg","status":"available","source_receipt_id":"receipt-50","width_mm":1000,"micron":25,"length_m":600,"payload_json":{}}})).unwrap()
}
async fn state() -> (AppState, String, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut state = AppState::verification(test_config(dir.path()));
    state.production_maps = ProductionMapService::new(
        Arc::new(ReprintStore { record: record() }),
        Arc::new(EmptyCatalog),
    );
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Admin,
            ref_: "admin".into(),
            display_name: "Admin".into(),
            legal_name: "Admin".into(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .unwrap();
    (state, token, dir)
}
async fn post(state: AppState, token: &str, body: Value) -> (StatusCode, Value) {
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/admin/production-maps/layer-remainders/reprint")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    (status, body)
}
#[tokio::test]
async fn offline_raw_reprint_preserves_original_producer_tare_and_dimensions() {
    let (state, token, _dir) = state().await;
    let body = json!({"apparatus":record().apparatus,"order_id":"order-1","operation_id":"operation-20","printer":"godex","print_mode":"label","print_transport":"offline","print_count":1});
    for _ in 0..2 {
        let (status, response) = post(state.clone(), &token, body.clone()).await;
        assert_eq!(status, StatusCode::OK, "{response}");
        assert_eq!(response["remainder"]["qr_payload"], "2001REMAINDER20");
        assert_eq!(response["print"]["ok"], true);
        assert_eq!(response["print"]["executor_name"], "Original supplier");
        assert_eq!(response["print"]["item_name"], "CPP 1000/25");
        assert_eq!(response["print"]["gross_qty"].as_f64(), Some(22.0));
        assert_eq!(response["print"]["apparatus"], "");
        assert_eq!(response["print"]["net_qty"].as_f64(), Some(20.0));
        assert_eq!(response["print"]["tare_kg"].as_f64(), Some(2.0));
        assert_eq!(response["print"]["progress_qty"].as_f64(), Some(600.0));
        assert_eq!(response["print"]["width_mm"].as_f64(), Some(1000.0));
        assert_eq!(response["print"]["micron"].as_f64(), Some(25.0));
    }
    // Online failure and scoped authorization use the same saved operation.

    let body = json!({"apparatus":record().apparatus,"order_id":"order-1","operation_id":"operation-20","printer":"godex","print_mode":"label"});
    let (status, response) = post(state.clone(), &token, body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["ok"], true);
    assert_eq!(response["print"]["ok"], false);
    assert_eq!(
        response["remainder"]["remaining_net_kg"].as_f64(),
        Some(20.0)
    );
    let mut wrong = body;
    wrong["order_id"] = json!("other-order");
    assert_eq!(post(state, &token, wrong).await.0, StatusCode::FORBIDDEN);
}
