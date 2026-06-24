use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres};

const DEFAULT_MIN_CONNECTIONS: u32 = 2;
const DEFAULT_MAX_CONNECTIONS: u32 = 16;
const DEFAULT_ACQUIRE_TIMEOUT_MS: u64 = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresConfig {
    pub database_url: String,
    pub min_connections: u32,
    pub max_connections: u32,
    pub acquire_timeout: Duration,
}

impl PostgresConfig {
    #[allow(dead_code)]
    pub fn from_env() -> Result<Self, PostgresConfigError> {
        Self::from_env_with(|key| std::env::var(key).ok())
    }

    pub fn from_env_with(
        get_env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, PostgresConfigError> {
        let database_url = get_env("MINI_ERP_DATABASE_URL")
            .unwrap_or_default()
            .trim()
            .to_string();
        if database_url.is_empty() {
            return Err(PostgresConfigError::MissingDatabaseUrl);
        }

        let max_connections = env_u32(&get_env, "MINI_ERP_PG_MAX_CONNECTIONS")
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_MAX_CONNECTIONS);
        let min_connections = env_u32(&get_env, "MINI_ERP_PG_MIN_CONNECTIONS")
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_MIN_CONNECTIONS)
            .min(max_connections);
        let acquire_timeout = Duration::from_millis(
            env_u64(&get_env, "MINI_ERP_PG_ACQUIRE_TIMEOUT_MS")
                .filter(|value| *value > 0)
                .unwrap_or(DEFAULT_ACQUIRE_TIMEOUT_MS),
        );

