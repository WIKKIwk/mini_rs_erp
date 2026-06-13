use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;

use super::app_local_store::{LocalStoreBackend, derive_lmdb_path, local_store_backend_from};
use super::catalog_cache_sync_interval;
use crate::config::AppConfig;
use crate::core::apparatus_groups::ApparatusGroupUpsert;
use crate::core::production_map::{
    MemoryProductionMapStore, ProductionMapDefinition, ProductionMapEdge, ProductionMapNode,
    ProductionMapNodeKind, ProductionMapService,
};
use crate::db::postgres::apply_foundation_migration;
use crate::erpnext::production_order::{ProductionOrderErpError, ProductionOrderErpSource};

static MINI_ENGINE_ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn local_store_backend_defaults_to_lmdb_for_production() {
    assert_eq!(local_store_backend_from(None), LocalStoreBackend::Lmdb);
    assert_eq!(local_store_backend_from(Some("")), LocalStoreBackend::Lmdb);
    assert_eq!(
        local_store_backend_from(Some("unknown")),
        LocalStoreBackend::Lmdb
    );
}

#[test]
fn local_store_backend_accepts_explicit_json_and_lmdb() {
    assert_eq!(
        local_store_backend_from(Some("json")),
        LocalStoreBackend::Json
    );
    assert_eq!(
        local_store_backend_from(Some(" JSON ")),
        LocalStoreBackend::Json
    );
    assert_eq!(
        local_store_backend_from(Some("lmdb")),
        LocalStoreBackend::Lmdb
    );
    assert_eq!(
        local_store_backend_from(Some(" LMDB ")),
        LocalStoreBackend::Lmdb
    );
}

#[test]
fn lmdb_path_defaults_next_to_legacy_json_path() {
    assert_eq!(
        derive_lmdb_path(Path::new("data/mobile_sessions.json"), "fallback.lmdb"),
        PathBuf::from("data/mobile_sessions.lmdb")
    );
    assert_eq!(
        derive_lmdb_path(Path::new(""), "fallback.lmdb"),
        PathBuf::from("fallback.lmdb")
    );
}

#[test]
fn catalog_cache_sync_interval_defaults_to_one_second() {
    unsafe {
        std::env::remove_var("ERP_CATALOG_CACHE_SYNC_INTERVAL_MS");
    }

    assert_eq!(
        catalog_cache_sync_interval(),
        std::time::Duration::from_secs(1)
    );
}

#[tokio::test]
async fn app_state_leaves_mini_engine_disabled_without_database_url() {
    let _guard = MINI_ENGINE_ENV_LOCK.lock().expect("env lock");
    unsafe {
        std::env::remove_var("MINI_ERP_DATABASE_URL");
    }

    let state = super::AppState::new(test_app_config());

    assert!(state.mini_engine.is_none());
}

#[tokio::test]
async fn app_state_builds_lazy_mini_engine_when_database_url_is_configured() {
    let _guard = MINI_ENGINE_ENV_LOCK.lock().expect("env lock");
    unsafe {
        std::env::set_var(
            "MINI_ERP_DATABASE_URL",
            "postgres://wikki@127.0.0.1:5432/mini_rs_erp_test_lazy",
        );
    }

    let state = super::AppState::new(test_app_config());

    assert!(state.mini_engine.is_some());

    unsafe {
        std::env::remove_var("MINI_ERP_DATABASE_URL");
    }
}

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_app_apparatus_groups"]
async fn app_state_routes_apparatus_groups_to_postgres_when_database_url_is_configured() {
    let _guard = MINI_ENGINE_ENV_LOCK.lock().expect("env lock");
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let db_name = "mini_rs_erp_test_app_apparatus_groups";
    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create test db");
    admin_pool.close().await;

    let test_url = format!("postgres://wikki@127.0.0.1:5432/{db_name}");
    let pool = sqlx::PgPool::connect(&test_url).await.expect("test db");
    apply_foundation_migration(&pool)
        .await
        .expect("apply migration");
    unsafe {
        std::env::set_var("MINI_ERP_DATABASE_URL", &test_url);
    }

    let state = super::AppState::new(test_app_config());
    state
        .apparatus_groups
        .upsert_group(ApparatusGroupUpsert {
            name: "pechat".to_string(),
            apparatus: vec!["7 ta rangli pechat".to_string()],
        })
        .await
        .expect("save group through app state");

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mini_apparatus_groups")
        .fetch_one(&pool)
        .await
        .expect("count postgres groups");
    assert_eq!(count, 1);

    unsafe {
        std::env::remove_var("MINI_ERP_DATABASE_URL");
    }
    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("cleanup test db");
    admin_pool.close().await;
}

