use super::*;
use crate::core::production_map::{
    OrderControlRecord, OrderControlState, OrderProgressBatch, OrderRunSession, OrderRunStatus,
    ProductionMapDefinition, ProductionMapError, ProductionMapStorePort, QueueActionActor,
    RawMaterialAssignment,
};

const CUT: &str = "apparatus:default:asset-010";
const ORDER: &str = "card-report-order";
const CYCLE: &str = "run:123:card-report";

struct CardOnlyStore {
    inner: Arc<MemoryProductionMapStore>,
    map_reads: AtomicUsize,
    control_reads: AtomicUsize,
    session_reads: AtomicUsize,
    batch_reads: AtomicUsize,
}

#[async_trait]
impl ProductionMapStorePort for CardOnlyStore {
    async fn map_by_id(
        &self,
        id: &str,
    ) -> Result<Option<ProductionMapDefinition>, ProductionMapError> {
        self.map_reads.fetch_add(1, Ordering::SeqCst);
        ProductionMapStorePort::map_by_id(self.inner.as_ref(), id).await
    }
    async fn order_control_by_id(
        &self,
        id: &str,
    ) -> Result<Option<OrderControlRecord>, ProductionMapError> {
        self.control_reads.fetch_add(1, Ordering::SeqCst);
        ProductionMapStorePort::order_control_by_id(self.inner.as_ref(), id).await
    }
    async fn active_order_run_session(
        &self,
        apparatus: &str,
        id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        self.session_reads.fetch_add(1, Ordering::SeqCst);
        ProductionMapStorePort::active_order_run_session(self.inner.as_ref(), apparatus, id).await
    }
    async fn progress_batch(
        &self,
        id: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        self.batch_reads.fetch_add(1, Ordering::SeqCst);
        ProductionMapStorePort::progress_batch(self.inner.as_ref(), id).await
    }
    async fn progress_batch_by_qr(
        &self,
        qr: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        self.batch_reads.fetch_add(1, Ordering::SeqCst);
        ProductionMapStorePort::progress_batch_by_qr(self.inner.as_ref(), qr).await
    }
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        panic!("card read must not list maps")
    }
    async fn order_run_sessions_for_orders(
        &self,
        _: &[String],
    ) -> Result<BTreeMap<String, Vec<OrderRunSession>>, ProductionMapError> {
        panic!("card read must not list sessions")
    }
    async fn progress_batches_for_order(
        &self,
        _: &str,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        panic!("card read must not list batches")
    }
    async fn progress_batches_for_orders(
        &self,
        _: &[String],
    ) -> Result<BTreeMap<String, Vec<OrderProgressBatch>>, ProductionMapError> {
        panic!("card read must not list batches")
    }
    async fn put_map(&self, _: ProductionMapDefinition) -> Result<(), ProductionMapError> {
        panic!("read only")
    }
    async fn put_maps_batch(
        &self,
        _: &[ProductionMapDefinition],
    ) -> Result<(), ProductionMapError> {
        panic!("read only")
    }
    async fn delete_map(&self, _: &str) -> Result<(), ProductionMapError> {
        panic!("read only")
    }
    async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        panic!("card read must not list queues")
    }
    async fn put_apparatus_sequence(
        &self,
        _: &str,
        _: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        panic!("read only")
    }
    async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        panic!("card read must not list queues")
    }
    async fn put_apparatus_queue_states(
        &self,
        _: &str,
        _: BTreeMap<String, String>,
    ) -> Result<(), ProductionMapError> {
        panic!("read only")
    }
    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        panic!("card read must not list materials")
    }
    async fn put_raw_material_assignment(
        &self,
        _: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError> {
        panic!("read only")
    }
    async fn delete_raw_material_assignment(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Option<RawMaterialAssignment>, ProductionMapError> {
        panic!("read only")
    }
}

fn card_map() -> ProductionMapDefinition {
    serde_json::from_value(serde_json::json!({
        "id":ORDER, "product_code":"CARD", "title":"Card report",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"cut","kind":"apparatus","title":"Cut","apparatus_id":CUT,"rezka_kadr_count":3},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"cut"},{"from":"cut","to":"end"}]
    }))
    .unwrap()
}

