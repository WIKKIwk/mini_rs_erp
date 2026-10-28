use std::{
    hint::black_box,
    net::SocketAddr,
    time::{Duration, Instant},
};

use axum::http::StatusCode;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use super::{
    Options,
    fixture::{Fixture, LIVE, PAGE, QUEUE},
    micro, summary,
};

const WORKER: &str = "worker_scope=true&queue_limit=20&projection=worker_list_v1";
const PAGE_QUERY: &str = "view=orders&limit=20&projection=orders_list_v1";

pub async fn datasets(fixture: &Fixture) -> Vec<(&'static str, Value)> {
    vec![
        ("initial_full_snapshot", fixture.json(QUEUE, false).await),
        (
            "initial_worker_list_20",
            fixture.json(&format!("{QUEUE}?{WORKER}"), true).await,
        ),
        (
            "initial_admin_page_20",
            fixture.json(&format!("{PAGE}?{PAGE_QUERY}"), false).await,
        ),
    ]
}

pub async fn http(fixture: &Fixture, options: &Options) -> Vec<Value> {
    let mut results = Vec::new();
    for (name, uri, worker, gzip) in [
        (
            "http_full_snapshot_identity",
            QUEUE.to_string(),
            false,
            false,
        ),
        ("http_full_snapshot_gzip", QUEUE.to_string(), false, true),
        (
            "http_worker_list_20",
            format!("{QUEUE}?{WORKER}"),
            true,
            false,
        ),
        (
            "http_admin_page_20",
            format!("{PAGE}?{PAGE_QUERY}"),
            false,
            false,
        ),
    ] {
        let (body, encoding) = fixture.get(&uri, worker, gzip).await;
        let mut samples = Vec::new();
        for sample in 0..options.samples + 5 {
            let started = Instant::now();
            let (next, next_encoding) = fixture.get(&uri, worker, gzip).await;
            let elapsed = started.elapsed().as_secs_f64() * 1e6;
            assert_eq!(next.len(), body.len());
            assert_eq!(next_encoding, encoding);
            black_box(next);
            if sample >= 5 {
                samples.push(elapsed);
            }
        }
        results.push(
            json!({"case":name,"kind":"router_http","wire_bytes":body.len(),
            "encoding":encoding,"timing":summary(&samples)}),
        );
    }
    let current = fixture.json(&format!("{QUEUE}?{WORKER}"), true).await;
    let uri = format!(
        "{QUEUE}?{WORKER}&if_epoch={}&if_rev={}&if_scope={}",
        urlencoding::encode(current["epoch"].as_str().unwrap()),
        current["rev"],
        urlencoding::encode(current["scope"].as_str().unwrap())
    );
    use tower::ServiceExt;
    let mut samples = Vec::new();
    for sample in 0..options.samples + 5 {
        let request = axum::http::Request::builder()
            .uri(&uri)
            .header("authorization", format!("Bearer {}", fixture.worker))
            .body(axum::body::Body::empty())
            .unwrap();
        let started = Instant::now();
        let response = fixture.router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert!(
            axum::body::to_bytes(response.into_body(), 0)
                .await
                .unwrap()
                .is_empty()
        );
        if sample >= 5 {
            samples.push(started.elapsed().as_secs_f64() * 1e6);
        }
    }
    results.push(
        json!({"case":"http_worker_unchanged_304","kind":"router_http",
        "wire_bytes":0,"timing":summary(&samples)}),
    );
    results
}

pub async fn request_decode(fixture: &Fixture, options: &Options) -> Value {
    let current = fixture
        .state
        .production_maps
        .raw_map("zakaz-research-00000")
        .await
        .unwrap()
        .unwrap();
    let body = serde_json::to_vec(&current).unwrap();
    let mut samples = Vec::new();
    for sample in 0..options.samples + 5 {
        let started = Instant::now();
        let (status, reply) = fixture.put(&body).await;
        let elapsed = started.elapsed().as_secs_f64() * 1e6;
        assert_eq!(
            status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&reply)
        );
        black_box(reply);
        if sample >= 5 {
            samples.push(elapsed);
        }
    }
    let (status, _) = fixture.put(b"{\"id\":").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    json!({"case":"http_production_map_put","kind":"router_request",
        "input_bytes":body.len(),"timing":summary(&samples)})
}

async fn connect(address: SocketAddr, token: &str, query: &str) -> TcpStream {
    let mut socket = TcpStream::connect(address).await.unwrap();
    socket.set_nodelay(true).unwrap();
    socket.write_all(format!("GET {LIVE}?token={token}&{query} HTTP/1.1\r\nHost: {address}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").as_bytes()).await.unwrap();
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        headers.push(socket.read_u8().await.unwrap());
        assert!(headers.len() < 8192);
    }
    assert!(
        headers.starts_with(b"HTTP/1.1 101"),
        "{}",
        String::from_utf8_lossy(&headers)
    );
    socket
}

