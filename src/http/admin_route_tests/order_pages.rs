use super::*;
use crate::core::pending_orders::{
    PendingOrder, PendingOrderCompletion, PendingOrderError, PendingOrderStore,
};
use crate::core::production_map::{ProductionMapStorePort, queue_state};

const PAGE_APPARATUS: &str = "apparatus:default:bosma_8";

struct OrderCreationTimeSink {
    reads: Mutex<Vec<String>>,
}

#[async_trait]
impl MiniOrderSink for OrderCreationTimeSink {
    async fn order_created_at_unix(&self, order_id: &str) -> Result<Option<i64>, MiniOrderError> {
        self.reads.lock().await.push(order_id.to_string());
        Ok(Some(1_793_083_020))
    }

    async fn save_order(
        &self,
        _: &crate::core::production_map::ProductionMapDefinition,
        _: &CalculateOrderTemplate,
    ) -> Result<(), MiniOrderError> {
        unreachable!("reading an order's creation time must not save it")
    }
}

#[tokio::test]
async fn admin_order_detail_loads_creation_time_only_for_the_requested_order() {
    let mut state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let sink = Arc::new(OrderCreationTimeSink {
        reads: Mutex::new(Vec::new()),
    });
    state.production_orders = sink.clone();
    for (id, number) in [("zakaz-created-at", "0101"), ("zakaz-unrelated", "0102")] {
        state
            .production_maps
            .upsert_map(
                serde_json::from_str(&pechat_order_map_json(
                    id, "Creation time", number, PAGE_APPARATUS,
                ))
                .unwrap(),
            )
            .await
            .unwrap();
    }
    let app = build_router(state);
    let list = app
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/order-page?view=orders&projection=orders_list_v1",
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    assert!(
        sink.reads.lock().await.is_empty(),
        "list reads must not load detail metadata"
    );
    let response = app
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/sequence?order_id=zakaz-created-at&detail=true",
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let detail = json_body(response).await;
    assert_eq!(detail["maps"].as_array().unwrap().len(), 1);
    assert_eq!(detail["maps"][0]["map"]["id"], "zakaz-created-at");
    assert_eq!(detail["maps"][0]["created_at_unix"], 1_793_083_020_i64);
    assert_eq!(*sink.reads.lock().await, ["zakaz-created-at"]);
}

