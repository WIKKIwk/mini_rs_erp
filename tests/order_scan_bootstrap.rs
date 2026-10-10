#![cfg(feature = "verification")]
//! Real HTTP contracts over production memory stores. No library cfg(test) hooks.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode, header},
};
use mini_rs_erp::{
    app::AppState,
    config::AppConfig,
    core::{
        admin::service::AdminService,
        apparatus_standard::{
            ApparatusId, ApparatusOperationalPolicies, CanonicalApparatusPatch,
            CanonicalCommandMetadata, MaterialExecutionPolicy,
        },
        auth::models::{Principal, PrincipalRole},
        authz::{Capability, RoleAssignmentUpsert, RoleDefinitionUpsert, capability_code},
        production_map::*,
        qolip::*,
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
use tokio::sync::Semaphore;
use tower::ServiceExt;

const PRINT: &str = "apparatus:default:bosma_7";
const CUT: &str = "apparatus:default:asset-010";
const ORDER: &str = "scan-bootstrap-order";
const BOOTSTRAP: &str = "/v1/mobile/admin/production-maps/order-scan-bootstrap";
const SEQUENCE: &str = "/v1/mobile/admin/production-maps/sequence";
const MATERIALS: &str = "/v1/mobile/admin/raw-material-start-requirements";
const QOLIPS: &str = "/v1/mobile/admin/production-maps/qolip-validate";
type MapResult<T> = Result<T, ProductionMapError>;
type QolipResult<T> = Result<T, QolipError>;

/// Holds a chosen number of real store reads. Semaphores avoid timing sleeps
/// and lost wakeups, including when the reader enters before the test awaits.
struct ReadGate {
    remaining: AtomicUsize,
    entered: Semaphore,
    release: Semaphore,
}
impl Default for ReadGate {
    fn default() -> Self {
        Self {
            remaining: AtomicUsize::new(0),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
        }
    }
}
impl ReadGate {
    fn arm(&self, count: usize) {
        self.remaining.store(count, Ordering::SeqCst);
    }
    async fn read(&self) {
        let mut remaining = self.remaining.load(Ordering::SeqCst);
        loop {
            if remaining == 0 {
                return;
            }
            match self.remaining.compare_exchange_weak(
                remaining,
                remaining - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(actual) => remaining = actual,
            }
        }
        self.entered.add_permits(1);
        self.release.acquire().await.unwrap().forget();
    }
    async fn wait_entered(&self) {
        tokio::time::timeout(Duration::from_secs(10), self.entered.acquire())
            .await
            .expect("the section reader must start")
            .unwrap()
            .forget();
    }
    fn resume(&self) {
        self.release.add_permits(1);
    }
}

#[derive(Default)]
struct CountedMapStore {
    inner: MemoryProductionMapStore,
    writes: AtomicUsize,
    material_reads: AtomicUsize,
    fail_materials: AtomicBool,
    materials_gate: ReadGate,
    snapshot_gate: ReadGate,
}
#[async_trait::async_trait]
impl ProductionMapStorePort for CountedMapStore {
    async fn maps(&self) -> MapResult<Vec<ProductionMapDefinition>> {
        self.inner.maps().await
    }
    async fn maps_for_snapshot_scope(
        &self, _apparatus: &[String], _extra_order_ids: &[String],
    ) -> MapResult<Vec<ProductionMapDefinition>> {
        self.snapshot_gate.read().await;
        self.inner.maps().await
    }
    async fn map_by_id(&self, id: &str) -> MapResult<Option<ProductionMapDefinition>> {
        self.inner.map_by_id(id).await
    }
    async fn put_map(&self, map: ProductionMapDefinition) -> MapResult<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.put_map(map).await
    }
    async fn put_maps_batch(&self, maps: &[ProductionMapDefinition]) -> MapResult<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.put_maps_batch(maps).await
    }
    async fn delete_map(&self, id: &str) -> MapResult<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.delete_map(id).await
    }
    async fn apparatus_sequences(&self) -> MapResult<BTreeMap<String, Vec<String>>> {
        self.inner.apparatus_sequences().await
    }
    async fn put_apparatus_sequence(&self, id: &str, orders: Vec<String>) -> MapResult<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.put_apparatus_sequence(id, orders).await
    }
    async fn apparatus_queue_states(
        &self,
    ) -> MapResult<BTreeMap<String, BTreeMap<String, String>>> {
        self.inner.apparatus_queue_states().await
    }
    async fn put_apparatus_queue_states(
        &self,
        id: &str,
        states: BTreeMap<String, String>,
    ) -> MapResult<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.put_apparatus_queue_states(id, states).await
    }
    async fn raw_material_assignments(&self) -> MapResult<Vec<RawMaterialAssignment>> {
        self.inner.raw_material_assignments().await
    }
    async fn raw_material_assignments_for_order(
        &self,
        id: &str,
    ) -> MapResult<Vec<RawMaterialAssignment>> {
        self.material_reads.fetch_add(1, Ordering::SeqCst);
        self.materials_gate.read().await;
        if self.fail_materials.load(Ordering::SeqCst) {
            return Err(ProductionMapError::StoreFailed);
        }
        self.inner.raw_material_assignments_for_order(id).await
    }
    async fn put_raw_material_assignment(&self, row: RawMaterialAssignment) -> MapResult<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.put_raw_material_assignment(row).await
    }
    async fn delete_raw_material_assignment(
        &self,
        order: &str,
        barcode: &str,
    ) -> MapResult<Option<RawMaterialAssignment>> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner
            .delete_raw_material_assignment(order, barcode)
            .await
    }
    async fn production_order_lifecycles(
        &self,
        ids: &[String],
    ) -> MapResult<BTreeMap<String, ProductionOrderLifecycleRecord>> {
        self.inner.production_order_lifecycles(ids).await
    }
    async fn order_control_states(&self) -> MapResult<BTreeMap<String, OrderControlRecord>> {
        self.inner.order_control_states().await
    }
    async fn put_order_control_state(&self, row: OrderControlRecord) -> MapResult<()> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.inner.put_order_control_state(row).await
    }
}