#[tokio::test]
async fn erp_work_order_sync_once_upserts_maps_into_local_cache() {
    let service = ProductionMapService::new(Arc::new(MemoryProductionMapStore::new()));
    let source: Arc<dyn ProductionOrderErpSource> = Arc::new(FakeProductionOrderSource {
        maps: vec![test_production_map("zakaz-333", "333")],
    });

    let synced = super::sync_erp_work_orders_once(service.clone(), source)
        .await
        .expect("sync");

    assert_eq!(synced, 1);
    let saved = service
        .map("zakaz-333")
        .await
        .expect("map read")
        .expect("saved map");
    assert_eq!(saved.map.id, "zakaz-333");
    assert_eq!(saved.map.order_number, "333");
}

#[derive(Debug)]
struct FakeProductionOrderSource {
    maps: Vec<ProductionMapDefinition>,
}

#[async_trait]
impl ProductionOrderErpSource for FakeProductionOrderSource {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionOrderErpError> {
        Ok(self.maps.clone())
    }
}

fn test_production_map(id: &str, order_number: &str) -> ProductionMapDefinition {
    ProductionMapDefinition {
        id: id.to_string(),
        product_code: "ITEM-333".to_string(),
        title: "test map".to_string(),
        code: order_number.to_string(),
        order_number: order_number.to_string(),
        roll_count: Some(7.0),
        width_mm: Some(630.0),
        nodes: vec![
            test_node("start", ProductionMapNodeKind::Start, "Start", 0.0, 0.0),
            test_node("task", ProductionMapNodeKind::Task, "test map", 0.0, 120.0),
            test_node(
                "apparatus-1",
                ProductionMapNodeKind::Apparatus,
                "7 ta rangli pechat - A",
                0.0,
                240.0,
            ),
            test_node("end", ProductionMapNodeKind::End, "test map", 0.0, 360.0),
        ],
        edges: vec![
            test_edge("start", "task"),
            test_edge("task", "apparatus-1"),
            test_edge("apparatus-1", "end"),
        ],
    }
}

fn test_app_config() -> AppConfig {
    AppConfig {
        bind_addr: "127.0.0.1:0".parse().expect("addr"),
        erp_url: String::new(),
        erp_api_key: String::new(),
        erp_api_secret: String::new(),
        default_target_warehouse: String::new(),
        erp_timeout: std::time::Duration::from_secs(1),
        session_store_path: temp_file_path("sessions.json"),
        profile_store_path: temp_file_path("profiles.json"),
        push_token_store_path: temp_file_path("push.json"),
        admin_supplier_store_path: temp_file_path("admin.json"),
        session_ttl_seconds: Some(3600),
        supplier_prefix: "10".to_string(),
        werka_prefix: "20".to_string(),
        werka_code: String::new(),
        werka_name: "Werka".to_string(),
        werka_phone: "+99888862440".to_string(),
        admin_phone: "+998880000000".to_string(),
        admin_name: "Admin".to_string(),
        admin_code: "19621978".to_string(),
        direct_read_enabled: false,
        direct_site_config_path: String::new(),
        direct_db_host: String::new(),
        direct_db_port: None,
        direct_db_user: String::new(),
        direct_db_password: String::new(),
        direct_db_name: String::new(),
        catalog_cache_enabled: false,
        catalog_cache_fallback_direct_db: true,
        catalog_cache_path: temp_file_path("catalog.sqlite"),
    }
}

fn temp_file_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "mini-rs-erp-app-test-{}-{name}",
        std::process::id()
    ))
}

fn test_node(
    id: &str,
    kind: ProductionMapNodeKind,
    title: &str,
    x: f64,
    y: f64,
) -> ProductionMapNode {
    ProductionMapNode {
        id: id.to_string(),
        kind,
        title: title.to_string(),
        formula: None,
        role_code: String::new(),
        item_code: String::new(),
        qty_formula: String::new(),
        from_location: String::new(),
        to_location: String::new(),
        alternative_group_id: String::new(),
        alternative_group_label: String::new(),
        alternative_assigned_title: String::new(),
        x,
        y,
    }
}

fn test_edge(from: &str, to: &str) -> ProductionMapEdge {
    ProductionMapEdge {
        from: from.to_string(),
        to: to.to_string(),
        branch: String::new(),
    }
}