        Ok(Self {
            database_url,
            min_connections,
            max_connections,
            acquire_timeout,
        })
    }

    #[allow(dead_code)]
    pub fn pool_options(&self) -> PgPoolOptions {
        PgPoolOptions::new()
            .min_connections(self.min_connections)
            .max_connections(self.max_connections)
            .acquire_timeout(self.acquire_timeout)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostgresConfigError {
    MissingDatabaseUrl,
}

#[derive(Debug, thiserror::Error)]
pub enum PostgresBootstrapError {
    #[error("MINI_ERP_DATABASE_URL is required")]
    MissingDatabaseUrl,
    #[error("postgres connection failed: {0}")]
    Connect(#[source] sqlx::Error),
    #[error("postgres migration failed: {0}")]
    Migrate(#[source] sqlx::Error),
}

pub async fn connect_and_migrate_required() -> Result<PgPool, PostgresBootstrapError> {
    let config =
        PostgresConfig::from_env().map_err(|_| PostgresBootstrapError::MissingDatabaseUrl)?;
    let pool = config
        .pool_options()
        .connect(&config.database_url)
        .await
        .map_err(PostgresBootstrapError::Connect)?;
    apply_foundation_migration(&pool)
        .await
        .map_err(PostgresBootstrapError::Migrate)?;
    Ok(pool)
}

#[allow(dead_code)]
pub fn foundation_migration_sql() -> &'static str {
    include_str!("../../migrations/postgres/0001_mini_erp_foundation.sql")
}

#[allow(dead_code)]
pub async fn apply_foundation_migration(pool: &PgPool) -> Result<(), sqlx::Error> {
    for statement in split_sql_statements(foundation_migration_sql()) {
        sqlx::query::<Postgres>(&statement).execute(pool).await?;
    }
    Ok(())
}

fn env_u32(get_env: &impl Fn(&str) -> Option<String>, key: &str) -> Option<u32> {
    get_env(key).and_then(|raw| raw.trim().parse::<u32>().ok())
}

fn env_u64(get_env: &impl Fn(&str) -> Option<String>, key: &str) -> Option<u64> {
    get_env(key).and_then(|raw| raw.trim().parse::<u64>().ok())
}

fn split_sql_statements(sql: &str) -> Vec<String> {
    sql.split(';')
        .map(str::trim)
        .filter(|statement| !statement.is_empty())
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_config_uses_mini_erp_database_url() {
        let config = PostgresConfig::from_env_with(|key| match key {
            "MINI_ERP_DATABASE_URL" => {
                Some("postgres://mini:secret@127.0.0.1:5432/mini_rs_erp".to_string())
            }
            _ => None,
        })
        .expect("config");

        assert_eq!(
            config.database_url,
            "postgres://mini:secret@127.0.0.1:5432/mini_rs_erp"
        );
        assert_eq!(config.max_connections, 16);
        assert_eq!(config.min_connections, 2);
    }

    #[test]
    fn postgres_config_rejects_blank_database_url() {
        let error = PostgresConfig::from_env_with(|_| Some(" ".to_string()))
            .expect_err("blank url rejected");

        assert_eq!(error, PostgresConfigError::MissingDatabaseUrl);
    }

    #[tokio::test]
    async fn postgres_bootstrap_requires_database_url() {
        let previous = std::env::var("MINI_ERP_DATABASE_URL").ok();
        unsafe {
            std::env::remove_var("MINI_ERP_DATABASE_URL");
        }

        let error = connect_and_migrate_required()
            .await
            .expect_err("missing database url must fail");

        assert!(matches!(error, PostgresBootstrapError::MissingDatabaseUrl));
        unsafe {
            if let Some(value) = previous {
                std::env::set_var("MINI_ERP_DATABASE_URL", value);
            }
        }
    }

    #[test]
    fn postgres_foundation_migration_defines_core_tables() {
        let migration = foundation_migration_sql();

        for table in [
            "mini_orders",
            "mini_order_products",
            "mini_quick_order_templates",
            "mini_quick_order_images",
            "mini_push_tokens",
            "mini_items",
            "mini_item_groups",
            "mini_production_maps",
            "mini_production_map_nodes",
            "mini_production_map_edges",
            "mini_apparatus",
            "mini_apparatus_groups",
            "mini_workers",
            "mini_worker_groups",
            "mini_queue_sequences",
            "mini_queue_states",
            "mini_warehouses",
            "mini_qolip_locations",
            "mini_gscale_receipts",
            "mini_raw_material_stock",
            "mini_finished_goods_stock",
            "mini_rps_batches",
            "mini_engine_events",
            "mini_idempotency_keys",
        ] {
            assert!(
                migration.contains(&format!("CREATE TABLE IF NOT EXISTS {table}")),
                "missing table {table}"
            );
        }

        for forbidden in ["tabWork Order", "tabBOM", "tabStock Entry", "doctype"] {
            assert!(
                !migration.to_lowercase().contains(&forbidden.to_lowercase()),
                "migration must not contain legacy term {forbidden}"
            );
        }
    }

    #[test]
    fn postgres_migration_runner_splits_foundation_sql() {
        let statements = split_sql_statements(foundation_migration_sql());

        assert!(statements.len() > 12);
        assert!(
            statements
                .iter()
                .any(|statement| statement.starts_with("CREATE TABLE IF NOT EXISTS mini_orders"))
        );
        assert!(statements.iter().all(|statement| !statement.contains(';')));
    }

    #[test]
    fn postgres_foundation_migration_indexes_apparatus_case_insensitively() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(
            migration.contains("idx_mini_apparatus_groups_lower_name")
                && migration.contains("lower(name)")
        );
        assert!(migration.contains("idx_mini_apparatus_lower_name"));
    }

    #[test]
    fn postgres_foundation_migration_keeps_quick_template_codes_unique() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains("idx_mini_quick_order_templates_owner_lower_code"));
        assert!(migration.contains("idx_mini_quick_order_templates_owner_quick_key"));
        assert!(!migration.contains("owner_key_unique unique"));
    }

    #[test]
    fn postgres_foundation_migration_backfills_quick_template_frame_fields() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains("quick_template_dimensions"));
        assert!(migration.contains("frame_product_size_mm"));
        assert!(migration.contains("frame_count"));
        assert!(migration.contains("jsonb_set"));
    }

    #[test]
    fn postgres_foundation_migration_leaves_production_order_number_to_store_logic() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains("idx_mini_production_maps_order_number"));
        assert!(!migration.contains("mini_production_maps_order_number_unique"));
    }

    #[test]
    fn postgres_foundation_migration_guards_one_open_order_run_session() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains("idx_mini_order_run_sessions_one_open"));
        assert!(migration.contains("where status in ('active', 'paused')"));
    }

    #[test]
    fn postgres_foundation_migration_persists_bosma_progress_metrics() {
        let migration = foundation_migration_sql().to_lowercase();

        for column in [
            "return_ink_kg",
            "lamination_print_leftover_rolls",
            "lamination_film_leftover_rolls",
            "rezka_bosma_waste",
            "rezka_lamination_waste",
            "rezka_edge_waste",
            "total_waste",
            "finished_goods_kg",
            "finished_goods_meter",
            "description",
        ] {
            assert!(
                migration.contains(column),
                "missing progress metric column {column}"
            );
        }
    }

    #[test]
    fn postgres_foundation_migration_indexes_wip_apparatus_key() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains("current_apparatus_key"));
        assert!(migration.contains("idx_mini_progress_batches_wip_status_apparatus_key"));
        assert!(migration.contains("wip_status, current_apparatus_key, updated_at desc"));
    }

    #[tokio::test]
    #[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test"]
    async fn postgres_live_foundation_migration_applies_to_clean_database() {
        let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
        let db_name = std::env::var("MINI_ERP_TEST_DATABASE_NAME")
            .unwrap_or_else(|_| "mini_rs_erp_test".to_string());
        assert!(
            db_name.starts_with("mini_rs_erp_test"),
            "test database name must start with mini_rs_erp_test"
        );

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
            .expect("apply foundation migration");

        let table_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM information_schema.tables
             WHERE table_schema = 'public'
               AND table_name IN (
                 'mini_orders',
                 'mini_order_products',
                 'mini_quick_order_templates',
                 'mini_quick_order_images',
                 'mini_items',
                 'mini_item_groups',
                 'mini_production_maps',
                 'mini_production_map_nodes',
                 'mini_production_map_edges',
                 'mini_apparatus',
                 'mini_apparatus_groups',
                 'mini_workers',
                 'mini_worker_groups',
                 'mini_qolip_locations',
                 'mini_queue_sequences',
                 'mini_queue_states',
                 'mini_apparatus_queue_policies',
                 'mini_queue_action_events',
                 'mini_engine_events',
                 'mini_idempotency_keys'
               )",
        )
        .fetch_one(&pool)
        .await
        .expect("count tables");
        assert_eq!(table_count, 20);

        sqlx::query(
            "INSERT INTO mini_idempotency_keys (key, domain, action, entity_id)
             VALUES ('test-key-1', 'production_maps', 'batch_move', 'zakaz-1')",
        )
        .execute(&pool)
        .await
        .expect("insert idempotency key");

        let duplicate = sqlx::query(
            "INSERT INTO mini_idempotency_keys (key, domain, action)
             VALUES ('test-key-1', 'production_maps', 'batch_move')",
        )
        .execute(&pool)
        .await;
        assert!(duplicate.is_err(), "idempotency key must be unique");

        pool.close().await;

        let admin_pool = sqlx::PgPool::connect(&admin_url)
            .await
            .expect("admin db cleanup");
        sqlx::query(&format!(
            r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
        ))
        .execute(&admin_pool)
        .await
        .expect("cleanup test db");
        admin_pool.close().await;
    }
}