fn assert_sequence_positions(page: &serde_json::Value, queue: &[String]) {
    let expected = page["order_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| {
            let id = id.as_str().unwrap();
            (
                id.to_string(),
                queue.iter().position(|item| item == id).unwrap() + 1,
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(page["sequence_positions"], serde_json::json!(expected));
}

struct InvalidatingIntake {
    service: ProductionMapService,
    remaining: AtomicUsize,
    failure: AtomicUsize,
}

impl InvalidatingIntake {
    fn new(service: ProductionMapService, remaining: usize) -> Self {
        Self {
            service,
            remaining: AtomicUsize::new(remaining),
            failure: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl PendingOrderStore for InvalidatingIntake {
    async fn list(&self) -> Result<Vec<PendingOrder>, PendingOrderError> {
        match self.failure.load(Ordering::SeqCst) {
            1 => return Err(PendingOrderError::Store),
            2 => return Err(PendingOrderError::Conflict),
            _ => {}
        }
        // Invalidate exactly the armed number of reads, then become stable.
        if self
            .remaining
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            self.service.notify_live();
        }
        Ok(Vec::new())
    }

    async fn create(
        &self,
        _: PendingOrder,
        _: CalculateOrderImage,
    ) -> Result<PendingOrder, PendingOrderError> {
        unreachable!()
    }

    async fn get(&self, _: &str) -> Result<PendingOrder, PendingOrderError> {
        unreachable!()
    }

    async fn image(&self, _: &str) -> Result<CalculateOrderImage, PendingOrderError> {
        unreachable!()
    }

    async fn complete(
        &self,
        _: &str,
        _: &str,
        _: PendingOrderCompletion,
        _: crate::core::production_map::ProductionMapSaved,
    ) -> Result<(PendingOrderCompletion, bool), PendingOrderError> {
        unreachable!()
    }
}

#[tokio::test]
async fn admin_order_pages_reject_intake_read_invalidated_during_list() {
    let mut state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    state.pending_orders = Some(Arc::new(InvalidatingIntake::new(
        state.production_maps.clone(),
        1,
    )));
    let response = build_router(state)
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/order-page?view=orders",
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(response).await["error"], "order_page_changed");
}

async fn page_window_socket(
    state: AppState,
    windows: serde_json::Value,
    token: &str,
) -> (tokio::task::JoinHandle<()>, reqwest::Upgraded) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, build_router(state)).await.unwrap();
    });
    let response = reqwest::Client::new()
        .get(format!(
            "http://{address}/v1/mobile/admin/production-maps/live?protocol=page_delta_v1&pages={}",
            urlencoding::encode(&windows.to_string()),
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
    (server, response.upgrade().await.unwrap())
}

#[tokio::test]
async fn admin_order_pages_live_retries_transient_initial_and_established_windows() {
    let mut state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let intake = Arc::new(InvalidatingIntake::new(state.production_maps.clone(), 0));
    state.pending_orders = Some(intake.clone());
    // Three invalidated reads exhaust read_window's bounded staging retries.
    intake.remaining.store(3, Ordering::SeqCst);
    let windows = serde_json::json!([
        {"id":"intake", "view":"orders", "length":20},
        {"id":"closed", "view":"closed", "length":20},
    ]);
    let (server, mut socket) = page_window_socket(state.clone(), windows, &token).await;
    let mut baselines = BTreeMap::new();
    for _ in 0..2 {
        let ready = paged_live_json(&mut socket).await;
        assert_eq!(
            ready["type"], "page_window_ready",
            "transient read must defer, not close: {ready}"
        );
        let id = ready["window"].as_str().unwrap().to_string();
        assert!(
            baselines.insert(id, ready.clone()).is_none(),
            "each window becomes ready once"
        );
        assert_eq!(ready["rev"], state.production_maps.snapshot_revision());
    }
    assert_eq!(intake.remaining.load(Ordering::SeqCst), 0);
    assert_eq!(baselines.len(), 2);
    // Repeat the conflict with a retained baseline. Recovery must advance that
    // baseline only after a whole consistent window has been staged.
    intake.remaining.store(3, Ordering::SeqCst);
    state.production_maps.notify_live();
    let mut updates = BTreeMap::new();
    for _ in 0..2 {
        let checkpoint = paged_live_json(&mut socket).await;
        assert_eq!(checkpoint["type"], "page_window_checkpoint", "{checkpoint}");
        let id = checkpoint["window"].as_str().unwrap().to_string();
        let baseline = &baselines[&id];
        assert_eq!(checkpoint["epoch"], baseline["epoch"]);
        assert_eq!(checkpoint["base_version"], baseline["version"]);
        assert!(checkpoint["rev"].as_u64().unwrap() > baseline["rev"].as_u64().unwrap());
        assert_eq!(checkpoint["rev"], state.production_maps.snapshot_revision());
        assert!(checkpoint.get("changes").is_none());
        assert!(updates.insert(id, checkpoint).is_none());
    }
    let app = build_router(state.clone());
    for (id, view) in [("intake", "orders"), ("closed", "closed")] {
        let fresh = json_body(
            app.clone()
                .oneshot(request(
                    "GET",
                    &format!("/v1/mobile/admin/production-maps/order-page?view={view}"),
                    &token,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(updates[id]["version"], fresh["version"]);
        assert_eq!(updates[id]["rev"], fresh["rev"]);
    }
    server.abort();
}

#[tokio::test]
async fn admin_order_pages_live_closes_on_fatal_read_errors() {
    use tokio::io::AsyncReadExt;
    for (failure, expected) in [
        (1, "pending order store failed"),
        (2, "pending_order_conflict"),
    ] {
        let mut state = test_state();
        let token = session(&state, PrincipalRole::Admin).await;
        let intake = Arc::new(InvalidatingIntake::new(state.production_maps.clone(), 0));
        state.pending_orders = Some(intake.clone());
        let windows = serde_json::json!([{"id":"intake", "view":"orders", "length":20}]);
        let (server, mut socket) = page_window_socket(state.clone(), windows, &token).await;
        assert_eq!(
            paged_live_json(&mut socket).await["type"],
            "page_window_ready"
        );
        intake.failure.store(failure, Ordering::SeqCst);
        state.production_maps.notify_live();
        assert_eq!(paged_live_json(&mut socket).await["error"], expected);
        let mut tail = Vec::new();
        tokio::time::timeout(Duration::from_secs(2), socket.read_to_end(&mut tail))
            .await
            .expect("fatal read must close the socket")
            .unwrap();
        server.abort();
    }
}

#[tokio::test]
async fn admin_order_pages_move_includes_unassigned_print_alternatives() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    for index in 0..25 {
        let map = serde_json::from_value(serde_json::json!({
            "id": format!("zakaz-unassigned-{index:04}"),
            "title": "Unassigned print", "product_code": "ALT",
            "nodes": [
                {"id":"start","kind":"start","title":"Start"},
                {"id":"print-7","kind":"apparatus","title":"Print 7",
                 "apparatus_id":"apparatus:default:bosma_7",
                 "alternative_group_id":"print","alternative_group_label":"pechat"},
                {"id":"print-8","kind":"apparatus","title":"Print 8",
                 "apparatus_id":PAGE_APPARATUS,
                 "alternative_group_id":"print","alternative_group_label":"pechat"},
                {"id":"end","kind":"end","title":"End"}
            ],
            "edges": [
                {"from":"start","to":"print-7"},{"from":"print-7","to":"end"},
                {"from":"start","to":"print-8"},{"from":"print-8","to":"end"}
            ]
        }))
        .unwrap();
        state.production_maps.upsert_map(map).await.unwrap();
    }
    let app = build_router(state);
    let response = app.clone().oneshot(request("GET",
        &format!("/v1/mobile/admin/production-maps/order-page?view=move&apparatus=unassigned&target={PAGE_APPARATUS}"),
        &token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page = json_body(response).await;
    assert_eq!(page["total"], 25);
    assert_eq!(page["maps"].as_array().unwrap().len(), 20);
    let response = app.oneshot(request("GET",
        &format!("/v1/mobile/admin/production-maps/order-page?view=sequence&apparatus={PAGE_APPARATUS}"),
        &token)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        json_body(response).await["total"],
        0,
        "unassigned print orders belong only in the transfer pane"
    );
}

#[tokio::test]
async fn admin_order_pages_preserve_order_and_reject_changed_cursors() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    for index in 0..45 {
        let id = format!("zakaz-page-{index:04}");
        let map = serde_json::from_str(&pechat_order_map_json(
            &id,
            &format!("Page order {index:04}"),
            &format!("{index:04}"),
            PAGE_APPARATUS,
        ))
        .unwrap();
        state.production_maps.upsert_map(map).await.unwrap();
    }
    let snapshot = state.production_maps.live_snapshot().await.unwrap();
    let visible = &snapshot.visible_order_ids[PAGE_APPARATUS];
    let expected = if snapshot.queue_policies.iter().any(|p| {
        p.apparatus_id.as_str() == PAGE_APPARATUS
            && p.policy == crate::core::production_map::ApparatusQueuePolicy::FreePick
    }) {
        visible.clone()
    } else {
        queue_state::effective_apparatus_sequence(&snapshot.sequences[PAGE_APPARATUS], visible)
    };
    let url = format!(
        "/v1/mobile/admin/production-maps/order-page?view=sequence&apparatus={PAGE_APPARATUS}"
    );
    let app = build_router(state.clone());
    let orders_response = app
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/order-page?view=orders",
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(orders_response.status(), StatusCode::OK);
    let orders_page = json_body(orders_response).await;
    assert_eq!(orders_page["total"], 45);
    assert_eq!(orders_page["maps"].as_array().unwrap().len(), 20);
    assert_eq!(
        orders_page["order_ids"],
        serde_json::json!(
            snapshot
                .maps
                .iter()
                .take(20)
                .map(|saved| &saved.map.id)
                .collect::<Vec<_>>()
        )
    );
    let first_response = app
        .clone()
        .oneshot(request("GET", &url, &token))
        .await
        .unwrap();
    assert_eq!(first_response.status(), StatusCode::OK);
    let first = json_body(first_response).await;
    assert_eq!(first["maps"].as_array().unwrap().len(), 20);
    assert_eq!(first["total"], 45);
    assert_eq!(first["order_ids"], serde_json::json!(&expected[..20]));
    assert_sequence_positions(&first, &expected);
    let cursor = first["next_cursor"].as_str().unwrap();
    let second = app
        .clone()
        .oneshot(request("GET", &format!("{url}&cursor={cursor}"), &token))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    let second = json_body(second).await;
    assert_eq!(second["order_ids"], serde_json::json!(&expected[20..40]));
    assert_sequence_positions(&second, &expected);
    let tail = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("{url}&cursor={}", second["next_cursor"].as_str().unwrap()),
            &token,
        ))
        .await
        .unwrap();
    let tail = json_body(tail).await;
    assert_eq!(tail["order_ids"], serde_json::json!(&expected[40..]));
    assert_sequence_positions(&tail, &expected);
    assert!(tail["next_cursor"].is_null());
    let conditional = format!("{url}&if_version={}", first["version"].as_str().unwrap());
    assert_eq!(
        app.clone()
            .oneshot(request("GET", &conditional, &token))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_MODIFIED
    );
    assert_eq!(
        app.clone()
            .oneshot(request(
                "GET",
                &format!("{url}&search=0044&cursor={cursor}"),
                &token
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT,
        "cursor must bind its search"
    );
    let search = app
        .clone()
        .oneshot(request("GET", &format!("{url}&search=0044"), &token))
        .await
        .unwrap();
    let search = json_body(search).await;
    assert_eq!(
        search["total"], 1,
        "search must run before the twenty-row limit"
    );
    assert_eq!(search["order_ids"], serde_json::json!(["zakaz-page-0044"]));
    assert_sequence_positions(&search, &expected);

    let matches = expected
        .iter()
        .filter(|id| id.starts_with("zakaz-page-00"))
        .cloned()
        .collect::<Vec<_>>();
    let filtered = json_body(
        app.clone()
            .oneshot(request(
                "GET",
                &format!("{url}&search=zakaz-page-00"),
                &token,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(filtered["order_ids"], serde_json::json!(&matches[..20]));
    assert_sequence_positions(&filtered, &expected);
    let filtered_second = json_body(
        app.clone()
            .oneshot(request(
                "GET",
                &format!(
                    "{url}&search=zakaz-page-00&cursor={}",
                    filtered["next_cursor"].as_str().unwrap()
                ),
                &token,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        filtered_second["order_ids"],
        serde_json::json!(&matches[20..40])
    );
    assert_sequence_positions(&filtered_second, &expected);

    let mut changed = expected.clone();
    changed.rotate_right(1);
    state
        .production_maps
        .set_apparatus_sequence(PAGE_APPARATUS, changed)
        .await
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(request("GET", &format!("{url}&cursor={cursor}"), &token))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT,
        "never mix revisions across pages"
    );
    assert_eq!(
        app.clone()
            .oneshot(request("GET", &conditional, &token))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let worker = session(&state, PrincipalRole::Aparatchi).await;
    assert_eq!(
        app.oneshot(request("GET", &url, &worker))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn admin_order_pages_search_keeps_eligible_queue_positions_in_both_policies() {
    use crate::core::apparatus_standard::{
        ApparatusOperationalPolicies, CanonicalApparatusPatch, CanonicalCommandMetadata,
        QueueDiscipline,
    };
    for discipline in [QueueDiscipline::StrictSequence, QueueDiscipline::FreePick] {
        let state = test_state();
        let token = session(&state, PrincipalRole::Admin).await;
        let apparatus_id = ApparatusId::new(PAGE_APPARATUS).unwrap();
        let current = state
            .apparatus
            .current_configuration(&apparatus_id)
            .await
            .unwrap()
            .unwrap();
        state
            .apparatus
            .patch(
                apparatus_id,
                current.runtime.source_revision,
                CanonicalApparatusPatch {
                    policies: Some(ApparatusOperationalPolicies {
                        worker_material_assignment_allowed: false,
                        queue: discipline,
                        material: current.material.policy.clone(),
                        tooling: current.material.tooling.clone(),
                    }),
                    ..Default::default()
                },
                CanonicalCommandMetadata::new("user:admin", "page-rank-policy"),
            )
            .await
            .unwrap();
        for index in 0..26 {
            let title = if index == 3 || index == 23 {
                "Sparse rank match"
            } else {
                "Other row"
            };
            state
                .production_maps
                .upsert_map(
                    serde_json::from_str(&pechat_order_map_json(
                        &format!("zakaz-rank-{index:04}"),
                        title,
                        &format!("{index:04}"),
                        PAGE_APPARATUS,
                    ))
                    .unwrap(),
                )
                .await
                .unwrap();
        }
        let snapshot = state.production_maps.live_snapshot().await.unwrap();
        let visible = snapshot.visible_order_ids[PAGE_APPARATUS].clone();
        let mut reversed = visible.clone();
        reversed.reverse();
        state
            .production_maps
            .set_apparatus_sequence(PAGE_APPARATUS, reversed.clone())
            .await
            .unwrap();
        let snapshot = state.production_maps.live_snapshot().await.unwrap();
        let visible = &snapshot.visible_order_ids[PAGE_APPARATUS];
        let expected = if discipline == QueueDiscipline::FreePick {
            visible.clone()
        } else {
            queue_state::effective_apparatus_sequence(&snapshot.sequences[PAGE_APPARATUS], visible)
        };
        let url = format!(
            "/v1/mobile/admin/production-maps/order-page?view=sequence&apparatus={PAGE_APPARATUS}&limit=50"
        );
        let app = build_router(state);
        let full = json_body(
            app.clone()
                .oneshot(request("GET", &url, &token))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(full["order_ids"], serde_json::json!(expected));
        assert_sequence_positions(&full, &expected);
        let fourth = &expected[3];
        let searched = json_body(
            app.clone()
                .oneshot(request("GET", &format!("{url}&search={fourth}"), &token))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(searched["order_ids"], serde_json::json!([fourth]));
        assert_eq!(
            searched["sequence_positions"],
            serde_json::json!({fourth:4})
        );
        assert_eq!(searched["maps"].as_array().unwrap().len(), 1);
        let sparse = json_body(
            app.clone()
                .oneshot(request(
                    "GET",
                    &format!("{url}&search=Sparse%20rank%20match"),
                    &token,
                ))
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(sparse["total"], 2);
        assert_eq!(sparse["maps"].as_array().unwrap().len(), 2);
        assert_sequence_positions(&sparse, &expected);
        assert!(
            sparse["sequence_positions"]
                .as_object()
                .unwrap()
                .values()
                .any(|value| value.as_u64().unwrap() > 20)
        );
        let absent = json_body(
            app.oneshot(request(
                "GET",
                &format!("{url}&search=does-not-exist"),
                &token,
            ))
            .await
            .unwrap(),
        )
        .await;
        assert_eq!(absent["sequence_positions"], serde_json::json!({}));
    }
}

#[tokio::test]
async fn admin_order_pages_search_streams_rank_only_reorder_as_bounded_delta() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let ids = (0..25)
        .map(|index| format!("zakaz-rank-live-{index:04}"))
        .collect::<Vec<_>>();
    for (index, id) in ids.iter().enumerate() {
        state
            .production_maps
            .upsert_map(
                serde_json::from_str(&pechat_order_map_json(
                    id,
                    "Rank only live",
                    &format!("{index:04}"),
                    PAGE_APPARATUS,
                ))
                .unwrap(),
            )
            .await
            .unwrap();
    }
    state
        .production_maps
        .set_apparatus_sequence(PAGE_APPARATUS, ids.clone())
        .await
        .unwrap();
    let selected = &ids[3];
    let url = format!(
        "/v1/mobile/admin/production-maps/order-page?view=sequence&apparatus={PAGE_APPARATUS}&search={selected}"
    );
    let app = build_router(state.clone());
    let before = json_body(
        app.clone()
            .oneshot(request("GET", &url, &token))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        before["sequence_positions"],
        serde_json::json!({selected:4})
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let pages = serde_json::json!([{"id":"rank", "view":"sequence", "apparatus":PAGE_APPARATUS,
        "search":selected, "length":20, "version":before["version"]}]);
    let client = reqwest::Client::new();
    let response = client
        .get(format!(
            "http://{address}/v1/mobile/admin/production-maps/live?protocol=page_delta_v1&pages={}",
            urlencoding::encode(&pages.to_string())
        ))
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
    let ready = paged_live_json(&mut socket).await;
    assert_eq!(ready["type"], "page_window_ready");
    assert_eq!(ready["resync"], false);
    assert_eq!(ready["version"], before["version"]);
    let mut reordered = ids.clone();
    reordered.swap(3, 4);
    state
        .production_maps
        .set_apparatus_sequence(PAGE_APPARATUS, reordered)
        .await
        .unwrap();
    let delta = paged_live_json(&mut socket).await;
    assert_eq!(delta["type"], "page_window_delta");
    assert_eq!(delta["epoch"], before["epoch"]);
    assert_eq!(delta["base_version"], before["version"]);
    assert!(delta["rev"].as_u64().unwrap() > before["rev"].as_u64().unwrap());
    assert_eq!(delta["page_count"], 1);
    let changes = delta["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1);
    let ops = changes[0]["ops"].as_array().unwrap();
    assert!(ops.iter().any(|op| op["op"] == "set"
        && op["path"] == serde_json::json!(["sequence_positions", selected])
        && op["value"] == 5));
    assert!(
        ops.iter().all(|op| {
            let path = op["path"].as_array().unwrap();
            !path.is_empty()
                && !matches!(
                    path[0].as_str(),
                    Some("maps" | "order_ids" | "sequences" | "visible_order_ids")
                )
        }),
        "rank-only change must not retransmit unchanged orders or full queues"
    );
    let after: serde_json::Value = client
        .get(format!("http://{address}{url}"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after["version"], delta["version"]);
    assert_eq!(after["sequence_positions"], serde_json::json!({selected:5}));
    for field in ["maps", "order_ids", "sequences", "visible_order_ids"] {
        assert_eq!(before[field], after[field], "{field} did not change");
    }
    server.abort();
}

#[tokio::test]
async fn admin_order_pages_audit_keeps_global_counts_and_pages_findings() {
    let mut state = test_state();
    let store = Arc::new(MemoryProductionMapStore::new());
    state.production_maps = production_map_service_with_store(&state, store.clone());
    store
        .put_apparatus_queue_states(
            PAGE_APPARATUS,
            (0..25)
                .map(|i| (format!("zakaz-missing-{i:04}"), "in_progress".to_string()))
                .collect(),
        )
        .await
        .unwrap();
    let token = session(&state, PrincipalRole::Admin).await;
    let mut reference = state
        .production_maps
        .audit_production_workflow()
        .await
        .unwrap();
    reference.violations.sort_by(|a, b| {
        (&a.order_id, &a.code, &a.subject, &a.detail).cmp(&(
            &b.order_id,
            &b.code,
            &b.subject,
            &b.detail,
        ))
    });
    assert!(reference.violations.len() >= 25);
    let app = build_router(state);
    let first = app
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/order-page?view=audit",
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let page = json_body(first).await;
    assert_eq!(page["total"], reference.violations.len());
    assert_eq!(
        page["workflow_audit"]["total_violations"],
        reference.violations.len()
    );
    assert_eq!(page["workflow_audit"]["ok"], false);
    assert_eq!(
        page["workflow_audit"]["violations"],
        serde_json::json!(&reference.violations[..20])
    );
    let cursor = page["next_cursor"].as_str().unwrap();
    let next = app
        .oneshot(request(
            "GET",
            &format!("/v1/mobile/admin/production-maps/order-page?view=audit&cursor={cursor}"),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(next.status(), StatusCode::OK);
    let next = json_body(next).await;
    assert_eq!(next["offset"], 20);
    assert_eq!(
        next["workflow_audit"]["violations"],
        serde_json::json!(&reference.violations[20..reference.violations.len().min(40)])
    );
}

#[tokio::test]
async fn admin_order_pages_validate_window_and_keep_details_scoped() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let app = build_router(state);
    for query in [
        "view=orders&limit=0",
        "view=orders&limit=1000",
        "view=unknown",
        "view=sequence",
        "view=move&apparatus=unassigned&target=unassigned",
        "view=orders&cursor=broken",
    ] {
        let response = app
            .clone()
            .oneshot(request(
                "GET",
                &format!("/v1/mobile/admin/production-maps/order-page?{query}"),
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
    }
    let response = app
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/order-page?view=closed",
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page = json_body(response).await;
    assert_eq!(page["total"], 0);
    assert!(page["closed_orders"].as_array().unwrap().is_empty());
    assert!(page["next_cursor"].is_null());
}

#[tokio::test]
async fn admin_order_pages_archive_uses_summaries_and_loads_one_history() {
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let app = build_router(state.clone());
    for index in 0..25 {
        let id = format!("zakaz-archive-{index:04}");
        state
            .production_maps
            .upsert_map(
                serde_json::from_str(&pechat_order_map_json(
                    &id,
                    "Archive page",
                    &format!("{index:04}"),
                    PAGE_APPARATUS,
                ))
                .unwrap(),
            )
            .await
            .unwrap();
        let response = app.clone().oneshot(request_with_body("POST",
            "/v1/mobile/admin/production-maps/order-control", &token,
            &serde_json::json!({"order_id": id, "action": "close_early", "comment": "Archive reason"}).to_string(),
        )).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let response = app
        .clone()
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/order-page?view=closed",
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let page = json_body(response).await;
    assert_eq!(page["total"], 25);
    let orders = page["closed_orders"].as_array().unwrap();
    assert_eq!(orders.len(), 20);
    for order in orders {
        assert!(order["logs"].as_array().unwrap().is_empty());
        assert!(order["progress_batches"].as_array().unwrap().is_empty());
    }
    let reference = state
        .production_maps
        .fully_completed_orders(200)
        .await
        .unwrap();
    assert_eq!(
        orders
            .iter()
            .map(|o| o["order_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        reference
            .iter()
            .take(20)
            .map(|o| o.order_id.as_str())
            .collect::<Vec<_>>()
    );
    let id = orders[0]["order_id"].as_str().unwrap();
    let detail = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/v1/mobile/admin/production-maps/closed-order?id={id}"),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(detail.status(), StatusCode::OK);
    let detail = json_body(detail).await;
    assert_eq!(detail["order_id"], id);
    assert_eq!(detail["early_close"]["comment"], "Archive reason");
    let search = app
        .oneshot(request(
            "GET",
            "/v1/mobile/admin/production-maps/order-page?view=closed&search=close_early",
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(json_body(search).await["total"], 25);
}

async fn paged_live_json(socket: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin)) -> serde_json::Value {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let opcode = socket.read_u8().await.unwrap() & 0x0f;
            let byte = socket.read_u8().await.unwrap();
            assert_eq!(byte & 0x80, 0);
            let length = match byte & 0x7f {
                126 => socket.read_u16().await.unwrap() as usize,
                127 => socket.read_u64().await.unwrap() as usize,
                n => n as usize,
            };
            assert!(length < 2048, "paged socket must never send the order list: {length} bytes");
            let mut payload = vec![0; length];
            socket.read_exact(&mut payload).await.unwrap();
            if opcode == 1 { return serde_json::from_slice(&payload).unwrap(); }
            if opcode == 9 {
                let mut pong = vec![0x8a, 0x80 | length as u8, 0, 0, 0, 0];
                pong.extend(payload);
                socket.write_all(&pong).await.unwrap();
            }
        }
    }).await.expect("paged socket event")
}

#[tokio::test]
async fn admin_order_pages_socket_preserves_delta_without_loading_1000_orders() {
    use crate::core::production_map::{QueueActionActor, SequenceMove};
    let state = test_state();
    let token = session(&state, PrincipalRole::Admin).await;
    let worker = session(&state, PrincipalRole::Aparatchi).await;
    for index in 0..1000 {
        state.production_maps.upsert_map(serde_json::from_str(&pechat_order_map_json(
            &format!("zakaz-live-{index:04}"), "Live page", &format!("{index:04}"), PAGE_APPARATUS,
        )).unwrap()).await.unwrap();
    }
    let app = build_router(state.clone());
    let url = format!("/v1/mobile/admin/production-maps/order-page?view=sequence&apparatus={PAGE_APPARATUS}");
    let first = json_body(app.clone().oneshot(request("GET", &url, &token)).await.unwrap()).await;
    assert_eq!(first["total"], 1000);
    assert_eq!(first["maps"].as_array().unwrap().len(), 20);
    let first_id = first["order_ids"][0].as_str().unwrap().to_string();
    let second_id = first["order_ids"][1].as_str().unwrap().to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = reqwest::Client::new();
    let connect = |token: String| client.get(format!("http://{address}/v1/mobile/admin/production-maps/live?protocol=delta&paged=true"))
        .bearer_auth(token).header("Connection", "Upgrade").header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13").header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==");
    assert_eq!(connect(worker).send().await.unwrap().status(), StatusCode::FORBIDDEN);
    let response = connect(token.clone()).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    let mut socket = response.upgrade().await.unwrap();
    let ready = paged_live_json(&mut socket).await;
    assert_eq!(ready["type"], "page_ready");
    assert_eq!(ready["rev"], first["rev"]);
    assert!(ready.get("maps").is_none());
    let moved = state.production_maps.move_apparatus_sequence(SequenceMove {
        apparatus: PAGE_APPARATUS.into(), order_id: first_id.clone(),
        before_order_id: None, after_order_id: Some(second_id.clone()),
        expected_version: first["sequence_versions"][PAGE_APPARATUS].as_str().unwrap().into(),
        idempotency_key: "paged-live-move".into(),
    }, QueueActionActor { role: "admin".into(), ref_: "admin".into(), display_name: "Admin".into() }).await.unwrap();
    let delta = paged_live_json(&mut socket).await;
    assert_eq!(delta["type"], "delta");
    assert_eq!(delta["ops"][0]["id"], first_id);
    assert_eq!(delta["ops"][0]["after_id"], second_id);
    assert_eq!(delta["version"], moved.version);
    assert_eq!(delta["snapshot_rev"], state.production_maps.snapshot_revision());
    assert!(delta.get("maps").is_none());
    let refreshed: serde_json::Value = client.get(format!("http://{address}{url}"))
        .bearer_auth(&token).send().await.unwrap().json().await.unwrap();
    assert_eq!(refreshed["order_ids"][0], second_id);
    assert_eq!(refreshed["order_ids"][1], first_id);
    assert_eq!(refreshed["maps"].as_array().unwrap().len(), 20);
    assert_eq!(refreshed["total"], 1000);
    state.production_maps.notify_live();
    let invalidated = paged_live_json(&mut socket).await;
    assert_eq!(invalidated["type"], "page_invalidated");
    assert!(invalidated.get("maps").is_none());
    drop(socket);
    let mut reconnected = connect(token).send().await.unwrap().upgrade().await.unwrap();
    let ready = paged_live_json(&mut reconnected).await;
    assert_eq!(ready["type"], "page_ready");
    assert_eq!(ready["rev"], invalidated["rev"]);
    assert!(ready.get("maps").is_none());
    server.abort();
}