#[derive(Default)]
struct CountedQolipStore {
    products_reads: AtomicUsize,
    writes: AtomicUsize,
    fail_products: AtomicBool,
    oversized: AtomicBool,
    products_gate: ReadGate,
}
impl CountedQolipStore {
    fn unexpected_write<T>(&self) -> QolipResult<T> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        Err(QolipError::StoreFailed)
    }
}
#[async_trait::async_trait]
impl QolipStorePort for CountedQolipStore {
    async fn assigned_warehouses(&self, _: &Principal) -> QolipResult<Vec<String>> {
        Ok(vec![])
    }
    async fn assigned_blocks(&self, _: &Principal) -> QolipResult<Vec<QolipBlock>> {
        Ok(vec![])
    }
    async fn all_blocks(&self) -> QolipResult<Vec<QolipBlock>> {
        Ok(vec![])
    }
    async fn products(
        &self,
        _: &str,
        _: usize,
        _: bool,
        _: Option<&[String]>,
    ) -> QolipResult<Vec<QolipProduct>> {
        self.products_reads.fetch_add(1, Ordering::SeqCst);
        self.products_gate.read().await;
        if self.fail_products.load(Ordering::SeqCst) {
            return Err(QolipError::StoreFailed);
        }
        Ok(vec![QolipProduct {
            code: "P-1".into(),
            name: "Print order".into(),
            item_group: "Printed film".into(),
            has_qolip_spec: true,
            ..Default::default()
        }])
    }
    async fn product_spec(&self, code: &str) -> QolipResult<Option<QolipProductSpec>> {
        Ok(self.product_specs(code).await?.into_iter().next())
    }
    async fn product_specs(&self, _: &str) -> QolipResult<Vec<QolipProductSpec>> {
        Ok([("mold-z", "Black"), ("mold-a", "Blue")]
            .into_iter()
            .map(|(code, color)| QolipProductSpec {
                item_code: "P-1".into(),
                item_name: "Print order".into(),
                item_group: "Printed film".into(),
                qolip_code: code.into(),
                color: if self.oversized.load(Ordering::SeqCst) && code == "mold-z" {
                    "x".repeat(4 * 1024 * 1024 + 1)
                } else {
                    color.into()
                },
                ..Default::default()
            })
            .collect())
    }
    async fn put_product_spec(&self, _: QolipProductSpec) -> QolipResult<QolipProductSpec> {
        self.unexpected_write()
    }
    async fn locations(&self, _: &str) -> QolipResult<Vec<QolipLocation>> {
        Ok(vec![])
    }
    async fn location_by_id(&self, _: &str) -> QolipResult<Option<QolipLocation>> {
        Ok(None)
    }
    async fn put_location(&self, _: QolipLocation) -> QolipResult<QolipLocation> {
        self.unexpected_write()
    }
    async fn get_or_create_cell_qr(&self, _: QolipCellQr) -> QolipResult<QolipCellQr> {
        self.unexpected_write()
    }
    async fn issue_checkout(&self, _: QolipCheckout) -> QolipResult<QolipCheckout> {
        self.unexpected_write()
    }
    async fn checkouts(
        &self,
        _: Option<&str>,
        _: Option<&[String]>,
        _: &str,
        _: usize,
    ) -> QolipResult<Vec<QolipCheckout>> {
        Ok(vec![])
    }
    async fn checkout_by_id(&self, _: &str) -> QolipResult<Option<QolipCheckout>> {
        Ok(None)
    }
    async fn return_checkout(
        &self,
        _: &str,
        _: &str,
        _: Option<i32>,
    ) -> QolipResult<QolipCheckout> {
        self.unexpected_write()
    }
    async fn move_location(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: i32,
        _: i32,
    ) -> QolipResult<QolipLocation> {
        self.unexpected_write()
    }
    async fn cell_qr_by_payload(&self, _: &str) -> QolipResult<Option<QolipCellQr>> {
        Ok(None)
    }
}

