use std::time::Duration;

use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres, Transaction};

const DEFAULT_MIN_CONNECTIONS: u32 = 2;
const DEFAULT_MAX_CONNECTIONS: u32 = 16;
const DEFAULT_ACQUIRE_TIMEOUT_MS: u64 = 500;
const MIGRATION_LOCK_KEY: i64 = 6_514_811_918_052_026_001;

const POSTGRES_MIGRATIONS: [(&str, &str); 12] = [
    (
        "0001_mini_erp_foundation",
        include_str!("../../migrations/postgres/0001_mini_erp_foundation.sql"),
    ),
    (
        "0002_order_integrity",
        include_str!("../../migrations/postgres/0002_order_integrity.sql"),
    ),
    (
        "0003_erp_data_integrity",
        include_str!("../../migrations/postgres/0003_erp_data_integrity.sql"),
    ),
    (
        "0004_system_users",
        include_str!("../../migrations/postgres/0004_system_users.sql"),
    ),
    (
        "0005_chat",
        include_str!("../../migrations/postgres/0005_chat.sql"),
    ),
    (
        "0006_boyoqchi_returned_paint",
        include_str!("../../migrations/postgres/0006_boyoqchi_returned_paint.sql"),
    ),
    (
        "0007_runtime_table_ownership",
        include_str!("../../migrations/postgres/0007_runtime_table_ownership.sql"),
    ),
    (
        "0008_returned_paint_calculations",
        include_str!("../../migrations/postgres/0008_returned_paint_calculations.sql"),
    ),
    (
        "0009_returned_paint_solvent_calculations",
        include_str!("../../migrations/postgres/0009_returned_paint_solvent_calculations.sql"),
    ),
    (
        "0010_returned_paint_image_workflow",
        include_str!("../../migrations/postgres/0010_returned_paint_image_workflow.sql"),
    ),
    (
        "0011_chat_media_foundation",
        include_str!("../../migrations/postgres/0011_chat_media_foundation.sql"),
    ),
    (
        "0012_chat_media_v1",
        include_str!("../../migrations/postgres/0012_chat_media_v1.sql"),
    ),
];

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
    POSTGRES_MIGRATIONS[0].1
}

#[allow(dead_code)]
pub async fn apply_foundation_migration(pool: &PgPool) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(MIGRATION_LOCK_KEY)
        .execute(&mut *tx)
        .await?;
    ensure_migration_history(&mut tx).await?;
    for (version, sql) in POSTGRES_MIGRATIONS {
        apply_migration(&mut tx, version, sql).await?;
    }
    tx.commit().await
}

