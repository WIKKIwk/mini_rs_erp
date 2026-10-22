//! Exercises a disk-restored cursor through the actual HTTP and WS routes.
//! HTTP snapshots deliberately do not prime the socket's retained feed, so
//! these distinguish a cold server cache from an offline retained baseline.
use super::*;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const APPARATUS: &str = "apparatus:default:bosma_8";
const OTHER_APPARATUS: &str = "apparatus:default:bosma_7";
const ORDER: &str = "zakaz-persisted-resume";
const WORKER_BASE: &str =
    "/v1/mobile/admin/production-maps/sequence?worker_scope=true&queue_limit=10";

struct Server {
    address: std::net::SocketAddr,
    task: tokio::task::JoinHandle<()>,
    client: reqwest::Client,
}

impl Server {
    async fn start(state: AppState) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, build_router(state)).await.unwrap();
        });
        Self {
            address,
            task,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        }
    }

    async fn get(&self, path: &str, token: &str) -> reqwest::Response {
        self.client
            .get(format!("http://{}{path}", self.address))
            .bearer_auth(token)
            .send()
            .await
            .unwrap()
    }

    async fn socket(&self, query: &str, token: &str) -> reqwest::Upgraded {
        let response = self
            .client
            .get(format!(
                "http://{}/v1/mobile/admin/production-maps/live?{query}",
                self.address
            ))
            .bearer_auth(token)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        // reqwest wraps hyper's upgraded IO in its public trait object.
        response.upgrade().await.unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn message(socket: &mut (impl AsyncRead + AsyncWrite + Unpin)) -> Value {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let opcode = socket.read_u8().await.unwrap() & 0x0f;
            let byte = socket.read_u8().await.unwrap();
            assert_eq!(byte & 0x80, 0, "server frames must be unmasked");
            let length = match byte & 0x7f {
                126 => socket.read_u16().await.unwrap() as usize,
                127 => socket.read_u64().await.unwrap() as usize,
                n => n as usize,
            };
            assert!(
                length < 128 * 1024,
                "resume must use bounded control/delta frames"
            );
            let mut payload = vec![0; length];
            socket.read_exact(&mut payload).await.unwrap();
            if opcode == 1 {
                return serde_json::from_slice(&payload).unwrap();
            }
            if opcode == 9 {
                assert!(length <= 125);
                let mut pong = vec![0x8a, 0x80 | length as u8, 0, 0, 0, 0];
                pong.extend(payload);
                socket.write_all(&pong).await.unwrap();
            }
            assert_ne!(opcode, 8, "server closed before resume decision");
        }
    })
    .await
    .expect("bounded resume response")
}

async fn close(socket: &mut (impl AsyncRead + AsyncWrite + Unpin)) {
    socket.write_all(&[0x88, 0x80, 0, 0, 0, 0]).await.unwrap();
    let mut tail = Vec::new();
    tokio::time::timeout(Duration::from_secs(2), socket.read_to_end(&mut tail))
        .await
        .expect("server must release closed subscriber")
        .unwrap();
}

fn disk_roundtrip(snapshot: &Value) -> Value {
    serde_json::from_slice(&serde_json::to_vec(snapshot).unwrap()).unwrap()
}

