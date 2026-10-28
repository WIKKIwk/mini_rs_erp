#![cfg(feature = "verification")]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use mini_rs_erp::{
    app::AppState,
    config::AppConfig,
    core::{
        admin::service::AdminService,
        auth::models::{Principal, PrincipalRole},
        authz::RoleAssignmentUpsert,
        production_map::*,
        session::manager::SessionManager,
    },
    http::router::build_router,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;

const CUT: &str = "apparatus:default:asset-010";
const OTHER: &str = "apparatus:default:asset-007";
const QUEUE: &str = "/v1/mobile/admin/production-maps/sequence";
// Verification AppState shares the process-wide RPS store.
static FIXTURE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[path = "worker_queue_window/page_stream.rs"]
mod page_stream;

#[path = "worker_queue_window/sequence_positions.rs"]
mod sequence_positions;

#[path = "worker_queue_window/worker_detail.rs"]
mod worker_detail;

struct Fixture {
    _dir: tempfile::TempDir,
    state: AppState,
    router: Router,
    store: Arc<MemoryProductionMapStore>,
    token: String,
}

impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::verification(config(dir.path()));
        state.apparatus.bootstrap_factory_defaults().await.unwrap();
        state.sessions = SessionManager::memory(Some(3600));
        state.admin = AdminService::new(&state.config);
        let store = Arc::new(MemoryProductionMapStore::new());
        state.production_maps = ProductionMapService::new(
            store.clone(),
            Arc::new(CanonicalServiceApparatusResolver::new(
                state.apparatus.clone(),
            )),
        );
        state
            .admin
            .upsert_role_assignment(RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: "window-worker".into(),
                role_id: "aparatchi".into(),
                assigned_apparatus: vec![CUT.into()],
                assigned_item_groups: vec![],
            })
            .await
            .unwrap();
        let token = state
            .sessions
            .create(Principal {
                role: PrincipalRole::Aparatchi,
                ref_: "window-worker".into(),
                display_name: "Worker".into(),
                legal_name: String::new(),
                phone: String::new(),
                avatar_url: String::new(),
            })
            .await
            .unwrap();
        let mut ids = Vec::new();
        for i in 0..60 {
            let id = format!("zakaz-{i:03}");
            state
                .production_maps
                .upsert_map(map(&id, CUT))
                .await
                .unwrap();
            ids.push(id);
        }
        state
            .production_maps
            .upsert_map(map("zakaz-unrelated", OTHER))
            .await
            .unwrap();
        state
            .production_maps
            .set_apparatus_sequence(CUT, ids)
            .await
            .unwrap();
        let router = build_router(state.clone());
        Self {
            _dir: dir,
            state,
            router,
            store,
            token,
        }
    }

    async fn get(&self, query: &str) -> (StatusCode, Value) {
        self.get_as(&self.token, query).await
    }

    async fn get_as(&self, token: &str, query: &str) -> (StatusCode, Value) {
        let response = self
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("{QUEUE}?{query}"))
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 10 * 1024 * 1024)
            .await
            .unwrap();
        (
            status,
            if bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&bytes).unwrap()
            },
        )
    }

    async fn free_pick(&self) {
        use mini_rs_erp::core::apparatus_standard::{
            ApparatusId, ApparatusOperationalPolicies, CanonicalApparatusPatch,
            CanonicalCommandMetadata, QueueDiscipline,
        };
        let id = ApparatusId::new(CUT).unwrap();
        let config = self
            .state
            .apparatus
            .current_configuration(&id)
            .await
            .unwrap()
            .unwrap();
        self.state
            .apparatus
            .patch(
                id,
                config.runtime.source_revision,
                CanonicalApparatusPatch {
                    policies: Some(ApparatusOperationalPolicies {
                        worker_material_assignment_allowed: false,
                        queue: QueueDiscipline::FreePick,
                        material: config.material.policy.clone(),
                        tooling: config.material.tooling.clone(),
                    }),
                    ..Default::default()
                },
                CanonicalCommandMetadata::new("test", "shared-display-free-pick"),
            )
            .await
            .unwrap();
        self.state.production_maps.notify_live();
    }
}