fn card_session() -> OrderRunSession {
    serde_json::from_value(serde_json::json!({
        "session_id":CYCLE,"apparatus":CUT,"order_id":ORDER,"stage_node_id":"cut","status":"active",
        "worker_role":"aparatchi","worker_ref":"card-report-worker","worker_display_name":"Worker",
        "started_at_unix":1,"updated_at_unix":2,
        "payload_json":{"rezka_output_cycle":CYCLE,"rezka_recorded_kadr_counts":[1,1,1],
            "rezka_output_report":[{"frame_index":2,"batch_id":"saved-card","qr_payload":"saved-card-qr",
                "input":{"produced_qty":100.0,"gross_qty":10.0,"finished_goods_meter":100.0,
                    "finished_goods_kg":10.0,"bobina_kg":0.5,"diameter":45.0}}]}
    })).unwrap()
}

async fn fixture() -> (AppState, Arc<CardOnlyStore>, String) {
    let mut state = test_state();
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "card-report-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![CUT.into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let inner = Arc::new(MemoryProductionMapStore::new());
    ProductionMapStorePort::put_map(inner.as_ref(), card_map())
        .await
        .unwrap();
    ProductionMapStorePort::put_order_run_session(inner.as_ref(), card_session())
        .await
        .unwrap();
    let store = Arc::new(CardOnlyStore {
        inner,
        map_reads: AtomicUsize::new(0),
        control_reads: AtomicUsize::new(0),
        session_reads: AtomicUsize::new(0),
        batch_reads: AtomicUsize::new(0),
    });
    state.production_maps = ProductionMapService::new(
        store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(
            state.apparatus.clone(),
        )),
    );
    let token = session_for(&state, PrincipalRole::Aparatchi, "card-report-worker").await;
    (state, store, token)
}

fn uri(apparatus: &str, order: &str) -> String {
    format!(
        "/v1/mobile/admin/production-maps/rezka-output-report?apparatus={}&order_id={}",
        urlencoding::encode(apparatus),
        urlencoding::encode(order)
    )
}

