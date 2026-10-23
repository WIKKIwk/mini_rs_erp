use super::*;

#[path = "sequence_list.rs"]
mod sequence_list;

#[path = "orders_list.rs"]
mod orders_list;

#[path = "http_compression.rs"]
mod http_compression;

async fn admin(f: &Fixture, name: &str) -> String {
    f.state
        .sessions
        .create(Principal {
            role: PrincipalRole::Admin,
            ref_: name.into(),
            display_name: name.into(),
            legal_name: String::new(),
            phone: String::new(),
            avatar_url: String::new(),
        })
        .await
        .unwrap()
}

async fn page(f: &Fixture, token: &str, query: &str) -> Value {
    let response = f
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/mobile/admin/production-maps/order-page?{query}"
                ))
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    serde_json::from_slice(
        &to_bytes(response.into_body(), 10 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}

async fn prefix(f: &Fixture, token: &str, query: &str, count: usize) -> Vec<Value> {
    let mut result = vec![page(f, token, query).await];
    while result.len() < count {
        let Some(cursor) = result.last().unwrap()["next_cursor"].as_str() else {
            break;
        };
        result.push(
            page(
                f,
                token,
                &format!("{query}&cursor={}", urlencoding::encode(cursor)),
            )
            .await,
        );
    }
    result
}

async fn start(f: &Fixture) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = f.router.clone();
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (address, task)
}

async fn connect(
    address: std::net::SocketAddr,
    token: &str,
    query: &str,
) -> (tokio::net::TcpStream, String) {
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let uri = format!("/v1/mobile/admin/production-maps/live?token={token}&{query}");
    socket.write_all(format!("GET {uri} HTTP/1.1\r\nHost: {address}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").as_bytes()).await.unwrap();
    let mut response = Vec::new();
    while !response.ends_with(b"\r\n\r\n") {
        response.push(socket.read_u8().await.unwrap());
        assert!(response.len() < 8192);
    }
    (socket, String::from_utf8(response).unwrap())
}

fn subscription(version: &Value, length: usize) -> String {
    format!(
        "protocol=page_delta_v1&pages={}",
        urlencoding::encode(
            &json!([{"id":"1","view":"sequence","apparatus":CUT,
                "length":length,"version":version}])
            .to_string()
        )
    )
}

fn apply_operation(target: &mut Value, op: &Value) {
    let path = op["path"].as_array().unwrap();
    if path.is_empty() {
        assert_eq!(op["op"], "set");
        *target = op["value"].clone();
        return;
    }
    let mut cursor = target;
    for key in &path[..path.len() - 1] {
        cursor = if let Some(index) = key.as_u64() {
            &mut cursor[index as usize]
        } else {
            &mut cursor[key.as_str().unwrap()]
        };
    }
    let last = path.last().unwrap();
    if op["op"] == "remove" {
        cursor
            .as_object_mut()
            .unwrap()
            .remove(last.as_str().unwrap());
    } else if let Some(index) = last.as_u64() {
        cursor[index as usize] = op["value"].clone();
    } else {
        cursor[last.as_str().unwrap()] = op["value"].clone();
    }
}

fn apply(pages: &mut Vec<Value>, frame: &Value) {
    assert_eq!(frame["base_version"], pages[0]["version"]);
    if frame["type"] == "page_window_checkpoint" {
        assert_eq!(frame["next_cursors"].as_array().unwrap().len(), pages.len());
        for (index, page) in pages.iter_mut().enumerate() {
            page["version"] = frame["version"].clone();
            page["epoch"] = frame["epoch"].clone();
            page["rev"] = frame["rev"].clone();
            page["next_cursor"] = frame["next_cursors"][index].clone();
        }
    } else {
        assert_eq!(frame["type"], "page_window_delta", "{frame}");
        pages.resize(frame["page_count"].as_u64().unwrap() as usize, Value::Null);
        for change in frame["changes"].as_array().unwrap() {
            let target = &mut pages[change["index"].as_u64().unwrap() as usize];
            for op in change["ops"].as_array().unwrap() {
                apply_operation(target, op);
            }
        }
    }
}