fn map(id: &str, apparatus: &str) -> ProductionMapDefinition {
    serde_json::from_value(json!({"id":id, "product_code":id, "code":id, "title":id,
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"work","kind":"apparatus","title":"Work","apparatus_id":apparatus,"rezka_kadr_count":1},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"work"},{"from":"work","to":"end"}]})).unwrap()
}

async fn ws_json(stream: &mut tokio::net::TcpStream) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let mut header = [0u8; 2];
            stream.read_exact(&mut header).await.unwrap();
            let mut len = u64::from(header[1] & 127);
            if len == 126 {
                len = u64::from(stream.read_u16().await.unwrap());
            } else if len == 127 {
                len = stream.read_u64().await.unwrap();
            }
            assert!(len < 1024 * 1024);
            let mut payload = vec![0; len as usize];
            stream.read_exact(&mut payload).await.unwrap();
            if header[0] & 15 == 1 {
                return serde_json::from_slice(&payload).unwrap();
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn window_is_bounded_revision_scoped_searchable_and_refills_over_websocket() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let (status, five) = f.get("worker_scope=true&queue_limit=5").await;
    assert_eq!(status, StatusCode::OK, "{five}");
    assert_eq!(five["maps"].as_array().unwrap().len(), 5);
    assert_eq!(five["visible_order_ids"][CUT].as_array().unwrap().len(), 5);
    assert_eq!(five["sequences"][CUT][0], "zakaz-000");
    assert!(five["visible_order_ids"].get(OTHER).is_none());
    assert!(five.get("sequence_positions_by_apparatus").is_none(), "old clients keep their wire contract");
    let (_, positions) = f.get("worker_scope=true&queue_limit=5&queue_positions=true").await;
    assert_eq!(positions["sequence_positions_by_apparatus"], json!({CUT: {
        "zakaz-000":1, "zakaz-001":2, "zakaz-002":3, "zakaz-003":4, "zakaz-004":5
    }}));
    assert_ne!(positions["scope"], five["scope"]);
    let (_, legacy) = f.get("worker_scope=true").await;
    assert_eq!(
        legacy["visible_order_ids"][CUT].as_array().unwrap().len(),
        60
    );
    for limit in [10, 20, 50] {
        let (_, page) = f
            .get(&format!("worker_scope=true&queue_limit={limit}"))
            .await;
        assert_eq!(
            page["visible_order_ids"][CUT].as_array().unwrap().len(),
            limit
        );
        assert_eq!(page["maps"].as_array().unwrap().len(), limit);
        for saved in page["maps"].as_array().unwrap() {
            let id = saved["map"]["id"].as_str().unwrap();
            assert_eq!(
                page["queue_action_controls"][CUT][id],
                legacy["queue_action_controls"][CUT][id]
            );
        }
    }
    let cursor = format!(
        "if_epoch={}&if_rev={}&if_scope={}",
        five["epoch"].as_str().unwrap(),
        five["rev"],
        five["scope"].as_str().unwrap()
    );
    assert_eq!(
        f.get(&format!("worker_scope=true&queue_limit=5&{cursor}"))
            .await
            .0,
        StatusCode::NOT_MODIFIED
    );
    let (status, ten) = f
        .get(&format!("worker_scope=true&queue_limit=10&{cursor}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(five["rev"], ten["rev"]);
    assert_ne!(five["scope"], ten["scope"]);
    let (status, all) = f
        .get(&format!("worker_scope=true&queue_limit=0&{cursor}"))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(all["visible_order_ids"][CUT].as_array().unwrap().len(), 60);
    assert_eq!(all["maps"].as_array().unwrap().len(), 60);
    assert_eq!(all["sequences"][CUT], legacy["sequences"][CUT]);
    assert_eq!(
        all["queue_action_controls"][CUT],
        legacy["queue_action_controls"][CUT]
    );
    assert!(all["visible_order_ids"].get(OTHER).is_none());
    assert_ne!(all["scope"], five["scope"]);
    assert_ne!(all["scope"], legacy["scope"]);
    let (_, all_search) = f
        .get("worker_scope=true&queue_limit=0&queue_search=zakaz-059&queue_positions=true")
        .await;
    assert_eq!(all_search["visible_order_ids"][CUT], json!(["zakaz-059"]));
    assert_ne!(all_search["scope"], all["scope"]);
    let (status, search) = f
        .get(&format!(
            "worker_scope=true&queue_limit=5&queue_search=zakaz-059&queue_positions=true&{cursor}"
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(search["visible_order_ids"][CUT], json!(["zakaz-059"]));
    assert_eq!(search["sequence_positions_by_apparatus"], json!({CUT:{"zakaz-059":60}}));
    assert_eq!(all_search["sequence_positions_by_apparatus"], search["sequence_positions_by_apparatus"]);
    assert_ne!(five["scope"], search["scope"]);
    for query in [
        "worker_scope=true&queue_limit=1",
        "worker_scope=true&queue_limit=10000",
        "queue_limit=5",
        "queue_limit=0",
        "worker_scope=true&queue_search=test",
    ] {
        assert_eq!(f.get(query).await.0, StatusCode::BAD_REQUEST, "{query}");
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = f.router.clone();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let uri = format!(
        "/v1/mobile/admin/production-maps/live?token={}&protocol=state_delta_v1&worker_scope=true&queue_limit=5&epoch={}&rev={}&scope={}",
        f.token,
        five["epoch"].as_str().unwrap(),
        five["rev"],
        five["scope"].as_str().unwrap()
    );
    socket.write_all(format!("GET {uri} HTTP/1.1\r\nHost: {address}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").as_bytes()).await.unwrap();
    let mut response = Vec::new();
    while !response.ends_with(b"\r\n\r\n") {
        response.push(socket.read_u8().await.unwrap());
        assert!(response.len() < 8192);
    }
    assert!(
        String::from_utf8(response)
            .unwrap()
            .starts_with("HTTP/1.1 101")
    );
    let ready = ws_json(&mut socket).await;
    assert_eq!(ready["type"], "state_ready");
    assert_eq!(ready["resync"], false);
    let mut all_socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let all_uri = format!(
        "/v1/mobile/admin/production-maps/live?token={}&protocol=state_delta_v1&worker_scope=true&queue_limit=0&epoch={}&rev={}&scope={}",
        f.token,
        all["epoch"].as_str().unwrap(),
        all["rev"],
        all["scope"].as_str().unwrap()
    );
    all_socket.write_all(format!("GET {all_uri} HTTP/1.1\r\nHost: {address}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").as_bytes()).await.unwrap();
    let mut response = Vec::new();
    while !response.ends_with(b"\r\n\r\n") {
        response.push(all_socket.read_u8().await.unwrap());
        assert!(response.len() < 8192);
    }
    assert!(
        String::from_utf8(response)
            .unwrap()
            .starts_with("HTTP/1.1 101")
    );
    let ready = ws_json(&mut all_socket).await;
    assert_eq!(ready["type"], "state_ready");
    assert_eq!(ready["resync"], false);
    assert_eq!(ready["scope"], all["scope"]);
    f.store
        .put_apparatus_queue_states(
            CUT,
            BTreeMap::from([("zakaz-000".into(), "completed".into())]),
        )
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    let delta = ws_json(&mut socket).await;
    assert_eq!(delta["type"], "state_delta", "{delta}");
    assert_eq!(delta["scope"], five["scope"]);
    assert!(delta["patch"].get("sequence_positions_by_apparatus").is_none(), "old client delta compatibility");
    assert_eq!(
        delta["patch"]["visible_order_ids"]["upsert"][CUT],
        json!([
            "zakaz-001",
            "zakaz-002",
            "zakaz-003",
            "zakaz-004",
            "zakaz-005"
        ])
    );
    assert!(
        delta["patch"]["maps"]["remove"]
            .as_array()
            .unwrap()
            .contains(&json!("zakaz-000"))
    );
    assert_eq!(
        delta["patch"]["maps"]["upsert"].as_object().unwrap().len(),
        1
    );
    let all_delta = ws_json(&mut all_socket).await;
    assert_eq!(all_delta["type"], "state_delta", "{all_delta}");
    assert_eq!(all_delta["scope"], all["scope"]);
    let all_ids = all_delta["patch"]["visible_order_ids"]["upsert"][CUT]
        .as_array()
        .unwrap();
    assert_eq!(all_ids.len(), 59);
    assert_eq!(all_ids.last(), Some(&json!("zakaz-059")));
    assert!(!all_ids.contains(&json!("zakaz-000")));
    server.abort();
    let _ = server.await;

    // Even when the search hides a busy job, the visible job's permissions
    // still match the complete authoritative queue.
    f.store
        .put_apparatus_queue_states(
            CUT,
            BTreeMap::from([
                ("zakaz-059".into(), "in_progress".into()),
                ("zakaz-058".into(), "paused".into()),
            ]),
        )
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    let (_, full) = f.get("worker_scope=true").await;
    let (_, filtered) = f
        .get("worker_scope=true&queue_limit=5&queue_search=zakaz-001")
        .await;
    assert_eq!(
        filtered["queue_action_controls"][CUT]["zakaz-001"],
        full["queue_action_controls"][CUT]["zakaz-001"]
    );
    assert!(
        !filtered["queue_action_controls"][CUT]["zakaz-001"]["allowed_actions"]
            .as_array()
            .unwrap()
            .contains(&json!("start"))
    );
    let (_, strict) = f.get("worker_scope=true&queue_limit=5&queue_positions=true").await;
    assert_eq!(
        strict["visible_order_ids"][CUT].as_array().unwrap().len(),
        5
    );
    assert_eq!(strict["visible_order_ids"][CUT][0], "zakaz-001");
    assert!(
        strict["visible_order_ids"][CUT]
            .as_array()
            .unwrap()
            .contains(&json!("zakaz-059")),
        "a running job beyond the strict queue window must remain accessible"
    );
    assert_eq!(strict["sequence_positions_by_apparatus"][CUT]["zakaz-059"], 59,
        "active pinning preserves the pre-window rank after the completed order leaves");
    use mini_rs_erp::core::apparatus_standard::{
        ApparatusId, ApparatusOperationalPolicies, CanonicalApparatusPatch,
        CanonicalCommandMetadata, QueueDiscipline,
    };
    let id = ApparatusId::new(CUT).unwrap();
    let configuration = f
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
            configuration.runtime.source_revision,
            CanonicalApparatusPatch {
                policies: Some(ApparatusOperationalPolicies {
                    worker_material_assignment_allowed: false,
                    queue: QueueDiscipline::FreePick,
                    material: configuration.material.policy.clone(),
                    tooling: configuration.material.tooling.clone(),
                }),
                ..Default::default()
            },
            CanonicalCommandMetadata::new("test", "queue-window-free-pick"),
        )
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    let (_, ranked) = f.get("worker_scope=true&queue_limit=5").await;
    assert_eq!(ranked["visible_order_ids"][CUT][0], "zakaz-059");
    assert_eq!(ranked["visible_order_ids"][CUT][1], "zakaz-058");
}

#[tokio::test]
async fn all_window_keeps_the_workers_complete_history() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    for index in 0..61 {
        let (order_id, actor_ref, apparatus) = if index < 60 {
            // History belongs to the assigned apparatus across worker shifts.
            (
                format!("zakaz-{index:03}"),
                if index == 59 {
                    "previous-shift"
                } else {
                    "window-worker"
                },
                CUT,
            )
        } else {
            ("zakaz-unrelated".into(), "other-worker", OTHER)
        };
        f.store
            .append_apparatus_queue_action_event(ApparatusQueueActionEvent {
                event_id: format!("history-{index}"),
                apparatus: apparatus.into(),
                order_id,
                stage_node_id: "work".into(),
                action: queue_state::ApparatusQueueAction::Complete,
                from_state: queue_state::ApparatusQueueOrderState::InProgress,
                to_state: queue_state::ApparatusQueueOrderState::Completed,
                policy: ApparatusQueuePolicy::StrictSequence,
                actor: QueueActionActor {
                    role: "aparatchi".into(),
                    ref_: actor_ref.into(),
                    display_name: "Worker".into(),
                },
                assigned_apparatus: vec![apparatus.into()],
                payload_json: json!({}),
            })
            .await
            .unwrap();
    }
    f.state.production_maps.notify_live();
    let (status, bounded) = f.get("worker_scope=true&queue_limit=50").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bounded["completed_orders"].as_array().unwrap().len(), 50);
    let (status, all) = f.get("worker_scope=true&queue_limit=0").await;
    assert_eq!(status, StatusCode::OK);
    let history = all["completed_orders"].as_array().unwrap();
    assert_eq!(history.len(), 60);
    assert!(
        history
            .iter()
            .all(|item| item["order_id"] != "zakaz-unrelated")
    );
    assert!(
        all["maps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["map"]["id"] != "zakaz-unrelated")
    );
}

#[tokio::test]
async fn free_queue_order_is_shared_by_admin_workers_and_live_updates() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    f.free_pick().await;
    for (id, status, time) in [
        ("zakaz-059", OrderRunStatus::Active, 100),
        ("zakaz-058", OrderRunStatus::Paused, 300),
        ("zakaz-057", OrderRunStatus::Paused, 200),
        ("zakaz-055", OrderRunStatus::RollDetached, 1000),
    ] {
        f.store
            .put_order_run_session(OrderRunSession {
                session_id: id.into(),
                apparatus: CUT.into(),
                order_id: id.into(),
                stage_node_id: "work".into(),
                status,
                worker_role: "aparatchi".into(),
                worker_ref: "window-worker".into(),
                worker_display_name: "First shift".into(),
                started_at_unix: time,
                updated_at_unix: time,
                payload_json: json!({}),
            })
            .await
            .unwrap();
    }
    let mut states = BTreeMap::from([
        ("zakaz-059".into(), "in_progress".into()),
        ("zakaz-058".into(), "paused".into()),
        ("zakaz-057".into(), "paused".into()),
        ("zakaz-056".into(), "frozen".into()),
        ("zakaz-055".into(), "pending".into()),
    ]);
    f.store
        .put_apparatus_queue_states(CUT, states.clone())
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    let admin_token = f
        .state
        .sessions
        .create(Principal {
            role: PrincipalRole::Admin,
            ref_: "admin".into(),
            display_name: "Admin".into(),
            legal_name: String::new(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .unwrap();
    let (status, admin) = f.get_as(&admin_token, "").await;
    assert_eq!(status, StatusCode::OK);
    let expected = json!([
        "zakaz-059",
        "zakaz-058",
        "zakaz-057",
        "zakaz-055",
        "zakaz-000"
    ]);
    assert_eq!(
        json!(&admin["visible_order_ids"][CUT].as_array().unwrap()[..5]),
        expected
    );
    assert_eq!(
        admin["visible_order_ids"][CUT]
            .as_array()
            .unwrap()
            .last()
            .unwrap(),
        "zakaz-056"
    );
    assert_eq!(
        admin["sequences"][CUT][0], "zakaz-000",
        "display order never rewrites scheduling"
    );
    let (_, worker) = f.get("worker_scope=true&queue_limit=5").await;
    assert_eq!(worker["visible_order_ids"][CUT], expected);
    for limit in [0, 10, 20, 50] {
        let (_, page) = f
            .get(&format!("worker_scope=true&queue_limit={limit}"))
            .await;
        let expected_window = admin["visible_order_ids"][CUT]
            .as_array()
            .unwrap()
            .iter()
            .filter(|id| id.as_str() != Some("zakaz-056"))
            .take(if limit == 0 { usize::MAX } else { limit })
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(page["visible_order_ids"][CUT], json!(expected_window));
        assert_eq!(
            page["sequence_versions"][CUT],
            admin["sequence_versions"][CUT]
        );
    }
    let (_, search) = f
        .get("worker_scope=true&queue_limit=5&queue_search=zakaz-055&queue_positions=true")
        .await;
    assert_eq!(search["visible_order_ids"][CUT], json!(["zakaz-055"]));
    assert_eq!(search["sequence_positions_by_apparatus"], json!({CUT:{"zakaz-055":4}}),
        "free-pick rank follows shared display order, not the stored manual sequence");
    let (_, after_reads) = f.get_as(&admin_token, "").await;
    for field in [
        "rev",
        "sequences",
        "visible_order_ids",
        "queue_states",
        "sequence_versions",
    ] {
        assert_eq!(
            after_reads[field], admin[field],
            "window reads must not change {field}"
        );
    }

    f.state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "replacement-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![CUT.into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let replacement_token = f
        .state
        .sessions
        .create(Principal {
            role: PrincipalRole::Aparatchi,
            ref_: "replacement-worker".into(),
            display_name: "Second shift".into(),
            legal_name: String::new(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .unwrap();
    let (_, replacement) = f
        .get_as(&replacement_token, "worker_scope=true&queue_limit=5")
        .await;
    assert_eq!(
        replacement["visible_order_ids"],
        worker["visible_order_ids"]
    );
    for id in expected.as_array().unwrap() {
        assert_eq!(
            replacement["queue_states"][CUT][id.as_str().unwrap()],
            worker["queue_states"][CUT][id.as_str().unwrap()]
        );
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = f.router.clone();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut sockets = Vec::new();
    for (token, snapshot, scope, protocol) in [
        (&admin_token, &admin, String::new(), "delta"),
        (&admin_token, &admin, String::new(), "state_delta_v1"),
        (
            &f.token,
            &worker,
            format!(
                "&worker_scope=true&queue_limit=5&scope={}",
                worker["scope"].as_str().unwrap()
            ),
            "state_delta_v1",
        ),
    ] {
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let uri = format!(
            "/v1/mobile/admin/production-maps/live?token={token}&protocol={protocol}&epoch={}&rev={}{}",
            snapshot["epoch"].as_str().unwrap(),
            snapshot["rev"],
            scope
        );
        socket.write_all(format!("GET {uri} HTTP/1.1\r\nHost: {address}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").as_bytes()).await.unwrap();
        let mut response = Vec::new();
        while !response.ends_with(b"\r\n\r\n") {
            response.push(socket.read_u8().await.unwrap());
        }
        assert!(
            String::from_utf8(response)
                .unwrap()
                .starts_with("HTTP/1.1 101")
        );
        let ready = ws_json(&mut socket).await;
        if protocol == "delta" {
            assert_eq!(ready["type"], "snapshot");
            assert_eq!(
                ready["visible_order_ids"][CUT],
                admin["visible_order_ids"][CUT]
            );
        } else {
            assert_eq!(ready["type"], "state_ready");
        }
        sockets.push((socket, protocol));
    }
    states.insert("zakaz-059".into(), "paused".into());
    states.insert("zakaz-058".into(), "in_progress".into());
    for (id, status) in [
        ("zakaz-059", OrderRunStatus::Paused),
        ("zakaz-058", OrderRunStatus::Active),
    ] {
        let mut session = f
            .store
            .active_order_run_session(CUT, id)
            .await
            .unwrap()
            .unwrap();
        session.status = status;
        f.store.put_order_run_session(session).await.unwrap();
    }
    f.store
        .put_apparatus_queue_states(CUT, states)
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    for (mut socket, protocol) in sockets {
        let update = ws_json(&mut socket).await;
        let visible = if protocol == "delta" {
            assert_eq!(update["type"], "snapshot", "{update}");
            &update["visible_order_ids"][CUT]
        } else {
            assert_eq!(update["type"], "state_delta", "{update}");
            &update["patch"]["visible_order_ids"]["upsert"][CUT]
        }
        .as_array()
        .unwrap();
        assert_eq!(
            &visible[..4],
            &[
                json!("zakaz-058"),
                json!("zakaz-057"),
                json!("zakaz-059"),
                json!("zakaz-055")
            ]
        );
    }
    server.abort();
    let _ = server.await;
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