async fn assign(state: &AppState, who: &str, apparatus: &str) -> String {
    state
        .admin
        .upsert_role_assignment(crate::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: who.into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![apparatus.into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    session_for(state, PrincipalRole::Aparatchi, who).await
}

async fn put_order(state: &AppState, id: &str, title: &str, apparatus: &str) {
    let order_number = if id == ORDER { "1" } else { "2" };
    state
        .production_maps
        .upsert_map(
            serde_json::from_str(&pechat_order_map_json(id, title, order_number, apparatus))
                .unwrap(),
        )
        .await
        .unwrap();
}

async fn start_preflight(state: &AppState) {
    state
        .production_maps
        .begin_print_preflight(
            APPARATUS,
            ORDER,
            "persisted-colour",
            "persisted-colour",
            crate::core::production_map::QueueActionActor {
                role: "admin".into(),
                ref_: "admin".into(),
                display_name: "Admin".into(),
            },
        )
        .await
        .unwrap();
}

fn worker_socket_query(snapshot: &Value) -> String {
    format!(
        "protocol=state_delta_v2&worker_scope=true&queue_limit=10&epoch={}&rev={}&scope={}",
        urlencoding::encode(snapshot["epoch"].as_str().unwrap()),
        snapshot["rev"],
        urlencoding::encode(snapshot["scope"].as_str().unwrap())
    )
}

fn worker_conditional(snapshot: &Value) -> String {
    format!(
        "{WORKER_BASE}&if_epoch={}&if_rev={}&if_scope={}",
        urlencoding::encode(snapshot["epoch"].as_str().unwrap()),
        snapshot["rev"],
        urlencoding::encode(snapshot["scope"].as_str().unwrap())
    )
}

fn page_path() -> String {
    format!("/v1/mobile/admin/production-maps/order-page?view=sequence&apparatus={APPARATUS}")
}

fn page_socket_query(snapshot: &Value) -> String {
    let pages = json!([{"id":"persisted", "view":"sequence", "apparatus":APPARATUS,
        "length":20, "version":snapshot["version"]}]);
    format!(
        "protocol=page_delta_v1&pages={}",
        urlencoding::encode(&pages.to_string())
    )
}

#[tokio::test]
async fn persistent_worker_cold_unchanged_cursor_needs_neither_http_body_nor_ws_snapshot() {
    let mut state = test_state();
    state.worker_show_all_apparatus_tabs = false;
    let token = assign(&state, "persisted-worker", APPARATUS).await;
    put_order(&state, ORDER, "Persisted", APPARATUS).await;
    let server = Server::start(state).await;
    let response = server.get(WORKER_BASE, &token).await;
    assert_eq!(response.status(), StatusCode::OK);
    let stored = disk_roundtrip(&response.json::<Value>().await.unwrap());
    assert_eq!(stored["maps"].as_array().unwrap().len(), 1);
    let unchanged = server.get(&worker_conditional(&stored), &token).await;
    assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(unchanged.headers()["cache-control"], "no-store");
    assert!(unchanged.bytes().await.unwrap().is_empty());

    // No previous WS subscription or retained delta exists on this server.
    let mut socket = server.socket(&worker_socket_query(&stored), &token).await;
    let ready = message(&mut socket).await;
    assert_eq!(ready["type"], "state_ready");
    assert_eq!(ready["protocol"], "state_delta_v2");
    assert_eq!(ready["rev"], stored["rev"]);
    assert_eq!(ready["scope"], stored["scope"]);
    assert_eq!(ready["resync"], false);
    assert_eq!(ready["recovered"], true);
    assert_eq!(ready["recovery_supported"], true);
    assert!(ready.get("maps").is_none());
    assert!(ready.get("patch").is_none());
}

#[tokio::test]
async fn persistent_worker_empty_snapshot_is_a_valid_recoverable_baseline() {
    let mut state = test_state();
    state.worker_show_all_apparatus_tabs = false;
    let token = assign(&state, "empty-persisted-worker", APPARATUS).await;
    let server = Server::start(state).await;
    let response = server.get(WORKER_BASE, &token).await;
    assert_eq!(response.status(), StatusCode::OK);
    let stored = disk_roundtrip(&response.json::<Value>().await.unwrap());
    assert!(stored["maps"].as_array().unwrap().is_empty());
    assert!(stored["rev"].is_u64());
    let unchanged = server.get(&worker_conditional(&stored), &token).await;
    assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);
    assert!(unchanged.bytes().await.unwrap().is_empty());
    let mut socket = server.socket(&worker_socket_query(&stored), &token).await;
    let ready = message(&mut socket).await;
    assert_eq!(ready["type"], "state_ready");
    assert_eq!(ready["recovered"], true);
    assert_eq!(ready["resync"], false);
    assert_eq!(ready["rev"], stored["rev"]);
}

#[tokio::test]
async fn persistent_worker_reconnect_replays_changes_made_with_no_live_subscriber() {
    let mut state = test_state();
    state.worker_show_all_apparatus_tabs = false;
    let token = assign(&state, "offline-worker", APPARATUS).await;
    put_order(&state, ORDER, "Before offline", APPARATUS).await;
    let server = Server::start(state.clone()).await;
    let stored = disk_roundtrip(
        &server
            .get(WORKER_BASE, &token)
            .await
            .json::<Value>()
            .await
            .unwrap(),
    );
    let query = worker_socket_query(&stored);
    let mut original = server.socket(&query, &token).await;
    assert_eq!(message(&mut original).await["recovered"], true);
    close(&mut original).await;
    drop(original);
    start_preflight(&state).await;

    let mut resumed = server.socket(&query, &token).await;
    let delta = message(&mut resumed).await;
    assert_eq!(delta["type"], "state_delta");
    assert_eq!(delta["protocol"], "state_delta_v2");
    assert_eq!(delta["base_rev"], stored["rev"]);
    assert_eq!(
        delta["patch"]["queue_action_controls"]["scopes"][APPARATUS]["upsert"][ORDER]["print_preflight"]
            ["status"],
        "running"
    );
    let ready = message(&mut resumed).await;
    assert_eq!(ready["type"], "state_ready");
    assert_eq!(ready["recovered"], true);
    assert_eq!(ready["rev"], delta["rev"]);
    let current = server
        .get(WORKER_BASE, &token)
        .await
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(current["rev"], ready["rev"]);
    assert_eq!(
        current["queue_action_controls"][APPARATUS][ORDER]["print_preflight"]["status"],
        "running"
    );
}

#[tokio::test]
async fn persistent_worker_missing_replay_requires_canonical_http_reconciliation() {
    let mut state = test_state();
    state.worker_show_all_apparatus_tabs = false;
    let token = assign(&state, "missing-history-worker", APPARATUS).await;
    put_order(&state, ORDER, "Before", APPARATUS).await;
    let server = Server::start(state.clone()).await;
    let stored = disk_roundtrip(
        &server
            .get(WORKER_BASE, &token)
            .await
            .json::<Value>()
            .await
            .unwrap(),
    );
    // HTTP bootstrap never primes WS replay. A changed revision therefore has
    // no retained predecessor, matching a server-evicted/restarted feed.
    start_preflight(&state).await;
    let mut socket = server.socket(&worker_socket_query(&stored), &token).await;
    let ready = message(&mut socket).await;
    assert_eq!(ready["type"], "state_ready");
    assert_eq!(ready["resync"], true);
    assert_eq!(ready["recovered"], false);
    assert!(ready.get("maps").is_none());
    assert!(ready.get("patch").is_none());
    let response = server.get(&worker_conditional(&stored), &token).await;
    assert_eq!(response.status(), StatusCode::OK);
    let current = response.json::<Value>().await.unwrap();
    assert_eq!(current["rev"], ready["rev"]);
    assert_eq!(
        current["queue_action_controls"][APPARATUS][ORDER]["print_preflight"]["status"],
        "running"
    );
}

#[tokio::test]
async fn persistent_worker_cursor_cannot_cross_actor_scope_epoch_window_or_auth() {
    let mut state = test_state();
    state.worker_show_all_apparatus_tabs = false;
    let token = assign(&state, "private-worker-a", APPARATUS).await;
    let other_token = assign(&state, "private-worker-b", OTHER_APPARATUS).await;
    put_order(&state, ORDER, "Private A", APPARATUS).await;
    put_order(&state, "zakaz-private-b", "Private B", OTHER_APPARATUS).await;
    let server = Server::start(state.clone()).await;
    let stored = disk_roundtrip(
        &server
            .get(WORKER_BASE, &token)
            .await
            .json::<Value>()
            .await
            .unwrap(),
    );
    for field in ["epoch", "scope"] {
        let mut invalid = stored.clone();
        invalid[field] = "old-or-foreign".into();
        let mut socket = server.socket(&worker_socket_query(&invalid), &token).await;
        let ready = message(&mut socket).await;
        assert_eq!(ready["resync"], true, "must reject {field}");
        assert!(ready.get("patch").is_none());
        assert_eq!(
            server
                .get(&worker_conditional(&invalid), &token)
                .await
                .status(),
            StatusCode::OK
        );
    }
    let mut foreign = server
        .socket(&worker_socket_query(&stored), &other_token)
        .await;
    let ready = message(&mut foreign).await;
    assert_eq!(ready["resync"], true);
    assert_ne!(ready["scope"], stored["scope"]);
    assert!(ready.get("patch").is_none());
    let foreign_http = server.get(&worker_conditional(&stored), &other_token).await;
    assert_eq!(foreign_http.status(), StatusCode::OK);
    let foreign_http = foreign_http.json::<Value>().await.unwrap();
    assert_eq!(foreign_http["maps"].as_array().unwrap().len(), 1);
    assert_eq!(foreign_http["maps"][0]["map"]["id"], "zakaz-private-b");
    let different_window = worker_conditional(&stored).replace("queue_limit=10", "queue_limit=5");
    assert_eq!(
        server.get(&different_window, &token).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        server
            .get(&worker_conditional(&stored), "invalid-token")
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );

    // Assignment changes do not necessarily increment the production revision.
    // An old persisted actor cursor must nevertheless lose its authority.
    let revision = state.production_maps.snapshot_revision();
    assign(&state, "private-worker-a", OTHER_APPARATUS).await;
    assert_eq!(state.production_maps.snapshot_revision(), revision);
    let reassigned = server.get(&worker_conditional(&stored), &token).await;
    assert_eq!(reassigned.status(), StatusCode::OK);
    let reassigned = reassigned.json::<Value>().await.unwrap();
    assert_ne!(reassigned["scope"], stored["scope"]);
    assert_eq!(reassigned["maps"][0]["map"]["id"], "zakaz-private-b");
    let mut socket = server.socket(&worker_socket_query(&stored), &token).await;
    let ready = message(&mut socket).await;
    assert_eq!(ready["resync"], true);
    assert_eq!(ready["scope"], reassigned["scope"]);
    assert!(ready.get("patch").is_none());
}

#[tokio::test]
async fn persistent_admin_cold_page_version_confirms_without_reloading_page() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    put_order(&state, ORDER, "Persisted admin page", APPARATUS).await;
    let server = Server::start(state).await;
    let stored = disk_roundtrip(
        &server
            .get(&page_path(), &token)
            .await
            .json::<Value>()
            .await
            .unwrap(),
    );
    let unchanged = server
        .get(
            &format!(
                "{}&if_version={}",
                page_path(),
                stored["version"].as_str().unwrap()
            ),
            &token,
        )
        .await;
    assert_eq!(unchanged.status(), StatusCode::NOT_MODIFIED);
    assert!(unchanged.bytes().await.unwrap().is_empty());
    let mut socket = server.socket(&page_socket_query(&stored), &token).await;
    let ready = message(&mut socket).await;
    assert_eq!(ready["type"], "page_window_ready");
    assert_eq!(ready["window"], "persisted");
    assert_eq!(ready["version"], stored["version"]);
    assert_eq!(ready["epoch"], stored["epoch"]);
    assert_eq!(ready["rev"], stored["rev"]);
    assert_eq!(ready["resync"], false);
    assert!(ready.get("maps").is_none());
}

