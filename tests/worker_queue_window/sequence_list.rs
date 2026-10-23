use super::*;

const PROJECTION: &str = "sequence_list_v1";

fn list_subscription(version: &Value, length: usize) -> String {
    format!(
        "protocol=page_delta_v1&pages={}",
        urlencoding::encode(
            &json!([
                {"id":"list","view":"sequence","apparatus":CUT,"length":length,
                    "version":version,"projection":PROJECTION}
            ])
            .to_string()
        )
    )
}

async fn page_response(f: &Fixture, token: &str, query: &str) -> (StatusCode, Value) {
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

#[tokio::test]
async fn slim_bootstrap_preserves_order_paging_policy_and_isolates_schema_and_accounts() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let token = admin(&f, "list-admin").await;
    let other_token = admin(&f, "other-admin").await;
    let full_query = format!("view=sequence&apparatus={CUT}");
    let query = format!("{full_query}&projection={PROJECTION}");
    let full = page(&f, &token, &full_query).await;
    let slim = page(&f, &token, &query).await;
    assert_eq!(slim["projection"], PROJECTION);
    for key in [
        "order_ids",
        "sequence_positions",
        "total",
        "offset",
        "epoch",
        "rev",
    ] {
        assert_eq!(slim[key], full[key], "{key}");
    }
    assert_eq!(
        slim["sequence_versions"][CUT],
        full["sequence_versions"][CUT]
    );
    assert_eq!(
        slim["sequence_revisions"][CUT],
        full["sequence_revisions"][CUT].as_u64().unwrap_or(0)
    );
    assert_eq!(slim["queue_policies"].as_array().unwrap().len(), 1);
    assert_ne!(slim["scope"], full["scope"]);
    assert_ne!(slim["version"], full["version"]);
    assert_eq!(slim["rows"].as_object().unwrap().len(), 20);
    for id in slim["order_ids"].as_array().unwrap() {
        let id = id.as_str().unwrap();
        let saved = full["maps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|map| map["map"]["id"] == id)
            .unwrap();
        let row = &slim["rows"][id];
        assert_eq!(row["id"], id);
        assert_eq!(row["code"], saved["map"]["code"]);
        assert_eq!(row["title"], saved["map"]["title"]);
        assert_eq!(row["queue_state"], full["queue_states"][CUT][id]);
        assert_eq!(row["display_state"], "pending");
        assert_eq!(row["can_reorder"], true);
    }
    for absent in [
        "maps",
        "program",
        "calculation_templates",
        "queue_action_controls",
        "queue_states",
        "stage_states",
        "order_controls",
        "stage_order_controls",
        "frozen_orders_by_apparatus",
        "order_statuses",
        "order_customers",
        "pending_orders",
    ] {
        assert!(
            slim.get(absent).is_none(),
            "unexpected heavy block {absent}"
        );
    }
    assert_eq!(
        page_response(
            &f,
            &token,
            &format!("{query}&if_version={}", slim["version"].as_str().unwrap())
        )
        .await
        .0,
        StatusCode::NOT_MODIFIED
    );
    // Legacy baseline and cursor cannot certify a different representation.
    assert_eq!(
        page_response(
            &f,
            &token,
            &format!("{query}&if_version={}", full["version"].as_str().unwrap())
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        page_response(
            &f,
            &token,
            &format!("{query}&cursor={}", full["next_cursor"].as_str().unwrap())
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        page_response(
            &f,
            &other_token,
            &format!("{query}&cursor={}", slim["next_cursor"].as_str().unwrap())
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let second = page(
        &f,
        &token,
        &format!("{query}&cursor={}", slim["next_cursor"].as_str().unwrap()),
    )
    .await;
    assert_eq!(second["offset"], 20);
    assert_eq!(second["rows"].as_object().unwrap().len(), 20);
    let search = page(&f, &token, &format!("{query}&search=zakaz-059")).await;
    assert_eq!(search["order_ids"], json!(["zakaz-059"]));
    assert_eq!(search["sequence_positions"]["zakaz-059"], 60);
    assert_eq!(search["rows"]["zakaz-059"]["can_reorder"], false);
    for invalid in [
        format!("{full_query}&projection=unknown"),
        format!("view=orders&projection={PROJECTION}"),
    ] {
        assert_eq!(
            page_response(&f, &token, &invalid).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    // A worker cannot opt into the admin list/detail authority.
    assert_eq!(
        page_response(&f, &f.token, &query).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.get(&format!("apparatus={CUT}&order_id=zakaz-000&detail=true"))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn empty_queue_has_valid_selected_authority_and_free_pick_rows_cannot_drag() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let token = admin(&f, "empty-list").await;
    let empty = f
        .state
        .apparatus
        .list_runtime_projections()
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.apparatus_id.as_str() != CUT && p.apparatus_id.as_str() != OTHER)
        .unwrap();
    let result = page(
        &f,
        &token,
        &format!(
            "view=sequence&apparatus={}&projection={PROJECTION}",
            empty.apparatus_id
        ),
    )
    .await;
    assert_eq!(result["rows"], json!({}));
    assert_eq!(result["order_ids"], json!([]));
    assert_eq!(result["sequence_positions"], json!({}));
    assert_eq!(result["sequence_versions"].as_object().unwrap().len(), 1);
    assert!(
        result["sequence_versions"][empty.apparatus_id.as_str()]
            .as_str()
            .is_some_and(|v| !v.is_empty())
    );
    assert_eq!(result["sequence_revisions"][empty.apparatus_id.as_str()], 0);
    assert_eq!(result["queue_policies"].as_array().unwrap().len(), 1);
    f.free_pick().await;
    let result = page(
        &f,
        &token,
        &format!("view=sequence&apparatus={CUT}&projection={PROJECTION}"),
    )
    .await;
    assert!(
        result["rows"]
            .as_object()
            .unwrap()
            .values()
            .all(|row| row["can_reorder"] == false)
    );
    assert_eq!(
        page_response(
            &f,
            &token,
            &format!("view=sequence&apparatus=apparatus:default:absent&projection={PROJECTION}")
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn slim_unfiltered_page_skips_off_page_customer_and_calculation_archive_reads() {
    use mini_rs_erp::core::calculate_orders::{
        CalculateOrderError, CalculateOrderImage, CalculateOrderStorePort, CalculateOrderTemplate,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Archive {
        reads: AtomicUsize,
    }
    #[async_trait::async_trait]
    impl CalculateOrderStorePort for Archive {
        async fn list(&self, _: &str) -> Result<Vec<CalculateOrderTemplate>, CalculateOrderError> {
            unreachable!()
        }
        async fn list_all(&self) -> Result<Vec<CalculateOrderTemplate>, CalculateOrderError> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            Ok(vec![CalculateOrderTemplate {
                source_map_id: "zakaz-059".into(),
                customer: "Legacy customer".into(),
                ..Default::default()
            }])
        }
        async fn upsert(
            &self,
            _: &str,
            _: CalculateOrderTemplate,
        ) -> Result<CalculateOrderTemplate, CalculateOrderError> {
            unreachable!()
        }
        async fn delete(&self, _: &str, _: &str) -> Result<(), CalculateOrderError> {
            unreachable!()
        }
        async fn save_image(
            &self,
            _: &str,
            _: CalculateOrderImage,
        ) -> Result<CalculateOrderImage, CalculateOrderError> {
            unreachable!()
        }
        async fn get_image(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Option<CalculateOrderImage>, CalculateOrderError> {
            unreachable!()
        }
    }
    let _guard = FIXTURE_LOCK.lock().await;
    let mut f = Fixture::new().await;
    for index in 0..59 {
        let mut value = map(&format!("zakaz-{index:03}"), CUT);
        value.customer_name = "Stored customer".into();
        // Deliberately leave kg/metraj empty: slim rows do not need a full
        // calculation template even if their old order has no stored metrics.
        f.state.production_maps.upsert_map(value).await.unwrap();
    }
    let archive = Arc::new(Archive {
        reads: AtomicUsize::new(0),
    });
    f.state.calculate_orders = archive.clone();
    f.router = build_router(f.state.clone());
    let token = admin(&f, "archive-list").await;
    let query = format!("view=sequence&apparatus={CUT}&projection={PROJECTION}");
    let first = page(&f, &token, &query).await;
    assert_eq!(
        first["rows"]["zakaz-000"]["customer_name"],
        "Stored customer"
    );
    assert_eq!(archive.reads.load(Ordering::Relaxed), 0);
    let searched = page(&f, &token, &format!("{query}&search=Legacy%20customer")).await;
    assert_eq!(searched["order_ids"], json!(["zakaz-059"]));
    assert_eq!(
        searched["rows"]["zakaz-059"]["customer_name"],
        "Legacy customer"
    );
    assert_eq!(archive.reads.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn slim_stream_matches_http_after_colour_checkpoint_reorder_and_reconnect() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let token = admin(&f, "live-list").await;
    let query = format!("view=sequence&apparatus={CUT}&projection={PROJECTION}");
    let mut pages = prefix(&f, &token, &query, 2).await;
    let (address, server) = start(&f).await;
    let (mut socket, upgrade) = connect(
        address,
        &token,
        &list_subscription(&pages[0]["version"], 40),
    )
    .await;
    assert!(upgrade.starts_with("HTTP/1.1 101"), "{upgrade}");
    assert_eq!(ws_json(&mut socket).await["resync"], false);
    f.state.production_maps.notify_live();
    let checkpoint = ws_json(&mut socket).await;
    assert_eq!(checkpoint["type"], "page_window_checkpoint");
    apply(&mut pages, &checkpoint);
    assert_eq!(pages, prefix(&f, &token, &query, 2).await);

    f.store
        .put_apparatus_queue_states(CUT, BTreeMap::from([("zakaz-000".into(), "paused".into())]))
        .await
        .unwrap();
    f.state.production_maps.notify_live();
    let delta = ws_json(&mut socket).await;
    assert_eq!(delta["type"], "page_window_delta");
    assert!(!delta.to_string().contains("queue_action_controls"));
    assert!(!delta.to_string().contains("\"maps\""));
    apply(&mut pages, &delta);
    assert_eq!(pages[0]["rows"]["zakaz-000"]["display_state"], "paused");
    assert_eq!(pages[0]["rows"]["zakaz-000"]["can_reorder"], false);
    assert_eq!(pages, prefix(&f, &token, &query, 2).await);
    let mut ids = (0..60).map(|i| format!("zakaz-{i:03}")).collect::<Vec<_>>();
    ids.rotate_left(25);
    f.state
        .production_maps
        .set_apparatus_sequence(CUT, ids)
        .await
        .unwrap();
    let delta = ws_json(&mut socket).await;
    apply(&mut pages, &delta);
    assert_eq!(pages, prefix(&f, &token, &query, 2).await);
    drop(socket);
    let (mut resumed, _) = connect(
        address,
        &token,
        &list_subscription(&pages[0]["version"], 40),
    )
    .await;
    assert_eq!(ws_json(&mut resumed).await["resync"], false);
    drop(resumed);
    server.abort();
}

#[tokio::test]
async fn admin_detail_is_explicit_single_order_canonical_and_legacy_scoped_get_stays_small() {
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let token = admin(&f, "detail-admin").await;
    let base = format!("apparatus={CUT}&order_id=zakaz-000");
    let (_, legacy) = f.get_as(&token, &base).await;
    assert_eq!(legacy["maps"], json!([]));
    let (status, detail) = f.get_as(&token, &format!("{base}&detail=true")).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["maps"].as_array().unwrap().len(), 1);
    assert_eq!(detail["maps"][0]["map"]["id"], "zakaz-000");
    assert_eq!(detail["projection"], "order_detail_v1");
    assert_eq!(detail["apparatus"], CUT);
    assert_eq!(detail["rev"], f.state.production_maps.snapshot_revision());
    assert!(!detail["scope"].as_str().unwrap().is_empty());
    assert!(detail["queue_action_controls"][CUT]["zakaz-000"].is_object());
    for controls in detail["queue_action_controls"]
        .as_object()
        .unwrap()
        .values()
    {
        assert!(
            controls
                .as_object()
                .unwrap()
                .keys()
                .all(|id| id == "zakaz-000")
        );
    }
    assert_eq!(
        f.get_as(&token, "detail=true").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        f.get_as(&token, &format!("{base}&detail=true&worker_scope=true"))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        f.get_as(
            &token,
            &format!("apparatus={CUT}&order_id=absent&detail=true")
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn report_slim_page_wire_size_and_warm_http_cost() {
    use std::io::Write;
    let _guard = FIXTURE_LOCK.lock().await;
    let f = Fixture::new().await;
    let token = admin(&f, "size-list").await;
    let full_query = format!("view=sequence&apparatus={CUT}");
    let slim_query = format!("{full_query}&projection={PROJECTION}");
    let full = page(&f, &token, &full_query).await;
    let slim = page(&f, &token, &slim_query).await;
    let wire = |v: &Value| {
        let raw = serde_json::to_vec(v).unwrap();
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(&raw).unwrap();
        (raw.len(), gzip.finish().unwrap().len())
    };
    let (full_raw, full_gzip) = wire(&full);
    let (slim_raw, slim_gzip) = wire(&slim);
    let mut full_times = Vec::new();
    let mut slim_times = Vec::new();
    // Alternate to reduce ordering bias; these are memory-store warm HTTP
    // costs, not physical phone/network or cold Postgres latency claims.
    for _ in 0..10 {
        let started = std::time::Instant::now();
        page(&f, &token, &full_query).await;
        full_times.push(started.elapsed().as_micros());
        let started = std::time::Instant::now();
        page(&f, &token, &slim_query).await;
        slim_times.push(started.elapsed().as_micros());
    }
    full_times.sort();
    slim_times.sort();
    eprintln!(
        "sequence_list_v1 fixture=60_orders page=20 full_raw={full_raw} slim_raw={slim_raw} full_gzip={full_gzip} slim_gzip={slim_gzip} warm_full_median_us={} warm_slim_median_us={}",
        full_times[5], slim_times[5]
    );
    assert!(slim_raw < full_raw / 2);
    assert!(slim_gzip < full_gzip);
}
