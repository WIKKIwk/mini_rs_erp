
async fn apply_postgres_migrations(
    pool: &PgPool,
    migrations: &[(&str, &str)],
) -> Result<(), sqlx::Error> {
    validate_migration_registry(migrations)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(MIGRATION_LOCK_KEY)
        .execute(&mut *tx)
        .await?;
    ensure_migration_history(&mut tx).await?;
    validate_migration_history(&mut tx, migrations).await?;
    let applied_count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_schema_migrations")
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("SELECT set_config('mini_rs_erp.fresh_database_bootstrap', $1, true)")
        .bind(if applied_count == 0 { "on" } else { "off" })
        .execute(&mut *tx)
        .await?;
    for &(version, sql) in migrations {
        apply_migration(&mut tx, version, sql).await?;
    }
    tx.commit().await
}

pub async fn apply_postgres_migrations_through_version(
    pool: &PgPool,
    target_version: &str,
) -> Result<(), sqlx::Error> {
    let target_version = target_version.trim();
    let matches = POSTGRES_MIGRATIONS
        .iter()
        .enumerate()
        .filter(|(_, (version, _))| {
            *version == target_version
                || (target_version.len() == 4
                    && version
                        .strip_prefix(target_version)
                        .is_some_and(|suffix| suffix.starts_with('_')))
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [target_index] = matches.as_slice() else {
        return Err(sqlx::Error::Protocol(format!(
            "unknown or ambiguous postgres migration target: {target_version}"
        )));
    };
    apply_postgres_migrations(pool, &POSTGRES_MIGRATIONS[..=*target_index]).await
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

#[cfg(test)]
pub(crate) fn postgres_test_database_options(
    admin_url: &str,
    database_name: &str,
) -> PgConnectOptions {
    admin_url
        .parse::<PgConnectOptions>()
        .expect("admin database options")
        .database(database_name)
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

fn validate_migration_registry(migrations: &[(&str, &str)]) -> Result<(), sqlx::Error> {
    let mut previous_number = 0_usize;
    for &(version, sql) in migrations {
        let number = version
            .split_once('_')
            .and_then(|(prefix, _)| prefix.parse::<usize>().ok())
            .ok_or_else(|| {
                sqlx::Error::Protocol(format!("invalid postgres migration version: {version}"))
            })?;
        if number != previous_number + 1 {
            return Err(sqlx::Error::Protocol(format!(
                "postgres migration registry is not contiguous at {version}"
            )));
        }
        if sql.trim().is_empty() {
            return Err(sqlx::Error::Protocol(format!(
                "postgres migration is empty: {version}"
            )));
        }
        previous_number = number;
    }
    Ok(())
}

async fn validate_migration_history(
    tx: &mut Transaction<'_, Postgres>,
    migrations: &[(&str, &str)],
) -> Result<(), sqlx::Error> {
    let applied_versions = sqlx::query_scalar::<_, String>(
        "SELECT version FROM mini_schema_migrations ORDER BY version",
    )
    .fetch_all(&mut **tx)
    .await?;

    validate_applied_migration_versions(&applied_versions, migrations)
}

fn validate_applied_migration_versions(
    applied_versions: &[String],
    migrations: &[(&str, &str)],
) -> Result<(), sqlx::Error> {
    for applied_version in applied_versions {
        if !migrations
            .iter()
            .any(|(version, _)| *version == applied_version.as_str())
        {
            return Err(sqlx::Error::Protocol(format!(
                "unknown postgres migration in history: {applied_version}"
            )));
        }
    }

    let mut first_missing = None;
    for &(version, _) in migrations {
        let applied = applied_versions
            .iter()
            .any(|applied_version| applied_version.as_str() == version);
        if applied {
            if let Some(missing_version) = first_missing {
                return Err(sqlx::Error::Protocol(format!(
                    "postgres migration history is out of order: {version} is applied after missing {missing_version}"
                )));
            }
        } else if first_missing.is_none() {
            first_missing = Some(version);
        }
    }
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
    let rewrites_append_only_raw_material_events =
        version == "0065_canonical_apparatus_cutover";
    if rewrites_append_only_raw_material_events {
        // 0065 performs a one-time canonical identity backfill on historical
        // raw-material events. Keep the audit trigger disabled only for the
        // exact, checksum-validated migration inside this transaction. A
        // migration failure rolls this DDL back together with every backfill.
        sqlx::query(
            "ALTER TABLE mini_raw_material_events
             DISABLE TRIGGER mini_rme_no_update_delete_trg",
        )
        .execute(&mut **tx)
        .await?;
    }
    for statement in split_sql_statements(sql) {
        sqlx::query::<Postgres>(&statement)
            .execute(&mut **tx)
            .await?;
    }
    if rewrites_append_only_raw_material_events {
        sqlx::query(
            "ALTER TABLE mini_raw_material_events
             ENABLE TRIGGER mini_rme_no_update_delete_trg",
        )
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
    let mut in_line_comment = false;
    let mut block_comment_depth = 0_usize;

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
        if in_line_comment {
            if ch == '\n' || ch == '\r' {
                in_line_comment = false;
            }
            index += ch.len_utf8();
            continue;
        }

        if block_comment_depth > 0 {
            if sql[index..].starts_with("/*") {
                block_comment_depth += 1;
                index += 2;
                continue;
            }
            if sql[index..].starts_with("*/") {
                block_comment_depth -= 1;
                index += 2;
                continue;
            }
            index += ch.len_utf8();
            continue;
        }

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

        if sql[index..].starts_with("--") {
            in_line_comment = true;
            index += 2;
            continue;
        }

        if sql[index..].starts_with("/*") {
            block_comment_depth = 1;
            index += 2;
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

include!("../postgres_inline_tests.rs");
