use std::path::{Path, PathBuf};

use super::app_local_store::{LocalStoreBackend, derive_lmdb_path, local_store_backend_from};
use crate::config::AppConfig;
use crate::core::apparatus_groups::ApparatusGroupUpsert;
use crate::core::auth::models::{Principal, PrincipalRole};
use crate::core::calculate_orders::{CalculateOrderError, CalculateOrderTemplate};
use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapEdge, ProductionMapError, ProductionMapNode,
    ProductionMapNodeKind,
};
use crate::core::push::ports::PushServiceError;
use crate::core::rps_batch::{RpsBatchServiceError, RpsBatchStartRequest};
use crate::db::postgres::apply_foundation_migration;

static MINI_ENGINE_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

#[tokio::test]
async fn app_state_leaves_mini_engine_disabled_without_database_url() {
    let _guard = MINI_ENGINE_ENV_LOCK.lock().await;
    unsafe {
        std::env::remove_var("MINI_ERP_DATABASE_URL");
    }

    let state = super::AppState::new(test_app_config());

    assert!(state.mini_engine.is_none());
}

#[tokio::test]
async fn app_state_does_not_fallback_production_maps_to_sqlite_without_database_url() {
    let _guard = MINI_ENGINE_ENV_LOCK.lock().await;
    unsafe {
        std::env::remove_var("MINI_ERP_DATABASE_URL");
    }

    let state = super::AppState::new(test_app_config());
    let result = state.production_maps.maps().await;

    assert_eq!(result, Err(ProductionMapError::StoreFailed));
}