#[tokio::test]
async fn page_stream_preserves_two_page_boundary_colours_order_and_recovery() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let token = admin(&f, "admin").await;
    let query = format!("view=sequence&apparatus={CUT}");
    let mut pages = prefix(&f, &token, &query, 2).await;
    assert_eq!(pages.len(), 2);
    let (address, server) = start(&f).await;
    let (mut socket, upgrade) =
        connect(address, &token, &subscription(&pages[0]["version"], 40)).await;
    assert!(upgrade.starts_with("HTTP/1.1 101"), "{upgrade}");
    assert_eq!(ws_json(&mut socket).await["resync"], false);

    // No-op revisions still advance every cursor, allowing page append without
    // throwing away the already loaded prefix.
    f.state.production_maps.notify_live();
    let checkpoint = ws_json(&mut socket).await;
    assert_eq!(checkpoint["type"], "page_window_checkpoint");
    apply(&mut pages, &checkpoint);
    assert_eq!(pages, prefix(&f, &token, &query, 2).await);
    let cursor = pages[1]["next_cursor"].as_str().unwrap();
    let third = page(&f, &token, &format!("{query}&cursor={cursor}")).await;
    assert_eq!(third["offset"], 40);

    // A colour-changing queue state can also affect priority and button
    // controls. Compare the complete two-page projection atomically.
    f.store
        .put_apparatus_queue_states(CUT, BTreeMap::from([("zakaz-038".into(), "paused".into())]))
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    let delta = ws_json(&mut socket).await;
    assert_eq!(delta["type"], "page_window_delta");
    apply(&mut pages, &delta);
    assert_eq!(pages, prefix(&f, &token, &query, 2).await);
    assert!(
        pages
            .iter()
            .any(|p| p["queue_states"][CUT]["zakaz-038"] == "paused")
    );

    // Reordering across the 20-row boundary must not leave a duplicate/missing
    // row in any consumer's retained prefix.
    let mut ids = (0..60).map(|i| format!("zakaz-{i:03}")).collect::<Vec<_>>();
    ids.rotate_left(25);
    f.state
        .production_maps
        .set_apparatus_sequence(CUT, ids)
        .await
        .unwrap();
    apply(&mut pages, &ws_json(&mut socket).await);
    assert_eq!(pages, prefix(&f, &token, &query, 2).await);
    let rows = pages
        .iter()
        .flat_map(|p| p["order_ids"].as_array().unwrap())
        .collect::<Vec<_>>();
    let unique = rows
        .iter()
        .map(|row| row.as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(rows.len(), 40);
    assert_eq!(unique.len(), 40);

    // Keep an older version, disconnect, and reconnect after an update. The
    // bounded feed history composes the canonical latest patch before ready.
    let saved_version = pages[0]["version"].clone();
    drop(socket);
    let mut changed = map("zakaz-035", CUT);
    changed.title = "Changed while disconnected".into();
    f.state.production_maps.upsert_map(changed).await.unwrap();
    let (mut socket, upgrade) = connect(address, &token, &subscription(&saved_version, 40)).await;
    assert!(upgrade.starts_with("HTTP/1.1 101"));
    apply(&mut pages, &ws_json(&mut socket).await);
    let ready = ws_json(&mut socket).await;
    assert_eq!(ready["type"], "page_window_ready");
    assert_eq!(ready["resync"], false);
    assert_eq!(pages, prefix(&f, &token, &query, 2).await);
    server.abort();
}

#[tokio::test]
async fn page_stream_rejects_worker_and_foreign_cursor_and_supports_shared_admin_sessions() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let first = admin(&f, "first-admin").await;
    let second = admin(&f, "second-admin").await;
    let query = format!("view=sequence&apparatus={CUT}");
    let first_pages = prefix(&f, &first, &query, 1).await;
    let mut second_pages = prefix(&f, &second, &query, 1).await;
    assert_ne!(first_pages[0]["version"], second_pages[0]["version"]);
    let (address, server) = start(&f).await;
    let (_, denied) = connect(
        address,
        &f.token,
        &subscription(&first_pages[0]["version"], 20),
    )
    .await;
    assert!(denied.starts_with("HTTP/1.1 403"), "{denied}");
    let (mut foreign, _) = connect(
        address,
        &second,
        &subscription(&first_pages[0]["version"], 20),
    )
    .await;
    assert_eq!(ws_json(&mut foreign).await["resync"], true);
    let (mut a, _) = connect(
        address,
        &second,
        &subscription(&second_pages[0]["version"], 20),
    )
    .await;
    let (mut b, _) = connect(
        address,
        &second,
        &subscription(&second_pages[0]["version"], 20),
    )
    .await;
    assert_eq!(ws_json(&mut a).await["resync"], false);
    assert_eq!(ws_json(&mut b).await["resync"], false);
    f.state.production_maps.notify_live();
    let a_frame = ws_json(&mut a).await;
    let b_frame = ws_json(&mut b).await;
    assert_eq!(a_frame, b_frame);
    apply(&mut second_pages, &a_frame);
    assert_eq!(second_pages, prefix(&f, &second, &query, 1).await);
    server.abort();
}

