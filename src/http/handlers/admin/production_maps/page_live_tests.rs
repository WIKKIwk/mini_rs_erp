use super::*;

fn window(value: Value, version: &str) -> Window {
    Window {
        pages: vec![value],
        version: version.into(),
        epoch: "epoch".into(),
        revision: 1,
        bytes: 1,
        created: Instant::now(),
        prepared: Mutex::new(BTreeMap::new()),
        prepared_bytes: AtomicUsize::new(0),
    }
}

#[test]
fn sparse_colour_patch_keeps_maps_and_explicit_nulls() {
    let before = json!({"maps":[{"id":"a","program":"x".repeat(10000)}],
        "queue_states":{"press":{"a":"pending"}},"optional":"old","removed":true});
    let after = json!({"maps":[{"id":"a","program":"x".repeat(10000)}],
        "queue_states":{"press":{"a":"paused"}},"optional":null});
    let mut ops = Vec::new();
    diff(&before, &after, &mut Vec::new(), &mut ops);
    assert_eq!(ops.len(), 3);
    assert!(ops.iter().all(|op| op["path"][0] != "maps"));
    assert!(
        ops.iter()
            .any(|op| op["op"] == "set" && op["value"].is_null())
    );
    assert!(ops.iter().any(|op| op["op"] == "remove"));
    assert!(json!(ops).to_string().len() < 512);
}

#[tokio::test]
async fn checkpoint_advances_cursors_without_repeating_content() {
    let a = window(
        json!({"maps":[],"version":"old","rev":1,"epoch":"epoch","next_cursor":"a"}),
        "old",
    );
    let b = window(
        json!({"maps":[],"version":"new","rev":2,"epoch":"epoch","next_cursor":"b"}),
        "new",
    );
    let first = prepared(&a, &b, "1").await;
    let second = prepared(&a, &b, "1").await;
    assert!(Arc::ptr_eq(&first, &second));
    let value: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(value["type"], "page_window_checkpoint");
    assert_eq!(value["next_cursors"], json!(["b"]));
    assert!(value.get("changes").is_none());
}

#[test]
fn subscriptions_are_bounded_and_ids_unique() {
    assert!(requests("[]").is_err());
    assert!(requests(r#"[{"id":"x","view":"orders","length":10001}]"#).is_err());
    assert!(
        requests(
            r#"[{"id":"x","view":"orders","length":20},{"id":"x","view":"closed","length":20}]"#
        )
        .is_err()
    );
    assert!(
        requests(r#"[{"id":"1","view":"sequence","apparatus":"apparatus:a","length":40}]"#)
            .is_ok()
    );
}

#[tokio::test]
async fn oversized_patches_require_resync_and_cached_variants_are_bounded() {
    let a = window(json!({"maps":[]}), "old");
    let b = window(
        json!({"maps":[{"program":"x".repeat(MAX_DELTA_BYTES)}]}),
        "new",
    );
    let frame = prepared(&a, &b, "1").await;
    let message: Value = serde_json::from_str(&frame).unwrap();
    assert_eq!(message["type"], "page_window_resync");
    assert!(frame.len() < 1024);
    for id in 0..100 {
        prepared(&a, &b, &id.to_string()).await;
    }
    let variants = b.prepared.lock().await;
    assert!(variants.len() <= 8);
    assert!(variants.values().map(|value| value.len()).sum::<usize>() <= MAX_DELTA_BYTES);
    assert_eq!(
        b.retained_bytes(),
        b.bytes + variants.values().map(|value| value.len()).sum::<usize>()
    );
}

#[tokio::test]
async fn active_subscription_registry_and_expired_recovery_are_bounded() {
    let hub = PageLiveHub::default();
    let mut active = Vec::new();
    for index in 0..MAX_FEEDS + 16 {
        active.push(hub.subscribe(index.to_string()).await);
    }
    assert_eq!(hub.feeds.lock().await.len(), MAX_FEEDS);
    assert!(Arc::ptr_eq(&active[0], &hub.subscribe("0".into()).await));
    let mut expired = window(json!({}), "expired");
    expired.created = Instant::now() - HISTORY_TTL * 2;
    let feed = Feed {
        current: Some(Arc::new(expired)),
        history: VecDeque::new(),
    };
    assert!(feed.version("expired").is_none());
    assert!(feed.version("unknown").is_none());
    assert!(feed.version("").is_none());
}

#[tokio::test]
async fn stalled_shared_projection_releases_its_lock_for_safe_retry() {
    let lock = Mutex::new(());
    let guard = lock.lock().await;
    let error = read_before_deadline(
        tokio::time::Instant::now() + Duration::from_millis(5),
        async move {
            let _guard = guard;
            std::future::pending::<Result<(), AdminError>>().await
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.0, StatusCode::CONFLICT);
    assert_eq!(json!(error.1.0)["error"], "order_page_changed");
    assert!(lock.try_lock().is_ok());
}
