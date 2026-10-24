use super::*;
use crate::core::authz::RoleAssignmentUpsert;
use crate::core::mini_orders::OrderMaterialTask;
use crate::core::qolip::{
    MemoryQolipStore, QolipBlock, QolipProduct, QolipProductSpec, QolipService, QolipStorePort,
};

const APPARATUS: &str = "apparatus:default:bosma_8";

#[derive(Default)]
struct TaskMaterials {
    reads: AtomicUsize,
    assigned: Mutex<std::collections::BTreeSet<String>>,
    fail: std::sync::atomic::AtomicBool,
}

#[async_trait]
impl MiniOrderSink for TaskMaterials {
    async fn order_material_tasks(
        &self,
        ids: &[String],
        groups: &[String],
    ) -> Result<Vec<OrderMaterialTask>, MiniOrderError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        assert!(ids.len() <= 50);
        assert!(
            groups
                .iter()
                .any(|group| group.eq_ignore_ascii_case("Rulon"))
        );
        assert!(
            !groups
                .iter()
                .any(|group| group.eq_ignore_ascii_case("Kraska"))
        );
        if self.fail.load(Ordering::SeqCst) {
            return Err(MiniOrderError::StoreFailed);
        }
        let assigned = self.assigned.lock().await;
        Ok(ids
            .iter()
            .map(|id| OrderMaterialTask {
                order_id: id.clone(),
                item_code: "PET".into(),
                material: "PET".into(),
                micron: 12.0,
                assigned: assigned.contains(id),
            })
            .collect())
    }
    async fn save_order(
        &self,
        _: &crate::core::production_map::ProductionMapDefinition,
        _: &CalculateOrderTemplate,
    ) -> Result<(), MiniOrderError> {
        unreachable!("task reads must never write")
    }
}

async fn fixture() -> (
    AppState,
    String,
    String,
    Arc<MemoryQolipStore>,
    Arc<TaskMaterials>,
) {
    let mut state = test_state();
    let qolip = Arc::new(MemoryQolipStore::new());
    qolip
        .seed_blocks(vec![QolipBlock {
            name: "A".into(),
            warehouse: "Molds A".into(),
        }])
        .await;
    qolip
        .seed_products(
            (0..7)
                .map(|index| QolipProduct {
                    code: format!("P-{index}"),
                    name: format!("Product {index}"),
                    item_group: "Tayyor mahsulot".into(),
                    ..Default::default()
                })
                .collect(),
        )
        .await;
    state.qolip = QolipService::new(qolip.clone());
    let materials = Arc::new(TaskMaterials::default());
    materials
        .assigned
        .lock()
        .await
        .insert("zakaz-task-0".into());
    state.production_orders = materials.clone();
    state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::MaterialTaminotchi,
            principal_ref: "task-material".into(),
            role_id: "material_taminotchi".into(),
            assigned_apparatus: vec![],
            assigned_item_groups: vec!["Rulon".into()],
        })
        .await
        .unwrap();
    for index in 0..7 {
        let mut map: crate::core::production_map::ProductionMapDefinition =
            serde_json::from_str(&production_order_map_json_with_product(
                &format!("zakaz-task-{index}"),
                &format!("Product {index}"),
                &format!("P-{index}"),
                &format!("{index:04}"),
                APPARATUS,
                1,
                1000.0,
            ))
            .unwrap();
        map.customer_name = format!("Customer {index}");
        state.production_maps.upsert_map(map).await.unwrap();
    }
    state
        .production_maps
        .set_apparatus_sequence(
            APPARATUS,
            (0..7).map(|index| format!("zakaz-task-{index}")).collect(),
        )
        .await
        .unwrap();
    // Queue 0 has a ready visible mold and must not be replaced by item 5
    // when the selected prefix length is five.
    attach(&qolip, "P-0", "Q-0", "Molds A").await;
    let qolip_token = session_for(&state, PrincipalRole::Qolipchi, "task-qolip").await;
    let material_token =
        session_for(&state, PrincipalRole::MaterialTaminotchi, "task-material").await;
    (state, qolip_token, material_token, qolip, materials)
}