#[tokio::test]
async fn notification_stream_omits_maps_and_suppresses_unrelated_revisions() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let token = admin(&f, "admin").await;
    let (address, server) = start(&f).await;
    let (mut socket, upgrade) = connect(address, &token, "protocol=notifications_v1").await;
    assert!(upgrade.starts_with("HTTP/1.1 101"));
    let initial = ws_json(&mut socket).await;
    assert_eq!(initial["type"], "notifications_snapshot");
    assert_eq!(initial["completion_requests"], json!([]));
    assert!(initial.get("maps").is_none());
    assert!(initial.to_string().len() < 300);
    for _ in 0..4 {
        f.state.production_maps.notify_live();
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(100), ws_json(&mut socket))
            .await
            .is_err()
    );
    f.store
        .append_apparatus_queue_action_event(ApparatusQueueActionEvent {
            event_id: "completion-request".into(),
            apparatus: CUT.into(),
            order_id: "zakaz-001".into(),
            stage_node_id: "work".into(),
            action: queue_state::ApparatusQueueAction::Complete,
            from_state: queue_state::ApparatusQueueOrderState::InProgress,
            to_state: queue_state::ApparatusQueueOrderState::Paused,
            policy: ApparatusQueuePolicy::StrictSequence,
            actor: QueueActionActor {
                role: "aparatchi".into(),
                ref_: "window-worker".into(),
                display_name: "Worker".into(),
            },
            assigned_apparatus: vec![CUT.into()],
            payload_json: json!({"completion_request":true,"description":"Needs approval"}),
        })
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    let changed = ws_json(&mut socket).await;
    assert_eq!(
        changed["completion_requests"][0]["event_id"],
        "completion-request"
    );
    assert_eq!(
        changed["completion_requests"],
        json!(
            f.state
                .production_maps
                .completion_requests(200)
                .await
                .unwrap()
        )
    );
    assert!(changed.get("maps").is_none());
    let (mut worker, upgrade) = connect(address, &f.token, "protocol=notifications_v1").await;
    assert!(upgrade.starts_with("HTTP/1.1 101"));
    assert_eq!(ws_json(&mut worker).await["completion_requests"], json!([]));
    server.abort();
}

#[tokio::test]
async fn page_stream_filters_search_and_updates_both_transfer_panes() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let token = admin(&f, "admin").await;
    let left_query = format!("view=sequence&apparatus={CUT}&search=zakaz-03");
    let right_query = format!("view=sequence&apparatus={OTHER}");
    let mut left = prefix(&f, &token, &left_query, 1).await;
    let mut right = prefix(&f, &token, &right_query, 1).await;
    assert_eq!(left[0]["total"], 10);
    assert_eq!(right[0]["total"], 1);
    let windows = json!([
        {"id":"left","view":"sequence","apparatus":CUT,"search":"zakaz-03",
         "length":20,"version":left[0]["version"]},
        {"id":"right","view":"sequence","apparatus":OTHER,
         "length":20,"version":right[0]["version"]},
    ]);
    let (address, server) = start(&f).await;
    let (mut socket, upgrade) = connect(
        address,
        &token,
        &format!(
            "protocol=page_delta_v1&pages={}",
            urlencoding::encode(&windows.to_string())
        ),
    )
    .await;
    assert!(upgrade.starts_with("HTTP/1.1 101"));
    assert_eq!(ws_json(&mut socket).await["resync"], false);
    assert_eq!(ws_json(&mut socket).await["resync"], false);
    // Moving the same order to another apparatus alters both views. Applying
    // both server projections must remove it on the left and insert it on the
    // right, without leaking orders outside the active search.
    f.state
        .production_maps
        .upsert_map(map("zakaz-035", OTHER))
        .await
        .unwrap();
    for _ in 0..2 {
        let frame = ws_json(&mut socket).await;
        match frame["window"].as_str().unwrap() {
            "left" => apply(&mut left, &frame),
            "right" => apply(&mut right, &frame),
            other => panic!("unexpected window {other}"),
        }
    }
    assert_eq!(left, prefix(&f, &token, &left_query, 1).await);
    assert_eq!(right, prefix(&f, &token, &right_query, 1).await);
    assert_eq!(left[0]["total"], 9);
    assert_eq!(right[0]["total"], 2);
    assert!(
        right[0]["order_ids"]
            .as_array()
            .unwrap()
            .contains(&json!("zakaz-035"))
    );
    server.abort();
}
