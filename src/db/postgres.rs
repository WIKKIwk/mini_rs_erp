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

    #[test]
    fn postgres_foundation_migration_defines_core_tables() {
        let migration = foundation_migration_sql();

        for table in [
            "mini_orders",
            "mini_order_products",
            "mini_quick_order_templates",
            "mini_quick_order_images",
            "mini_production_maps",
            "mini_production_map_nodes",
            "mini_production_map_edges",
            "mini_apparatus",
            "mini_apparatus_groups",
            "mini_queue_sequences",
            "mini_queue_states",
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
    fn postgres_foundation_migration_indexes_apparatus_groups_case_insensitively() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(
            migration.contains("idx_mini_apparatus_groups_lower_name")
                && migration.contains("lower(name)")
        );
    }

    #[test]
    fn postgres_foundation_migration_keeps_quick_template_codes_unique() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains("idx_mini_quick_order_templates_owner_lower_code"));
        assert!(migration.contains("idx_mini_quick_order_templates_owner_quick_key"));
        assert!(!migration.contains("owner_key_unique unique"));
    }

    #[test]
    fn postgres_foundation_migration_leaves_production_order_number_to_store_logic() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains("idx_mini_production_maps_order_number"));
        assert!(!migration.contains("mini_production_maps_order_number_unique"));
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
                 'mini_production_maps',
                 'mini_production_map_nodes',
                 'mini_production_map_edges',
                 'mini_apparatus',
                 'mini_apparatus_groups',
                 'mini_queue_sequences',
                 'mini_queue_states',
                 'mini_engine_events',
                 'mini_idempotency_keys'
               )",
        )
        .fetch_one(&pool)
        .await
        .expect("count tables");
        assert_eq!(table_count, 13);

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
