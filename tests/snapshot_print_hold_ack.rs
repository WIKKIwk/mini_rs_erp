#![cfg(feature = "verification")]
//! The actual HTTP mutation + ACK + live snapshot over the production memory store.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use mini_rs_erp::{
    app::AppState,
    config::AppConfig,
    core::{
        auth::models::{Principal, PrincipalRole},
        production_map::*,
        session::manager::SessionManager,
    },
    http::router::build_router,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;

type Result<T> = std::result::Result<T, ProductionMapError>;
const PRINT: &str = "apparatus:default:bosma_7";
const ORDER: &str = "zakaz-print-ack-order";

struct CountedStore {
    inner: MemoryProductionMapStore,
    hold_reads: AtomicUsize,
    hold_writes: AtomicUsize,
    fail_holds: AtomicBool,
}
#[async_trait::async_trait]
impl ProductionMapStorePort for CountedStore {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>> {
        self.inner.maps().await
    }
    async fn put_map(&self, map: ProductionMapDefinition) -> Result<()> {
        self.inner.put_map(map).await
    }
    async fn put_maps_batch(&self, maps: &[ProductionMapDefinition]) -> Result<()> {
        self.inner.put_maps_batch(maps).await
    }
    async fn delete_map(&self, id: &str) -> Result<()> {
        self.inner.delete_map(id).await
    }
    async fn apparatus_sequences(&self) -> Result<BTreeMap<String, Vec<String>>> {
        self.inner.apparatus_sequences().await
    }
    async fn put_apparatus_sequence(&self, id: &str, orders: Vec<String>) -> Result<()> {
        self.inner.put_apparatus_sequence(id, orders).await
    }
    async fn apparatus_queue_states(&self) -> Result<BTreeMap<String, BTreeMap<String, String>>> {
        self.inner.apparatus_queue_states().await
    }
    async fn put_apparatus_queue_states(
        &self,
        id: &str,
        states: BTreeMap<String, String>,
    ) -> Result<()> {
        self.inner.put_apparatus_queue_states(id, states).await
    }
    async fn raw_material_assignments(&self) -> Result<Vec<RawMaterialAssignment>> {
        self.inner.raw_material_assignments().await
    }
    async fn put_raw_material_assignment(&self, row: RawMaterialAssignment) -> Result<()> {
        self.inner.put_raw_material_assignment(row).await
    }
    async fn delete_raw_material_assignment(
        &self,
        order: &str,
        barcode: &str,
    ) -> Result<Option<RawMaterialAssignment>> {
        self.inner
            .delete_raw_material_assignment(order, barcode)
            .await
    }
    async fn production_order_lifecycles(
        &self,
        ids: &[String],
    ) -> Result<BTreeMap<String, ProductionOrderLifecycleRecord>> {
        self.inner.production_order_lifecycles(ids).await
    }
    async fn active_print_preflight_holds(&self) -> Result<Vec<PrintPreflightHold>> {
        self.hold_reads.fetch_add(1, Ordering::SeqCst);
        if self.fail_holds.load(Ordering::SeqCst) {
            return Err(ProductionMapError::StoreFailed);
        }
        self.inner.active_print_preflight_holds().await
    }
    async fn print_preflight_hold_by_id(&self, id: &str) -> Result<Option<PrintPreflightHold>> {
        self.inner.print_preflight_hold_by_id(id).await
    }
    async fn update_print_preflight_hold(&self, hold: PrintPreflightHold) -> Result<()> {
        self.hold_writes.fetch_add(1, Ordering::SeqCst);
        self.inner.update_print_preflight_hold(hold).await
    }
}