#[tokio::test]
async fn persistent_admin_reconnect_diffs_retained_window_after_all_sockets_close() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    put_order(&state, ORDER, "Before admin offline", APPARATUS).await;
    let server = Server::start(state.clone()).await;
    let stored = disk_roundtrip(
        &server
            .get(&page_path(), &token)
            .await
            .json::<Value>()
            .await
            .unwrap(),
    );
    let query = page_socket_query(&stored);
    let mut original = server.socket(&query, &token).await;
    assert_eq!(message(&mut original).await["resync"], false);
    close(&mut original).await;
    drop(original);
    start_preflight(&state).await;
    let mut resumed = server.socket(&query, &token).await;
    let delta = message(&mut resumed).await;
    assert_eq!(delta["type"], "page_window_delta");
    assert_eq!(delta["base_version"], stored["version"]);
    assert_eq!(delta["page_count"], 1);
    assert!(delta["changes"].as_array().unwrap().iter().any(|change| {
        change["ops"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| op["value"] == "running" || op["value"]["status"] == "running")
    }));
    let ready = message(&mut resumed).await;
    assert_eq!(ready["type"], "page_window_ready");
    assert_eq!(ready["resync"], false);
    assert_eq!(ready["version"], delta["version"]);
    let current = server
        .get(&page_path(), &token)
        .await
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(current["version"], ready["version"]);
    assert_eq!(
        current["queue_action_controls"][APPARATUS][ORDER]["print_preflight"]["status"],
        "running"
    );
}

