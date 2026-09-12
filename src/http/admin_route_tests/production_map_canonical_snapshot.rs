use super::*;

#[tokio::test]
async fn live_sockets_answer_client_ping_while_queue_is_quiet() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server =
        tokio::spawn(async move { axum::serve(listener, build_router(state)).await.unwrap() });
    for path in [
        "/v1/mobile/admin/production-maps/live",
        "/v1/mobile/admin/warehouses/live",
    ] {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .bearer_auth(&token)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        let mut socket = response.upgrade().await.unwrap();
        // A masked client Ping with payload 'test'. No application-level event
        // is emitted: the server must continuously drive its WebSocket reader.
        socket
            .write_all(&[0x89, 0x84, 0, 0, 0, 0, b't', b'e', b's', b't'])
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let opcode = socket.read_u8().await.unwrap() & 0x0f;
                let length_byte = socket.read_u8().await.unwrap();
                assert_eq!(length_byte & 0x80, 0, "server frames are unmasked");
                let length = match length_byte & 0x7f {
                    126 => socket.read_u16().await.unwrap() as usize,
                    127 => socket.read_u64().await.unwrap() as usize,
                    n => n as usize,
                };
                assert!(length < 4 * 1024 * 1024, "bounded test snapshot");
                let mut payload = vec![0; length];
                socket.read_exact(&mut payload).await.unwrap();
                if opcode == 0x0a && payload == b"test" {
                    break;
                }
                if opcode == 0x09 {
                    assert!(length <= 125);
                    let mut pong = vec![0x8a, 0x80 | length as u8, 0, 0, 0, 0];
                    pong.extend_from_slice(&payload);
                    socket.write_all(&pong).await.unwrap();
                }
            }
        })
        .await
        .expect("quiet live socket must answer client ping");
        socket.write_all(&[0x88, 0x80, 0, 0, 0, 0]).await.unwrap();
        let mut remaining = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), socket.read_to_end(&mut remaining))
            .await
            .expect("server must release closed stream")
            .unwrap();
    }
    server.abort();
}

#[tokio::test]
async fn sequence_returns_canonical_rev_and_maps() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"zakaz-0002",
                "product_code":"MAGNUS",
                "title":"Magnus",
                "code":"0002",
                "order_number":"0002",
                "customer_name":"Magnus",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"pechat","kind":"apparatus","title":"apparatus:default:bosma_7","apparatus_id":"apparatus:default:bosma_7"},
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"pechat"},
                    {"from":"pechat","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("save map");
    assert_eq!(save.status(), StatusCode::OK);

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &token,
        ))
        .await
        .expect("sequence");
    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await;

    assert_eq!(body["ok"], serde_json::json!(true));
    assert!(body["rev"].is_number(), "sequence must expose rev");
    assert!(body["maps"].is_array(), "sequence must expose maps");
    let maps = body["maps"].as_array().expect("maps array");
    assert!(!maps.is_empty(), "maps must not be empty");
    let saved = maps
        .iter()
        .find(|item| item["map"]["id"] == "zakaz-0002")
        .expect("zakaz-0002 in maps");
    // Title authority: backend never reformats the stored title.
    assert_eq!(saved["map"]["title"], serde_json::json!("Magnus"));
    assert_eq!(saved["map"]["code"], serde_json::json!("0002"));
}

#[tokio::test]
async fn sequence_and_live_share_same_map_contract() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"zakaz-0003",
                "product_code":"MONO",
                "title":"Mono elektrik",
                "code":"0003",
                "order_number":"0003",
                "customer_name":"Mono",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"pechat","kind":"apparatus","title":"apparatus:default:bosma_7","apparatus_id":"apparatus:default:bosma_7"},
                    {"id":"end","kind":"end","title":"Mono 007"}
                ],
                "edges":[
                    {"from":"start","to":"pechat"},
                    {"from":"pechat","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("save map");
    assert_eq!(save.status(), StatusCode::OK);

    let response = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &token,
        ))
        .await
        .expect("sequence");
    let body = json_body(response).await;

    // Live authority uses the same shared snapshot builder.
    let (snapshot, revision) = state
        .production_maps
        .live_snapshot_shared_with_revision()
        .await
        .expect("live snapshot");
    let sequence_maps = body["maps"].as_array().expect("sequence maps");
    assert_eq!(
        sequence_maps.len(),
        snapshot.maps.len(),
        "sequence and live must share maps authority"
    );
    assert_eq!(
        body["rev"].as_u64().expect("rev number"),
        revision,
        "sequence rev must match live revision"
    );
    // order_customers contract present on both paths.
    assert!(body["order_customers"].is_object());
    assert_eq!(
        body["order_customers"]["zakaz-0003"],
        serde_json::json!("Mono")
    );
}

#[tokio::test]
async fn notify_live_bumps_sequence_revision() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    let first = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &token,
        ))
        .await
        .expect("first sequence");
    let first_body = json_body(first).await;
    let first_rev = first_body["rev"].as_u64().expect("first rev");

    state.production_maps.notify_live();

    let second = build_router(state.clone())
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &token,
        ))
        .await
        .expect("second sequence");
    let second_body = json_body(second).await;
    let second_rev = second_body["rev"].as_u64().expect("second rev");

    assert!(
        second_rev > first_rev,
        "notify_live must bump revision ({} -> {})",
        first_rev,
        second_rev
    );
}

#[tokio::test]
async fn sequence_preserves_title_and_legacy_code_fallback() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;

    // Legacy map without `code`: service exposes order_number as code.
    let save = build_router(state.clone())
        .oneshot(request_with_body(
            "PUT",
            "/v1/mobile/admin/production-maps",
            &token,
            r#"{
                "id":"zakaz-legacy",
                "product_code":"LEGACY",
                "title":"Legacy title stays",
                "order_number":"0099",
                "nodes":[
                    {"id":"start","kind":"start","title":"Start"},
                    {"id":"end","kind":"end","title":"End"}
                ],
                "edges":[
                    {"from":"start","to":"end"}
                ]
            }"#,
        ))
        .await
        .expect("save legacy map");
    assert_eq!(save.status(), StatusCode::OK);

    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence",
            &token,
        ))
        .await
        .expect("sequence");
    let body = json_body(response).await;
    let maps = body["maps"].as_array().expect("maps");
    let saved = maps
        .iter()
        .find(|item| item["map"]["id"] == "zakaz-legacy")
        .expect("legacy map present");
    assert_eq!(
        saved["map"]["title"],
        serde_json::json!("Legacy title stays"),
        "title must not be reformatted"
    );
    // Legacy fallback: code mirrors order_number when code was empty.
    assert_eq!(saved["map"]["code"], serde_json::json!("0099"));
    assert_eq!(saved["map"]["order_number"], serde_json::json!("0099"));
}
