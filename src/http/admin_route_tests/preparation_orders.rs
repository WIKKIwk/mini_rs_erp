use super::*;
use crate::core::apparatus_standard::CanonicalApparatusService;
use crate::core::preparation::{MaterialResponsibilityAssign, MaterialResponsibilityDelete};
use crate::core::production_map::{ProductionMapDefinition, QueueActionActor};
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    postgres_test_database_options,
};
use crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository;
use crate::db::postgres_preparation::PostgresPreparationStore;
use crate::db::postgres_production_map::PostgresProductionMapStore;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::collections::BTreeSet;

const PRINT: &str = "apparatus:default:bosma_7";
const LAMINATION: &str = "apparatus:default:asset-007";
const MASTER: &str = "preparation-list-master";
const OTHER_MASTER: &str = "preparation-list-other";
const LIST: &str = "/v1/mobile/preparation/orders";
static DATABASE_GATE: Mutex<()> = Mutex::const_new(());

struct Fixture {
    state: AppState,
    pool: PgPool,
    admin_pool: PgPool,
    database: String,
    master: String,
    other: String,
}

impl Fixture {
    async fn create() -> Self {
        // An explicit disposable admin URL is required. Never fall back to the
        // developer's runtime PostgreSQL database when running this suite.
        let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .expect("explicit isolated PostgreSQL test admin URL required");
        let admin_pool = PgPool::connect(&url).await.expect("test admin database");
        let database = format!("preparation_orders_{:016x}", rand::random::<u64>());
        sqlx::query(&format!("CREATE DATABASE \"{database}\""))
            .execute(&admin_pool)
            .await
            .expect("create isolated preparation orders database");
        let pool = PgPool::connect_with(postgres_test_database_options(&url, &database))
            .await
            .unwrap();
        apply_postgres_migrations_through_version(&pool, "0121")
            .await
            .unwrap();
        let apparatus = CanonicalApparatusService::new(Arc::new(
            PostgresCanonicalApparatusRepository::new(pool.clone()),
        ));
        apparatus.bootstrap_factory_defaults().await.unwrap();
        apply_foundation_migration(&pool).await.unwrap();

        sqlx::raw_sql(
            r#"
            INSERT INTO mini_system_users(id,role,name,phone) VALUES
                ('preparation-list-master','tayyorlov_masteri','Master','901234581'),
                ('preparation-list-other','tayyorlov_masteri','Other master','901234582');
            INSERT INTO mini_calculate_materials(id,lower_name,payload_json) VALUES
                ('list-film','list film','{"id":"list-film","name":"List film","active":true}'),
                ('list-foil','list foil','{"id":"list-foil","name":"List foil","active":true}');
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let preparation = PostgresPreparationStore::new(pool.clone());
        for (principal_ref, material_id) in [(MASTER, "list-film"), (OTHER_MASTER, "list-foil")] {
            preparation
                .assign_responsibility(MaterialResponsibilityAssign {
                    principal_ref: principal_ref.into(),
                    material_id: material_id.into(),
                })
                .await
                .unwrap();
        }
        let mut state = test_state();
        state.apparatus = apparatus;
        state.production_maps = ProductionMapService::new(
            Arc::new(PostgresProductionMapStore::new(pool.clone())),
            Arc::new(CanonicalServiceApparatusResolver::new(
                state.apparatus.clone(),
            )),
        );
        state.preparation = Some(preparation);
        let master = session_for(&state, PrincipalRole::TayyorlovMasteri, MASTER).await;
        let other = session_for(&state, PrincipalRole::TayyorlovMasteri, OTHER_MASTER).await;
        Self {
            state,
            pool,
            admin_pool,
            database,
            master,
            other,
        }
    }

    async fn seed_order(&self, number: &str, material: &str, cross_machine: bool) -> String {
        let id = format!("zakaz-{number}");
        let source = if cross_machine {
            two_apparatus_order_map_json(
                &id,
                &format!("Master label {number}"),
                number,
                PRINT,
                LAMINATION,
            )
        } else {
            pechat_order_map_json_with_dims(
                &id,
                &format!("Master label {number}"),
                number,
                PRINT,
                7,
                700.0,
            )
        };
        let mut payload: Value = serde_json::from_str(&source).unwrap();
        payload["order_kg"] = json!(120.0);
        payload["width_mm"] = json!(700.0);
        payload["roll_count"] = json!(7);
        payload["customer_name"] = json!(format!("Customer {number}"));
        payload["image_id"] = json!(format!("image-{number}"));
        let map: ProductionMapDefinition = serde_json::from_value(payload).unwrap();
        self.state.production_maps.upsert_map(map).await.unwrap();
        sqlx::query(
            "INSERT INTO mini_orders(id,code,order_number,product_name,kg)
             VALUES($1,$2,$2,$3,120)",
        )
        .bind(&id)
        .bind(number)
        .bind(format!("Master label {number}"))
        .execute(&self.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO mini_order_products(id,order_id,product_name,layers_json)
             VALUES($1,$2,$3,$4)",
        )
        .bind(format!("product-{number}"))
        .bind(&id)
        .bind(format!("Master label {number}"))
        .bind(json!([{"material_id":material}]))
        .execute(&self.pool)
        .await
        .unwrap();
        id
    }

    async fn get(&self, path: &str, token: &str) -> axum::response::Response {
        build_router(self.state.clone())
            .oneshot(request("GET", path, token))
            .await
            .unwrap()
    }

    async fn page(&self, query: &str) -> Value {
        let response = self
            .get(&format!("{LIST}?apparatus={PRINT}&{query}"), &self.master)
            .await;
        let status = response.status();
        let value = json_body(response).await;
        assert_eq!(status, StatusCode::OK, "{value}");
        value
    }

    async fn close(self) {
        drop(self.state);
        self.pool.close().await;
        sqlx::query(&format!("DROP DATABASE \"{}\" WITH (FORCE)", self.database))
            .execute(&self.admin_pool)
            .await
            .unwrap();
        self.admin_pool.close().await;
    }
}

fn ids(page: &Value) -> Vec<&str> {
    page["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect()
}

fn assert_compact(page: &Value) {
    for excluded in [
        "maps",
        "program",
        "queue_states",
        "queue_action_controls",
        "stage_states",
        "order_controls",
        "calculation_templates",
        "materials",
        "warehouses",
        "history",
    ] {
        assert!(
            page.get(excluded).is_none(),
            "unexpected initial field {excluded}"
        );
    }
    let expected = [
        "id",
        "code",
        "order_number",
        "title",
        "customer_name",
        "image_id",
        "position",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    for row in page["rows"].as_array().unwrap() {
        assert_eq!(
            row.as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            expected
        );
    }
    for apparatus in page["apparatus"].as_array().unwrap() {
        assert_eq!(
            apparatus
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["id", "name"])
        );
    }
}

#[tokio::test]
async fn preparation_orders_compact_pages_preserve_scope_position_and_cached_pagination() {
    let _serial = DATABASE_GATE.lock().await;
    let fixture = Fixture::create().await;
    let unrelated = fixture.seed_order("8101", "list-foil", false).await;
    let own = fixture.seed_order("8102", "list-film", false).await;
    let last = fixture.seed_order("8103", "list-film", false).await;
    fixture
        .state
        .production_maps
        .set_apparatus_sequence(PRINT, vec![unrelated.clone(), own.clone(), last.clone()])
        .await
        .unwrap();

    let first = fixture.page("limit=1").await;
    assert_eq!(first["projection"], "preparation_order_list_v1");
    assert_eq!(first["total"], 2);
    assert_eq!(ids(&first), vec![own.as_str()]);
    assert_eq!(first["rows"][0]["position"], 2);
    assert_eq!(first["rows"][0]["title"], "Master label 8102");
    assert_eq!(first["rows"][0]["order_number"], "8102");
    assert_eq!(first["rows"][0]["customer_name"], "Customer 8102");
    assert_eq!(first["rows"][0]["image_id"], "image-8102");
    assert_compact(&first);
    let cursor = first["next_cursor"].as_str().expect("second compact page");
    let second_path = format!(
        "{LIST}?apparatus={PRINT}&limit=1&cursor={}",
        urlencoding::encode(cursor)
    );

    // A cached page must not rebuild production maps just to advance its
    // cursor. Hold the table unavailable while requesting the next row.
    let mut blocked = fixture.pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE mini_production_maps IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocked)
        .await
        .unwrap();
    let second = tokio::time::timeout(
        Duration::from_secs(1),
        fixture.get(&second_path, &fixture.master),
    )
    .await
    .expect("cached pagination must not reread production maps");
    assert_eq!(second.status(), StatusCode::OK);
    let second = json_body(second).await;
    blocked.rollback().await.unwrap();
    assert_eq!(ids(&second), vec![last.as_str()]);
    assert_eq!(second["rows"][0]["position"], 3);
    assert_eq!(second["version"], first["version"]);
    assert!(second["next_cursor"].is_null());
    assert_compact(&second);

    let searched = fixture.page("search=8103").await;
    assert_eq!(ids(&searched), vec![last.as_str()]);
    assert_eq!(
        searched["rows"][0]["position"], 3,
        "search must retain the full apparatus queue rank"
    );
    assert_eq!(
        fixture.get(&second_path, &fixture.other).await.status(),
        StatusCode::CONFLICT,
        "another master's scope cannot reuse the cursor"
    );

    fixture
        .state
        .production_maps
        .request_order_freeze(
            &last,
            QueueActionActor {
                role: "admin".into(),
                ref_: "admin".into(),
                display_name: "Admin".into(),
            },
        )
        .await
        .unwrap();
    let frozen = fixture.page("limit=30").await;
    assert!(
        ids(&frozen).contains(&last.as_str()),
        "frozen orders stay visible without status colors"
    );
    let canonical = fixture
        .state
        .production_maps
        .live_snapshot_shared()
        .await
        .unwrap();
    let canonical_ids =
        crate::http::handlers::admin::production_maps::order_page::ordered_ids(&canonical, PRINT);
    for row in frozen["rows"].as_array().unwrap() {
        let id = row["id"].as_str().unwrap();
        assert_eq!(
            row["position"].as_u64().unwrap() as usize,
            canonical_ids
                .iter()
                .position(|candidate| candidate == id)
                .unwrap()
                + 1,
            "frozen display rows must preserve the canonical full queue rank"
        );
    }
    assert_compact(&frozen);
    // A cursor belongs to display content, rather than every unrelated global
    // revision. Freezing the final row may leave its labels/positions intact.
    let same_query = fixture.page("limit=1").await;
    assert_eq!(
        fixture.get(&second_path, &fixture.master).await.status(),
        if same_query["version"] == first["version"] {
            StatusCode::OK
        } else {
            StatusCode::CONFLICT
        }
    );
    fixture
        .state
        .production_maps
        .set_apparatus_sequence(PRINT, vec![own.clone(), unrelated.clone(), last.clone()])
        .await
        .unwrap();
    assert_eq!(
        fixture.get(&second_path, &fixture.master).await.status(),
        StatusCode::CONFLICT,
        "a changed canonical queue position must invalidate the old content cursor"
    );

    let admin = session(&fixture.state, PrincipalRole::Admin).await;
    for (token, status) in [
        ("", StatusCode::UNAUTHORIZED),
        (admin.as_str(), StatusCode::FORBIDDEN),
    ] {
        assert_eq!(fixture.get(LIST, token).await.status(), status);
    }
    assert_eq!(
        fixture
            .get(&format!("{LIST}?limit=0"), &fixture.master)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    fixture.close().await;
}

#[tokio::test]
async fn preparation_orders_conditional_cache_rechecks_independent_label_and_material_changes() {
    let _serial = DATABASE_GATE.lock().await;
    let fixture = Fixture::create().await;
    let own = fixture.seed_order("8301", "list-film", false).await;
    fixture
        .state
        .production_maps
        .set_apparatus_sequence(PRINT, vec![own.clone()])
        .await
        .unwrap();
    sqlx::query("UPDATE mini_production_maps SET map_json=jsonb_set(map_json,'{customer_name}','\"\"') WHERE id=$1")
        .bind(&own).execute(&fixture.pool).await.unwrap();
    sqlx::query("INSERT INTO mini_quick_order_templates(id,owner_key,code,name,product_name,payload_json,quick_key)
        VALUES('master-customer-template','master-label-test','8301','Master label','Master label',$1,'customer-test')")
        .bind(json!({"source_map_id":own,"customer":"Original customer"})).execute(&fixture.pool).await.unwrap();
    let first = fixture.page("limit=30").await;
    assert_eq!(first["rows"][0]["customer_name"], "Original customer");
    let conditional = format!(
        "{LIST}?apparatus={PRINT}&limit=30&if_version={}",
        first["version"].as_str().unwrap()
    );
    // Catalog construction touches run sessions; metadata validation does not.
    // Hold that table unavailable to prove a first-page304 reuses compact rows.
    let mut blocked = fixture.pool.begin().await.unwrap();
    sqlx::query("LOCK TABLE mini_order_run_sessions IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *blocked)
        .await
        .unwrap();
    let cached = tokio::time::timeout(
        Duration::from_secs(2),
        fixture.get(&conditional, &fixture.master),
    )
    .await
    .expect("conditional first page must not rebuild production authority");
    assert_eq!(cached.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        cached.headers()["x-preparation-version"],
        first["version"].as_str().unwrap()
    );
    blocked.rollback().await.unwrap();
    let revision = fixture.state.production_maps.snapshot_revision();
    sqlx::query("UPDATE mini_quick_order_templates SET payload_json=jsonb_set(payload_json,'{customer}','\"Renamed customer\"') WHERE id='master-customer-template'")
        .execute(&fixture.pool).await.unwrap();
    let renamed = fixture.get(&conditional, &fixture.master).await;
    assert_eq!(renamed.status(), StatusCode::OK);
    let renamed = json_body(renamed).await;
    assert_eq!(renamed["rows"][0]["customer_name"], "Renamed customer");
    assert_ne!(renamed["version"], first["version"]);
    assert_eq!(fixture.state.production_maps.snapshot_revision(), revision);
    sqlx::query("UPDATE mini_order_products SET layers_json=$1 WHERE order_id=$2")
        .bind(json!([{"material_id":"list-foil"}]))
        .bind(&own)
        .execute(&fixture.pool)
        .await
        .unwrap();
    let withdrawn = fixture.page("limit=30").await;
    assert!(
        ids(&withdrawn).is_empty(),
        "independent product material edits must retire cached membership"
    );
    let other = fixture.get(&conditional, &fixture.other).await;
    assert_eq!(
        other.status(),
        StatusCode::OK,
        "another account cannot reuse304"
    );
    fixture.close().await;
}

#[tokio::test]
async fn preparation_orders_live_updates_keep_one_socket_for_checkpoints_deltas_and_reassignment() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _serial = DATABASE_GATE.lock().await;
    let fixture = Fixture::create().await;
    let own = fixture.seed_order("8401", "list-film", false).await;
    fixture
        .state
        .production_maps
        .set_apparatus_sequence(PRINT, vec![own.clone()])
        .await
        .unwrap();
    let first = fixture.page("limit=30").await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = build_router(fixture.state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let path = format!(
        "{LIST}/live?apparatus={PRINT}&limit=30&length=30&token={}&epoch={}&rev={}&scope={}&version={}",
        urlencoding::encode(&fixture.master),
        first["epoch"].as_str().unwrap(),
        first["rev"],
        first["scope"].as_str().unwrap(),
        first["version"].as_str().unwrap()
    );
    stream.write_all(format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").as_bytes()).await.unwrap();
    let mut headers = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !headers.ends_with(b"\r\n\r\n") {
            headers.push(stream.read_u8().await.unwrap());
            assert!(headers.len() < 8192);
        }
    })
    .await
    .unwrap();
    assert!(
        String::from_utf8(headers)
            .unwrap()
            .starts_with("HTTP/1.1 101 ")
    );
    async fn frame(stream: &mut tokio::net::TcpStream) -> Value {
        tokio::time::timeout(Duration::from_secs(5), async {
            let first = stream.read_u8().await.unwrap();
            let second = stream.read_u8().await.unwrap();
            assert_eq!(first & 15, 1, "expected uncompressed server text");
            assert_eq!(first & 0x40, 0);
            assert_eq!(second & 0x80, 0);
            let size = match second & 127 {
                126 => stream.read_u16().await.unwrap() as usize,
                127 => stream.read_u64().await.unwrap() as usize,
                size => size as usize,
            };
            assert!(size < 128 * 1024);
            let mut bytes = vec![0; size];
            stream.read_exact(&mut bytes).await.unwrap();
            serde_json::from_slice(&bytes).unwrap()
        })
        .await
        .expect("preparation live frame")
    }
    assert_eq!(frame(&mut stream).await["type"], "ready");
    fixture.state.production_maps.notify_live();
    let checkpoint = frame(&mut stream).await;
    assert_eq!(checkpoint["type"], "checkpoint");
    assert_eq!(checkpoint["version"], first["version"]);
    sqlx::query("UPDATE mini_production_maps SET map_json=jsonb_set(map_json,'{title}','\"Renamed live order\"') WHERE id=$1")
        .bind(&own).execute(&fixture.pool).await.unwrap();
    fixture.state.production_maps.notify_live();
    let delta = frame(&mut stream).await;
    assert_eq!(delta["type"], "list_delta");
    assert_eq!(delta["base_version"], first["version"]);
    assert_eq!(delta["order_ids"], json!([own]));
    assert_eq!(delta["upserts"][0]["title"], "Renamed live order");
    fixture
        .state
        .preparation
        .as_ref()
        .unwrap()
        .unassign_responsibility(MaterialResponsibilityDelete {
            principal_ref: MASTER.into(),
            material_id: "list-film".into(),
        })
        .await
        .unwrap();
    fixture.state.production_maps.notify_live();
    let reassigned = frame(&mut stream).await;
    assert_eq!(reassigned["type"], "list_snapshot");
    assert_eq!(reassigned["rows"], json!([]));
    assert_ne!(reassigned["scope"], first["scope"]);
    drop(stream);
    server.abort();
    fixture.close().await;
}

#[tokio::test]
async fn preparation_orders_detail_and_reassignment_recheck_current_master_scope() {
    let _serial = DATABASE_GATE.lock().await;
    let fixture = Fixture::create().await;
    let own = fixture.seed_order("8201", "list-film", true).await;
    let unrelated = fixture.seed_order("8202", "list-foil", false).await;
    let own_second = fixture.seed_order("8203", "list-film", false).await;
    fixture
        .state
        .production_maps
        .set_apparatus_sequence(PRINT, vec![own.clone(), unrelated.clone(), own_second])
        .await
        .unwrap();
    let first = fixture.page("limit=1").await;
    let stale_cursor = first["next_cursor"].as_str().unwrap().to_string();

    let detail_path = format!("{LIST}/{own}");
    let response = fixture.get(&detail_path, &fixture.master).await;
    let status = response.status();
    let detail = json_body(response).await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["projection"], "order_detail_v1");
    assert_eq!(detail["maps"].as_array().unwrap().len(), 1);
    assert_eq!(detail["maps"][0]["map"]["id"], own);
    assert_eq!(detail["maps"][0]["map"]["order_kg"], 120.0);
    let apparatus_ids = detail["maps"][0]["map"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|node| node["apparatus_id"].as_str())
        .collect::<BTreeSet<_>>();
    assert!(apparatus_ids.contains(PRINT));
    assert!(
        apparatus_ids.contains(LAMINATION),
        "one order's full cross-machine map survives detail loading"
    );
    assert!(
        detail["maps"][0]["program"]["operations"]
            .as_array()
            .is_some_and(|ops| !ops.is_empty())
    );
    assert!(
        detail["queue_action_controls"]
            .as_object()
            .unwrap()
            .is_empty(),
        "master summary detail must not construct hidden worker action authority"
    );
    assert!(detail["queue_states"].as_object().unwrap().is_empty());
    assert_eq!(
        fixture
            .get(&format!("{LIST}/{unrelated}"), &fixture.master)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fixture.get(&detail_path, &fixture.other).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        fixture.get(&detail_path, "").await.status(),
        StatusCode::UNAUTHORIZED
    );

    let revision = fixture.state.production_maps.snapshot_revision();
    let preparation = fixture.state.preparation.as_ref().unwrap();
    preparation
        .unassign_responsibility(MaterialResponsibilityDelete {
            principal_ref: MASTER.into(),
            material_id: "list-film".into(),
        })
        .await
        .unwrap();
    preparation
        .assign_responsibility(MaterialResponsibilityAssign {
            principal_ref: MASTER.into(),
            material_id: "list-foil".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        fixture.state.production_maps.snapshot_revision(),
        revision,
        "responsibility changes have independent authority from queue revisions"
    );
    let changed = fixture.page("limit=1").await;
    assert_ne!(changed["scope"], first["scope"]);
    assert_eq!(ids(&changed), vec![unrelated.as_str()]);
    assert_eq!(changed["rows"][0]["position"], 2);
    let stale_path = format!(
        "{LIST}?apparatus={PRINT}&limit=1&cursor={}",
        urlencoding::encode(&stale_cursor)
    );
    assert_eq!(
        fixture.get(&stale_path, &fixture.master).await.status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        fixture.get(&detail_path, &fixture.master).await.status(),
        StatusCode::NOT_FOUND,
        "previously loaded order details must not survive responsibility withdrawal"
    );
    assert_eq!(
        fixture
            .get(&format!("{LIST}/{unrelated}"), &fixture.master)
            .await
            .status(),
        StatusCode::OK
    );

    preparation
        .unassign_responsibility(MaterialResponsibilityDelete {
            principal_ref: MASTER.into(),
            material_id: "list-foil".into(),
        })
        .await
        .unwrap();
    let empty = fixture.page("limit=30").await;
    assert_eq!(empty["total"], 0);
    assert!(ids(&empty).is_empty());
    assert_eq!(
        fixture
            .get(&format!("{LIST}/{unrelated}"), &fixture.master)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    fixture.close().await;
}
