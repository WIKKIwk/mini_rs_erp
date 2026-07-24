use std::time::Duration;

use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres, Transaction};

const DEFAULT_MIN_CONNECTIONS: u32 = 2;
const DEFAULT_MAX_CONNECTIONS: u32 = 16;
const DEFAULT_ACQUIRE_TIMEOUT_MS: u64 = 500;
const MIGRATION_LOCK_KEY: i64 = 6_514_811_918_052_026_001;

const POSTGRES_MIGRATIONS: [(&str, &str); 26] = [
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
    (
        "0013_chat_media_incident_video",
        include_str!("../../migrations/postgres/0013_chat_media_incident_video.sql"),
    ),
    (
        "0014_raw_material_stock_corrections",
        include_str!("../../migrations/postgres/0014_raw_material_stock_corrections.sql"),
    ),
    (
        "0015_item_identity_updates",
        include_str!("../../migrations/postgres/0015_item_identity_updates.sql"),
    ),
    (
        "0016_chat_delivery_reliability",
        include_str!("../../migrations/postgres/0016_chat_delivery_reliability.sql"),
    ),
    (
        "0017_chat_delivery_reliability_followup",
        include_str!("../../migrations/postgres/0017_chat_delivery_reliability_followup.sql"),
    ),
    (
        "0018_item_master_without_warehouse",
        include_str!("../../migrations/postgres/0018_item_master_without_warehouse.sql"),
    ),
    (
        "0019_chat_voice_messages",
        include_str!("../../migrations/postgres/0019_chat_voice_messages.sql"),
    ),
    (
        "0020_worker_identity_lifecycle",
        include_str!("../../migrations/postgres/0020_worker_identity_lifecycle.sql"),
    ),
    (
        "0021_rps_batch_history",
        include_str!("../../migrations/postgres/0021_rps_batch_history.sql"),
    ),
    (
        "0022_rps_batch_codes",
        include_str!("../../migrations/postgres/0022_rps_batch_codes.sql"),
    ),
    (
        "0023_qolip_13_rows",
        include_str!("../../migrations/postgres/0023_qolip_13_rows.sql"),
    ),
    (
        "0024_qolip_legacy_lookup_index",
        include_str!("../../migrations/postgres/0024_qolip_legacy_lookup_index.sql"),
    ),
    (
        "0025_order_control_state",
        include_str!("../../migrations/postgres/0025_order_control_state.sql"),
    ),
    (
        "0026_order_freeze_request_chat_cards",
        include_str!("../../migrations/postgres/0026_order_freeze_request_chat_cards.sql"),
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
    apply_postgres_migrations(pool, &POSTGRES_MIGRATIONS).await
}

async fn apply_postgres_migrations(
    pool: &PgPool,
    migrations: &[(&str, &str)],
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(MIGRATION_LOCK_KEY)
        .execute(&mut *tx)
        .await?;
    ensure_migration_history(&mut tx).await?;
    for &(version, sql) in migrations {
        apply_migration(&mut tx, version, sql).await?;
    }
    tx.commit().await
}

#[cfg(test)]
pub(crate) async fn apply_postgres_migrations_through(
    pool: &PgPool,
    migration_count: usize,
) -> Result<(), sqlx::Error> {
    apply_postgres_migrations(
        pool,
        &POSTGRES_MIGRATIONS[..migration_count.min(POSTGRES_MIGRATIONS.len())],
    )
    .await
}

async fn ensure_migration_history(tx: &mut Transaction<'_, Postgres>) -> Result<(), sqlx::Error> {
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
    sqlx::query("INSERT INTO mini_schema_migrations (version, checksum) VALUES ($1, $2)")
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
    sql[index..].chars().next().map(char::len_utf8).unwrap_or(1)
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

include!("postgres_inline_tests.rs");
