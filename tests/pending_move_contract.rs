#![cfg(feature = "verification")]
//! Pending moves use the real HTTP handlers and persist queue order with the map.
use mini_rs_erp::{
    app::AppState,
    config::AppConfig,
    core::{
        auth::models::{Principal, PrincipalRole},
        production_map::{
            CanonicalServiceApparatusResolver, MemoryProductionMapStore, ProductionMapDefinition,
            ProductionMapService, ProductionMapStorePort,
        },
        session::manager::SessionManager,
    },
    db::postgres_production_map::PostgresProductionMapStore,
    http::router::build_router,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

const FROM: &str = "apparatus:default:bosma_7";
const TO: &str = "apparatus:default:bosma_8";
const A: &str = "zakaz-colour-a";
const B: &str = "zakaz-waiting-b";
const C: &str = "zakaz-target-c";
const D: &str = "zakaz-target-d";
const ROOT: &str = "/v1/mobile/admin/production-maps";

async fn fixture(
    pool: Option<&sqlx::PgPool>,
) -> (AppState, Arc<dyn ProductionMapStorePort>, String) {
    static BASE: tokio::sync::OnceCell<(tempfile::TempDir, AppState)> =
        tokio::sync::OnceCell::const_new();
    let (_, base) = BASE
        .get_or_init(|| async {
            let dir = tempfile::tempdir().unwrap();
            let state = AppState::verification(AppConfig {
                bind_addr: "127.0.0.1:0".parse().unwrap(),
                default_target_warehouse: String::new(),
                http_timeout: Duration::from_secs(5),
                session_store_path: dir.path().join("sessions"),
                profile_store_path: dir.path().join("profiles"),
                push_token_store_path: dir.path().join("push"),
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
            });
            (dir, state)
        })
        .await;
    let mut state = base.clone();
    state.sessions = SessionManager::memory(Some(3600));
    let store: Arc<dyn ProductionMapStorePort> = if let Some(pool) = pool {
        state.apparatus = mini_rs_erp::db::postgres::canonical_apparatus_service(pool.clone());
        Arc::new(PostgresProductionMapStore::new(pool.clone()))
    } else {
        Arc::new(MemoryProductionMapStore::new())
    };
    state.apparatus.bootstrap_factory_defaults().await.unwrap();
    state.production_maps = ProductionMapService::new(
        store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(
            state.apparatus.clone(),
        )),
    );
    for (id, apparatus) in [(A, FROM), (B, FROM), (C, TO), (D, TO)] {
        let map: ProductionMapDefinition = serde_json::from_value(json!({
            "id":id,"title":id,"product_code":"SYNTHETIC-PRINT","roll_count":7,"width_mm":650,
            "nodes":[{"id":"start","kind":"start","title":"Start"},
                {"id":"print","kind":"apparatus","title":"Bosma","apparatus_id":apparatus},
                {"id":"end","kind":"end","title":"End"}],
            "edges":[{"from":"start","to":"print"},{"from":"print","to":"end"}]
        }))
        .unwrap();
        state.production_maps.upsert_map(map).await.unwrap();
    }
    store
        .put_apparatus_sequence(FROM, vec![A.into(), B.into()])
        .await
        .unwrap();
    // Preserve D's implicit queue position, even though only C was saved.
    store
        .put_apparatus_sequence(TO, vec![C.into()])
        .await
        .unwrap();
    let token = state
        .sessions
        .create(Principal {
            role: PrincipalRole::Admin,
            ref_: "pending-move-admin".into(),
            display_name: "Admin".into(),
            legal_name: String::new(),
            avatar_url: String::new(),
            phone: String::new(),
        })
        .await
        .unwrap();
    (state, store, token)
}

async fn call(
    client: &reqwest::Client,
    base: &str,
    token: &str,
    method: &str,
    suffix: &str,
    payload: Value,
) -> (u16, Value) {
    let response = client
        .request(method.parse().unwrap(), format!("{base}{ROOT}{suffix}"))
        .bearer_auth(token)
        .json(&payload)
        .send()
        .await
        .unwrap();
    (response.status().as_u16(), response.json().await.unwrap())
}