async fn request(router: &Router, token: &str) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/mobile/admin/production-maps/print-preflight")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"apparatus":PRINT,"order_id":ORDER,"hold_id":"hold-ack",
            "action":"passed","include_control":true})
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn committed_mutation_ack_and_next_live_snapshot_preserve_authority_and_soft_errors() {
    let directory = tempfile::tempdir().unwrap();
    let base = AppState::verification(config(directory.path()));
    base.apparatus.bootstrap_factory_defaults().await.unwrap();
    // Both paths use the real store mutation and HTTP handler. Only the read
    // failure is injected, after the held record has already been committed.
    for fail_ack in [false, true] {
        let store = Arc::new(CountedStore {
            inner: MemoryProductionMapStore::new(),
            hold_reads: AtomicUsize::new(0),
            hold_writes: AtomicUsize::new(0),
            fail_holds: AtomicBool::new(false),
        });
        let map:ProductionMapDefinition=serde_json::from_value(json!({"id":ORDER,"title":"Print order","product_code":"P",
            "nodes":[{"id":"start","kind":"start","title":"Start"},
                {"id":"print","kind":"apparatus","title":"7 ta rangli bosma aparat","apparatus_id":PRINT},
                {"id":"end","kind":"end","title":"End"}],
            "edges":[{"from":"start","to":"print"},{"from":"print","to":"end"}]})).unwrap();
        store.inner.put_map(map).await.unwrap();
        store
            .inner
            .put_print_preflight_hold(PrintPreflightHold {
                hold_id: "hold-ack".into(),
                idempotency_key: "hold-ack".into(),
                order_id: ORDER.into(),
                apparatus: PRINT.into(),
                stage_node_id: "print".into(),
                status: PrintPreflightStatus::Running,
                actor: QueueActionActor {
                    role: "admin".into(),
                    ref_: "fixture".into(),
                    display_name: "Fixture".into(),
                },
                created_at_unix: 1,
                updated_at_unix: 2,
                previous_queue_state: None,
                expires_at_unix: 0,
            })
            .await
            .unwrap();
        let mut state = base.clone();
        state.sessions = SessionManager::memory(Some(3600));
        state.production_maps = ProductionMapService::new(
            store.clone(),
            Arc::new(CanonicalServiceApparatusResolver::new(
                state.apparatus.clone(),
            )),
        );
        let token = state
            .sessions
            .create(Principal {
                role: PrincipalRole::Admin,
                ref_: "print-operator".into(),
                display_name: "Operator".into(),
                legal_name: String::new(),
                phone: String::new(),
                avatar_url: String::new(),
            })
            .await
            .unwrap();
        let service = &state.production_maps;
        let before = service.live_snapshot_shared().await.unwrap();
        assert_eq!(
            before.queue_action_controls[PRINT][ORDER]
                .print_preflight
                .as_ref()
                .unwrap()
                .status,
            PrintPreflightStatus::Running
        );
        assert_eq!(store.hold_reads.load(Ordering::SeqCst), 1);
        let mut events = service.subscribe_live();
        store.fail_holds.store(fail_ack, Ordering::SeqCst);
        let router = build_router(state.clone());
        let (status, ack) = request(&router, &token).await;
        assert_eq!(status, StatusCode::OK, "{ack}");
        assert_eq!(ack["ok"], true);
        assert_eq!(ack["hold"]["status"], "passed");
        assert_eq!(store.hold_writes.load(Ordering::SeqCst), 1);
        assert_eq!(store.hold_reads.load(Ordering::SeqCst), 2);
        assert_eq!(
            events.try_recv().unwrap(),
            ProductionMapLiveEvent::PrintPreflight { revision: 1 }
        );
        let committed = store
            .inner
            .print_preflight_hold_by_id("hold-ack")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ack["hold"], json!(committed));
        assert_eq!(
            store.inner.apparatus_queue_states().await.unwrap()[PRINT][ORDER],
            "print_preflight"
        );
        if fail_ack {
            assert!(ack["control_state"].is_null());
        }
        store.fail_holds.store(false, Ordering::SeqCst);
        let (live, revision) = service.live_snapshot_shared_with_revision().await.unwrap();
        assert_eq!(revision, 1);
        assert!(!Arc::ptr_eq(&before, &live));
        assert_eq!(
            live.queue_action_controls[PRINT][ORDER].print_preflight,
            Some(committed)
        );
        assert_eq!(
            store.hold_reads.load(Ordering::SeqCst),
            if fail_ack { 3 } else { 2 }
        );
        if !fail_ack {
            assert_eq!(ack["control_state"]["rev"], revision);
            assert_eq!(ack["control_state"]["epoch"], service.snapshot_epoch());
            assert_eq!(
                ack["control_state"]["control"],
                json!(live.queue_action_controls[PRINT][ORDER])
            );
            assert_eq!(
                ack["control_state"]["stage_states"],
                json!(live.stage_states[ORDER])
            );
        }
        let (status, retry) = request(&router, &token).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(retry["hold"], ack["hold"]);
        assert_eq!(
            retry["control_state"]["control"],
            json!(live.queue_action_controls[PRINT][ORDER])
        );
        assert_eq!(
            store.hold_writes.load(Ordering::SeqCst),
            1,
            "idempotent retry must not rewrite mutation"
        );
        assert_eq!(
            store.hold_reads.load(Ordering::SeqCst),
            if fail_ack { 3 } else { 2 }
        );
        assert!(events.try_recv().is_err());
    }
}

fn config(path: &Path) -> AppConfig {
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
        werka_name: String::new(),
        werka_phone: String::new(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: String::new(),
        material_taminotchi_phone: String::new(),
        admin_phone: String::new(),
        admin_name: String::new(),
        admin_code: String::new(),
    }
}
