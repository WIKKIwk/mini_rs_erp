use super::*;
use time::OffsetDateTime;
use tokio::io::AsyncReadExt;
use tracing_subscriber::{fmt::format::FmtSpan, prelude::*};

const WAREHOUSE: &str = "/v1/mobile/admin/warehouses/live";
const MONITOR: &str = "/v1/mobile/admin/system/monitor/live";

struct Server {
    origin: String,
    task: tokio::task::JoinHandle<()>,
    client: reqwest::Client,
}

impl Server {
    async fn start(state: AppState) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = build_router(state);
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            origin,
            task,
            client: reqwest::Client::new(),
        }
    }

    fn upgrade(&self, path: &str, token: &str) -> reqwest::RequestBuilder {
        self.client
            .get(format!("{}{path}", self.origin))
            .query(&[("token", token)])
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn legacy_ws_unset_invalid_and_expired_deadlines_fail_closed() {
    for deadline in [None, Some("tomorrow"), Some("2000-01-01T10:00:00+05:00")] {
        let mut state = test_state();
        state.legacy_ws_query_token_until = crate::config::legacy_ws_query_token_until(deadline);
        let admin = session(&state, PrincipalRole::Admin).await;
        let server = Server::start(state).await;
        assert_eq!(
            server
                .upgrade(WAREHOUSE, &admin)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn legacy_ws_deadline_expires_on_the_running_router() {
    let mut state = test_state();
    let admin = session(&state, PrincipalRole::Admin).await;
    let until = OffsetDateTime::now_utc() + time::Duration::seconds(2);
    let setting = until
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    state.legacy_ws_query_token_until = crate::config::legacy_ws_query_token_until(Some(&setting));
    let server = Server::start(state).await;
    assert_eq!(
        server
            .upgrade(WAREHOUSE, &admin)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::SWITCHING_PROTOCOLS
    );
    tokio::time::sleep((until - OffsetDateTime::now_utc()).unsigned_abs()).await;
    assert_eq!(
        server
            .upgrade(WAREHOUSE, &admin)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_ws_enforces_scope_and_session_and_redacts_all_request_logs() {
    // Isolate the actual deployment environment and tracing dispatcher from other
    // parallel tests, including Axum's independently spawned upgrade tasks.
    if std::env::var_os("MINI_ERP_WS_LOG_TEST_CHILD").is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "http::admin_route_tests::legacy_ws::legacy_ws_enforces_scope_and_session_and_redacts_all_request_logs"])
            .env("MINI_ERP_WS_LOG_TEST_CHILD", "1")
            .env("MINI_ERP_LEGACY_WS_QUERY_TOKEN_UNTIL", "2099-01-01T00:00:00Z")
            .output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        return;
    }
    let log = tempfile::NamedTempFile::new().unwrap();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            "mini_rs_erp=trace,tower_http=trace,axum=trace",
        ))
        .with(
            tracing_subscriber::fmt::layer()
                .with_span_events(FmtSpan::NEW)
                .with_ansi(false)
                .with_writer(log.reopen().unwrap())
                .with_filter(crate::http::safe_request_log_levels()),
        );
    tracing::subscriber::set_global_default(subscriber).unwrap();

    let state = test_state();
    let admin = session(&state, PrincipalRole::Admin).await;
    let same_principal = session(&state, PrincipalRole::Admin).await;
    let sales = session(&state, PrincipalRole::SalesManager).await;
    let customer = session(&state, PrincipalRole::Customer).await;
    let server = Server::start(state.clone()).await;
    for (path, token) in [(WAREHOUSE, &sales), (MONITOR, &customer)] {
        assert_eq!(
            server.upgrade(path, token).send().await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    for request in [
        server.upgrade(WAREHOUSE, &admin).bearer_auth(&admin),
        server
            .upgrade(WAREHOUSE, &admin)
            .query(&[("ticket", "private-ticket")]),
        server.upgrade(WAREHOUSE, "invalid-session"),
    ] {
        assert_eq!(
            request.send().await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let bad_filter = server
        .upgrade("/v1/mobile/production-tasks/live", &admin)
        .query(&[("limit", "private-filter")])
        .send()
        .await
        .unwrap();
    assert_ne!(bad_filter.status(), StatusCode::SWITCHING_PROTOCOLS);
    let mirrored_version = server
        .upgrade(WAREHOUSE, &admin)
        .header("x-app-version", admin.as_str())
        .send()
        .await
        .unwrap();
    assert_eq!(mirrored_version.status(), StatusCode::SWITCHING_PROTOCOLS);
    drop(mirrored_version);
    let response = server
        .upgrade(WAREHOUSE, &admin)
        .header("x-app-version", "2.4.1 (build 13)")
        .query(&[("filter", "private-filter")])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    let mut socket = response.upgrade().await.unwrap();
    state.sessions.delete(&admin).await.unwrap();
    assert!(state.sessions.get(&same_principal).await.is_ok());
    assert_eq!(
        server
            .upgrade(WAREHOUSE, &admin)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    // Trigger a protected send even if the handler subscribes after the handshake.
    let mut ticks = tokio::time::interval(Duration::from_millis(10));
    let opcode = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            tokio::select! {
                byte = socket.read_u8() => break byte.unwrap() & 0x0f,
                _ = ticks.tick() => state.warehouse_events.notify_updated("warehouse", "changed"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(opcode, 8, "revoked socket must close before sending data");

    let logs = std::fs::read_to_string(log.path()).unwrap();
    assert_eq!(logs.matches("legacy_ws_query_connected").count(), 2);
    for safe in ["http.request", WAREHOUSE, "role=Admin", "2.4.1 (build 13)", "[redacted]"] {
        assert!(logs.contains(safe), "missing field {safe}");
    }
    for secret in [
        &*admin,
        &sales,
        &customer,
        "invalid-session",
        "private-filter",
        "private-ticket",
        "?token=",
    ] {
        assert!(
            !logs.contains(secret),
            "request logs contained a credential or query"
        );
    }
}