async fn scenario(outcome: &str, pool: Option<&sqlx::PgPool>) {
    let (state, store, token) = fixture(pool).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server_state = state.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, build_router(server_state))
            .await
            .unwrap();
    });
    let client = reqwest::Client::new();
    let (status, mut colour) = call(&client, &base, &token, "POST", "/print-preflight", json!({
        "apparatus":FROM,"order_id":A,"action":"hold","hold_id":"trial-a","idempotency_key":"trial-a"
    })).await;
    assert_eq!(status, 200, "{colour}");
    assert_eq!(colour["hold"]["status"], "running");
    if outcome == "passed" {
        let result = call(
            &client,
            &base,
            &token,
            "POST",
            "/print-preflight",
            json!({
                "apparatus":FROM,"order_id":A,"action":"passed","hold_id":"trial-a"
            }),
        )
        .await;
        assert_eq!(result.0, 200, "{}", result.1);
        colour = result.1;
    }
    let original_maps = store.maps().await.unwrap();
    let original_sequences = store.apparatus_sequences().await.unwrap();
    let states = store.apparatus_queue_states().await.unwrap();
    let (status, blocked) = call(
        &client,
        &base,
        &token,
        "POST",
        "/move-batch",
        json!({
            "from_apparatus":FROM,"to_apparatus":TO,"map_ids":[B,A]
        }),
    )
    .await;
    assert_eq!(status, 409, "{blocked}");
    assert_eq!(blocked["error"], "print_preflight_active");
    assert_eq!(store.maps().await.unwrap(), original_maps);
    assert_eq!(
        store.apparatus_sequences().await.unwrap(),
        original_sequences
    );
    let request = json!({"from_apparatus":FROM,"to_apparatus":TO,"map_ids":[B,B]});
    if let Some(pool) = pool {
        // A late queue failure must roll back the already written map as well.
        sqlx::raw_sql(&format!("CREATE FUNCTION reject_move_queue() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.canonical_apparatus_id = '{TO}' THEN RAISE EXCEPTION 'synthetic queue failure'; END IF; RETURN NEW; END $$; CREATE TRIGGER reject_move_queue BEFORE INSERT OR UPDATE ON mini_queue_sequences FOR EACH ROW EXECUTE FUNCTION reject_move_queue();"))
            .execute(pool).await.unwrap();
        let failed = call(
            &client,
            &base,
            &token,
            "POST",
            "/move-batch",
            request.clone(),
        )
        .await;
        assert_eq!(failed.0, 500, "{}", failed.1);
        assert_eq!(store.maps().await.unwrap(), original_maps);
        assert_eq!(
            store.apparatus_sequences().await.unwrap(),
            original_sequences
        );
        sqlx::raw_sql("DROP TRIGGER reject_move_queue ON mini_queue_sequences; DROP FUNCTION reject_move_queue();").execute(pool).await.unwrap();
    }
    let (status, moved) = call(
        &client,
        &base,
        &token,
        "POST",
        "/move-batch",
        request.clone(),
    )
    .await;
    assert_eq!(status, 200, "{moved}");
    assert_eq!(moved["saved"].as_array().unwrap().len(), 1);
    let sequences = store.apparatus_sequences().await.unwrap();
    assert_eq!(sequences[FROM], vec![A.to_string()]);
    assert_eq!(
        sequences[TO],
        vec![C.to_string(), D.to_string(), B.to_string()]
    );
    assert_eq!(store.apparatus_queue_states().await.unwrap(), states);
    assert!(
        store
            .order_run_sessions_for_order(B)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .progress_batches_for_order(B)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        serde_json::to_value(
            store
                .print_preflight_hold_by_idempotency_key("trial-a")
                .await
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        colour["hold"]
    );
    let stale_write = store
        .put_maps_with_sequences(&original_maps, &original_sequences, &original_sequences)
        .await;
    assert!(matches!(
        stale_write,
        Err(mini_rs_erp::core::production_map::ProductionMapError::QueueReorderConflict)
    ));
    assert_eq!(store.apparatus_sequences().await.unwrap(), sequences);
    assert_eq!(
        store.map_by_id(B).await.unwrap().unwrap().nodes[1].apparatus_id,
        TO
    );
    for _ in 0..2 {
        let (status, snapshot) =
            call(&client, &base, &token, "GET", "/sequence", Value::Null).await;
        assert_eq!(status, 200, "{snapshot}");
        assert_eq!(snapshot["sequences"][TO], json!([C, D, B]));
        assert_eq!(snapshot["sequences"][FROM], json!([A]));
        assert_eq!(snapshot["queue_action_controls"][TO][B]["state"], "pending");
        let b = snapshot["maps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|saved| saved["map"]["id"] == B)
            .unwrap();
        assert_eq!(b["map"]["nodes"][1]["apparatus_id"], TO);
    }
    // Repeating a completed move cannot add a second queue entry or start work.
    let repeated = call(&client, &base, &token, "POST", "/move-batch", request).await;
    assert_eq!(repeated.0, 400, "{}", repeated.1);
    assert_eq!(store.apparatus_sequences().await.unwrap(), sequences);
    if let Some(pool) = pool {
        let reloaded = PostgresProductionMapStore::new(pool.clone());
        assert_eq!(reloaded.apparatus_sequences().await.unwrap(), sequences);
        assert_eq!(
            reloaded.map_by_id(B).await.unwrap(),
            store.map_by_id(B).await.unwrap()
        );
    }
    server.abort();
}

#[tokio::test]
async fn pending_move_real_http_preserves_running_and_passed_reservations() {
    for outcome in ["running", "passed"] {
        scenario(outcome, None).await;
    }
}

#[tokio::test]
#[ignore = "requires PENDING_MOVE_TEST_DATABASE_URL pointing at a disposable PostgreSQL cluster"]
async fn pending_move_postgres_http_commits_map_and_tail_atomically() {
    let url =
        std::env::var("PENDING_MOVE_TEST_DATABASE_URL").expect("isolated PostgreSQL admin URL");
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    for outcome in ["running", "passed"] {
        let database = format!("pending_move_{:032x}", rand::random::<u128>());
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
        scenario(outcome, Some(&pool)).await;
        pool.close().await;
        sqlx::query(&format!("DROP DATABASE {database}"))
            .execute(&admin)
            .await
            .unwrap();
    }
    admin.close().await;
}
