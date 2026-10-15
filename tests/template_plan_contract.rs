#![cfg(feature = "verification")]
//! Synthetic SKU/template contracts through the real router and both stores.
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use mini_rs_erp::{
    app::AppState,
    config::AppConfig,
    core::{
        auth::models::{Principal, PrincipalRole},
        calculate_orders::{CalculateOrderStorePort, CalculateOrderTemplate},
        session::manager::SessionManager,
    },
    http::router::build_router,
    store::calculate_order_store::CalculateOrderStore,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc, time::Duration};
use tower::ServiceExt;

const CUT: &str = "apparatus:default:asset-010";
const SAVE: &str = "/v1/mobile/admin/production-maps/with-order";
fn template() -> Value {
    json!({
        "id":"T-A", "code":"Q-A", "name":"Product A", "item_code":"SKU-A", "product":"Product A", "source_map_id":"MAP-A",
        "frame_product_size_mm":132, "frame_count":5, "width_mm":675, "roll_count":4,
        "first_layer_material":"PET", "first_layer_micron":"12"
    })
}
fn map(id: &str, count: i64, groups: Value) -> Value {
    json!({
        "id":id, "product_code":"SKU-A", "title":"Synthetic cutting plan", "width_mm":675, "roll_count":4,
        "nodes":[{"id":"start","kind":"start","title":"Start"},
          {"id":"cut","kind":"apparatus","title":"Rezka A","apparatus_id":CUT,"rezka_kadr_count":count,"rezka_frame_groups":groups},
          {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"cut"},{"from":"cut","to":"end"}]
    })
}
fn config(p: &Path) -> AppConfig {
    AppConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        default_target_warehouse: String::new(),
        http_timeout: Duration::from_secs(2),
        session_store_path: p.join("sessions"),
        profile_store_path: p.join("profiles"),
        push_token_store_path: p.join("push"),
        session_ttl_seconds: Some(3600),
        supplier_prefix: "10".into(),
        werka_prefix: "20".into(),
        werka_code: String::new(),
        werka_name: String::new(),
        werka_phone: String::new(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: String::new(),
        material_taminotchi_phone: String::new(),
        admin_phone: String::new(),
        admin_name: String::new(),
        admin_code: String::new(),
    }
}
async fn fixture() -> (tempfile::TempDir, AppState, String) {
    let dir = tempfile::tempdir().unwrap();
    static BASE: tokio::sync::OnceCell<(tempfile::TempDir, AppState)> =
        tokio::sync::OnceCell::const_new();
    let (_, base) = BASE
        .get_or_init(|| async {
            let directory = tempfile::tempdir().unwrap();
            let state = AppState::verification(config(directory.path()));
            (directory, state)
        })
        .await;
    let mut state = base.clone();
    state.production_maps = mini_rs_erp::core::production_map::ProductionMapService::new(
        Arc::new(mini_rs_erp::core::production_map::MemoryProductionMapStore::new()),
        Arc::new(
            mini_rs_erp::core::production_map::CanonicalServiceApparatusResolver::new(
                state.apparatus.clone(),
            ),
        ),
    );
    state.sessions = SessionManager::memory(Some(3600));
    state.calculate_orders =
        Arc::new(CalculateOrderStore::open(dir.path().join("templates.sqlite")).unwrap());
    state.apparatus.bootstrap_factory_defaults().await.unwrap();
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Admin,
            ref_: "plan-admin".into(),
            display_name: "Plan admin".into(),
            legal_name: String::new(),
            avatar_url: String::new(),
            phone: String::new(),
        })
        .await
        .unwrap();
    (dir, state, token)
}
async fn call(
    state: &AppState,
    token: &str,
    method: &str,
    path: &str,
    payload: Value,
) -> (StatusCode, Value) {
    let response = build_router(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn explicit_cut_plan_mismatch_has_node_details_and_no_writes() {
    let (_dir, state, token) = fixture().await;
    let source = map("MAP-A", 4, json!([1, 1, 1, 1]));
    let original = state
        .production_maps
        .upsert_map(serde_json::from_value(source.clone()).unwrap())
        .await
        .unwrap();
    let mut order = source.clone();
    order["id"] = json!("zakaz-draft-plan-regression");
    let (status, error) = call(
        &state,
        &token,
        "PUT",
        SAVE,
        json!({"map":order,"template":template()}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(error["error"], "rezka_frame_groups_invalid");
    assert!(
        error["message"].as_str().unwrap_or("").contains("5"),
        "specific reason required: {error}"
    );
    assert_eq!(error["cutting_plan_conflicts"][0]["node_id"], "cut");
    assert_eq!(error["cutting_plan_conflicts"][0]["planned_frames"], 4);
    assert_eq!(error["cutting_plan_conflicts"][0]["required_frames"], 5);
    assert_eq!(state.production_maps.maps().await.unwrap().len(), 1);
    assert_eq!(
        state
            .production_maps
            .map("MAP-A")
            .await
            .unwrap()
            .unwrap()
            .map,
        original.map
    );
    assert!(
        state
            .calculate_orders
            .list("admin:plan-admin")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn template_identity_cannot_be_retargeted_by_id_or_code() {
    let dir = tempfile::tempdir().unwrap();
    let store = CalculateOrderStore::open(dir.path().join("templates.sqlite")).unwrap();
    assert_identity_contract(&store).await;
}

#[test]
fn sqlite_parallel_store_instances_cannot_replace_another_owner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("parallel.sqlite");
    let first = CalculateOrderStore::open(&path).unwrap();
    let second = CalculateOrderStore::open(&path).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let tasks = [first, second]
        .into_iter()
        .enumerate()
        .map(|(index, store)| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let mut t: CalculateOrderTemplate = serde_json::from_value(template()).unwrap();
                t.item_code = format!("SKU-{index}");
                barrier.wait();
                runtime
                    .block_on(store.upsert(&format!("owner:{index}"), t))
                    .is_ok()
            })
        })
        .collect::<Vec<_>>();
    let successes = tasks
        .into_iter()
        .map(|task| task.join().unwrap())
        .filter(|ok| *ok)
        .count();
    assert_eq!(
        successes, 1,
        "only one owner can claim a new global identity"
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    assert_eq!(
        runtime
            .block_on(CalculateOrderStore::open(&path).unwrap().list_all())
            .unwrap()
            .len(),
        1
    );
}
async fn assert_identity_contract(store: &dyn CalculateOrderStorePort) {
    let a: CalculateOrderTemplate = serde_json::from_value(template()).unwrap();
    let saved = store.upsert("owner:A", a.clone()).await.unwrap();
    for (id, code) in [("T-A", "Q-A"), ("", "Q-A"), ("T-A", "Q-B")] {
        let mut b = a.clone();
        b.id = id.into();
        b.code = code.into();
        b.item_code = "SKU-B".into();
        b.product = "Product B".into();
        assert!(
            store.upsert("owner:A", b).await.is_err(),
            "SKU retarget accepted via id={id}, code={code}"
        );
        assert_eq!(store.list("owner:A").await.unwrap(), vec![saved.clone()]);
    }
    let mut rename = saved.clone();
    rename.name = "Renamed A".into();
    rename.product = "Renamed product A".into();
    assert_eq!(store.upsert("owner:A", rename).await.unwrap().id, saved.id);
    let mut foreign = saved.clone();
    foreign.code = "FOREIGN".into();
    assert!(store.upsert("owner:B", foreign).await.is_err());
    let mut b = a;
    b.id = "T-B".into();
    b.code = "Q-B".into();
    b.item_code = "SKU-B".into();
    b.product = "Product B".into();
    let first = store.upsert("owner:A", b.clone()).await.unwrap();
    assert_eq!(store.upsert("owner:A", b).await.unwrap().id, first.id);
    assert_eq!(store.list("owner:A").await.unwrap().len(), 2);
    assert!(store.list("owner:B").await.unwrap().is_empty());
    let mut variant = saved.clone();
    variant.id = "T-VARIANT".into();
    variant.code = "Q-VARIANT".into();
    variant.variant_id = variant.id.clone();
    store.upsert("owner:A", variant.clone()).await.unwrap();
    assert_eq!(
        store.list("owner:A").await.unwrap().len(),
        3,
        "explicit same-SKU variants must survive business dedupe"
    );
    variant.variant_id.clear();
    assert_eq!(
        store.upsert("owner:A", variant).await.unwrap().variant_id,
        "T-VARIANT",
        "legacy edits retain the variant marker"
    );
    let mut shared = saved.clone();
    shared.id = "SHARED-TEMPLATE".into();
    shared.code = "Q-SHARED".into();
    let shared = store.upsert("telegram:shared", shared).await.unwrap();
    assert!(
        store
            .validate_read_identity("admin:reader", &shared)
            .await
            .is_ok()
    );
    assert!(store.upsert("admin:reader", shared.clone()).await.is_err());
    assert!(
        store
            .validate_read_identity("owner:unrelated", &shared)
            .await
            .is_err()
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn postgres_template_identity_contract() {
    let pool = pg_pool().await;
    let store =
        mini_rs_erp::db::postgres_calculate_order::PostgresCalculateOrderStore::new(pool.clone());
    sqlx::query("DELETE FROM mini_quick_order_templates WHERE owner_key IN ('owner:A','owner:B')")
        .execute(&pool)
        .await
        .unwrap();
    assert_identity_contract(&store).await;
}

async fn pg_pool() -> sqlx::PgPool {
    let url = std::env::var("TEMPLATE_PLAN_TEST_DATABASE_URL").expect("isolated database URL");
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let database = format!("template_plan_{:032x}", rand::random::<u128>());
    sqlx::query(&format!("CREATE DATABASE {database}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(
        url.parse::<sqlx::postgres::PgConnectOptions>()
            .unwrap()
            .database(&database),
    )
    .await
    .unwrap();
    mini_rs_erp::db::postgres::apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    mini_rs_erp::db::postgres::canonical_apparatus_service(pool.clone())
        .bootstrap_factory_defaults()
        .await
        .unwrap();
    mini_rs_erp::db::postgres::apply_foundation_migration(&pool)
        .await
        .unwrap();
    pool
}

#[tokio::test]
async fn cutting_plans_preserve_custom_groups_and_reject_invalid_values_on_all_nodes() {
    let (_dir, state, token) = fixture().await;
    for (index, groups) in [
        json!([1, 1, 1, 1, 1]),
        json!([2, 3]),
        json!([1, 2, 2]),
        json!([]),
    ]
    .into_iter()
    .enumerate()
    {
        let mut t = template();
        t["source_map_id"] = json!("");
        t["id"] = json!(format!("custom-{index}"));
        t["code"] = t["id"].clone();
        let (status,result)=call(&state,&token,"PUT",SAVE,json!({"map":map(&format!("zakaz-draft-custom-{index}"),5,groups.clone()),"template":t})).await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert_eq!(result["saved"]["map"]["nodes"][1]["rezka_kadr_count"], 5);
        assert_eq!(
            result["saved"]["map"]["nodes"][1]
                .get("rezka_frame_groups")
                .cloned()
                .unwrap_or(json!([])),
            groups
        );
        assert_eq!(
            result["saved"]["map"]["roll_count"], 4,
            "roll/val count is independent of frames"
        );
    }
    for groups in [
        json!([2, 2]),
        json!([0, 5]),
        json!([-1, 6]),
        json!([i64::MAX, 1]),
    ] {
        let mut order = map("zakaz-draft-invalid", 5, json!([2, 3]));
        let mut second = order["nodes"][1].clone();
        second["id"] = json!("cut-alternative");
        second["apparatus_id"] = json!("apparatus:default:asset-011");
        second["rezka_frame_groups"] = groups;
        order["nodes"][1]["alternative_group_id"] = json!("cuts");
        second["alternative_group_id"] = json!("cuts");
        order["nodes"].as_array_mut().unwrap().push(second);
        order["edges"].as_array_mut().unwrap().extend([
            json!({"from":"start","to":"cut-alternative"}),
            json!({"from":"cut-alternative","to":"end"}),
        ]);
        let mut invalid_template = template();
        invalid_template["source_map_id"] = json!("");
        let (status, error) = call(
            &state,
            &token,
            "PUT",
            SAVE,
            json!({"map":order,"template":invalid_template}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
        assert_eq!(
            error["cutting_plan_conflicts"][0]["node_id"],
            "cut-alternative"
        );
    }
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn postgres_http_fork_failure_atomicity_and_parallel_retry() {
    let pool = pg_pool().await;
    let (_dir, mut state, token) = fixture().await;
    state.apparatus = mini_rs_erp::db::postgres::canonical_apparatus_service(pool.clone());
    state.production_maps = mini_rs_erp::core::production_map::ProductionMapService::new(
        Arc::new(
            mini_rs_erp::db::postgres_production_map::PostgresProductionMapStore::new(pool.clone()),
        ),
        Arc::new(
            mini_rs_erp::core::production_map::CanonicalServiceApparatusResolver::new(
                state.apparatus.clone(),
            ),
        ),
    );
    state.calculate_orders = Arc::new(
        mini_rs_erp::db::postgres_calculate_order::PostgresCalculateOrderStore::new(pool.clone()),
    );
    state.production_orders =
        Arc::new(mini_rs_erp::db::postgres_mini_order::PostgresMiniOrderSink::new(pool.clone()));
    let original = state
        .production_maps
        .upsert_map(serde_json::from_value(map("MAP-A", 4, json!([1, 1, 1, 1]))).unwrap())
        .await
        .unwrap()
        .map;
    let (status, a) = call(&state, &token, "POST", "/v1/mobile/calculate/orders", {
        let mut t = template();
        t["id"] = json!("HTTP-T-A");
        t
    })
    .await;
    assert_eq!(status, StatusCode::OK, "{a}");
    let a = a["template"].clone();
    let mut b = template();
    b["id"] = json!("HTTP-T-B");
    b["code"] = json!("Q-B");
    b["item_code"] = json!("SKU-B");
    b["product"] = json!("Product B");
    b["kg"] = json!(100);
    let mut order = map("zakaz-draft-SKU-B", 4, json!([1, 1, 1, 1]));
    order["product_code"] = json!("SKU-B");
    let request = json!({"template_action":"fork","map":order,"template":b});
    let (status, error) = call(&state, &token, "PUT", SAVE, request.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(
        state
            .calculate_orders
            .list("admin:plan-admin")
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        state
            .production_maps
            .map("template-http-t-b")
            .await
            .unwrap()
            .is_none()
    );
    let mut corrected = request;
    corrected["map"]["nodes"][1]["rezka_kadr_count"] = json!(5);
    corrected["map"]["nodes"][1]["rezka_frame_groups"] = json!([2, 3]);
    let (first, retry) = tokio::join!(
        call(&state, &token, "PUT", SAVE, corrected.clone()),
        call(&state, &token, "PUT", SAVE, corrected.clone())
    );
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(retry.0, StatusCode::OK, "{}", retry.1);
    assert_eq!(
        first.1, retry.1,
        "parallel replay returns the committed snapshot"
    );
    assert_eq!(first.1["template"]["id"], "HTTP-T-B");
    assert_eq!(first.1["template"]["source_map_id"], "template-http-t-b");
    let order_id = first.1["saved"]["map"]["id"].as_str().unwrap();
    let snapshot: Value =
        sqlx::query_scalar("SELECT calculation_json FROM mini_orders WHERE id=$1")
            .bind(order_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_ne!(snapshot["id"], "HTTP-T-A");
    assert_ne!(snapshot["id"], "HTTP-T-B");
    assert_eq!(snapshot["source_map_id"], order_id);
    assert_eq!(
        state
            .production_maps
            .map("MAP-A")
            .await
            .unwrap()
            .unwrap()
            .map,
        original
    );
    let (_, list) = call(
        &state,
        &token,
        "GET",
        "/v1/mobile/calculate/orders",
        json!({}),
    )
    .await;
    assert_eq!(
        list["templates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == "HTTP-T-A")
            .unwrap(),
        &a
    );
    assert_eq!(list["templates"].as_array().unwrap().len(), 2);
    // Editing B's custom route must leave A and the opened order snapshot alone.
    let mut variant = first.1["template"].clone();
    let mut variant_map = first.1["saved"]["map"].clone();
    variant_map["id"] = variant["source_map_id"].clone();
    variant_map["code"] = json!("");
    variant_map["order_number"] = json!("");
    variant_map["nodes"][1]["rezka_frame_groups"] = json!([1, 2, 2]);
    variant["kg"] = json!(0);
    let (edited, body) = call(
        &state,
        &token,
        "PUT",
        SAVE,
        json!({"map":variant_map,"template":variant,"template_action":"update"}),
    )
    .await;
    assert_eq!(edited, StatusCode::OK, "{body}");
    assert_eq!(
        state
            .production_maps
            .map("MAP-A")
            .await
            .unwrap()
            .unwrap()
            .map,
        original
    );
    assert_eq!(
        state
            .production_maps
            .map(order_id)
            .await
            .unwrap()
            .unwrap()
            .map
            .nodes[1]
            .rezka_frame_groups,
        vec![2, 3]
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mini_order_creation_receipts WHERE owner_key='admin:plan-admin'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    let mut different = corrected.clone();
    different["template"]["frame_count"] = json!(6);
    let (status, error) = call(&state, &token, "PUT", SAVE, different).await;
    assert_eq!(status, StatusCode::CONFLICT, "{error}");
    // Force a persistence failure after the order/source maps have been written
    // inside create_order_atomic; no effect may escape the transaction.
    sqlx::raw_sql("CREATE FUNCTION template_fixture_fail() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.id = 'HTTP-T-FAULT' THEN RAISE EXCEPTION 'synthetic persistence failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER template_fixture_fail BEFORE INSERT ON mini_quick_order_templates FOR EACH ROW EXECUTE FUNCTION template_fixture_fail();").execute(&pool).await.unwrap();
    let mut failed = corrected;
    failed["map"]["id"] = json!("zakaz-draft-late-fault");
    failed["template"]["id"] = json!("HTTP-T-FAULT");
    failed["template"]["code"] = json!("Q-FAULT");
    failed["template"]["item_code"] = json!("SKU-FAULT");
    failed["map"]["product_code"] = json!("SKU-FAULT");
    let (status, error) = call(&state, &token, "PUT", SAVE, failed.clone()).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{error}");
    assert!(
        state
            .production_maps
            .map("template-http-t-fault")
            .await
            .unwrap()
            .is_none()
    );
    failed["map"]["id"] = json!("design-only-fault");
    failed["template"]["kg"] = json!(0);
    let (status, error) = call(&state, &token, "PUT", SAVE, failed).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{error}");
    assert!(
        state
            .production_maps
            .map("template-http-t-fault")
            .await
            .unwrap()
            .is_none()
    );
    let remaining: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mini_orders WHERE product_code='SKU-FAULT'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, 0);
    assert_eq!(
        state
            .production_maps
            .map("MAP-A")
            .await
            .unwrap()
            .unwrap()
            .map,
        original
    );
    let mut shared: CalculateOrderTemplate = serde_json::from_value(template()).unwrap();
    shared.id = "SHARED-TEMPLATE".into();
    shared.code = "Q-SHARED".into();
    shared.note = "shared fixture".into();
    let shared = state
        .calculate_orders
        .upsert("telegram:shared", shared)
        .await
        .unwrap();
    let mut order_input = shared.clone();
    order_input.kg = 100.0;
    let (status,body)=call(&state,&token,"PUT",SAVE,json!({"template_action":"use","map":map("zakaz-draft-shared-readable",5,json!([2,3])),"template":order_input})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["template"].is_null());
    assert_eq!(
        state
            .calculate_orders
            .list("telegram:shared")
            .await
            .unwrap(),
        vec![shared]
    );
}

#[tokio::test]
async fn paused_rezka_http_resume_reuses_session_and_keeps_wip_ownership() {
    let (_dir, state, token) = fixture().await;
    assert_rezka_resume(
        state,
        token,
        Arc::new(mini_rs_erp::core::production_map::MemoryProductionMapStore::new()),
    )
    .await;
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL database"]
async fn postgres_paused_rezka_http_resume_preserves_persistent_input() {
    let pool = pg_pool().await;
    let (_dir, mut state, token) = fixture().await;
    state.apparatus = mini_rs_erp::db::postgres::canonical_apparatus_service(pool.clone());
    assert_rezka_resume(
        state,
        token,
        Arc::new(mini_rs_erp::db::postgres_production_map::PostgresProductionMapStore::new(pool)),
    )
    .await;
}

async fn assert_rezka_resume(
    mut state: AppState,
    token: String,
    store: Arc<dyn mini_rs_erp::core::production_map::ProductionMapStorePort>,
) {
    use mini_rs_erp::core::production_map::*;
    use std::collections::BTreeMap;
    const PRINT: &str = "apparatus:default:bosma_9";
    const ORDER: &str = "zakaz-resume-fixture";
    state.production_maps = ProductionMapService::new(
        store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(
            state.apparatus.clone(),
        )),
    );
    let mut definition = map(ORDER, 5, json!([2, 3]));
    definition["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"print","kind":"apparatus","title":"Bosma","apparatus_id":PRINT}));
    definition["edges"] = json!([{"from":"start","to":"print"},{"from":"print","to":"cut"},{"from":"cut","to":"end"}]);
    state
        .production_maps
        .upsert_map(serde_json::from_value(definition).unwrap())
        .await
        .unwrap();
    for (id, status) in [(PRINT, "completed"), (CUT, "paused")] {
        store
            .put_apparatus_sequence(id, vec![ORDER.into()])
            .await
            .unwrap();
        store
            .put_apparatus_queue_states(id, BTreeMap::from([(ORDER.into(), status.into())]))
            .await
            .unwrap();
    }
    let session:OrderRunSession=serde_json::from_value(json!({
        "session_id":"mounted-session","apparatus":CUT,"order_id":ORDER,"stage_node_id":"cut","status":"paused",
        "worker_role":"admin","worker_ref":"plan-admin","worker_display_name":"Plan admin","started_at_unix":10,"updated_at_unix":20,
        "payload_json":{"input_progress_batch_id":"mounted-input","input_progress_qr_payload":"4001MOUNTED","input_progress_apparatus":PRINT,
          "input_wip_source_kind":"progress_batch","input_stage_node_id":"cut","contained_kadr_count":5,
          "input_lineage":[{"input_batch_id":"mounted-input","input_qr_payload":"4001MOUNTED","source_apparatus":PRINT,"source_kind":"progress_batch","stage_node_id":"cut","sequence_no":1,"status":"in_use","linked_at_unix":10}]}
    })).unwrap();
    let mut upstream = session.clone();
    upstream.session_id = "upstream-session".into();
    upstream.apparatus = PRINT.into();
    upstream.stage_node_id = "print".into();
    upstream.status = OrderRunStatus::Completed;
    upstream.payload_json = json!({});
    store.put_order_run_session(upstream).await.unwrap();
    store.put_order_run_session(session.clone()).await.unwrap();
    state
        .admin
        .upsert_role_assignment(mini_rs_erp::core::authz::RoleAssignmentUpsert {
            principal_role: PrincipalRole::Admin,
            principal_ref: "plan-admin".into(),
            role_id: "admin".into(),
            assigned_apparatus: vec![PRINT.into(), CUT.into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    for (id, status) in [("mounted-input", "in_use"), ("waiting-input", "waiting")] {
        let batch:OrderProgressBatch=serde_json::from_value(json!({
            "batch_id":id,"session_id":"upstream-session","apparatus":PRINT,"order_id":ORDER,"started_at_unix":1,"completed_at_unix":2,
            "action":"pause","status":"paused","produced_qty":100,"uom":"m","qr_payload":if status=="in_use" {"4001MOUNTED"} else {"4001WAITING"},
            "label_item_code":"SKU-A","label_item_name":"Product A","executor_name":"Printer","worker_role":"admin","worker_ref":"printer","worker_display_name":"Printer",
            "wip_status":status,"next_apparatus":CUT,"current_apparatus":if status=="in_use" {CUT} else {PRINT},
            "used_by_session_id":if status=="in_use" {"mounted-session"} else {""},"used_by_apparatus":if status=="in_use" {CUT} else {""},
            "payload_json":{"stage_node_id":"print","next_stage_node_id":"cut","contained_kadr_count":5,"width_mm":675,"finished_goods_kg":20}
        })).unwrap();
        store.put_order_progress_batch(batch).await.unwrap();
    }
    let mut output = store
        .progress_batch("mounted-input")
        .await
        .unwrap()
        .unwrap();
    output.batch_id = "cut-pause-output".into();
    output.session_id = session.session_id.clone();
    output.apparatus = CUT.into();
    output.qr_payload = "4001OUTPUT".into();
    output.wip_status = OrderProgressBatchWipStatus::Waiting;
    output.used_by_session_id.clear();
    output.used_by_apparatus.clear();
    output.next_apparatus.clear();
    output.payload_json = json!({"stage_node_id":"cut","contained_kadr_count":5});
    store.put_order_progress_batch(output).await.unwrap();
    let (status, snapshot) = call(
        &state,
        &token,
        "GET",
        "/v1/mobile/admin/production-maps/sequence",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{snapshot}");
    let control = &snapshot["queue_action_controls"][CUT][ORDER];
    assert!(
        control["allowed_actions"]
            .as_array()
            .unwrap()
            .contains(&json!("resume")),
        "{control}"
    );
    assert_eq!(control["interaction"]["resume_input_required"], false);
    let input_before = store
        .progress_batch("mounted-input")
        .await
        .unwrap()
        .unwrap();
    let body = json!({"apparatus":CUT,"order_id":ORDER,"action":"resume"});
    let (status, resumed) = call(
        &state,
        &token,
        "POST",
        "/v1/mobile/admin/production-maps/queue-action",
        body.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resumed}");
    assert_eq!(resumed["session"]["session_id"], session.session_id);
    assert_eq!(
        resumed["session"]["payload_json"]["input_progress_batch_id"],
        "mounted-input"
    );
    let mounted = store
        .progress_batch("mounted-input")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(mounted.wip_status, OrderProgressBatchWipStatus::InUse);
    assert_eq!(mounted.used_by_session_id, session.session_id);
    assert_eq!(mounted.produced_qty, input_before.produced_qty);
    assert_eq!(mounted.finished_goods_kg, input_before.finished_goods_kg);
    assert_eq!(
        mounted.finished_goods_meter,
        input_before.finished_goods_meter
    );
    assert_eq!(
        store
            .progress_batch("waiting-input")
            .await
            .unwrap()
            .unwrap()
            .wip_status,
        OrderProgressBatchWipStatus::Waiting
    );
    let (repeat, _) = call(
        &state,
        &token,
        "POST",
        "/v1/mobile/admin/production-maps/queue-action",
        body,
    )
    .await;
    assert_ne!(repeat, StatusCode::OK);
    assert_eq!(
        store
            .active_order_run_sessions_for_orders(&[ORDER.into()])
            .await
            .unwrap()[ORDER]
            .len(),
        1
    );
}
