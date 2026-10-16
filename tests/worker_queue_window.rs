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
        let response = self
            .router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("{QUEUE}?{query}"))
                    .header(header::AUTHORIZATION, format!("Bearer {}", self.token))
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
    let f = Fixture::new().await;
    let (status, five) = f.get("worker_scope=true&queue_limit=5").await;
    assert_eq!(status, StatusCode::OK, "{five}");
    assert_eq!(five["maps"].as_array().unwrap().len(), 5);
    assert_eq!(five["visible_order_ids"][CUT].as_array().unwrap().len(), 5);
    assert_eq!(five["sequences"][CUT][0], "zakaz-000");
    assert!(five["visible_order_ids"].get(OTHER).is_none());
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
    let (status, search) = f
        .get(&format!(
            "worker_scope=true&queue_limit=5&queue_search=zakaz-059&{cursor}"
        ))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(search["visible_order_ids"][CUT], json!(["zakaz-059"]));
    assert_ne!(five["scope"], search["scope"]);
    for query in [
        "worker_scope=true&queue_limit=0",
        "worker_scope=true&queue_limit=10000",
        "queue_limit=5",
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
    server.abort();

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
    let (_, strict) = f.get("worker_scope=true&queue_limit=5").await;
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