struct SharedApp {
    _directory: tempfile::TempDir,
    state: AppState,
}
static SHARED_APP: tokio::sync::OnceCell<SharedApp> = tokio::sync::OnceCell::const_new();
struct Fixture {
    state: AppState,
    router: Router,
    maps: Arc<CountedMapStore>,
    qolips: Arc<CountedQolipStore>,
    requests: AtomicUsize,
    admin: String,
    worker: String,
    reader: String,
}
impl Fixture {
    async fn new(apparatus: &str) -> Self {
        let shared = SHARED_APP
            .get_or_init(|| async {
                let directory = tempfile::tempdir().unwrap();
                let state = AppState::verification(config(directory.path()));
                state.apparatus.bootstrap_factory_defaults().await.unwrap();
                // Factory defaults intentionally have no material policy. This
                // fixture enables the production requirement through the public
                // canonical configuration command, once before any test reads.
                let id = ApparatusId::new(PRINT).unwrap();
                let current = state
                    .apparatus
                    .current_configuration(&id)
                    .await
                    .unwrap()
                    .unwrap();
                state
                    .apparatus
                    .patch(
                        id,
                        current.runtime.source_revision,
                        CanonicalApparatusPatch {
                            policies: Some(ApparatusOperationalPolicies {
                                queue: current.queue.discipline,
                                material: MaterialExecutionPolicy::AllRequired {
                                    item_group_ids: vec!["Rulon".into()],
                                },
                                tooling: current.material.tooling.clone(),
                            }),
                            ..Default::default()
                        },
                        CanonicalCommandMetadata::new(
                            "user:bootstrap-fixture",
                            "bootstrap-required-materials",
                        ),
                    )
                    .await
                    .unwrap();
                SharedApp {
                    _directory: directory,
                    state,
                }
            })
            .await;
        let mut state = shared.state.clone();
        state.sessions = SessionManager::memory(Some(3600));
        state.admin = AdminService::new(&state.config);
        let maps = Arc::new(CountedMapStore::default());
        maps.inner.put_map(map(apparatus)).await.unwrap();
        state.production_maps = ProductionMapService::new(
            maps.clone(),
            Arc::new(CanonicalServiceApparatusResolver::new(
                state.apparatus.clone(),
            )),
        );
        let qolips = Arc::new(CountedQolipStore::default());
        state.qolip = QolipService::new(qolips.clone());
        define_role(&state, "scan-reader", &[Capability::ApparatusQueueRead]).await;
        for (who, role) in [("worker", "aparatchi"), ("reader", "scan-reader")] {
            assign(&state, who, role, &[PRINT, CUT]).await;
        }
        let admin = session(&state, PrincipalRole::Admin, "administrator").await;
        let worker = session(&state, PrincipalRole::Aparatchi, "worker").await;
        let reader = session(&state, PrincipalRole::Aparatchi, "reader").await;
        let router = build_router(state.clone());
        Self {
            state,
            router,
            maps,
            qolips,
            requests: AtomicUsize::new(0),
            admin,
            worker,
            reader,
        }
    }
    async fn request(&self, method: &str, token: &str, uri: &str, body: Value) -> Reply {
        self.requests.fetch_add(1, Ordering::SeqCst);
        request(&self.router, method, token, uri, body).await
    }
    async fn bootstrap(&self, token: &str, apparatus: &str) -> Reply {
        self.request(
            "GET",
            token,
            &query(BOOTSTRAP, apparatus, ORDER, ""),
            Value::Null,
        )
        .await
    }
    async fn warm(&self) {
        self.state
            .production_maps
            .live_snapshot_shared()
            .await
            .unwrap();
    }
    fn assert_no_writes(&self) {
        assert_eq!(
            self.maps.writes.load(Ordering::SeqCst),
            0,
            "bootstrap must not mutate production state"
        );
        assert_eq!(
            self.qolips.writes.load(Ordering::SeqCst),
            0,
            "empty-code Qolip reader must never issue checkout/QR/write"
        );
    }
}
struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
    bytes: usize,
}
async fn request(router: &Router, method: &str, token: &str, uri: &str, body: Value) -> Reply {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(if body.is_null() {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"text":String::from_utf8_lossy(&bytes)}))
    };
    Reply {
        status,
        headers,
        body,
        bytes: bytes.len(),
    }
}
fn query(path: &str, apparatus: &str, order: &str, barcodes: &str) -> String {
    format!(
        "{path}?apparatus={}&order_id={}&material_barcodes={}",
        urlencoding::encode(apparatus),
        urlencoding::encode(order),
        urlencoding::encode(barcodes)
    )
}
fn map(apparatus: &str) -> ProductionMapDefinition {
    serde_json::from_value(json!({"id":ORDER,"title":"Print order","product_code":"P-1","code":"P-1",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"work","kind":"apparatus","title":if apparatus == PRINT {"7 ta rangli bosma aparat"} else {"Rezka"},"apparatus_id":apparatus,"rezka_kadr_count":1},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"work"},{"from":"work","to":"end"}]})).unwrap()
}
async fn define_role(state: &AppState, id: &str, capabilities: &[Capability]) {
    state
        .admin
        .upsert_role_definition(RoleDefinitionUpsert {
            id: id.into(),
            label: id.into(),
            base_role: Some(PrincipalRole::Aparatchi),
            capability_codes: capabilities
                .iter()
                .map(|cap| capability_code(*cap).unwrap().into())
                .collect(),
        })
        .await
        .unwrap();
}
async fn assign(state: &AppState, who: &str, role: &str, apparatus: &[&str]) {
    state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: who.into(),
            role_id: role.into(),
            assigned_apparatus: apparatus.iter().map(|s| (*s).into()).collect(),
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
}
async fn session(state: &AppState, role: PrincipalRole, who: &str) -> String {
    state
        .sessions
        .create(Principal {
            role,
            ref_: who.into(),
            display_name: who.into(),
            legal_name: String::new(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .unwrap()
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

#[tokio::test]
async fn bootstrap_matches_three_legacy_requests_and_reports_cold_warm_bytes_without_writes() {
    let f = Fixture::new(PRINT).await;
    let mut events = f.state.production_maps.subscribe_live();
    for warm in [false, true] {
        if !warm {
            f.state.production_maps.notify_live();
            let _ = events.try_recv();
        }
        let before = f.requests.load(Ordering::SeqCst);
        let snapshot = f
            .request(
                "GET",
                &f.worker,
                &format!(
                    "{SEQUENCE}?apparatus={}&order_id={}",
                    urlencoding::encode(PRINT),
                    urlencoding::encode(ORDER)
                ),
                Value::Null,
            )
            .await;
        let materials = f
            .request(
                "GET",
                &f.worker,
                &query(MATERIALS, PRINT, ORDER, "roll-1,roll-2"),
                Value::Null,
            )
            .await;
        let qolips = f
            .request(
                "POST",
                &f.worker,
                QOLIPS,
                json!({"apparatus":PRINT,"order_id":ORDER,"qolip_code":""}),
            )
            .await;
        assert_eq!(f.requests.load(Ordering::SeqCst) - before, 3);
        for response in [&snapshot, &materials, &qolips] {
            assert_eq!(response.status, StatusCode::OK, "{}", response.body);
        }
        // Invalidate only the cache to measure the new path cold as well.
        if !warm {
            f.state.production_maps.notify_live();
            let _ = events.try_recv();
        }
        let before = f.requests.load(Ordering::SeqCst);
        let bootstrap = f
            .request(
                "GET",
                &f.worker,
                &query(BOOTSTRAP, PRINT, ORDER, "roll-1,roll-2"),
                Value::Null,
            )
            .await;
        assert_eq!(f.requests.load(Ordering::SeqCst) - before, 1);
        assert_eq!(bootstrap.status, StatusCode::OK, "{}", bootstrap.body);
        assert_eq!(bootstrap.headers[header::CACHE_CONTROL], "no-store");
        assert_eq!(bootstrap.body["ok"], true);
        let control = &bootstrap.body["control_state"];
        assert_eq!(control["apparatus"], PRINT);
        assert_eq!(control["order_id"], ORDER);
        assert_eq!(control["rev"], f.state.production_maps.snapshot_revision());
        assert_eq!(control["epoch"], snapshot.body["epoch"]);
        assert_eq!(control["scope"].as_str().unwrap().len(), 64);
        // The actual legacy sheet query omits worker_scope and returns an
        // empty scope. Bootstrap fingerprints its own authorization context.
        assert_eq!(snapshot.body["scope"], "");
        assert_ne!(control["scope"], snapshot.body["scope"]);
        assert_eq!(
            control["control"],
            snapshot.body["queue_action_controls"][PRINT][ORDER]
        );
        assert!(!control["control"].is_null());
        assert_eq!(
            control["queue_state"],
            snapshot.body["queue_states"][PRINT][ORDER]
                .as_str()
                .unwrap_or("pending")
        );
        assert_eq!(
            control["stage_states"],
            snapshot.body["stage_states"][ORDER]
        );
        assert_eq!(
            control["order_control"],
            snapshot.body["order_controls"][ORDER]
                .as_str()
                .unwrap_or("active")
        );
        assert_eq!(
            bootstrap.body["sections"]["materials"],
            json!({"status":"ready","data":materials.body})
        );
        assert_eq!(
            bootstrap.body["sections"]["qolips"],
            json!({"status":"ready","data":qolips.body})
        );
        assert_eq!(
            qolips.body["qolip"]["required_qolip_codes"],
            json!(["mold-a", "mold-z"])
        );
        let old_bytes = snapshot.bytes + materials.bytes + qolips.bytes;
        println!(
            "order-scan bootstrap warm={warm}: legacy_requests=3 bootstrap_requests=1 legacy_body_bytes={old_bytes} legacy_sequence_body_bytes={} legacy_materials_body_bytes={} legacy_qolips_body_bytes={} bootstrap_body_bytes={}",
            snapshot.bytes, materials.bytes, qolips.bytes, bootstrap.bytes
        );
        assert!(
            bootstrap.bytes < old_bytes,
            "the scoped bundle should be smaller than the three legacy response bodies"
        );
        f.assert_no_writes();
        assert!(matches!(
            events.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }
}

#[tokio::test]
async fn admin_needs_no_assignment_and_read_only_worker_keeps_qolip_denial() {
    let f = Fixture::new(PRINT).await;
    let admin = f.bootstrap(&f.admin, PRINT).await;
    assert_eq!(admin.status, StatusCode::OK, "{}", admin.body);
    let old_raw = f
        .request(
            "GET",
            &f.reader,
            &query(MATERIALS, PRINT, ORDER, ""),
            Value::Null,
        )
        .await;
    let old_qolip = f
        .request(
            "POST",
            &f.reader,
            QOLIPS,
            json!({"apparatus":PRINT,"order_id":ORDER,"qolip_code":""}),
        )
        .await;
    assert_eq!(old_raw.status, StatusCode::OK);
    assert_eq!(old_qolip.status, StatusCode::FORBIDDEN);
    let result = f.bootstrap(&f.reader, PRINT).await;
    assert_eq!(result.status, StatusCode::OK, "{}", result.body);
    assert_eq!(
        result.body["sections"]["materials"],
        json!({"status":"ready","data":old_raw.body})
    );
    assert_eq!(
        result.body["sections"]["qolips"],
        json!({"status":"error","status_code":old_qolip.status.as_u16(),"error":old_qolip.body})
    );
    f.assert_no_writes();
}

#[tokio::test]
async fn current_controls_skip_both_unneeded_sections() {
    let f = Fixture::new(CUT).await;
    f.maps.fail_materials.store(true, Ordering::SeqCst);
    f.qolips.fail_products.store(true, Ordering::SeqCst);
    let result = f.bootstrap(&f.worker, CUT).await;
    assert_eq!(result.status, StatusCode::OK, "{}", result.body);
    assert_eq!(
        result.body["sections"],
        json!({"materials":{"status":"not_required"},"qolips":{"status":"not_required"}})
    );
    assert_eq!(f.maps.material_reads.load(Ordering::SeqCst), 0);
    assert_eq!(f.qolips.products_reads.load(Ordering::SeqCst), 0);
    f.assert_no_writes();
}

#[tokio::test]
async fn section_errors_preserve_legacy_status_and_json_without_losing_control() {
    let f = Fixture::new(PRINT).await;
    f.warm().await;
    for (fail_materials, fail_qolips) in [(true, false), (false, true), (true, true)] {
        f.maps
            .fail_materials
            .store(fail_materials, Ordering::SeqCst);
        f.qolips.fail_products.store(fail_qolips, Ordering::SeqCst);
        let raw = f
            .request(
                "GET",
                &f.worker,
                &query(MATERIALS, PRINT, ORDER, ""),
                Value::Null,
            )
            .await;
        let qolip = f
            .request(
                "POST",
                &f.worker,
                QOLIPS,
                json!({"apparatus":PRINT,"order_id":ORDER,"qolip_code":""}),
            )
            .await;
        assert_eq!(raw.status.is_server_error(), fail_materials);
        assert_eq!(qolip.status.is_server_error(), fail_qolips);
        let result = f.bootstrap(&f.worker, PRINT).await;
        assert_eq!(result.status, StatusCode::OK, "{}", result.body);
        assert!(!result.body["control_state"]["control"].is_null());
        for (name, legacy) in [("materials", raw), ("qolips", qolip)] {
            let expected = if legacy.status.is_success() {
                json!({"status":"ready","data":legacy.body})
            } else {
                json!({"status":"error","status_code":legacy.status.as_u16(),"error":legacy.body})
            };
            assert_eq!(result.body["sections"][name], expected);
        }
        f.assert_no_writes();
    }
}

#[tokio::test]
async fn invalid_query_missing_control_training_and_wrong_methods_are_rejected() {
    let f = Fixture::new(PRINT).await;
    for uri in [
        BOOTSTRAP.to_string(),
        query(BOOTSTRAP, "Bosma", ORDER, ""),
        query(BOOTSTRAP, PRINT, "", ""),
    ] {
        let result = f.request("GET", &f.worker, &uri, Value::Null).await;
        assert_eq!(result.status, StatusCode::BAD_REQUEST, "{}", result.body);
    }
    for (order, status, error) in [
        ("not-present", StatusCode::NOT_FOUND, "order_not_available"),
        (
            "training-example",
            StatusCode::BAD_REQUEST,
            "training_order_scan_bootstrap_unsupported",
        ),
    ] {
        let result = f
            .request(
                "GET",
                &f.worker,
                &query(BOOTSTRAP, PRINT, order, ""),
                Value::Null,
            )
            .await;
        assert_eq!(result.status, status, "{}", result.body);
        assert_eq!(result.body["error"], error);
    }
    for method in ["POST", "PUT", "DELETE", "PATCH"] {
        let result = f
            .request(
                method,
                &f.worker,
                &query(BOOTSTRAP, PRINT, ORDER, ""),
                json!({}),
            )
            .await;
        assert_eq!(
            result.status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{}",
            result.body
        );
    }
    let missing_token = f.bootstrap("not-a-session", PRINT).await;
    assert_eq!(missing_token.status, StatusCode::UNAUTHORIZED);
    assign(&f.state, "worker", "aparatchi", &[CUT]).await;
    let unassigned = f.bootstrap(&f.worker, PRINT).await;
    assert!(
        matches!(
            unassigned.status,
            StatusCode::FORBIDDEN | StatusCode::BAD_REQUEST
        ),
        "{}",
        unassigned.body
    );
    assert_eq!(unassigned.status, StatusCode::FORBIDDEN);
    assert_eq!(unassigned.body["error"], "forbidden");
    f.assert_no_writes();
}

#[tokio::test]
async fn material_and_qolip_readers_start_in_parallel() {
    let f = Fixture::new(PRINT).await;
    f.warm().await;
    f.maps.materials_gate.arm(1);
    f.qolips.products_gate.arm(1);
    let router = f.router.clone();
    let token = f.worker.clone();
    let task = tokio::spawn(async move {
        request(
            &router,
            "GET",
            &token,
            &query(BOOTSTRAP, PRINT, ORDER, ""),
            Value::Null,
        )
        .await
    });
    // Neither reader is released until both have started: a serial bundle fails.
    tokio::join!(
        f.maps.materials_gate.wait_entered(),
        f.qolips.products_gate.wait_entered()
    );
    f.maps.materials_gate.resume();
    f.qolips.products_gate.resume();
    let result = task.await.unwrap();
    assert_eq!(result.status, StatusCode::OK, "{}", result.body);
    f.assert_no_writes();
}

#[tokio::test]
async fn session_assignment_and_capability_revocation_during_reads_discard_the_bundle() {
    for revoke in [
        "session",
        "assignment",
        "capability",
        "scope",
        "capability-retained",
    ] {
        let f = Fixture::new(PRINT).await;
        f.warm().await;
        f.qolips.products_gate.arm(1);
        let router = f.router.clone();
        let token = f.worker.clone();
        let task = tokio::spawn(async move {
            request(
                &router,
                "GET",
                &token,
                &query(BOOTSTRAP, PRINT, ORDER, ""),
                Value::Null,
            )
            .await
        });
        f.qolips.products_gate.wait_entered().await;
        match revoke {
            "session" => f.state.sessions.delete(&f.worker).await.unwrap(),
            "assignment" => assign(&f.state, "worker", "aparatchi", &[CUT]).await,
            "capability" => {
                define_role(&f.state, "no-scan-access", &[Capability::CustomerAccess]).await;
                assign(&f.state, "worker", "no-scan-access", &[PRINT, CUT]).await;
            }
            "scope" => assign(&f.state, "worker", "aparatchi", &[PRINT]).await,
            "capability-retained" => assign(&f.state, "worker", "scan-reader", &[PRINT, CUT]).await,
            _ => unreachable!(),
        }
        f.qolips.products_gate.resume();
        let result = task.await.unwrap();
        assert!(
            !result.status.is_success(),
            "{revoke} returned stale sections: {}",
            result.body
        );
        assert!(
            result.body.get("sections").is_none(),
            "{revoke}: {}",
            result.body
        );
        if revoke == "session" {
            assert_eq!(result.status, StatusCode::UNAUTHORIZED);
        }
        if revoke == "capability" {
            assert_eq!(result.status, StatusCode::FORBIDDEN);
        }
        if matches!(revoke, "scope" | "capability-retained") {
            assert_eq!(result.status, StatusCode::CONFLICT);
        }
        f.assert_no_writes();
    }
}

#[tokio::test]
async fn unrelated_revision_change_reuses_sections_with_current_control_cursor() {
    let f = Fixture::new(PRINT).await;
    f.warm().await;
    let revision = f.state.production_maps.snapshot_revision();
    f.qolips.products_gate.arm(1);
    let router = f.router.clone();
    let token = f.worker.clone();
    let task = tokio::spawn(async move {
        request(
            &router,
            "GET",
            &token,
            &query(BOOTSTRAP, PRINT, ORDER, ""),
            Value::Null,
        )
        .await
    });
    f.qolips.products_gate.wait_entered().await;
    f.state.production_maps.notify_live();
    f.qolips.products_gate.resume();
    let result = task.await.unwrap();
    assert_eq!(result.status, StatusCode::OK, "{}", result.body);
    assert_eq!(result.body["control_state"]["rev"], revision + 1);
    assert_eq!(f.qolips.products_reads.load(Ordering::SeqCst), 1);
    let snapshot = f
        .state
        .production_maps
        .live_snapshot_shared()
        .await
        .unwrap();
    assert_eq!(
        result.body["control_state"]["control"],
        json!(snapshot.queue_action_controls[PRINT][ORDER])
    );
    f.assert_no_writes();
}

#[tokio::test]
async fn repeated_revision_churn_stops_after_two_attempts_with_conflict() {
    let f = Fixture::new(PRINT).await;
    f.warm().await;
    f.qolips.products_gate.arm(2);
    let router = f.router.clone();
    let token = f.worker.clone();
    let task = tokio::spawn(async move {
        request(
            &router,
            "GET",
            &token,
            &query(BOOTSTRAP, PRINT, ORDER, ""),
            Value::Null,
        )
        .await
    });
    for index in 0..2 {
        f.qolips.products_gate.wait_entered().await;
        let mut changed = serde_json::to_value(map(PRINT)).unwrap();
        let next_node = format!("work-{index}");
        changed["nodes"][1]["id"] = json!(next_node);
        changed["edges"][0]["to"] = json!(next_node);
        changed["edges"][1]["from"] = json!(next_node);
        f.maps.inner.put_map(serde_json::from_value(changed).unwrap()).await.unwrap();
        f.state.production_maps.notify_live();
        f.qolips.products_gate.resume();
    }
    let result = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("revision churn must stop after two total attempts")
        .unwrap();
    assert_eq!(result.status, StatusCode::CONFLICT, "{}", result.body);
    assert_eq!(result.body["error"], "order_scan_bootstrap_changed");
    assert!(result.body.get("sections").is_none());
    assert_eq!(result.headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(f.qolips.products_reads.load(Ordering::SeqCst), 2);
    f.assert_no_writes();
}

#[tokio::test]
async fn a_stalled_reader_is_bounded_without_returning_a_partial_bundle() {
    let f = Fixture::new(PRINT).await;
    f.warm().await;
    f.qolips.products_gate.arm(1);
    let router = f.router.clone();
    let token = f.worker.clone();
    let task = tokio::spawn(async move {
        request(
            &router,
            "GET",
            &token,
            &query(BOOTSTRAP, PRINT, ORDER, ""),
            Value::Null,
        )
        .await
    });
    f.qolips.products_gate.wait_entered().await;
    let result = tokio::time::timeout(Duration::from_secs(15), task)
        .await
        .expect("an awaited reader must not hang indefinitely")
        .unwrap();
    assert_eq!(
        result.status,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        result.body
    );
    assert_eq!(result.body["error"], "order_scan_bootstrap_timeout");
    assert_eq!(result.headers[header::CACHE_CONTROL], "no-store");
    assert!(result.body.get("sections").is_none());
    f.assert_no_writes();
}

#[tokio::test]
async fn top_level_capability_union_does_not_expand_individual_section_permissions() {
    let f = Fixture::new(PRINT).await;
    for (index, cap) in [
        Capability::AdminAccess,
        Capability::ProductionMapManage,
        Capability::ApparatusQueueRead,
        Capability::RawMaterialAssign,
        Capability::QolipManage,
        Capability::PreparationAccess,
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("isolated-cap-{index}");
        define_role(&f.state, &id, &[cap]).await;
        assign(&f.state, "worker", &id, &[PRINT]).await;
        let raw = f
            .request(
                "GET",
                &f.worker,
                &query(MATERIALS, PRINT, ORDER, ""),
                Value::Null,
            )
            .await;
        let qolip = f
            .request(
                "POST",
                &f.worker,
                QOLIPS,
                json!({"apparatus":PRINT,"order_id":ORDER,"qolip_code":""}),
            )
            .await;
        let result = f.bootstrap(&f.worker, PRINT).await;
        assert_eq!(result.status, StatusCode::OK, "{cap:?}: {}", result.body);
        for (name, legacy) in [("materials", raw), ("qolips", qolip)] {
            let expected = if legacy.status.is_success() {
                json!({"status":"ready","data":legacy.body})
            } else {
                json!({"status":"error","status_code":legacy.status.as_u16(),"error":legacy.body})
            };
            assert_eq!(result.body["sections"][name], expected, "{cap:?} {name}");
        }
    }
    define_role(
        &f.state,
        "queue-manage-only",
        &[Capability::ApparatusQueueManage],
    )
    .await;
    assign(&f.state, "worker", "queue-manage-only", &[PRINT]).await;
    let result = f.bootstrap(&f.worker, PRINT).await;
    assert_eq!(result.status, StatusCode::FORBIDDEN);
    f.assert_no_writes();
}

#[tokio::test]
async fn retry_rechecks_current_modes_and_discards_sections_that_became_unneeded() {
    let f = Fixture::new(PRINT).await;
    f.warm().await;
    f.qolips.products_gate.arm(1);
    let router = f.router.clone();
    let token = f.worker.clone();
    let task = tokio::spawn(async move {
        request(
            &router,
            "GET",
            &token,
            &query(BOOTSTRAP, PRINT, ORDER, ""),
            Value::Null,
        )
        .await
    });
    f.qolips.products_gate.wait_entered().await;
    f.maps
        .inner
        .put_apparatus_queue_states(
            PRINT,
            BTreeMap::from([(ORDER.into(), "in_progress".into())]),
        )
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    f.qolips.products_gate.resume();
    let result = task.await.unwrap();
    assert_eq!(result.status, StatusCode::OK, "{}", result.body);
    assert_eq!(result.body["control_state"]["queue_state"], "in_progress");
    assert_eq!(
        result.body["sections"],
        json!({"materials":{"status":"not_required"},"qolips":{"status":"not_required"}})
    );
    assert_eq!(f.qolips.products_reads.load(Ordering::SeqCst), 1);
    f.assert_no_writes();
}

#[tokio::test]
async fn repeated_stages_and_nonactive_controls_are_copied_from_the_canonical_snapshot() {
    let f = Fixture::new(PRINT).await;
    let repeated: ProductionMapDefinition = serde_json::from_value(json!({
        "id":ORDER,"title":"Print order","product_code":"P-1","code":"P-1",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"print-first","kind":"apparatus","title":"7 ta rangli bosma aparat","apparatus_id":PRINT},
            {"id":"print-second","kind":"apparatus","title":"7 ta rangli bosma aparat","apparatus_id":PRINT},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"print-first"},{"from":"print-first","to":"print-second"},{"from":"print-second","to":"end"}]
    })).unwrap();
    f.maps.inner.put_map(repeated).await.unwrap();
    for queue_state in ["pending", "in_progress", "paused", "frozen", "completed"] {
        f.maps
            .inner
            .put_apparatus_queue_states(PRINT, BTreeMap::from([(ORDER.into(), queue_state.into())]))
            .await
            .unwrap();
        let mut order_control = OrderControlRecord::active(ORDER);
        if queue_state == "frozen" {
            order_control.state = OrderControlState::Frozen;
        }
        f.maps
            .inner
            .put_order_control_state(order_control)
            .await
            .unwrap();
        f.state.production_maps.notify_live();
        let snapshot = f
            .state
            .production_maps
            .live_snapshot_shared()
            .await
            .unwrap();
        let expected = snapshot
            .queue_action_controls
            .get(PRINT)
            .and_then(|orders| orders.get(ORDER));
        if queue_state == "pending" {
            assert!(
                expected.is_some(),
                "repeated-stage fixture must expose an authoritative control"
            );
        }
        let result = f.bootstrap(&f.worker, PRINT).await;
        if let Some(expected) = expected {
            assert_eq!(
                result.status,
                StatusCode::OK,
                "{queue_state}: {}",
                result.body
            );
            assert_eq!(result.body["control_state"]["control"], json!(expected));
            assert_eq!(
                result.body["control_state"]["queue_state"],
                expected.state.as_str()
            );
            assert_eq!(
                result.body["control_state"]["stage_states"],
                json!(
                    snapshot
                        .stage_states
                        .get(ORDER)
                        .cloned()
                        .unwrap_or_default()
                )
            );
            assert_eq!(
                result.body["control_state"]["order_control"],
                snapshot
                    .order_controls
                    .get(ORDER)
                    .map(|r| r.state.as_str())
                    .unwrap_or("active")
            );
        } else {
            assert_eq!(
                result.status,
                StatusCode::NOT_FOUND,
                "{queue_state}: {}",
                result.body
            );
            assert_eq!(result.body["error"], "order_not_available");
        }
        f.assert_no_writes();
    }
}

#[tokio::test]
async fn optional_barcodes_and_untrusted_qolip_query_remain_read_only() {
    let f = Fixture::new(PRINT).await;
    let baseline = f.bootstrap(&f.worker, PRINT).await;
    assert_eq!(baseline.status, StatusCode::OK, "{}", baseline.body);
    let uri = format!(
        "{BOOTSTRAP}?apparatus={}&order_id={}&qolip_code=mold-z",
        urlencoding::encode(&format!(" {PRINT} ")),
        urlencoding::encode(&format!(" {ORDER} "))
    );
    let result = f.request("GET", &f.worker, &uri, Value::Null).await;
    assert_eq!(result.status, StatusCode::OK, "{}", result.body);
    assert_eq!(result.body, baseline.body);
    assert_eq!(
        result.body["sections"]["qolips"]["data"]["qolip"]["qolip_code"],
        ""
    );
    let malformed = f
        .request(
            "GET",
            &f.worker,
            &(query(BOOTSTRAP, PRINT, ORDER, "") + "&order_id=second-order"),
            Value::Null,
        )
        .await;
    assert_eq!(
        malformed.status,
        StatusCode::BAD_REQUEST,
        "{}",
        malformed.body
    );
    assert_eq!(malformed.headers[header::CACHE_CONTROL], "no-store");
    f.assert_no_writes();
}

#[tokio::test]
async fn oversized_section_is_bounded_and_preserves_the_control_and_other_section() {
    let f = Fixture::new(PRINT).await;
    f.qolips.oversized.store(true, Ordering::SeqCst);
    let result = f.bootstrap(&f.worker, PRINT).await;
    assert_eq!(result.status, StatusCode::OK, "{}", result.body);
    assert!(!result.body["control_state"]["control"].is_null());
    assert_eq!(result.body["sections"]["materials"]["status"], "ready");
    assert_eq!(
        result.body["sections"]["qolips"],
        json!({"status":"error","status_code":500,"error":{"error":"order_scan_section_body_limit"}})
    );
    assert!(
        result.bytes < 64 * 1024,
        "oversized section must not leak into the response"
    );
    f.assert_no_writes();
}

#[tokio::test]
async fn control_only_read_skips_sections_and_preserves_authority() {
    let f = Fixture::new(PRINT).await;
    let full = f.bootstrap(&f.worker, PRINT).await;
    let material_reads = f.maps.material_reads.load(Ordering::SeqCst);
    let qolip_reads = f.qolips.products_reads.load(Ordering::SeqCst);
    let thin = f.request("GET", &f.worker,
        &(query(BOOTSTRAP, PRINT, ORDER, "") + "&include_sections=false"), Value::Null).await;
    assert_eq!(thin.status, StatusCode::OK, "{}", thin.body);
    assert_eq!(thin.body["control_state"], full.body["control_state"]);
    assert_eq!(f.maps.material_reads.load(Ordering::SeqCst), material_reads);
    assert_eq!(f.qolips.products_reads.load(Ordering::SeqCst), qolip_reads);
    assert!(thin.bytes < full.bytes);
    let denied = f.request("GET", &f.worker,
        &(query(BOOTSTRAP, "apparatus:default:bosma_6", ORDER, "") + "&include_sections=false"),
        Value::Null).await;
    assert!(!denied.status.is_success());
    f.assert_no_writes();
}

#[tokio::test]
async fn gzip_negotiation_is_lossless_and_conditional_snapshot_stays_empty() {
    use std::io::Read;
    let f = Fixture::new(PRINT).await;
    let uri = format!("{SEQUENCE}?worker_scope=true");
    let plain = f.request("GET", &f.worker, &uri, Value::Null).await;
    assert_eq!(plain.status, StatusCode::OK);
    for encoding in ["gzip", "identity", "gzip;q=0", "br"] {
        let response = f.router.clone().oneshot(Request::builder().uri(&uri)
            .header(header::AUTHORIZATION, format!("Bearer {}", f.worker))
            .header(header::ACCEPT_ENCODING, encoding).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let compressed = response.headers().get(header::CONTENT_ENCODING)
            .is_some_and(|value| value == "gzip");
        assert_eq!(compressed, encoding == "gzip");
        if compressed {
            assert!(response.headers().get(header::VARY).unwrap().to_str().unwrap()
                .to_lowercase().contains("accept-encoding"));
        }
        let bytes = to_bytes(response.into_body(), 8 * 1024 * 1024).await.unwrap();
        let decoded = if compressed {
            assert!(bytes.len() < plain.bytes);
            let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
            let mut decoded = Vec::new();
            decoder.read_to_end(&mut decoded).unwrap();
            decoded
        } else { bytes.to_vec() };
        assert_eq!(serde_json::from_slice::<Value>(&decoded).unwrap(), plain.body);
    }
    let conditional = format!("{uri}&if_rev={}&if_epoch={}&if_scope={}",
        plain.body["rev"], urlencoding::encode(plain.body["epoch"].as_str().unwrap()),
        urlencoding::encode(plain.body["scope"].as_str().unwrap()));
    let response = f.router.clone().oneshot(Request::builder().uri(conditional)
        .header(header::AUTHORIZATION, format!("Bearer {}", f.worker))
        .header(header::ACCEPT_ENCODING, "gzip").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    assert!(response.headers().get(header::CONTENT_ENCODING).is_none());
    assert!(to_bytes(response.into_body(), 1024).await.unwrap().is_empty());
    f.assert_no_writes();
}

#[tokio::test]
async fn queue_action_ack_cursor_covers_committed_state_and_rejected_replay_does_not_write() {
    let f = Fixture::new(CUT).await;
    let mut state = f.state.clone();
    let store = Arc::new(MemoryProductionMapStore::default());
    const ACK_ORDER: &str = "zakaz-queue-ack";
    let mut seeded_map = map(CUT);
    seeded_map.id = ACK_ORDER.into();
    store.put_map(seeded_map).await.unwrap();
    store.put_apparatus_sequence(CUT, vec![ACK_ORDER.into()]).await.unwrap();
    state.production_maps = ProductionMapService::new(store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(state.apparatus.clone())));
    let router = build_router(state.clone());
    let body = json!({"apparatus":CUT,"order_id":ACK_ORDER,"action":"start",
        "include_control":true});
    let result = request(&router, "POST", &f.worker,
        "/v1/mobile/admin/production-maps/queue-action", body.clone()).await;
    assert_eq!(result.status, StatusCode::OK, "{}", result.body);
    assert_eq!(result.body["epoch"], state.production_maps.snapshot_epoch());
    assert_eq!(result.body["rev"], state.production_maps.snapshot_revision());
    assert_eq!(result.body["states"][ACK_ORDER], "in_progress");
    let snapshot = state.production_maps.worker_snapshot_shared_with_revision(
        &[CUT.into()], &[ACK_ORDER.into()],
    ).await.unwrap();
    let control = &result.body["control_state"];
    assert_eq!(control["control"], json!(snapshot.0.queue_action_controls[CUT][ACK_ORDER]));
    assert_eq!(control["apparatus"], CUT);
    assert_eq!(control["order_id"], ACK_ORDER);
    assert_eq!(control["queue_state"], "in_progress");
    assert_eq!(control["rev"], result.body["rev"]);
    assert_eq!(control["epoch"], result.body["epoch"]);
    assert!(control["stage_states"].is_object());
    let committed = store.apparatus_queue_states().await.unwrap();
    assert_eq!(committed[CUT][ACK_ORDER], "in_progress");
    let revision = state.production_maps.snapshot_revision();
    let rejected = request(&router, "POST", &f.worker,
        "/v1/mobile/admin/production-maps/queue-action", body).await;
    assert!(!rejected.status.is_success());
    assert_eq!(store.apparatus_queue_states().await.unwrap(), committed);
    assert_eq!(state.production_maps.snapshot_revision(), revision);
}

#[tokio::test]
async fn queue_action_optional_control_timeout_preserves_the_committed_ack() {
    let f = queue_action_control_fixture().await;
    f.maps.snapshot_gate.arm(1);
    let router = f.router.clone();
    let token = f.worker.clone();
    let pending = tokio::spawn(async move {
        request(&router, "POST", &token,
            "/v1/mobile/admin/production-maps/queue-action",
            json!({"apparatus":CUT,"order_id":CONTROL_ACK_ORDER,"action":"start",
                "include_control":true})).await
    });
    f.maps.snapshot_gate.wait_entered().await;
    assert_eq!(f.maps.inner.apparatus_queue_states().await.unwrap()[CUT][CONTROL_ACK_ORDER],
        "in_progress", "the blocked reader must follow the business commit");
    let result = tokio::time::timeout(Duration::from_secs(1), pending).await
        .expect("optional presentation must not hold the committed ACK open")
        .unwrap();
    assert_eq!(result.status, StatusCode::OK, "{}", result.body);
    assert_eq!(result.body["states"][CONTROL_ACK_ORDER], "in_progress");
    assert!(result.body["control_state"].is_null());
    assert_eq!(result.body["rev"], f.state.production_maps.snapshot_revision());
}

#[tokio::test]
async fn queue_action_legacy_ack_skips_optional_control_reads() {
    let f = queue_action_control_fixture().await;
    f.maps.snapshot_gate.arm(1);
    let result = tokio::time::timeout(Duration::from_secs(1),
        f.request("POST", &f.worker,
            "/v1/mobile/admin/production-maps/queue-action",
            json!({"apparatus":CUT,"order_id":CONTROL_ACK_ORDER,"action":"start"}))).await
        .expect("legacy clients must not pay for an optional control read");
    assert_eq!(result.status, StatusCode::OK, "{}", result.body);
    assert!(result.body.get("control_state").is_none());
    assert_eq!(f.maps.snapshot_gate.remaining.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn queue_action_control_reauth_drops_presentation_after_assignment_change() {
    let f = queue_action_control_fixture().await;
    f.maps.snapshot_gate.arm(1);
    let router = f.router.clone();
    let token = f.worker.clone();
    let pending = tokio::spawn(async move {
        request(&router, "POST", &token,
            "/v1/mobile/admin/production-maps/queue-action",
            json!({"apparatus":CUT,"order_id":CONTROL_ACK_ORDER,"action":"start",
                "include_control":true})).await
    });
    f.maps.snapshot_gate.wait_entered().await;
    assign(&f.state, "worker", "aparatchi", &[PRINT]).await;
    f.maps.snapshot_gate.resume();
    let result = pending.await.unwrap();
    assert_eq!(result.status, StatusCode::OK, "{}", result.body);
    assert_eq!(result.body["states"][CONTROL_ACK_ORDER], "in_progress");
    assert!(result.body["control_state"].is_null());
}

const CONTROL_ACK_ORDER: &str = "zakaz-control-ack";

async fn queue_action_control_fixture() -> Fixture {
    let f = Fixture::new(CUT).await;
    let mut seeded_map = map(CUT);
    seeded_map.id = CONTROL_ACK_ORDER.into();
    f.maps.inner.put_map(seeded_map).await.unwrap();
    f.maps.inner.put_apparatus_sequence(CUT, vec![CONTROL_ACK_ORDER.into()])
        .await.unwrap();
    f
}