#[tokio::test]
async fn rezka_output_report_reads_only_the_active_card_and_preserves_saved_slots() {
    let (state, store, token) = fixture().await;
    let epoch = state.production_maps.snapshot_epoch().to_string();
    let revision = state.production_maps.snapshot_revision();
    let response = build_router(state)
        .oneshot(request("GET", &uri(CUT, ORDER), &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let body = json_body(response).await;
    assert_eq!(body["apparatus"], CUT);
    assert_eq!(body["order_id"], ORDER);
    assert_eq!(body["epoch"], epoch);
    assert_eq!(body["rev"], revision);
    assert_eq!(body["session_status"], "active");
    assert_eq!(body["order_control"], "active");
    assert_eq!(body["kadr_counts"], serde_json::json!([1, 1, 1]));
    assert_eq!(body["rezka_output_report"]["cycle_id"], CYCLE);
    assert_eq!(
        body["rezka_output_report"]["frames"],
        card_session().payload_json["rezka_output_report"]
    );
    assert_eq!(store.map_reads.load(Ordering::SeqCst), 1);
    assert_eq!(store.control_reads.load(Ordering::SeqCst), 1);
    assert_eq!(store.session_reads.load(Ordering::SeqCst), 1);
    assert_eq!(store.batch_reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn rezka_output_report_rejects_unassigned_workers_before_card_reads() {
    let (state, store, _) = fixture().await;
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "other-card-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec!["apparatus:default:asset-007".into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let token = session_for(&state, PrincipalRole::Aparatchi, "other-card-worker").await;
    let response = build_router(state)
        .oneshot(request("GET", &uri(CUT, ORDER), &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(store.session_reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn rezka_output_report_rejects_paused_frozen_missing_and_changed_cycles_as_conflicts() {
    let (state, store, token) = fixture().await;
    let router = build_router(state.clone());
    for status in [
        OrderRunStatus::Paused,
        OrderRunStatus::Frozen,
        OrderRunStatus::Completed,
    ] {
        let mut card = card_session();
        card.status = status;
        ProductionMapStorePort::put_order_run_session(store.inner.as_ref(), card)
            .await
            .unwrap();
        let response = router
            .clone()
            .oneshot(request("GET", &uri(CUT, ORDER), &token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT, "{status:?}");
    }
    ProductionMapStorePort::put_order_run_session(store.inner.as_ref(), card_session())
        .await
        .unwrap();
    for status in [
        OrderControlState::FreezeRequested,
        OrderControlState::Frozen,
    ] {
        ProductionMapStorePort::put_order_control_state(
            store.inner.as_ref(),
            OrderControlRecord {
                order_id: ORDER.into(),
                state: status,
                actor: QueueActionActor::default(),
                requested_at_unix: 1,
                frozen_at_unix: None,
                freeze_request: None,
                early_close: None,
            },
        )
        .await
        .unwrap();
        let response = router
            .clone()
            .oneshot(request("GET", &uri(CUT, ORDER), &token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT, "{status:?}");
    }
    let response = router
        .clone()
        .oneshot(request("GET", &uri(CUT, "missing-card-order"), &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn rezka_output_report_reads_one_legacy_input_for_missing_contained_kadr_count() {
    let (state, store, token) = fixture().await;
    let mut card = card_session();
    card.payload_json = serde_json::json!({"input_progress_batch_id":"legacy-input",
        "input_progress_qr_payload":"legacy-input-qr","input_wip_source_kind":"progress_batch"});
    ProductionMapStorePort::put_order_run_session(store.inner.as_ref(), card)
        .await
        .unwrap();
    let batch: OrderProgressBatch = serde_json::from_value(serde_json::json!({
        "batch_id":"legacy-input","revision":1,"session_id":"producer-session", "apparatus":CUT,
        "order_id":ORDER,"action":"roll_complete","status":"completed","produced_qty":10.0,"uom":"kg",
        "qr_payload":"legacy-input-qr","label_item_code":"CARD","label_item_name":"Card",
        "executor_name":"Worker","worker_role":"aparatchi","worker_ref":"producer","worker_display_name":"Producer",
        "started_at_unix":1,"completed_at_unix":1,"wip_status":"in_use","current_apparatus":CUT,
        "next_apparatus":CUT,"used_by_session_id":CYCLE,"used_by_apparatus":CUT,
        "payload_json":{"stage_node_id":"producer", "next_stage_node_id":"cut","contained_kadr_count":2}
    })).unwrap();
    ProductionMapStorePort::put_order_progress_batch(store.inner.as_ref(), batch)
        .await
        .unwrap();
    let response = build_router(state)
        .oneshot(request("GET", &uri(CUT, ORDER), &token))
        .await
        .unwrap();
    let status = response.status();
    let body = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["kadr_counts"], serde_json::json!([1, 1]));
    assert_eq!(body["rezka_output_report"]["cycle_id"], CYCLE);
    assert_eq!(store.batch_reads.load(Ordering::SeqCst), 1);
    assert_eq!(store.session_reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn rezka_output_report_rejects_non_cut_and_ambiguous_legacy_stages() {
    let (state, store, token) = fixture().await;
    let admin = session(&state, PrincipalRole::Admin).await;
    let router = build_router(state);
    let response = router
        .clone()
        .oneshot(request(
            "GET",
            &uri("apparatus:default:asset-007", ORDER),
            &admin,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let mut map = card_map();
    map.nodes.insert(2, serde_json::from_value(serde_json::json!({
        "id":"second-cut","kind":"apparatus","title":"Cut again","apparatus_id":CUT,"rezka_kadr_count":3
    })).unwrap());
    map.edges[1].to = "second-cut".into();
    map.edges
        .push(serde_json::from_value(serde_json::json!({"from":"second-cut","to":"end"})).unwrap());
    ProductionMapStorePort::put_map(store.inner.as_ref(), map)
        .await
        .unwrap();
    let mut card = card_session();
    card.stage_node_id.clear();
    ProductionMapStorePort::put_order_run_session(store.inner.as_ref(), card)
        .await
        .unwrap();
    let response = router
        .oneshot(request("GET", &uri(CUT, ORDER), &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}