#[tokio::test]
async fn app_state_builds_lazy_mini_engine_when_database_url_is_configured() {
    let _guard = MINI_ENGINE_ENV_LOCK.lock().await;
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
    let _guard = MINI_ENGINE_ENV_LOCK.lock().await;
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
async fn app_state_uses_postgres_calculate_orders_when_database_url_is_configured() {
    let _guard = MINI_ENGINE_ENV_LOCK.lock().await;
    unsafe {
        std::env::set_var(
            "MINI_ERP_DATABASE_URL",
            "postgres://wikki@127.0.0.1:1/mini_rs_erp_unavailable",
        );
        std::env::set_var("MINI_ERP_PG_ACQUIRE_TIMEOUT_MS", "50");
    }

    let state = super::AppState::new(test_app_config());
    let result = state
        .calculate_orders
        .upsert("admin:admin", calculate_order_template("Z-1001"))
        .await;

    assert!(matches!(result, Err(CalculateOrderError::StoreFailed)));

    unsafe {
        std::env::remove_var("MINI_ERP_DATABASE_URL");
        std::env::remove_var("MINI_ERP_PG_ACQUIRE_TIMEOUT_MS");
    }
}

#[tokio::test]
async fn app_state_uses_postgres_production_maps_when_database_url_is_configured() {
    let _guard = MINI_ENGINE_ENV_LOCK.lock().await;
    unsafe {
        std::env::set_var(
            "MINI_ERP_DATABASE_URL",
            "postgres://wikki@127.0.0.1:1/mini_rs_erp_unavailable",
        );
        std::env::set_var("MINI_ERP_PG_ACQUIRE_TIMEOUT_MS", "50");
    }

    let state = super::AppState::new(test_app_config());
    let result = state
        .production_maps
        .upsert_map(test_production_map("zakaz-404", "404"))
        .await;

    assert_eq!(result, Err(ProductionMapError::StoreFailed));

    unsafe {
        std::env::remove_var("MINI_ERP_DATABASE_URL");
        std::env::remove_var("MINI_ERP_PG_ACQUIRE_TIMEOUT_MS");
    }
}

#[tokio::test]
async fn app_state_uses_postgres_push_tokens_when_database_url_is_configured() {
    let _guard = MINI_ENGINE_ENV_LOCK.lock().await;
    unsafe {
        std::env::set_var(
            "MINI_ERP_DATABASE_URL",
            "postgres://wikki@127.0.0.1:1/mini_rs_erp_unavailable",
        );
        std::env::set_var("MINI_ERP_PG_ACQUIRE_TIMEOUT_MS", "50");
    }

    let state = super::AppState::new(test_app_config());
    let result = state
        .push
        .register(&test_principal(), "token-1", "ios")
        .await;

    assert!(matches!(result, Err(PushServiceError::StoreFailed)));

    unsafe {
        std::env::remove_var("MINI_ERP_DATABASE_URL");
        std::env::remove_var("MINI_ERP_PG_ACQUIRE_TIMEOUT_MS");
    }
}

#[tokio::test]
async fn app_state_uses_postgres_rps_batch_when_database_url_is_configured() {
    let _guard = MINI_ENGINE_ENV_LOCK.lock().await;
    unsafe {
        std::env::set_var(
            "MINI_ERP_DATABASE_URL",
            "postgres://wikki@127.0.0.1:1/mini_rs_erp_unavailable",
        );
        std::env::set_var("MINI_ERP_PG_ACQUIRE_TIMEOUT_MS", "50");
    }

    let state = super::AppState::new(test_app_config());
    let result = state
        .rps_batch
        .start(
            &test_principal(),
            RpsBatchStartRequest {
                driver_url: "http://127.0.0.1:39117".to_string(),
                item_code: "INK-001".to_string(),
                item_name: "Ink".to_string(),
                warehouse: "Stores - A".to_string(),
                ..RpsBatchStartRequest::default()
            },
        )
        .await;

    assert!(matches!(result, Err(RpsBatchServiceError::StoreFailed)));

    unsafe {
        std::env::remove_var("MINI_ERP_DATABASE_URL");
        std::env::remove_var("MINI_ERP_PG_ACQUIRE_TIMEOUT_MS");
    }
}

#[tokio::test]
async fn app_state_routes_gscale_receipts_to_postgres_when_database_url_is_configured() {
    let _guard = MINI_ENGINE_ENV_LOCK.lock().await;
    unsafe {
        std::env::set_var(
            "MINI_ERP_DATABASE_URL",
            "postgres://wikki@127.0.0.1:1/mini_rs_erp_unavailable",
        );
        std::env::set_var("MINI_ERP_PG_ACQUIRE_TIMEOUT_MS", "50");
    }

    let state = super::AppState::new(AppConfig {
        ..test_app_config()
    });

    assert!(state.gscale.receipt_store_configured_for_test());
    assert!(!state.rezka.repack_store_configured_for_test());

    unsafe {
        std::env::remove_var("MINI_ERP_DATABASE_URL");
        std::env::remove_var("MINI_ERP_PG_ACQUIRE_TIMEOUT_MS");
    }
}

#[tokio::test]
async fn app_state_never_attaches_legacy_remote_clients() {
    let _guard = MINI_ENGINE_ENV_LOCK.lock().await;
    unsafe {
        std::env::remove_var("MINI_ERP_DATABASE_URL");
    }

    let state = super::AppState::new(AppConfig {
        ..test_app_config()
    });

    assert!(!state.gscale.receipt_store_configured_for_test());
    assert!(!state.rezka.repack_store_configured_for_test());
}

#[tokio::test]
async fn app_state_admin_catalog_fails_closed_without_postgres() {
    let _guard = MINI_ENGINE_ENV_LOCK.lock().await;
    unsafe {
        std::env::remove_var("MINI_ERP_DATABASE_URL");
    }

    let state = super::AppState::new(test_app_config());

    assert!(state.admin.supplier_summary(300).await.is_err());
    assert!(state.admin.customers_page("", 20, 0).await.is_err());
    assert!(
        state
            .admin
            .items_page_by_group("", "", 20, 0)
            .await
            .is_err()
    );
    assert!(state.admin.item_groups("", 20).await.is_err());
}

fn calculate_order_template(code: &str) -> CalculateOrderTemplate {
    CalculateOrderTemplate {
        id: String::new(),
        code: code.to_string(),
        name: "CPP 600".to_string(),
        saved_at: String::new(),
        order_number: code.to_string(),
        customer_ref: "CUST-001".to_string(),
        customer: "Mijoz".to_string(),
        item_code: "ITEM-001".to_string(),
        product: "cpp / 20 mikron / 600".to_string(),
        status: String::new(),
        material_display: String::new(),
        color: String::new(),
        image_id: String::new(),
        image_name: String::new(),
        image_mime: String::new(),
        image_size_bytes: 0,
        image_url: String::new(),
        frame_product_size_mm: 515.0,
        frame_count: 1.0,
        edge_allowance_mm: 15.0,
        width_mm: 530.0,
        waste_percent: 3.0,
        roll_count: Some(7.0),
        first_layer_material: "pet".to_string(),
        first_layer_micron: "12".to_string(),
        second_layer_material: "pe oq".to_string(),
        second_layer_micron: "30".to_string(),
        third_layer_material: String::new(),
        third_layer_micron: String::new(),
        note: String::new(),
        kg: 0.0,
        source_map_id: String::new(),
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
        order_kg: None,
        base_length: None,
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
        default_target_warehouse: String::new(),
        http_timeout: std::time::Duration::from_secs(1),
        session_store_path: temp_file_path("sessions.json"),
        profile_store_path: temp_file_path("profiles.json"),
        push_token_store_path: temp_file_path("push.json"),
        session_ttl_seconds: Some(3600),
        supplier_prefix: "10".to_string(),
        werka_prefix: "20".to_string(),
        werka_code: String::new(),
        werka_name: "Werka".to_string(),
        werka_phone: "+99888862440".to_string(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: "Material taminotchisi".to_string(),
        material_taminotchi_phone: String::new(),
        admin_phone: "+998880000000".to_string(),
        admin_name: "Admin".to_string(),
        admin_code: "19621978".to_string(),
    }
}

fn test_principal() -> Principal {
    Principal {
        role: PrincipalRole::Werka,
        display_name: "Werka".to_string(),
        legal_name: "Werka".to_string(),
        ref_: "werka".to_string(),
        phone: "+99888862440".to_string(),
        avatar_url: String::new(),
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
        rezka_kadr_count: None,
        rezka_label_length: None,
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
