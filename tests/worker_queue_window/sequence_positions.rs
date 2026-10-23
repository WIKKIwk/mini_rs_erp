use super::*;

async fn connect(
    address: std::net::SocketAddr,
    f: &Fixture,
    baseline: &Value,
    protocol: &str,
    epoch: &str,
) -> tokio::net::TcpStream {
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    let uri = format!(
        "/v1/mobile/admin/production-maps/live?token={}&protocol={protocol}&worker_scope=true&queue_limit=5&queue_positions=true&queue_search=zakaz-003&epoch={epoch}&rev={}&scope={}",
        f.token,
        baseline["rev"],
        baseline["scope"].as_str().unwrap()
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
    socket
}

#[tokio::test]
async fn worker_search_positions_only_delta_is_sparse_replayable_and_epoch_scoped() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = f.router.clone();
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    for protocol in ["state_delta_v1", "state_delta_v2"] {
        let original = (0..60).map(|i| format!("zakaz-{i:03}")).collect::<Vec<_>>();
        f.state
            .production_maps
            .set_apparatus_sequence(CUT, original.clone())
            .await
            .unwrap();
        let (_, before) = f
            .get("worker_scope=true&queue_limit=5&queue_positions=true&queue_search=zakaz-003")
            .await;
        assert_eq!(
            before["sequence_positions_by_apparatus"],
            json!({CUT:{"zakaz-003":4}})
        );
        let epoch = before["epoch"].as_str().unwrap();
        let mut socket = connect(address, &f, &before, protocol, epoch).await;
        let ready = ws_json(&mut socket).await;
        assert_eq!(ready["type"], "state_ready", "{ready}");
        assert_eq!(ready["resync"], false);

        let mut moved = original;
        moved.swap(3, 4);
        f.state
            .production_maps
            .set_apparatus_sequence(CUT, moved)
            .await
            .unwrap();
        let delta = ws_json(&mut socket).await;
        assert_eq!(delta["type"], "state_delta", "{delta}");
        assert_eq!(delta["base_rev"], before["rev"]);
        assert_eq!(delta["epoch"], before["epoch"]);
        assert_eq!(delta["scope"], before["scope"]);
        assert!(delta["rev"].as_u64().unwrap() > before["rev"].as_u64().unwrap());
        assert_eq!(
            delta["patch"]["sequence_positions_by_apparatus"],
            json!({
                "scopes":{CUT:{"upsert":{"zakaz-003":5},"remove":[]}},"remove":[]
            })
        );
        for field in ["maps", "map_order", "sequences", "visible_order_ids"] {
            assert!(
                delta["patch"].get(field).is_none(),
                "rank-only change repeats {field}: {delta}"
            );
        }
        assert!(serde_json::to_vec(&delta).unwrap().len() < 1024, "{delta}");
        let (_, after) = f
            .get("worker_scope=true&queue_limit=5&queue_positions=true&queue_search=zakaz-003")
            .await;
        assert_eq!(
            after["sequence_positions_by_apparatus"],
            json!({CUT:{"zakaz-003":5}})
        );
        assert_eq!(after["visible_order_ids"], before["visible_order_ids"]);
        assert_eq!(after["rev"], delta["rev"]);
        drop(socket);

        // Reconnecting from the old HTTP cursor replays the same sparse patch.
        let mut resumed = connect(address, &f, &before, protocol, epoch).await;
        let replay = ws_json(&mut resumed).await;
        assert_eq!(replay, delta);
        let ready = ws_json(&mut resumed).await;
        assert_eq!(ready["type"], "state_ready");
        assert_eq!(ready["resync"], false);
        assert_eq!(ready["rev"], after["rev"]);
        drop(resumed);
        let mut stale = connect(address, &f, &before, protocol, "stale-epoch").await;
        let ready = ws_json(&mut stale).await;
        assert_eq!(ready["type"], "state_ready");
        assert_eq!(ready["resync"], true);
        drop(stale);
    }
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn worker_search_positions_are_independent_per_apparatus_and_assignment_scoped() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let shared = serde_json::from_value(json!({
        "id":"zakaz-003", "product_code":"zakaz-003", "code":"zakaz-003", "title":"zakaz-003",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"work","kind":"apparatus","title":"Work","apparatus_id":CUT,"rezka_kadr_count":1},
            {"id":"other","kind":"apparatus","title":"Other","apparatus_id":OTHER,"rezka_kadr_count":1},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"work"},{"from":"work","to":"other"},{"from":"other","to":"end"}]
    })).unwrap();
    f.state.production_maps.upsert_map(shared).await.unwrap();
    f.state
        .production_maps
        .set_apparatus_sequence(OTHER, vec!["zakaz-unrelated".into(), "zakaz-003".into()])
        .await
        .unwrap();
    f.state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: "window-worker".into(),
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![CUT.into(), OTHER.into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let (_, both) = f
        .get("worker_scope=true&queue_limit=5&queue_positions=true&queue_search=zakaz-003")
        .await;
    assert_eq!(
        both["sequence_positions_by_apparatus"],
        json!({
            CUT:{"zakaz-003":4}, OTHER:{"zakaz-003":2}
        })
    );
    f.state
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
    let (_, own) = f
        .get("worker_scope=true&queue_limit=5&queue_positions=true&queue_search=zakaz-003")
        .await;
    assert_eq!(
        own["sequence_positions_by_apparatus"],
        json!({CUT:{"zakaz-003":4}})
    );
    assert_ne!(own["scope"], both["scope"]);
}

#[tokio::test]
async fn worker_search_positions_number_after_visibility_and_exclude_frozen_rows() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    f.store
        .put_apparatus_queue_states(CUT, BTreeMap::from([("zakaz-000".into(), "frozen".into())]))
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    let (_, eligible) = f
        .get("worker_scope=true&queue_limit=0&queue_positions=true")
        .await;
    assert!(
        eligible["sequence_positions_by_apparatus"][CUT]
            .get("zakaz-000")
            .is_none()
    );
    assert_eq!(
        eligible["sequence_positions_by_apparatus"][CUT]["zakaz-003"],
        3
    );
    let (_, found) = f
        .get("worker_scope=true&queue_limit=5&queue_positions=true&queue_search=zakaz-003")
        .await;
    assert_eq!(
        found["sequence_positions_by_apparatus"],
        json!({CUT:{"zakaz-003":3}})
    );
    let (_, hidden) = f
        .get("worker_scope=true&queue_limit=5&queue_positions=true&queue_search=zakaz-000")
        .await;
    assert_eq!(hidden["sequence_positions_by_apparatus"], json!({CUT:{}}));
    assert!(hidden["maps"].as_array().unwrap().is_empty());
}