async fn ensure_migration_history(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS mini_schema_migrations (
             version TEXT PRIMARY KEY,
             checksum TEXT NOT NULL,
             applied_at TIMESTAMPTZ NOT NULL DEFAULT now(),
             CONSTRAINT mini_schema_migrations_version_not_blank
                 CHECK (btrim(version) <> ''),
             CONSTRAINT mini_schema_migrations_checksum_not_blank
                 CHECK (btrim(checksum) <> '')
         )",
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn apply_migration(
    tx: &mut Transaction<'_, Postgres>,
    version: &str,
    sql: &str,
) -> Result<(), sqlx::Error> {
    let checksum = migration_checksum(sql);
    let applied_checksum = sqlx::query_scalar::<_, String>(
        "SELECT checksum FROM mini_schema_migrations WHERE version = $1",
    )
    .bind(version)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(applied_checksum) = applied_checksum {
        if applied_checksum != checksum {
            return Err(sqlx::Error::Protocol(format!(
                "postgres migration checksum mismatch: {version}"
            )));
        }
        return Ok(());
    }
    for statement in split_sql_statements(sql) {
        sqlx::query::<Postgres>(&statement)
            .execute(&mut **tx)
            .await?;
    }
    sqlx::query(
        "INSERT INTO mini_schema_migrations (version, checksum) VALUES ($1, $2)",
    )
    .bind(version)
    .bind(checksum)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn migration_checksum(sql: &str) -> String {
    format!("{:x}", Sha256::digest(sql.as_bytes()))
}

fn env_u32(get_env: &impl Fn(&str) -> Option<String>, key: &str) -> Option<u32> {
    get_env(key).and_then(|raw| raw.trim().parse::<u32>().ok())
}

fn env_u64(get_env: &impl Fn(&str) -> Option<String>, key: &str) -> Option<u64> {
    get_env(key).and_then(|raw| raw.trim().parse::<u64>().ok())
}

fn split_sql_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut in_single_quote = false;
    let mut dollar_quote: Option<String> = None;

    while index < sql.len() {
        if let Some(tag) = dollar_quote.as_deref() {
            if sql[index..].starts_with(tag) {
                index += tag.len();
                dollar_quote = None;
                continue;
            }
            index += next_char_len(sql, index);
            continue;
        }

        let ch = sql[index..].chars().next().expect("char");
        if in_single_quote {
            if ch == '\'' {
                let next = index + ch.len_utf8();
                if sql[next..].starts_with('\'') {
                    index = next + 1;
                    continue;
                }
                in_single_quote = false;
            }
            index += ch.len_utf8();
            continue;
        }

        if ch == '\'' {
            in_single_quote = true;
            index += ch.len_utf8();
            continue;
        }

        if ch == '$'
            && let Some(tag) = dollar_quote_tag(&sql[index..])
        {
            index += tag.len();
            dollar_quote = Some(tag);
            continue;
        }

        if ch == ';' {
            let statement = sql[start..index].trim();
            if !statement.is_empty() {
                statements.push(statement.to_string());
            }
            start = index + ch.len_utf8();
        }
        index += ch.len_utf8();
    }

    let statement = sql[start..].trim();
    if !statement.is_empty() {
        statements.push(statement.to_string());
    }
    statements
}

fn next_char_len(sql: &str, index: usize) -> usize {
    sql[index..]
        .chars()
        .next()
        .map(char::len_utf8)
        .unwrap_or(1)
}

fn dollar_quote_tag(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    if bytes.first() != Some(&b'$') {
        return None;
    }
    let mut index = 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'$' {
            return Some(input[..=index].to_string());
        }
        if !byte.is_ascii_alphanumeric() && byte != b'_' {
            return None;
        }
        index += 1;
    }
    None
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
            "mini_customers",
            "mini_customer_items",
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
            "mini_raw_material_events",
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
        assert!(statements.iter().all(|statement| !statement.ends_with(';')));
    }

    #[test]
    fn postgres_migration_runner_keeps_dollar_quoted_functions_together() {
        let statements = split_sql_statements(
            "SELECT 1;\nCREATE FUNCTION demo() RETURNS void LANGUAGE plpgsql AS $$\nBEGIN\n  PERFORM 1;\nEND;\n$$;\nSELECT 2;",
        );

        assert_eq!(statements.len(), 3);
        assert!(statements[1].contains("PERFORM 1;"));
        assert!(statements[1].contains("END;"));
    }

    #[test]
    fn postgres_migrations_are_versioned_and_checksummed() {
        let versions = POSTGRES_MIGRATIONS
            .iter()
            .map(|(version, _)| *version)
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(versions.len(), POSTGRES_MIGRATIONS.len());
        assert!(POSTGRES_MIGRATIONS.iter().all(|(version, sql)| {
            !version.trim().is_empty() && migration_checksum(sql).len() == 64
        }));
    }

    #[test]
    fn postgres_chat_migration_defines_durable_message_flow() {
        let migration = POSTGRES_MIGRATIONS[4].1.to_lowercase();

        for table in [
            "mini_chat_principals",
            "mini_chat_conversations",
            "mini_chat_conversation_members",
            "mini_chat_messages",
            "mini_chat_device_cursors",
            "mini_chat_outbox_events",
        ] {
            assert!(
                migration.contains(&format!("create table if not exists {table}")),
                "missing chat table {table}"
            );
        }
        assert!(migration.contains("mini_chat_messages_client_id_unique"));
        assert!(migration.contains("last_read_sequence"));
        assert!(migration.contains("published_at is null"));
        assert!(!migration.contains("partition by"));
    }

    #[test]
    fn postgres_chat_media_migration_defines_private_upload_foundation() {
        let migration = POSTGRES_MIGRATIONS[10].1.to_lowercase();

        for table in [
            "mini_chat_media",
            "mini_chat_message_attachments",
            "mini_chat_media_jobs",
        ] {
            assert!(
                migration.contains(&format!("create table if not exists {table}")),
                "missing chat media table {table}"
            );
        }
        assert!(migration.contains("mini_chat_media_client_upload_unique"));
        assert!(migration.contains("declared_size_bytes > 0"));
        assert!(migration.contains("declared_duration_ms between 1 and 120000"));
        assert!(migration.contains("media_kind in ('image', 'video')"));
        assert!(migration.contains("message_id text not null unique"));
        assert!(migration.contains("media_id text not null unique"));
        assert!(migration.contains("job_status = 'pending'"));
        assert!(!migration.contains("public_url"));
    }

    #[test]
    fn postgres_chat_media_v1_migration_enables_processed_attachments() {
        let migration = POSTGRES_MIGRATIONS[11].1.to_lowercase();

        assert!(migration.contains("processed_content_type"));
        assert!(migration.contains("processed_size_bytes"));
        assert!(migration.contains("'image', 'video'"));
        assert!(migration.contains("char_length(body) between 0 and 4000"));
        assert!(migration.contains("idx_mini_chat_media_jobs_claim"));
        assert!(!migration.contains("public_url"));
    }

    #[test]
    fn postgres_boyoqchi_migration_defines_role_inbox() {
        let migration = POSTGRES_MIGRATIONS[5].1.to_lowercase();

        assert!(migration.contains("'qolipchi', 'boyoqchi'"));
        assert!(migration.contains("create table if not exists mini_returned_paint_requests"));
        assert!(migration.contains("target_role = 'boyoqchi'"));
        assert!(migration.contains("jsonb_array_length(items_json) > 0"));
    }

    #[test]
    fn postgres_runtime_ownership_migration_repairs_service_tables() {
        let migration = POSTGRES_MIGRATIONS[6].1.to_lowercase();

        assert!(migration.contains("rolname = 'mini_rs_erp'"));
        for table in [
            "mini_system_users",
            "mini_chat_principals",
            "mini_chat_conversations",
            "mini_chat_conversation_members",
            "mini_chat_messages",
            "mini_chat_device_cursors",
            "mini_chat_outbox_events",
            "mini_returned_paint_requests",
        ] {
            assert!(migration.contains(&format!("'{table}'")));
        }
        assert!(migration.contains("owner to mini_rs_erp"));
    }

    #[test]
    fn postgres_returned_paint_calculation_migration_uses_exact_numeric_columns() {
        let migration = POSTGRES_MIGRATIONS[7].1.to_lowercase();

        assert!(migration.contains("numeric(30, 12)"));
        assert!(migration.contains("rasxot_mix_total"));
        assert!(migration.contains("final_used_alcohol"));
        assert!(migration.contains("final_used_paint"));
        assert!(migration.contains("jsonb_each"));
        assert!(migration.contains("round(rasxot_mix_total, 12)"));
        assert!(migration.contains("999999999999999999"));
    }

    #[test]
    fn postgres_returned_paint_solvent_migration_adds_all_solvent_values_to_alcohol() {
        let migration = POSTGRES_MIGRATIONS[8].1.to_lowercase();

        assert!(migration.contains("category = 'solvents'"));
        assert!(migration.contains("jsonb_each"));
        assert!(migration.contains("rasxot_direct_alcohol"));
        assert!(migration.contains("astatka_direct_alcohol"));
        assert!(migration.contains("rasxot_mix_total * 0.30::numeric"));
        assert!(migration.contains("astatka_mix_total * 0.30::numeric"));
        assert!(migration.contains("final_used_alcohol"));
    }

    #[test]
    fn postgres_returned_paint_image_migration_supports_pending_and_idempotent_completion() {
        let migration = POSTGRES_MIGRATIONS[9].1.to_lowercase();

        assert!(migration.contains("create table if not exists mini_returned_paint_images"));
        assert!(migration.contains("waiting_for_boyoqchi_input"));
        assert!(migration.contains("mini_returned_paint_requests_workflow_consistent"));
        assert!(migration.contains("jsonb_array_length(items_json) = 0"));
        assert!(migration.contains("image_size_bytes = octet_length(body)"));
        assert!(migration.contains("create unique index"));
    }

    #[test]
    fn postgres_order_integrity_migration_links_orders_and_indexes_foreign_keys() {
        let migration = POSTGRES_MIGRATIONS[1].1.to_lowercase();

        assert!(migration.contains("idx_mini_order_products_order_id"));
        assert!(migration.contains("idx_mini_customer_items_item_code"));
        assert!(migration.contains("set order_id = orders.id"));
        assert!(migration.contains("maps.id = orders.id"));
    }

    #[test]
    fn postgres_erp_integrity_migration_uses_exact_quantities_and_constraints() {
        let migration = POSTGRES_MIGRATIONS[2].1.to_lowercase();

        assert!(migration.contains("numeric(24, 9)"));
        assert!(migration.contains("mini_production_maps_width_positive"));
        assert!(migration.contains("mini_gscale_receipts_qty_positive"));
        assert!(migration.contains("mini_raw_material_events_qty_finite"));
        assert!(migration.contains("idx_mini_customers_phone_key_unique"));
        assert!(migration.contains("mini_raw_material_assignments_order_fkey"));
        assert!(migration.contains("product_form"));
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
    fn postgres_foundation_migration_keeps_qolip_codes_unique_not_item_codes() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains(
            "alter table mini_qolip_product_specs drop constraint if exists mini_qolip_product_specs_pkey"
        ));
        assert!(migration.contains("idx_mini_qolip_product_specs_qolip_code_unique"));
        assert!(migration.contains("lower(qolip_code)"));
        assert!(!migration.contains("item_code text primary key"));
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

    #[test]
    fn postgres_foundation_migration_persists_material_requirement_groups() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains("mini_apparatus_material_rules"));
        assert!(migration.contains("requirement_groups jsonb not null default '[]'::jsonb"));
        assert!(
            migration.contains("mini_apparatus_material_rules_requirement_groups_array"),
            "missing requirement_groups array constraint"
        );
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
                 'mini_idempotency_keys',
                 'mini_chat_media',
                 'mini_chat_message_attachments',
                 'mini_chat_media_jobs'
               )",
        )
        .fetch_one(&pool)
        .await
        .expect("count tables");
        assert_eq!(table_count, 23);

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
