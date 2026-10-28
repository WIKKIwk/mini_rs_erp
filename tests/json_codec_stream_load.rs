#![cfg(feature = "verification")]
//! Real loopback HTTP/WS traffic on synthetic data and independent identities.
#[allow(dead_code)]
#[path = "../examples/json_codec_research/fixture.rs"]
mod fixture;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use fixture::{CUT, Fixture, LIVE, PAGE, QUEUE};
use mini_rs_erp::core::{
    auth::models::{Principal, PrincipalRole},
    authz::RoleAssignmentUpsert,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    net::SocketAddr,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tower::ServiceExt;

const WORKER: &str = "worker_scope=true&queue_limit=20&projection=worker_list_v1";
const PAGE_QUERY: &str = "view=orders&limit=20&projection=orders_list_v1";

async fn worker(fixture: &Fixture, name: &str, apparatus: &str) -> String {
    fixture
        .state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: name.into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![apparatus.into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    fixture
        .state
        .sessions
        .create(Principal {
            role: PrincipalRole::Aparatchi,
            ref_: name.into(),
            display_name: name.into(),
            legal_name: String::new(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .unwrap()
}

async fn get(fixture: &Fixture, token: &str, uri: &str) -> Value {
    let response = fixture
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(
        &to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}

fn subscription(view: &Value) -> String {
    format!(
        "protocol=state_delta_v2&{WORKER}&epoch={}&rev={}&scope={}",
        urlencoding::encode(view["epoch"].as_str().unwrap()),
        view["rev"],
        urlencoding::encode(view["scope"].as_str().unwrap())
    )
}

async fn connect(address: SocketAddr, token: &str, query: &str) -> TcpStream {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut socket = TcpStream::connect(address).await.unwrap();
        socket.set_nodelay(true).unwrap();
        socket.write_all(format!("GET {LIVE}?token={token}&{query} HTTP/1.1\r\nHost: {address}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").as_bytes()).await.unwrap();
        let mut headers = Vec::new();
        while !headers.ends_with(b"\r\n\r\n") {
            headers.push(socket.read_u8().await.unwrap());
            assert!(headers.len() < 8192);
        }
        assert!(headers.starts_with(b"HTTP/1.1 101"), "{}", String::from_utf8_lossy(&headers));
        socket
    }).await.expect("upgrade deadline")
}

async fn frame(socket: &mut TcpStream) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let first = socket.read_u8().await.unwrap();
            let second = socket.read_u8().await.unwrap();
            assert_eq!(first & 0x80, 0x80);
            assert_eq!(second & 0x80, 0);
            let size = match second & 127 {
                126 => socket.read_u16().await.unwrap() as usize,
                127 => usize::try_from(socket.read_u64().await.unwrap()).unwrap(),
                n => n as usize,
            };
            assert!(size < 16 * 1024 * 1024);
            let mut body = vec![0; size];
            socket.read_exact(&mut body).await.unwrap();
            match first & 15 {
                1 => return serde_json::from_slice(&body).unwrap(),
                9 => {
                    assert!(size <= 125);
                    let mut pong = vec![0x8a, 0x80 | size as u8, 0, 0, 0, 0];
                    pong.extend(body);
                    socket.write_all(&pong).await.unwrap();
                }
                opcode => panic!("unexpected opcode {opcode}"),
            }
        }
    })
    .await
    .expect("frame deadline")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn independent_workers_keep_scopes_replay_and_progress_with_a_stalled_reader() {
    let updates = std::env::var("MINI_TEST_JSON_LOAD_UPDATES")
        .map(|value| value.parse::<usize>().expect("integer load update count"))
        .unwrap_or(128);
    assert!((32..=1024).contains(&updates));
    mini_rs_erp::http::json_research::initialize_from_env().unwrap();
    let fixture = Fixture::new(285).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = fixture.router.clone();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    struct Server(tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _server = Server(server);
    let mut tokens = Vec::new();
    let mut views = Vec::new();
    let mut sockets = Vec::new();
    let mut scopes = BTreeSet::new();
    for index in 0..8 {
        let token = worker(&fixture, &format!("qualification-{index}"), CUT).await;
        let view = get(&fixture, &token, &format!("{QUEUE}?{WORKER}")).await;
        assert!(scopes.insert(view["scope"].as_str().unwrap().to_string()));
        let mut socket = connect(address, &token, &subscription(&view)).await;
        assert_eq!(frame(&mut socket).await["resync"], false);
        tokens.push(token);
        views.push(view);
        sockets.push(socket);
    }
    // A different actor cannot reuse another actor's baseline/cache key.
    let mut crossed = connect(address, &tokens[1], &subscription(&views[0])).await;
    let ready = frame(&mut crossed).await;
    assert_eq!(ready["resync"], true);
    assert_eq!(ready["scope"], views[1]["scope"]);
    drop(crossed);
    let foreign = worker(&fixture, "foreign-apparatus", "apparatus:default:asset-007").await;
    let outside = get(&fixture, &foreign, &format!("{QUEUE}?{WORKER}")).await;
    assert!(outside["worker_list_rows"].as_object().unwrap().is_empty());

    let mut legacy = connect(address, &fixture.admin, "").await;
    assert_eq!(frame(&mut legacy).await["type"], "snapshot");
    let page = get(&fixture, &fixture.admin, &format!("{PAGE}?{PAGE_QUERY}")).await;
    let query = format!("protocol=page_delta_v1&pages={}", urlencoding::encode(&json!([
        {"id":"orders","view":"orders","length":20,"projection":"orders_list_v1","version":page["version"]}
    ]).to_string()));
    let mut page_socket = connect(address, &fixture.admin, &query).await;
    assert_eq!(frame(&mut page_socket).await["resync"], false);
    // Never consume this client's large initial snapshot. Healthy identities
    // must continue even while its TCP receive window is exhausted.
    let stalled = connect(address, &fixture.admin, "").await;
    socket2::SockRef::from(&stalled)
        .set_recv_buffer_size(4096)
        .unwrap();

    let started = Instant::now();
    let mut latency_us = Vec::new();
    for step in 0..updates {
        let reconnect = step % 32 == 31;
        if reconnect {
            sockets.pop();
        }
        let saved = views[7].clone();
        let tick = Instant::now();
        let revision = fixture.change().await;
        let expected = get(&fixture, &tokens[0], &format!("{QUEUE}?{WORKER}")).await;
        for (index, socket) in sockets.iter_mut().enumerate() {
            let delta = frame(socket).await;
            assert_eq!(delta["type"], "state_delta", "{delta}");
            assert_eq!(delta["base_rev"], views[index]["rev"]);
            assert_eq!(delta["rev"], revision);
            assert_eq!(delta["scope"], views[index]["scope"]);
            assert_eq!(
                delta["patch"]["worker_list_rows"]["upsert"]["zakaz-research-00000"],
                expected["worker_list_rows"]["zakaz-research-00000"]
            );
            views[index]["rev"] = revision.into();
        }
        if reconnect {
            let mut socket = connect(address, &tokens[7], &subscription(&saved)).await;
            let replay = frame(&mut socket).await;
            assert_eq!(replay["type"], "state_delta");
            assert_eq!(replay["base_rev"], saved["rev"]);
            assert_eq!(replay["rev"], revision);
            assert_eq!(replay["scope"], saved["scope"]);
            assert_eq!(frame(&mut socket).await["resync"], false);
            views[7]["rev"] = revision.into();
            sockets.push(socket);
        }
        let snapshot = frame(&mut legacy).await;
        assert_eq!(snapshot["type"], "snapshot");
        assert_eq!(snapshot["rev"], revision);
        let delta = frame(&mut page_socket).await;
        assert_eq!(delta["type"], "page_window_delta", "{delta}");
        assert_eq!(delta["rev"], revision);
        latency_us.push(tick.elapsed().as_micros());
    }
    let outside = get(&fixture, &foreign, &format!("{QUEUE}?{WORKER}")).await;
    assert!(outside["worker_list_rows"].as_object().unwrap().is_empty());
    assert!(started.elapsed() < Duration::from_secs(120));
    latency_us.sort();
    println!(
        "{}",
        json!({"synthetic":true,"workers":8,"orders":285,"updates":updates,
        "worker_deltas_checked":updates*8,"legacy_snapshots_checked":updates,"page_deltas_checked":updates,
        "reconnections":updates/32,"stalled_readers":1,"wall_ms":started.elapsed().as_millis(),
        "update_p50_us":latency_us[updates/2],"update_p95_us":latency_us[(updates*95).div_ceil(100)-1]})
    );
}
