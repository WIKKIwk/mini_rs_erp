#![cfg(feature = "verification")]
//! Regression contract for apparatus-stage holds and whole-order freeze overlays.
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
        authz::RoleAssignmentUpsert,
        production_map::{queue_state::ApparatusQueueAction as Action, *},
        session::manager::SessionManager,
    },
    db::{postgres, postgres_production_map::PostgresProductionMapStore},
    http::router::build_router,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};
use tower::ServiceExt;

const PRINT: &str = "apparatus:default:bosma_9";
const LAM: &str = "apparatus:default:asset-007";
const CUT: &str = "apparatus:default:asset-010";
const ORDER: &str = "zakaz-scoped-freeze";
const CONTROL: &str = "/v1/mobile/admin/production-maps/order-control";
const ACTION: &str = "/v1/mobile/admin/production-maps/queue-action";
const LOOKUP: &str = "/v1/mobile/admin/production-maps/progress-qr/lookup";
static FIXTURE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "scope-worker".into(),
        display_name: "Worker".into(),
    }
}
fn admin_actor() -> QueueActionActor {
    QueueActionActor {
        role: "admin".into(),
        ref_: "scope-admin".into(),
        display_name: "Admin".into(),
    }
}
fn run(apparatus: &str, stage: &str, status: OrderRunStatus) -> OrderRunSession {
    OrderRunSession {
        session_id: format!("run-{stage}"),
        apparatus: apparatus.into(),
        order_id: ORDER.into(),
        stage_node_id: stage.into(),
        status,
        worker_role: actor().role,
        worker_ref: actor().ref_,
        worker_display_name: actor().display_name,
        started_at_unix: 1,
        updated_at_unix: 2,
        payload_json: json!({"stage_work_protocol":1}),
    }
}
async fn seed(service: &ProductionMapService, store: &dyn ProductionMapStorePort) {
    service.upsert_map(serde_json::from_value(json!({
        "id":ORDER,"title":"Scoped freeze","product_code":"SCOPED","roll_count":11,"width_mm":650,
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"print","kind":"apparatus","title":"Bosma","apparatus_id":PRINT},
            {"id":"lam","kind":"apparatus","title":"Laminatsiya","apparatus_id":LAM},
            {"id":"cut","kind":"apparatus","title":"Rezka","apparatus_id":CUT,"rezka_kadr_count":1},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"print"},{"from":"print","to":"lam"},
            {"from":"lam","to":"cut"},{"from":"cut","to":"end"}]
    })).unwrap()).await.unwrap();
    for (apparatus, state) in [(PRINT, "completed"), (LAM, "in_progress"), (CUT, "pending")] {
        store
            .put_apparatus_sequence(apparatus, vec![ORDER.into()])
            .await
            .unwrap();
        store
            .put_apparatus_queue_states(apparatus, [(ORDER.into(), state.into())].into())
            .await
            .unwrap();
    }
    store
        .put_order_run_session(run(PRINT, "print", OrderRunStatus::Completed))
        .await
        .unwrap();
    store
        .put_order_run_session(run(LAM, "lam", OrderRunStatus::Active))
        .await
        .unwrap();
    for i in 0..11 {
        let (apparatus, stage, next, next_stage) = if i == 10 {
            (PRINT, "print", LAM, "lam")
        } else {
            (LAM, "lam", CUT, "cut")
        };
        store.put_order_progress_batch(serde_json::from_value(json!({
            "batch_id":format!("healthy-{i}"),"session_id":format!("run-{stage}"),
            "started_at_unix":1,"completed_at_unix":2,"apparatus":apparatus,"order_id":ORDER,
            "action":"detach_roll","status":"roll_detached","produced_qty":100,"uom":"m",
            "qr_payload":format!("40010000000000000000{i:04X}"),"label_item_code":"SCOPED",
            "label_item_name":"Healthy film","executor_name":"Worker","worker_role":"aparatchi",
            "worker_ref":actor().ref_,"worker_display_name":"Worker","wip_status":"waiting",
            "next_apparatus":next,"current_apparatus":apparatus,"current_location":apparatus,
            "finished_goods_kg":11,"finished_goods_meter":100,"bobina_kg":1,
            "payload_json":{"stage_node_id":stage,"production_stage_node_id":stage,
                "next_stage_node_id":next_stage,"contained_kadr_count":1}
        })).unwrap()).await.unwrap();
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    state: AppState,
    store: Arc<MemoryProductionMapStore>,
    router: Router,
    admin: String,
    worker: String,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::verification(config(dir.path()));
        state.apparatus.bootstrap_factory_defaults().await.unwrap();
        state.sessions = SessionManager::memory(Some(3600));
        let store = Arc::new(MemoryProductionMapStore::new());
        state.production_maps = ProductionMapService::new(
            store.clone(),
            Arc::new(CanonicalServiceApparatusResolver::new(
                state.apparatus.clone(),
            )),
        );
        seed(&state.production_maps, store.as_ref()).await;
        state
            .admin
            .upsert_role_assignment(RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: actor().ref_,
                role_id: "aparatchi".into(),
                assigned_apparatus: vec![LAM.into(), CUT.into()],
                assigned_item_groups: vec![],
            })
            .await
            .unwrap();
        let mut tokens = Vec::new();
        for (role, id) in [
            (PrincipalRole::Admin, "scope-admin"),
            (PrincipalRole::Aparatchi, "scope-worker"),
        ] {
            tokens.push(
                state
                    .sessions
                    .create(Principal {
                        role,
                        ref_: id.into(),
                        display_name: id.into(),
                        legal_name: String::new(),
                        phone: String::new(),
                        avatar_url: String::new(),
                    })
                    .await
                    .unwrap(),
            );
        }
        let router = build_router(state.clone());
        Self {
            _dir: dir,
            state,
            store,
            router,
            admin: tokens.remove(0),
            worker: tokens.remove(0),
        }
    }
    async fn post(&self, token: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let response = self
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
    async fn ok(&self, token: &str, path: &str, body: Value) -> Value {
        let (status, body) = self.post(token, path, body).await;
        assert_eq!(status, StatusCode::OK, "{path}: {body}");
        body
    }
    async fn queue(&self) -> Value {
        let response = self
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(
                        "/v1/mobile/admin/production-maps/sequence?worker_scope=true&queue_limit=0",
                    )
                    .header(header::AUTHORIZATION, format!("Bearer {}", self.worker))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let body: Value = serde_json::from_slice(
            &to_bytes(response.into_body(), 4 * 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
    async fn scan(&self, i: usize, apparatus: &str) -> (StatusCode, Value) {
        self.post(
            &self.worker,
            LOOKUP,
            json!({"qr_payload":format!("40010000000000000000{i:04X}"),
            "apparatus":apparatus,"order_id":ORDER,"require_active_order":true}),
        )
        .await
    }
}

#[tokio::test]
async fn worker_issue_freezes_only_its_stage_and_ten_healthy_rolls_remain_usable() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    // Rezka's free-pick queue previously hid an order on any upstream issue.
    use mini_rs_erp::core::apparatus_standard::{
        ApparatusId, ApparatusOperationalPolicies, CanonicalApparatusPatch,
        CanonicalCommandMetadata, QueueDiscipline,
    };
    let id = ApparatusId::new(CUT).unwrap();
    let current = f
        .state
        .apparatus
        .current_configuration(&id)
        .await
        .unwrap()
        .unwrap();
    f.state
        .apparatus
        .patch(
            id,
            current.runtime.source_revision,
            CanonicalApparatusPatch {
                policies: Some(ApparatusOperationalPolicies {
                    worker_material_assignment_allowed: false,
                    queue: QueueDiscipline::FreePick,
                    material: current.material.policy.clone(),
                    tooling: current.material.tooling.clone(),
                }),
                ..Default::default()
            },
            CanonicalCommandMetadata::new("scope-test", "rezka-free-pick"),
        )
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    let before = f.queue().await;
    assert!(
        before["visible_order_ids"][CUT]
            .as_array()
            .unwrap()
            .contains(&json!(ORDER)),
        "{before}"
    );
    let frozen = f
        .ok(
            &f.worker,
            ACTION,
            json!({"apparatus":LAM,"order_id":ORDER,
        "action":"freeze","freeze_with_issue":true,"issue_note":"Laminator malfunction"}),
        )
        .await;
    assert_eq!(frozen["session"]["status"], "frozen");
    let after = f.queue().await;
    assert_eq!(
        f.state
            .production_maps
            .order_control_state(ORDER)
            .await
            .unwrap()
            .state,
        OrderControlState::Active
    );
    for i in 0..10 {
        let (status, body) = f.scan(i, CUT).await;
        assert_eq!(status, StatusCode::OK, "healthy roll {i}: {body}");
        assert_eq!(body["batch"]["wip_status"], "waiting");
    }
    let (status, body) = f.scan(10, LAM).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "order_frozen");
    let (status, body) = f
        .post(
            &f.worker,
            ACTION,
            json!({"apparatus":LAM,"order_id":ORDER,
        "action":"resume"}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let controls = f
        .state
        .production_maps
        .queue_action_controls()
        .await
        .unwrap();
    assert!(
        controls[CUT][ORDER]
            .allowed_actions
            .contains(&Action::Start),
        "{:?}",
        controls[CUT][ORDER]
    );
    let started = f
        .ok(
            &f.worker,
            ACTION,
            json!({"apparatus":CUT,"order_id":ORDER,
        "action":"start","qr_payload":"400100000000000000000000"}),
        )
        .await;
    assert_eq!(started["session"]["status"], "active");
    assert_eq!(
        started["session"]["payload_json"]["input_progress_batch_id"],
        "healthy-0"
    );
    let finished = f
        .state
        .production_maps
        .apply_apparatus_queue_action_with_progress(
            CUT,
            ORDER,
            Action::Complete,
            &[CUT.into()],
            actor(),
            QueueProgressInput {
                produced_qty: Some(100.0),
                gross_qty: Some(11.0),
                bobina_kg: Some(1.0),
                diameter: Some(45.0),
                uom: "m".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(
        finished.progress_batch.is_some(),
        "healthy downstream WIP must yield finished output"
    );
    let source = f.store.progress_batch("healthy-0").await.unwrap().unwrap();
    assert_eq!(source.wip_status, OrderProgressBatchWipStatus::Processed);
    assert_eq!(
        f.state
            .production_maps
            .stage_order_control_states()
            .await
            .unwrap()[0]
            .state,
        OrderControlState::Frozen
    );
    assert_ne!(
        before["rev"], after["rev"],
        "cached queue revision must advance"
    );
    assert!(
        after["visible_order_ids"][CUT]
            .as_array()
            .unwrap()
            .contains(&json!(ORDER)),
        "{after}"
    );
    assert_eq!(
        after["queue_action_controls"][LAM][ORDER]["stage_control_state"],
        "frozen"
    );
    assert_eq!(
        after["queue_action_controls"][CUT][ORDER]["stage_control_state"],
        Value::Null
    );
}

#[tokio::test]
async fn admin_stage_request_global_overlay_and_local_release_have_independent_lifetimes() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let local = f
        .ok(
            &f.admin,
            CONTROL,
            json!({"order_id":ORDER,"action":"freeze",
        "scope":"stage","apparatus":LAM,"stage_node_id":"lam"}),
        )
        .await;
    assert_eq!(local["control"]["stage_node_id"], "lam");
    assert_eq!(local["control"]["state"], "freeze_requested");
    assert_eq!(
        f.state
            .production_maps
            .order_control_state(ORDER)
            .await
            .unwrap()
            .state,
        OrderControlState::Active
    );
    assert_eq!(f.scan(0, CUT).await.0, StatusCode::OK);
    f.ok(
        &f.admin,
        CONTROL,
        json!({"order_id":ORDER,"action":"cancel_freeze",
        "scope":"stage","apparatus":LAM,"stage_node_id":"lam"}),
    )
    .await;
    assert_eq!(f.scan(10, LAM).await.0, StatusCode::OK);
    let local = f
        .ok(
            &f.admin,
            CONTROL,
            json!({"order_id":ORDER,"action":"freeze",
        "scope":"stage","apparatus":LAM,"stage_node_id":"lam"}),
        )
        .await;
    let request_id = local["control"]["freeze_request"]["request_id"]
        .as_str()
        .unwrap();
    f.ok(
        &f.worker,
        ACTION,
        json!({"apparatus":LAM,"order_id":ORDER,"action":"freeze",
        "freeze_request_id":request_id,"description":"Stopped for maintenance"}),
    )
    .await;
    let global = f
        .ok(
            &f.admin,
            CONTROL,
            json!({"order_id":ORDER,"action":"freeze"}),
        )
        .await;
    assert_eq!(global["control"]["state"], "frozen");
    let (status, body) = f.scan(0, CUT).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "order_frozen");
    f.ok(
        &f.admin,
        CONTROL,
        json!({"order_id":ORDER,"action":"unfreeze"}),
    )
    .await;
    assert_eq!(f.scan(0, CUT).await.0, StatusCode::OK);
    assert_eq!(f.scan(10, LAM).await.0, StatusCode::CONFLICT);
    let sessions = f.store.order_run_sessions_for_order(ORDER).await.unwrap();
    assert_eq!(
        sessions
            .iter()
            .find(|s| s.stage_node_id == "lam")
            .unwrap()
            .status,
        OrderRunStatus::Frozen
    );
    f.ok(
        &f.admin,
        CONTROL,
        json!({"order_id":ORDER,"action":"unfreeze",
        "scope":"stage","apparatus":LAM,"stage_node_id":"lam"}),
    )
    .await;
    assert_eq!(f.scan(10, LAM).await.0, StatusCode::OK);
    let (status, _) = f
        .post(
            &f.worker,
            CONTROL,
            json!({"order_id":ORDER,"action":"freeze"}),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn global_freeze_accepts_multiple_running_stages_and_requires_each_safe_stop() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    f.store
        .put_apparatus_queue_states(CUT, [(ORDER.into(), "in_progress".into())].into())
        .await
        .unwrap();
    f.store
        .put_order_run_session(run(CUT, "cut", OrderRunStatus::Active))
        .await
        .unwrap();
    let global = f
        .ok(
            &f.admin,
            CONTROL,
            json!({"order_id":ORDER,"action":"freeze"}),
        )
        .await;
    assert_eq!(global["control"]["state"], "freeze_requested");
    for apparatus in [LAM, CUT] {
        let controls = f
            .state
            .production_maps
            .queue_action_controls()
            .await
            .unwrap();
        let serialized = serde_json::to_value(&controls[apparatus][ORDER]).unwrap();
        let request_id = serialized["freeze_request"]["request_id"]
            .as_str()
            .or_else(|| global["control"]["freeze_request"]["request_id"].as_str())
            .unwrap();
        f.ok(
            &f.worker,
            ACTION,
            json!({"apparatus":apparatus,"order_id":ORDER,"action":"freeze",
            "freeze_request_id":request_id,"description":"Safe stop"}),
        )
        .await;
        let state = f
            .state
            .production_maps
            .order_control_state(ORDER)
            .await
            .unwrap()
            .state;
        assert_eq!(
            state,
            if apparatus == LAM {
                OrderControlState::FreezeRequested
            } else {
                OrderControlState::Frozen
            }
        );
    }
    assert_eq!(f.scan(0, CUT).await.0, StatusCode::CONFLICT);
    f.ok(
        &f.admin,
        CONTROL,
        json!({"order_id":ORDER,"action":"unfreeze"}),
    )
    .await;
    assert_eq!(f.scan(0, CUT).await.0, StatusCode::OK);
}

#[tokio::test]
async fn global_cancel_after_one_safe_stop_restores_only_stopped_work() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    f.store
        .put_apparatus_queue_states(CUT, [(ORDER.into(), "in_progress".into())].into())
        .await
        .unwrap();
    f.store
        .put_order_run_session(run(CUT, "cut", OrderRunStatus::Active))
        .await
        .unwrap();
    let global = f
        .ok(
            &f.admin,
            CONTROL,
            json!({"order_id":ORDER,"action":"freeze"}),
        )
        .await;
    let controls = f
        .state
        .production_maps
        .queue_action_controls()
        .await
        .unwrap();
    let request = serde_json::to_value(&controls[LAM][ORDER]).unwrap();
    let request_id = request["freeze_request"]["request_id"]
        .as_str()
        .or_else(|| global["control"]["freeze_request"]["request_id"].as_str())
        .unwrap();
    f.ok(
        &f.worker,
        ACTION,
        json!({"apparatus":LAM,"order_id":ORDER,"action":"freeze",
        "freeze_request_id":request_id,"description":"Safe stop"}),
    )
    .await;
    f.ok(
        &f.admin,
        CONTROL,
        json!({"order_id":ORDER,"action":"cancel_freeze"}),
    )
    .await;
    assert_eq!(
        f.state
            .production_maps
            .order_control_state(ORDER)
            .await
            .unwrap()
            .state,
        OrderControlState::Active
    );
    let sessions = f.store.order_run_sessions_for_order(ORDER).await.unwrap();
    assert_eq!(
        sessions
            .iter()
            .find(|s| s.stage_node_id == "lam")
            .unwrap()
            .status,
        OrderRunStatus::Paused
    );
    assert_eq!(
        sessions
            .iter()
            .find(|s| s.stage_node_id == "cut")
            .unwrap()
            .status,
        OrderRunStatus::Active
    );
    assert_eq!(f.scan(10, LAM).await.0, StatusCode::OK);
}

#[tokio::test]
async fn explicit_stage_identity_never_freezes_a_different_route_occurrence() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let mut map = f
        .state
        .production_maps
        .raw_map(ORDER)
        .await
        .unwrap()
        .unwrap();
    // The same laminator is used again later in this route.
    let mut repeat = map.nodes.iter().find(|n| n.id == "lam").unwrap().clone();
    repeat.id = "lam-second".into();
    map.nodes.push(repeat);
    map.edges
        .iter_mut()
        .find(|e| e.from == "cut" && e.to == "end")
        .unwrap()
        .to = "lam-second".into();
    map.edges
        .push(serde_json::from_value(json!({"from":"lam-second","to":"end"})).unwrap());
    f.store.put_map(map).await.unwrap();
    let (status, _) = f
        .post(
            &f.admin,
            CONTROL,
            json!({"order_id":ORDER,"action":"freeze","scope":"stage","apparatus":LAM}),
        )
        .await;
    assert_ne!(
        status,
        StatusCode::OK,
        "ambiguous occurrence must be rejected"
    );
    let (status, _) = f
        .post(
            &f.admin,
            CONTROL,
            json!({"order_id":ORDER,"action":"freeze","scope":"stage","apparatus":LAM,"stage_node_id":"cut"}),
        )
        .await;
    assert_ne!(
        status,
        StatusCode::OK,
        "stage from another apparatus must be rejected"
    );
    f.ok(
        &f.admin,
        CONTROL,
        json!({"order_id":ORDER,"action":"freeze","scope":"stage","apparatus":LAM,"stage_node_id":"lam-second"}),
    )
    .await;
    let controls = f
        .state
        .production_maps
        .stage_order_control_states()
        .await
        .unwrap();
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0].stage_node_id, "lam-second");
    assert_eq!(controls[0].state, OrderControlState::Frozen);
    let sessions = f.store.order_run_sessions_for_order(ORDER).await.unwrap();
    assert_eq!(
        sessions
            .iter()
            .find(|s| s.stage_node_id == "lam")
            .unwrap()
            .status,
        OrderRunStatus::Active
    );
    assert_eq!(
        f.state
            .production_maps
            .order_control_state(ORDER)
            .await
            .unwrap()
            .state,
        OrderControlState::Active
    );
    f.ok(
        &f.admin,
        CONTROL,
        json!({"order_id":ORDER,"action":"unfreeze","scope":"stage","apparatus":LAM,"stage_node_id":"lam-second"}),
    )
    .await;
    let sessions = f.store.order_run_sessions_for_order(ORDER).await.unwrap();
    assert_eq!(
        sessions
            .iter()
            .find(|s| s.stage_node_id == "lam")
            .unwrap()
            .status,
        OrderRunStatus::Active
    );
}

