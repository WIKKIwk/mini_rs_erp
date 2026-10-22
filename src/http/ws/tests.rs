use super::*;

use axum::{Router, routing::get};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, timeout};

fn settings(values: &[(&str, &str)]) -> Result<TransportSettings, AppError> {
    TransportSettings::from_lookup(|key| {
        Ok(values
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| (*value).to_owned()))
    })
}

#[test]
fn defaults_enable_negotiated_compression_and_native_heartbeat() {
    let config = low_traffic_config();
    assert_eq!(config.compression, Compression::Dedicated);
    assert!(config.auto_ping);
    assert_eq!(config.ping_interval, 25);
    assert_eq!(config.pong_timeout, 20);
    assert!(config.idle_timeout > config.ping_interval + config.pong_timeout);
    let deflate = config.deflate.unwrap();
    assert_eq!(deflate.server_max_window_bits, 15);
    assert_eq!(deflate.compression_level, 3);
    assert_eq!(deflate.compression_threshold, 128);
    assert!(!deflate.server_no_context_takeover);
}

#[test]
fn invalid_settings_fail_without_silently_disabling_compression() {
    for values in [
        vec![("MINI_ERP_WS_COMPRESSION", "auto")],
        vec![("MINI_ERP_WS_COMPRESSION_LEVEL", "10")],
        vec![("MINI_ERP_WS_COMPRESSION_THRESHOLD", "-1")],
        vec![("MINI_ERP_WS_CONTEXT_TAKEOVER", "yes")],
        vec![("MINI_ERP_WS_WINDOW_BITS", "8")],
        vec![("MINI_ERP_WS_MAX_MESSAGE_BYTES", "0")],
        vec![("MINI_ERP_WS_MAX_FRAME_BYTES", "67108865")],
        vec![("MINI_ERP_WS_MAX_BACKPRESSURE_BYTES", "1")],
        vec![("MINI_ERP_WS_PING_SECONDS", "0")],
        vec![("MINI_ERP_WS_PONG_SECONDS", "0")],
        vec![("MINI_ERP_WS_IDLE_SECONDS", "45")],
        vec![("MINI_ERP_WS_MAX_MESSAGE_BYTES", "1024")],
    ] {
        assert!(
            settings(&values).is_err(),
            "accepted invalid settings: {values:?}"
        );
    }
}

#[test]
fn shared_mode_explicitly_disables_server_context_takeover() {
    let config = settings(&[("MINI_ERP_WS_COMPRESSION", "shared")])
        .unwrap()
        .config();
    assert_eq!(config.compression, Compression::Shared);
    let deflate = config.deflate.unwrap();
    assert!(deflate.server_no_context_takeover);
    assert!(!deflate.client_no_context_takeover);
}

#[test]
fn disabled_mode_has_no_deflate_configuration() {
    let config = settings(&[("MINI_ERP_WS_COMPRESSION", "disabled")])
        .unwrap()
        .config();
    assert_eq!(config.compression, Compression::Disabled);
    assert!(config.deflate.is_none());
}

#[test]
fn prepared_text_conversion_and_cloning_keep_the_same_allocation() {
    let original = "{\"type\":\"state_delta\",\"revision\":123}".to_owned();
    let address = original.as_ptr();
    let text = Utf8Bytes::from(original);
    let duplicate = text.clone();
    assert_eq!(address, text.0.as_ptr());
    assert_eq!(text.0.as_ptr(), duplicate.0.as_ptr());
    let sockudo_ws::Message::Text(bytes) = sockudo_ws::Message::from(Message::Text(text)) else {
        panic!("text must remain a text frame");
    };
    assert_eq!(bytes.as_ptr(), address);
    assert_eq!(
        Message::try_from(sockudo_ws::Message::Text(bytes))
            .unwrap()
            .text(),
        duplicate.as_str()
    );
}

impl Message {
    fn text(&self) -> &str {
        match self {
            Self::Text(text) => text.as_str(),
            _ => panic!("expected text"),
        }
    }
}

#[test]
fn invalid_utf8_never_enters_the_text_cache() {
    let result = Message::try_from(sockudo_ws::Message::Text(Bytes::from_static(&[0xff])));
    assert!(matches!(result, Err(WebSocketError::InvalidText(_))));
}