async fn attach(store: &MemoryQolipStore, code: &str, qolip_code: &str, warehouse: &str) {
    store
        .put_product_spec(QolipProductSpec {
            warehouse: warehouse.into(),
            item_code: code.into(),
            item_name: code.into(),
            item_group: "Tayyor mahsulot".into(),
            qolip_code: qolip_code.into(),
            size: 40,
            ..Default::default()
        })
        .await
        .unwrap();
}

fn path(kind: &str, suffix: &str) -> String {
    format!("/v1/mobile/production-tasks/window?kind={kind}&apparatus={APPARATUS}&limit=5{suffix}")
}

async fn window(state: &AppState, token: &str, kind: &str, suffix: &str) -> serde_json::Value {
    let response = build_router(state.clone())
        .oneshot(request("GET", &path(kind, suffix), token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

#[tokio::test]
async fn supply_tasks_windows_are_compact_and_preserve_prefix_then_missing_then_search() {
    let (state, qtoken, mtoken, _, _) = fixture().await;
    for (kind, token) in [("qolip", &qtoken), ("material", &mtoken)] {
        let body = window(&state, token, kind, "").await;
        assert_eq!(body["projection"], "supply_tasks_v1");
        assert_eq!(body["checked_count"], 5);
        assert_eq!(body["missing_count"], 4);
        let rows = body["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0]["id"], "zakaz-task-1");
        assert_eq!(rows[0]["position"], 1);
        assert_eq!(rows[0]["customer_name"], "Customer 1");
        assert_eq!(rows[0]["apparatus_count"], 1);
        assert!(
            rows.iter()
                .all(|row| row.get("map").is_none() && row.get("program").is_none())
        );
        for key in [
            "maps",
            "queue_action_controls",
            "completed_orders",
            "materials",
            "products",
        ] {
            assert!(body.get(key).is_none(), "{key}");
        }
        let full = json_body(
            build_router(state.clone())
                .oneshot(request(
                    "GET",
                    "/v1/mobile/admin/production-maps/sequence",
                    token,
                ))
                .await
                .unwrap(),
        )
        .await;
        let full_bytes = serde_json::to_vec(&full).unwrap().len();
        let compact_bytes = serde_json::to_vec(&body).unwrap().len();
        eprintln!(
            "supply_tasks {kind}: full={full_bytes} compact={compact_bytes} row_bytes={} orders=7 window=5",
            serde_json::to_vec(rows).unwrap().len()
        );
        assert!(
            compact_bytes < full_bytes,
            "compact task payload must reduce this representative fixture"
        );
        let filtered = window(&state, token, kind, "&search=PRODUCT%204").await;
        assert_eq!(filtered["rows"].as_array().unwrap().len(), 1);
        assert_eq!(filtered["rows"][0]["position"], 4);
        assert_eq!(filtered["missing_count"], 4);
        let outside = window(&state, token, kind, "&search=Product%205").await;
        assert_eq!(outside["rows"], serde_json::json!([]));
        assert_eq!(outside["missing_count"], 4);
    }
}

#[tokio::test]
async fn supply_tasks_auth_rejects_wrong_role_kind_bad_limits_and_empty_scope_leaks() {
    let (state, qtoken, mtoken, _, materials) = fixture().await;
    let customer = session(&state, PrincipalRole::Customer).await;
    for (kind, token, status) in [
        ("qolip", &mtoken, StatusCode::FORBIDDEN),
        ("material", &qtoken, StatusCode::FORBIDDEN),
        ("qolip", &customer, StatusCode::FORBIDDEN),
        ("material", &"invalid".to_string(), StatusCode::UNAUTHORIZED),
    ] {
        assert_eq!(
            build_router(state.clone())
                .oneshot(request("GET", &path(kind, ""), token))
                .await
                .unwrap()
                .status(),
            status
        );
    }
    for query in [
        "kind=wrong",
        "kind=qolip&limit=6",
        "kind=material&limit=0",
        "kind=qolip&apparatus=not-canonical",
    ] {
        assert_eq!(
            build_router(state.clone())
                .oneshot(request(
                    "GET",
                    &format!("/v1/mobile/production-tasks/window?{query}"),
                    &qtoken
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let before = materials.reads.load(Ordering::SeqCst);
    let unassigned = session_for(&state, PrincipalRole::MaterialTaminotchi, "no-groups").await;
    let body = window(&state, &unassigned, "material", "").await;
    assert_eq!(body["rows"], serde_json::json!([]));
    assert_eq!(body["checked_count"], 5);
    assert_eq!(materials.reads.load(Ordering::SeqCst), before);
}

#[tokio::test]
async fn supply_tasks_cache_is_scoped_by_actor_and_assignments_and_does_not_repeat_task_queries() {
    let (state, _, token, _, materials) = fixture().await;
    let first = window(&state, &token, "material", "").await;
    let second = window(&state, &token, "material", "").await;
    assert_eq!(first, second);
    assert_eq!(materials.reads.load(Ordering::SeqCst), 1);
    state
        .admin
        .delete_role_assignment(&PrincipalRole::MaterialTaminotchi, "task-material")
        .await
        .unwrap();
    // An authorization change invalidates the scope even without a production event.
    let revoked = window(&state, &token, "material", "").await;
    assert_ne!(first["scope"], revoked["scope"]);
    assert_eq!(revoked["rows"], serde_json::json!([]));
    assert_eq!(materials.reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn supply_tasks_qolip_readiness_is_warehouse_scoped_and_failures_are_not_ready() {
    let (state, token, mtoken, store, materials) = fixture().await;
    attach(&store, "P-1", "FOREIGN", "Molds B").await;
    state.production_maps.notify_live();
    assert_eq!(
        window(&state, &token, "qolip", "").await["missing_count"],
        4
    );
    store
        .seed_blocks(vec![QolipBlock {
            name: "B".into(),
            warehouse: "Molds B".into(),
        }])
        .await;
    let changed = window(&state, &token, "qolip", "").await;
    assert_eq!(changed["rows"][0]["id"], "zakaz-task-0");
    assert!(
        changed["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["id"] != "zakaz-task-1")
    );
    materials.fail.store(true, Ordering::SeqCst);
    state.production_maps.notify_live();
    let failed = build_router(state.clone())
        .oneshot(request("GET", &path("material", ""), &mtoken))
        .await
        .unwrap();
    assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn supply_tasks_stale_selection_falls_back_and_qolip_picker_excludes_other_machines() {
    let (state, qtoken, _, _, _) = fixture().await;
    let body = json_body(build_router(state.clone()).oneshot(request("GET", "/v1/mobile/production-tasks/window?kind=qolip&apparatus=apparatus:removed:old&limit=10", &qtoken)).await.unwrap()).await;
    let picker = body["apparatus"].as_array().unwrap();
    assert!(!picker.is_empty());
    assert_eq!(body["apparatus_id"], picker[0]["apparatus_id"]);
    assert!(
        picker
            .iter()
            .all(|item| item["execution_profile"]["operation"] == "print"
                && item["capabilities"].get("tooling").is_some())
    );
    for item in picker {
        assert!(item["source_revision"].as_u64().unwrap() > 0);
        assert_eq!(item["source_aasx_sha256"].as_str().unwrap().len(), 64);
    }
}

#[tokio::test]
async fn supply_tasks_preserve_strict_sequence_and_free_queue_order() {
    use crate::core::apparatus_standard::{
        ApparatusOperationalPolicies, CanonicalApparatusPatch, CanonicalCommandMetadata,
        QueueDiscipline,
    };
    for discipline in [QueueDiscipline::StrictSequence, QueueDiscipline::FreePick] {
        let (state, _, token, _, _) = fixture().await;
        let id = ApparatusId::new(APPARATUS).unwrap();
        let configuration = state
            .apparatus
            .current_configuration(&id)
            .await
            .unwrap()
            .unwrap();
        state
            .apparatus
            .patch(
                id,
                configuration.runtime.source_revision,
                CanonicalApparatusPatch {
                    policies: Some(ApparatusOperationalPolicies {
                        worker_material_assignment_allowed: false,
                        queue: discipline,
                        material: configuration.material.policy.clone(),
                        tooling: configuration.material.tooling.clone(),
                    }),
                    ..Default::default()
                },
                CanonicalCommandMetadata::new("user:admin", "supply-task-policy"),
            )
            .await
            .unwrap();
        state
            .production_maps
            .set_apparatus_sequence(
                APPARATUS,
                (0..7)
                    .rev()
                    .map(|index| format!("zakaz-task-{index}"))
                    .collect(),
            )
            .await
            .unwrap();
        state.production_maps.notify_live();
        let snapshot = state.production_maps.live_snapshot().await.unwrap();
        let expected = if discipline == QueueDiscipline::FreePick {
            snapshot.visible_order_ids[APPARATUS].clone()
        } else {
            crate::core::production_map::queue_state::effective_apparatus_sequence(
                &snapshot.sequences[APPARATUS],
                &snapshot.visible_order_ids[APPARATUS],
            )
        };
        let body = window(&state, &token, "material", "").await;
        for row in body["rows"].as_array().unwrap() {
            assert_eq!(
                row["id"],
                expected[row["position"].as_u64().unwrap() as usize]
            );
        }
        assert_eq!(body["checked_count"], 5);
    }
}

async fn socket(
    state: AppState,
    token: &str,
    kind: &str,
    cursor: Option<&serde_json::Value>,
) -> (tokio::task::JoinHandle<()>, reqwest::Upgraded) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, build_router(state)).await.unwrap();
    });
    let suffix = cursor
        .map(|value| {
            format!(
                "&epoch={}&scope={}&rev={}",
                value["epoch"].as_str().unwrap(),
                value["scope"].as_str().unwrap(),
                value["rev"]
            )
        })
        .unwrap_or_default();
    let response = reqwest::Client::new().get(format!("http://{address}/v1/mobile/production-tasks/live?kind={kind}&apparatus={APPARATUS}&limit=5{suffix}"))
        .bearer_auth(token).header("Connection", "Upgrade").header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13").header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==").send().await.unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    (server, response.upgrade().await.unwrap())
}

async fn live_json(socket: &mut reqwest::Upgraded) -> serde_json::Value {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let opcode = socket.read_u8().await.unwrap() & 0x0f;
            let byte = socket.read_u8().await.unwrap();
            let length = match byte & 0x7f {
                126 => socket.read_u16().await.unwrap() as usize,
                127 => socket.read_u64().await.unwrap() as usize,
                n => n as usize,
            };
            assert!(length < 200_000, "bounded task message");
            let mut payload = vec![0; length];
            socket.read_exact(&mut payload).await.unwrap();
            if opcode == 1 {
                return serde_json::from_slice(&payload).unwrap();
            }
            if opcode == 9 {
                let mut pong = vec![0x8a, 0x80 | length as u8, 0, 0, 0, 0];
                pong.extend(payload);
                socket.write_all(&pong).await.unwrap();
            }
        }
    })
    .await
    .expect("task live event")
}

#[tokio::test]
async fn supply_tasks_live_sends_readiness_delta_without_queue_or_global_payload_changes() {
    let (state, qtoken, mtoken, store, materials) = fixture().await;
    for (kind, token) in [("qolip", qtoken), ("material", mtoken)] {
        let baseline = window(&state, &token, kind, "").await;
        let (server, mut stream) = socket(state.clone(), &token, kind, Some(&baseline)).await;
        let ready = live_json(&mut stream).await;
        assert_eq!(ready["type"], "tasks_ready");
        assert_eq!(ready["version"], baseline["version"]);
        if kind == "qolip" {
            attach(&store, "P-1", "Q-1", "Molds A").await;
        } else {
            materials
                .assigned
                .lock()
                .await
                .insert("zakaz-task-1".into());
        }
        // The queue's maps and execution state stay unchanged; readiness itself
        // must produce a delta immediately, without waiting for a heartbeat.
        state.production_maps.notify_live();
        let update = live_json(&mut stream).await;
        assert_eq!(update["type"], "tasks_delta");
        assert_eq!(update["base_rev"], baseline["rev"]);
        assert_eq!(update["missing_count"], 3);
        assert_eq!(update["remove"], serde_json::json!(["zakaz-task-1"]));
        assert!(update.get("maps").is_none());
        assert!(update.get("window").is_none());
        let latest = window(&state, &token, kind, "").await;
        assert_eq!(update["version"], latest["version"]);
        state.production_maps.notify_live();
        let unchanged = live_json(&mut stream).await;
        assert_eq!(unchanged["type"], "tasks_delta");
        assert_eq!(unchanged["base_rev"], update["rev"]);
        assert_eq!(unchanged["upsert"], serde_json::json!([]));
        assert_eq!(unchanged["remove"], serde_json::json!([]));
        assert_eq!(unchanged["version"], update["version"]);
        server.abort();
    }
}

#[tokio::test]
async fn supply_tasks_live_resets_scope_and_reconnects_with_full_current_window() {
    let (state, _, token, _, _) = fixture().await;
    let baseline = window(&state, &token, "material", "").await;
    let (server, mut stream) = socket(state.clone(), &token, "material", None).await;
    let initial = live_json(&mut stream).await;
    assert_eq!(initial["type"], "tasks_snapshot");
    assert_eq!(initial["window"], baseline);
    state
        .admin
        .delete_role_assignment(&PrincipalRole::MaterialTaminotchi, "task-material")
        .await
        .unwrap();
    state.production_maps.notify_live();
    let revoked = live_json(&mut stream).await;
    assert_eq!(revoked["type"], "tasks_snapshot");
    assert_eq!(revoked["window"]["rows"], serde_json::json!([]));
    assert_ne!(revoked["window"]["scope"], baseline["scope"]);
    server.abort();
    let (server, mut stream) = socket(state, &token, "material", Some(&baseline)).await;
    assert_eq!(live_json(&mut stream).await["type"], "tasks_snapshot");
    server.abort();
}

#[tokio::test]
async fn supply_tasks_live_reports_revoked_session_before_closing() {
    let (state, _, token, _, _) = fixture().await;
    let baseline = window(&state, &token, "material", "").await;
    let (server, mut stream) = socket(state.clone(), &token, "material", Some(&baseline)).await;
    assert_eq!(live_json(&mut stream).await["type"], "tasks_ready");
    state.sessions.delete(&token).await.unwrap();
    state.production_maps.notify_live();
    let denied = live_json(&mut stream).await;
    assert_eq!(denied["ok"], false);
    assert_eq!(denied["error"], "unauthorized");
    assert_eq!(denied["status"], 401);
    server.abort();
}

#[tokio::test]
async fn supply_tasks_material_row_expands_only_selected_authoritative_order_on_demand() {
    let (state, qtoken, token, _, _) = fixture().await;
    let mut tasks = window(&state, &token, "material", "").await;
    let id = tasks["rows"][0]["id"].as_str().unwrap().to_string();
    tasks["rows"][0]["title"] = "Stale cached list title".into();
    let path = format!("/v1/mobile/admin/production-maps?id={id}");
    let response = build_router(state.clone())
        .oneshot(request("GET", &path, &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let detail = json_body(response).await;
    assert_eq!(detail["map"]["id"], id);
    assert_ne!(detail["map"]["title"], tasks["rows"][0]["title"]);
    assert_eq!(detail["map"]["nodes"].as_array().unwrap().len(), 3);
    assert!(detail.get("program").is_some());
    assert!(
        detail.get("maps").is_none(),
        "detail is one order, never a global list"
    );
    let product = build_router(state.clone())
        .oneshot(request_with_body(
            "POST",
            "/v1/mobile/qolip/order-products",
            &qtoken,
            &serde_json::json!({"order_ids":[id]}).to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(product.status(), StatusCode::OK);
    let product = json_body(product).await;
    assert_eq!(product["orders"].as_array().unwrap().len(), 1);
    assert_eq!(product["orders"][0]["order_id"], id);
    assert_eq!(
        product["orders"][0]["product"]["code"],
        detail["map"]["product_code"]
    );
    assert_eq!(
        product["orders"][0]["product"]["item_group"],
        "Tayyor mahsulot"
    );
    let denied = session(&state, PrincipalRole::Customer).await;
    assert_eq!(
        build_router(state)
            .oneshot(request("GET", &path, &denied))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}