#[tokio::test]
#[ignore = "requires SCOPED_FREEZE_TEST_DATABASE_URL pointing at an isolated disposable PostgreSQL server"]
async fn postgres_stage_hold_survives_restart_and_global_overlay_release() {
    let url = std::env::var("SCOPED_FREEZE_TEST_DATABASE_URL")
        .expect("explicit disposable PG server required");
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let database = format!("scoped_freeze_{:032x}", rand::random::<u128>());
    sqlx::query(&format!("CREATE DATABASE {database}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(
        url.parse::<sqlx::postgres::PgConnectOptions>()
            .unwrap()
            .database(&database),
    )
    .await
    .unwrap();
    let checked = pool.clone();
    let result = tokio::task::LocalSet::new()
        .run_until(async move {
            tokio::task::spawn_local(async move { postgres_checks(checked).await }).await
        })
        .await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {database} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}
async fn postgres_checks(pool: sqlx::PgPool) {
    sqlx::raw_sql("DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='mini_rs_erp') THEN CREATE ROLE mini_rs_erp NOLOGIN; END IF; END $$;").execute(&pool).await.unwrap();
    postgres::apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    let canonical = postgres::canonical_apparatus_service(pool.clone());
    canonical.bootstrap_factory_defaults().await.unwrap();
    postgres::apply_postgres_migrations_through_version(&pool, "0161")
        .await
        .unwrap();
    for id in [
        "zakaz-legacy-frozen-no-control",
        "zakaz-legacy-explicit-active",
    ] {
        let map = json!({"id":id,"title":"Legacy freeze","product_code":"LEGACY",
            "nodes":[{"id":"start","kind":"start","title":"Start"},
                {"id":"lam","kind":"apparatus","title":"Lamination","apparatus_id":LAM},
                {"id":"end","kind":"end","title":"End"}],
            "edges":[{"from":"start","to":"lam"},{"from":"lam","to":"end"}]});
        sqlx::query("INSERT INTO mini_production_maps(id,product_code,title,map_json) VALUES($1,'LEGACY','Legacy freeze',$2)")
            .bind(id).bind(map).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO mini_queue_states(apparatus,canonical_apparatus_id,order_id,state) VALUES($1,$1,$2,'frozen')")
            .bind(LAM).bind(id).execute(&pool).await.unwrap();
    }
    sqlx::query("INSERT INTO mini_order_control_states(order_id,state) VALUES('zakaz-legacy-explicit-active','active')")
        .execute(&pool).await.unwrap();
    postgres::apply_foundation_migration(&pool).await.unwrap();
    let legacy: Option<String> = sqlx::query_scalar(
        "SELECT state FROM mini_order_control_states WHERE order_id='zakaz-legacy-frozen-no-control'",
    )
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert_eq!(
        legacy.as_deref(),
        Some("frozen"),
        "migration must preserve historical whole-order holds"
    );
    let active: String = sqlx::query_scalar(
        "SELECT state FROM mini_order_control_states WHERE order_id='zakaz-legacy-explicit-active'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        active, "active",
        "migration must not overwrite explicit existing controls"
    );
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new(
        store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(canonical.clone())),
    );
    let released = service
        .unfreeze_order("zakaz-legacy-frozen-no-control", admin_actor())
        .await
        .unwrap();
    assert_eq!(released.state, OrderControlState::Active);
    let request = released
        .freeze_request
        .as_ref()
        .expect("legacy recovery audit");
    assert!(request.target_session_id.is_empty());
    assert!(request.target_apparatus.is_empty());
    assert!(request.target_worker_role.is_empty());
    assert!(request.target_worker_ref.is_empty());
    assert!(request.target_worker_display_name.is_empty());
    assert_eq!(
        store.apparatus_queue_states().await.unwrap()[LAM]["zakaz-legacy-frozen-no-control"],
        "pending"
    );
    let outbox:i64=sqlx::query_scalar("SELECT count(*) FROM mini_order_freeze_chat_outbox o JOIN mini_order_freeze_requests r ON r.request_id=o.request_id WHERE r.order_id='zakaz-legacy-frozen-no-control'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(
        outbox, 0,
        "legacy queue-only recovery must not invent a worker recipient"
    );
    seed(&service, store.as_ref()).await;
    service
        .apply_apparatus_queue_action_with_progress(
            LAM,
            ORDER,
            Action::Freeze,
            &[LAM.into()],
            actor(),
            QueueProgressInput {
                freeze_with_issue: true,
                description: "Maintenance".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    drop(service);
    let restored = ProductionMapService::new(
        Arc::new(PostgresProductionMapStore::new(pool.clone())),
        Arc::new(CanonicalServiceApparatusResolver::new(canonical)),
    );
    assert_eq!(
        restored.order_control_state(ORDER).await.unwrap().state,
        OrderControlState::Active
    );
    assert!(
        restored
            .start_input_for_qr(CUT, ORDER, "healthy-0", "400100000000000000000000")
            .await
            .is_ok()
    );
    assert_eq!(
        restored
            .start_input_for_qr(LAM, ORDER, "healthy-10", "40010000000000000000000A")
            .await
            .unwrap_err(),
        ProductionMapError::OrderFrozen
    );
    restored
        .request_order_freeze(ORDER, admin_actor())
        .await
        .unwrap();
    assert_eq!(
        restored
            .start_input_for_qr(CUT, ORDER, "healthy-0", "400100000000000000000000")
            .await
            .unwrap_err(),
        ProductionMapError::OrderFrozen
    );
    restored.unfreeze_order(ORDER, admin_actor()).await.unwrap();
    assert!(
        restored
            .start_input_for_qr(CUT, ORDER, "healthy-0", "400100000000000000000000")
            .await
            .is_ok()
    );
    assert_eq!(
        restored
            .start_input_for_qr(LAM, ORDER, "healthy-10", "40010000000000000000000A")
            .await
            .unwrap_err(),
        ProductionMapError::OrderFrozen
    );
    restored
        .unfreeze_stage(ORDER, LAM, "lam", admin_actor())
        .await
        .unwrap();
    assert!(
        restored
            .start_input_for_qr(LAM, ORDER, "healthy-10", "40010000000000000000000A")
            .await
            .is_ok()
    );

    // The transaction must keep a multi-station request pending until its
    // final running station confirms, including after reconstructing service state.
    for (apparatus, stage) in [(LAM, "lam"), (CUT, "cut")] {
        store
            .put_apparatus_queue_states(apparatus, [(ORDER.into(), "in_progress".into())].into())
            .await
            .unwrap();
        store
            .put_order_run_session(run(apparatus, stage, OrderRunStatus::Active))
            .await
            .unwrap();
    }
    restored
        .request_order_freeze(ORDER, admin_actor())
        .await
        .unwrap();
    for apparatus in [LAM, CUT] {
        let controls = restored.queue_action_controls().await.unwrap();
        let request_id = controls[apparatus][ORDER]
            .freeze_request
            .as_ref()
            .unwrap()
            .request_id
            .clone();
        restored
            .apply_apparatus_queue_action_with_progress(
                apparatus,
                ORDER,
                Action::Freeze,
                &[apparatus.into()],
                actor(),
                QueueProgressInput {
                    freeze_request_id: request_id,
                    description: "Safe stop".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            restored.order_control_state(ORDER).await.unwrap().state,
            if apparatus == LAM {
                OrderControlState::FreezeRequested
            } else {
                OrderControlState::Frozen
            }
        );
    }
    restored.unfreeze_order(ORDER, admin_actor()).await.unwrap();
    assert!(
        restored
            .start_input_for_qr(CUT, ORDER, "healthy-0", "400100000000000000000000")
            .await
            .is_ok()
    );

    // Passed colour trials reserve machines without active production. Their
    // SQL drain may finalize the global stop before the paused session is visited.
    const MIXED: &str = "zakaz-pg-mixed-preflight";
    const PRINT_TWO: &str = "apparatus:default:bosma_8";
    restored.upsert_map(serde_json::from_value(json!({
        "id":MIXED,"title":"Mixed colour trial shutdown","product_code":"MIXED",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"print-a","kind":"apparatus","title":"First printer","apparatus_id":PRINT},
            {"id":"print-b","kind":"apparatus","title":"Second printer","apparatus_id":PRINT_TWO},
            {"id":"lam","kind":"apparatus","title":"Lamination","apparatus_id":LAM},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"print-a"},{"from":"print-a","to":"print-b"},
            {"from":"print-b","to":"lam"},{"from":"lam","to":"end"}]
    })).unwrap()).await.unwrap();
    let mut hold_ids = Vec::new();
    for (index, apparatus) in [PRINT, PRINT_TWO].into_iter().enumerate() {
        let key = format!("pg-mixed-colour-{index}");
        let hold = restored
            .begin_print_preflight(apparatus, MIXED, &key, &key, actor())
            .await
            .unwrap();
        restored
            .advance_print_preflight(apparatus, MIXED, &hold.hold_id, "passed", actor())
            .await
            .unwrap();
        hold_ids.push(hold.hold_id);
    }
    let mut paused = run(LAM, "lam", OrderRunStatus::Paused);
    paused.order_id = MIXED.into();
    paused.session_id = "pg-mixed-lam-session".into();
    store.put_order_run_session(paused).await.unwrap();
    let mut states = store
        .apparatus_queue_states()
        .await
        .unwrap()
        .remove(LAM)
        .unwrap_or_default();
    states.insert(MIXED.into(), "paused".into());
    store.put_apparatus_queue_states(LAM, states).await.unwrap();
    let frozen = restored
        .request_order_early_close(MIXED, admin_actor(), "Stop mixed colour trials")
        .await
        .unwrap();
    assert_eq!(frozen.state, OrderControlState::Frozen);
    assert!(
        frozen
            .early_close
            .as_ref()
            .unwrap()
            .closed_at_unix
            .is_some()
    );
    assert_eq!(
        frozen,
        restored.order_control_state(MIXED).await.unwrap(),
        "return the durable finalized record"
    );
    for id in hold_ids {
        assert_eq!(
            store
                .print_preflight_hold_by_id(&id)
                .await
                .unwrap()
                .unwrap()
                .status,
            PrintPreflightStatus::Cancelled
        );
    }
    assert!(
        store
            .order_run_sessions_for_order(MIXED)
            .await
            .unwrap()
            .iter()
            .all(|s| s.status != OrderRunStatus::Active)
    );
    let retried = restored
        .request_order_early_close(MIXED, admin_actor(), "Stop mixed colour trials")
        .await
        .unwrap();
    assert_eq!(
        retried, frozen,
        "retry must retain the immutable close timestamp"
    );
}