#[tokio::test]
async fn persistent_admin_missing_or_foreign_page_baseline_requires_reconciliation() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let other_token = session_for(&state, PrincipalRole::Admin, "other-admin").await;
    put_order(&state, ORDER, "Before", APPARATUS).await;
    let server = Server::start(state.clone()).await;
    let stored = disk_roundtrip(
        &server
            .get(&page_path(), &token)
            .await
            .json::<Value>()
            .await
            .unwrap(),
    );
    // Different actors cannot certify each other's persisted pages even when
    // the visible business data and global revision happen to be identical.
    let mut foreign = server
        .socket(&page_socket_query(&stored), &other_token)
        .await;
    let ready = message(&mut foreign).await;
    assert_eq!(ready["type"], "page_window_ready");
    assert_eq!(ready["resync"], true);
    assert_ne!(ready["version"], stored["version"]);
    assert!(ready.get("changes").is_none());

    start_preflight(&state).await;
    let mut socket = server.socket(&page_socket_query(&stored), &token).await;
    let ready = message(&mut socket).await;
    assert_eq!(ready["type"], "page_window_ready");
    assert_eq!(ready["resync"], true);
    assert_ne!(ready["version"], stored["version"]);
    assert!(ready.get("changes").is_none());
    let response = server
        .get(
            &format!(
                "{}&if_version={}",
                page_path(),
                stored["version"].as_str().unwrap()
            ),
            &token,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let current = response.json::<Value>().await.unwrap();
    assert_eq!(current["version"], ready["version"]);
    assert_eq!(
        current["queue_action_controls"][APPARATUS][ORDER]["print_preflight"]["status"],
        "running"
    );
}