struct TestServer {
    address: std::net::SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn server() -> TestServer {
    server_with_config(low_traffic_config()).await
}

async fn server_with_config(config: Config) -> TestServer {
    let router = Router::new().route(
        "/echo",
        get(move |ws: WebSocketUpgrade| {
            let config = config.clone();
            async move {
                ws.config(config).on_upgrade(|mut socket| async move {
                    while let Some(Ok(message)) = socket.recv().await {
                        match message {
                            Message::Text(_) | Message::Binary(_) => {
                                if socket.send(message).await.is_err() {
                                    break;
                                }
                            }
                            Message::Close(_) => break,
                            _ => {}
                        }
                    }
                })
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    TestServer { address, task }
}

async fn connect(server: &TestServer, extension: Option<&str>) -> (TcpStream, String) {
    let mut stream = TcpStream::connect(server.address).await.unwrap();
    let extension = extension
        .map(|value| format!("Sec-WebSocket-Extensions: {value}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "GET /echo HTTP/1.1\r\nHost: {}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n{extension}\r\n",
        server.address,
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    timeout(Duration::from_secs(3), async {
        while !response.ends_with(b"\r\n\r\n") {
            response.push(stream.read_u8().await.unwrap());
            assert!(response.len() < 8192);
        }
    })
    .await
    .unwrap();
    let response = String::from_utf8(response).unwrap();
    assert!(response.starts_with("HTTP/1.1 101 "), "{response}");
    (stream, response)
}

async fn send(stream: &mut TcpStream, opcode: u8, compressed: bool, payload: &[u8]) {
    let mut frame = vec![0x80 | if compressed { 0x40 } else { 0 } | opcode];
    if payload.len() < 126 {
        frame.push(0x80 | payload.len() as u8);
    } else {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    let mask = [0x12, 0x34, 0x56, 0x78];
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(i, byte)| byte ^ mask[i % 4]),
    );
    stream.write_all(&frame).await.unwrap();
}

async fn receive(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    timeout(Duration::from_secs(3), async {
        let first = stream.read_u8().await.unwrap();
        let second = stream.read_u8().await.unwrap();
        assert_eq!(second & 0x80, 0, "server frames must not be masked");
        let size = match second & 127 {
            126 => stream.read_u16().await.unwrap() as usize,
            127 => stream.read_u64().await.unwrap() as usize,
            size => size as usize,
        };
        assert!(size <= 128 * 1024);
        let mut payload = vec![0; size];
        stream.read_exact(&mut payload).await.unwrap();
        (first, payload)
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn legacy_client_receives_the_identical_uncompressed_text_contract() {
    let server = server().await;
    let (mut stream, response) = connect(&server, None).await;
    assert!(
        !response
            .to_ascii_lowercase()
            .contains("sec-websocket-extensions:")
    );
    let payload = br#"{"type":"state_delta","revision":1,"patch":{}}"#;
    send(&mut stream, 1, false, payload).await;
    let (frame, echo) = receive(&mut stream).await;
    assert_eq!(frame, 0x81);
    assert_eq!(echo, payload);
    send(&mut stream, 2, false, &[0, 1, 255]).await;
    let (frame, echo) = receive(&mut stream).await;
    assert_eq!(frame, 0x82);
    assert_eq!(echo, [0, 1, 255]);
}

#[tokio::test]
async fn dart_style_negotiation_compresses_json_and_preserves_native_ping_pong() {
    let server = server().await;
    let (mut stream, response) =
        connect(&server, Some("permessage-deflate; client_max_window_bits")).await;
    assert!(
        response
            .to_ascii_lowercase()
            .contains("sec-websocket-extensions: permessage-deflate")
    );
    send(&mut stream, 9, false, b"heartbeat-nonce").await;
    let (frame, pong) = receive(&mut stream).await;
    assert_eq!(frame, 0x8a);
    assert_eq!(pong, b"heartbeat-nonce");

    let mut encoder = sockudo_ws::deflate::DeflateEncoder::new(15, false, 3, 0);
    let mut decoder = sockudo_ws::deflate::DeflateDecoder::new(15, false);
    for revision in 1..=3 {
        let payload = serde_json::json!({"type":"state_delta","revision":revision,
            "patch":{"sequences":{"upsert":{"apparatus":vec!["order-123456789"; 100]}}}})
        .to_string();
        let compressed = encoder.compress(payload.as_bytes()).unwrap().unwrap();
        send(&mut stream, 1, true, &compressed).await;
        let (frame, echo) = receive(&mut stream).await;
        assert_eq!(frame, 0xc1);
        assert!(echo.len() < payload.len() / 2);
        let decoded = decoder.decompress(&echo, 128 * 1024).unwrap();
        assert_eq!(decoded.as_ref(), payload.as_bytes());
    }
}

#[tokio::test]
async fn native_heartbeat_replaces_application_pings_and_enforces_pong_deadlines() {
    let mut config = low_traffic_config();
    config.ping_interval = 1;
    config.pong_timeout = 1;
    config.idle_timeout = 5;
    let server = server_with_config(config).await;
    let (mut stream, _) = connect(&server, None).await;
    let (frame, nonce) = receive(&mut stream).await;
    assert_eq!(frame, 0x89);
    assert!(!nonce.is_empty(), "native Ping must correlate its Pong");
    send(&mut stream, 10, false, &nonce).await;
    let (frame, next_nonce) = receive(&mut stream).await;
    assert_eq!(frame, 0x89, "matching Pong keeps the connection alive");
    assert_ne!(nonce, next_nonce);
    send(&mut stream, 10, false, b"unrelated-pong").await;
    let (frame, close) = receive(&mut stream).await;
    assert_eq!(frame, 0x88, "an unrelated Pong must not clear the deadline");
    assert_eq!(u16::from_be_bytes([close[0], close[1]]), 1001);
}