async fn frame(socket: &mut TcpStream) -> Vec<u8> {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let first = socket.read_u8().await.unwrap();
            let second = socket.read_u8().await.unwrap();
            assert_eq!(first & 0x80, 0x80, "fixture expects complete frames");
            assert_eq!(second & 0x80, 0);
            let size = match second & 127 {
                126 => socket.read_u16().await.unwrap() as usize,
                127 => usize::try_from(socket.read_u64().await.unwrap()).unwrap(),
                n => n as usize,
            };
            assert!(size <= 32 * 1024 * 1024);
            let mut bytes = vec![0; size];
            socket.read_exact(&mut bytes).await.unwrap();
            match first & 15 {
                1 => return bytes,
                9 => {
                    assert!(size <= 125);
                    let mut pong = vec![0x8a, 0x80 | size as u8, 0, 0, 0, 0];
                    pong.extend(bytes);
                    socket.write_all(&pong).await.unwrap();
                }
                8 => panic!("stream closed unexpectedly"),
                _ => {}
            }
        }
    })
    .await
    .expect("stream deadline exceeded")
}

async fn subscription(fixture: &Fixture, kind: &str) -> (String, String) {
    match kind {
        "worker" => {
            let data = fixture.json(&format!("{QUEUE}?{WORKER}"), true).await;
            (
                fixture.worker.clone(),
                format!(
                    "protocol=state_delta_v2&{WORKER}&epoch={}&rev={}&scope={}",
                    urlencoding::encode(data["epoch"].as_str().unwrap()),
                    data["rev"],
                    urlencoding::encode(data["scope"].as_str().unwrap())
                ),
            )
        }
        "page" => {
            let page = fixture.json(&format!("{PAGE}?{PAGE_QUERY}"), false).await;
            let windows = json!([{"id":"research-orders","view":"orders","length":20,
                "projection":"orders_list_v1","version":page["version"]}])
            .to_string();
            (
                fixture.admin.clone(),
                format!(
                    "protocol=page_delta_v1&pages={}",
                    urlencoding::encode(&windows)
                ),
            )
        }
        "legacy" => (fixture.admin.clone(), String::new()),
        _ => panic!("unknown stream kind"),
    }
}

pub async fn websockets(fixture: &Fixture, options: &Options) -> Vec<Value> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = fixture.router.clone();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut results = Vec::new();
    let mut bootstraps = Vec::new();
    let mut initial_bytes = 0;
    for sample in 0..options.samples + 3 {
        let started = Instant::now();
        let mut socket = connect(address, &fixture.admin, "").await;
        let data = frame(&mut socket).await;
        let elapsed = started.elapsed().as_secs_f64() * 1e6;
        assert_eq!(
            serde_json::from_slice::<Value>(&data).unwrap()["type"],
            "snapshot"
        );
        initial_bytes = data.len();
        if sample >= 3 {
            bootstraps.push(elapsed);
        }
        socket.shutdown().await.unwrap();
    }
    results.push(
        json!({"case":"ws_legacy_initial_snapshot","kind":"loopback_ws",
        "payload_bytes":initial_bytes,"compression":false,"timing":summary(&bootstraps)}),
    );
    for (kind, clients) in [("worker", 1), ("worker", 8), ("page", 1), ("legacy", 1)] {
        let (token, query) = subscription(fixture, kind).await;
        let mut sockets = Vec::new();
        for _ in 0..clients {
            let mut socket = connect(address, &token, &query).await;
            let initial: Value = serde_json::from_slice(&frame(&mut socket).await).unwrap();
            let expected = match kind {
                "worker" => "state_ready",
                "page" => "page_window_ready",
                _ => "snapshot",
            };
            assert_eq!(initial["type"], expected);
            sockets.push(socket);
        }
        let mut samples = Vec::new();
        let mut bytes = 0;
        let mut captured = Value::Null;
        for sample in 0..options.samples + 3 {
            let started = Instant::now();
            let revision = fixture.change().await;
            let mut payloads = Vec::new();
            for socket in &mut sockets {
                payloads.push(frame(socket).await);
            }
            let elapsed = started.elapsed().as_secs_f64() * 1e6;
            bytes = payloads.iter().map(Vec::len).sum();
            for payload in &payloads {
                let value: Value = serde_json::from_slice(payload).unwrap();
                assert!(value["rev"].as_u64().unwrap() >= revision);
                let expected = match kind {
                    "worker" => "state_delta",
                    "page" => "page_window_delta",
                    _ => "snapshot",
                };
                assert_eq!(value["type"], expected, "{value}");
                captured = value;
            }
            if sample >= 3 {
                samples.push(elapsed);
            }
        }
        results.push(json!({"case":format!("ws_{kind}_update_{clients}_clients"),"kind":"loopback_ws",
            "clients":clients,"payload_bytes_total":bytes,"compression":false,"timing":summary(&samples)}));
        if clients == 1 {
            results.push(micro::encode(
                &format!("{kind}_actual_wire_update"),
                &captured,
                options,
            ));
        }
        for socket in &mut sockets {
            socket.shutdown().await.unwrap();
        }
        tokio::task::yield_now().await;
    }
    server.abort();
    results
}
