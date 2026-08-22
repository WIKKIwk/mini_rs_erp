use std::collections::BTreeSet;

use serde::Serialize;
use sqlx::{Postgres, Transaction};

use crate::core::apparatus_standard::{
    AasxSha256, CanonicalApparatusError, CutoverConfigurationSource, CutoverDiagnostic,
    CutoverPreflightReport, CutoverReferenceCount, CutoverTextReference, LegacyApparatusInventory,
};

const REQUIRED_HEAD: &str = "0069_canonical_apparatus_revision_authority";

pub(super) async fn collect(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<CutoverPreflightReport, CanonicalApparatusError> {
    let migration_head = migration_head(tx).await?;
    let (legacy_apparatuses, mut blocking_issues) = apparatus_inventory(tx).await?;
    let global_configuration_sources = global_sources(tx).await?;
    let dependent_references = dependent_references(tx).await?;
    let legacy_text_references = text_references(tx).await?;
    let diagnostics = diagnostics(tx).await?;
    if migration_head != REQUIRED_HEAD {
        blocking_issues.push(format!(
            "cutover requires migration head {REQUIRED_HEAD}, found {migration_head}"
        ));
    }
    if diagnostics
        .iter()
        .any(|item| item.unresolved_rows != 0 || item.orphan_rows != 0)
    {
        blocking_issues.push("unresolved or orphan apparatus references exist".to_string());
    }
    let canonical_rows: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM mini_canonical_apparatus_identities)
              + (SELECT count(*) FROM mini_canonical_apparatus_revisions)
              + (SELECT count(*) FROM mini_canonical_apparatus_heads)",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    if canonical_rows != 0 {
        blocking_issues.push("canonical cutover rows already exist".to_string());
    }
    blocking_issues.sort();
    blocking_issues.dedup();
    let fingerprint = fingerprint(&FingerprintInput {
        report_version: 1,
        required_migration_head: &migration_head,
        legacy_apparatuses: &legacy_apparatuses,
        global_configuration_sources: &global_configuration_sources,
        dependent_references: &dependent_references,
        legacy_text_references: &legacy_text_references,
        diagnostics: &diagnostics,
        blocking_issues: &blocking_issues,
    })?;
    Ok(CutoverPreflightReport {
        report_version: 1,
        required_migration_head: migration_head,
        fingerprint,
        legacy_apparatuses,
        global_configuration_sources,
        dependent_references,
        legacy_text_references,
        diagnostics,
        blocking_issues,
    })
}

async fn migration_head(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<String, CanonicalApparatusError> {
    sqlx::query_scalar(
        "SELECT version FROM mini_schema_migrations
         ORDER BY substring(version FROM 1 FOR 4)::integer DESC LIMIT 1",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)
}

async fn apparatus_inventory(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(Vec<LegacyApparatusInventory>, Vec<String>), CanonicalApparatusError> {
    let rows = sqlx::query_as::<_, (String, String, String, Option<i64>, serde_json::Value)>(
        "SELECT id, name, base_name, source_revision, payload_json
         FROM mini_apparatus ORDER BY id",
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    let mut apparatuses = Vec::with_capacity(rows.len());
    let mut blockers = Vec::new();
    for (id, name, base_name, source_revision, payload) in rows {
        if source_revision.is_some() {
            blockers.push(format!("apparatus {id} is already canonical"));
        }
        let mut identities = [id.clone(), name, base_name]
            .into_iter()
            .filter(|value| !value.trim().is_empty())
            .collect::<Vec<_>>();
        identities.sort();
        identities.dedup();
        let sources = apparatus_sources(tx, &id).await?;
        for (label, pointer, optional_projection) in [
            ("capabilities", "/canonical_apparatus/capabilities", None),
            (
                "queue policy",
                "/canonical_apparatus/policies/queue",
                Some("mini_apparatus_queue_policies"),
            ),
            (
                "material policy",
                "/canonical_apparatus/policies/material",
                Some("mini_apparatus_material_rules"),
            ),
            (
                "capacity",
                "/canonical_apparatus/capacity",
                Some("mini_apparatus_capacity_profiles"),
            ),
        ] {
            let has_embedded = payload
                .pointer(pointer)
                .is_some_and(|value| !value.is_null());
            let has_projection = optional_projection.is_some_and(|table| {
                sources
                    .iter()
                    .any(|source| source.source_key.starts_with(&format!("{table}:")))
            });
            if !has_embedded && !has_projection {
                blockers.push(format!("apparatus {id} has no explicit {label} source"));
            }
        }
        apparatuses.push(LegacyApparatusInventory {
            apparatus_id: id,
            observed_identities: identities,
            configuration_sources: sources,
        });
    }
    Ok((apparatuses, blockers))
}

async fn apparatus_sources(
    tx: &mut Transaction<'_, Postgres>,
    apparatus_id: &str,
) -> Result<Vec<CutoverConfigurationSource>, CanonicalApparatusError> {
    let tables = [
        ("mini_apparatus", "id"),
        ("mini_apparatus_queue_policies", "canonical_apparatus_id"),
        ("mini_apparatus_material_rules", "canonical_apparatus_id"),
        ("mini_apparatus_capacity_profiles", "canonical_apparatus_id"),
    ];
    let mut sources = Vec::new();
    for (table, column) in tables {
        let sql = format!(
            "SELECT to_jsonb(source) FROM {} source WHERE {} = $1",
            quoted_table("public", table),
            quote_identifier(column)
        );
        let rows = sqlx::query_scalar::<_, serde_json::Value>(&sql)
            .bind(apparatus_id)
            .fetch_all(&mut **tx)
            .await
            .map_err(|_| CanonicalApparatusError::Persistence)?;
        for (index, payload) in rows.into_iter().enumerate() {
            sources.push(configuration_source(
                format!("{table}:{apparatus_id}:{index}"),
                payload,
            )?);
        }
    }
    sources.sort_by(|left, right| left.source_key.cmp(&right.source_key));
    Ok(sources)
}

async fn global_sources(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Vec<CutoverConfigurationSource>, CanonicalApparatusError> {
    let rows = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT to_jsonb(source) FROM mini_apparatus_groups source
         ORDER BY to_jsonb(source)::text",
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    rows.into_iter()
        .enumerate()
        .map(|(index, payload)| {
            configuration_source(format!("mini_apparatus_groups:{index}"), payload)
        })
        .collect()
}

fn configuration_source(
    source_key: String,
    payload: serde_json::Value,
) -> Result<CutoverConfigurationSource, CanonicalApparatusError> {
    let bytes = serde_json::to_vec(&payload).map_err(|_| CanonicalApparatusError::Persistence)?;
    Ok(CutoverConfigurationSource {
        source_key,
        payload,
        sha256: AasxSha256::digest(&bytes),
    })
}

#[derive(Serialize)]
struct FingerprintInput<'a> {
    report_version: u32,
    required_migration_head: &'a str,
    legacy_apparatuses: &'a [LegacyApparatusInventory],
    global_configuration_sources: &'a [CutoverConfigurationSource],
    dependent_references: &'a [CutoverReferenceCount],
    legacy_text_references: &'a [CutoverTextReference],
    diagnostics: &'a [CutoverDiagnostic],
    blocking_issues: &'a [String],
}

fn fingerprint(value: &impl Serialize) -> Result<AasxSha256, CanonicalApparatusError> {
    serde_json::to_vec(value)
        .map(|bytes| AasxSha256::digest(&bytes))
        .map_err(|_| CanonicalApparatusError::Persistence)
}

async fn dependent_references(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Vec<CutoverReferenceCount>, CanonicalApparatusError> {
    let columns = reference_columns(tx).await?;
    let mut output = Vec::new();
    for (schema, table, column) in columns {
        let sql = format!(
            "SELECT {column}::text, count(*)::bigint FROM {table}
             WHERE {column} IS NOT NULL GROUP BY {column} ORDER BY {column}",
            column = quote_identifier(&column),
            table = quoted_table(&schema, &table),
        );
        for (apparatus_id, count) in sqlx::query_as::<_, (String, i64)>(&sql)
            .fetch_all(&mut **tx)
            .await
            .map_err(|_| CanonicalApparatusError::Persistence)?
        {
            output.push(CutoverReferenceCount {
                source_table: format!("{schema}.{table}"),
                source_column: column.clone(),
                apparatus_id,
                row_count: count_u64(count)?,
            });
        }
    }
    output.sort_by(|left, right| {
        (&left.source_table, &left.source_column, &left.apparatus_id).cmp(&(
            &right.source_table,
            &right.source_column,
            &right.apparatus_id,
        ))
    });
    Ok(output)
}

async fn reference_columns(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Vec<(String, String, String)>, CanonicalApparatusError> {
    sqlx::query_as(
        "SELECT namespace.nspname, relation.relname, attribute.attname
         FROM pg_constraint constraint_row
         JOIN pg_class relation ON relation.oid = constraint_row.conrelid
         JOIN pg_namespace namespace ON namespace.oid = relation.relnamespace
         JOIN LATERAL unnest(constraint_row.conkey) key(attnum) ON true
         JOIN pg_attribute attribute
           ON attribute.attrelid = relation.oid AND attribute.attnum = key.attnum
         WHERE constraint_row.contype = 'f'
           AND constraint_row.confrelid = 'public.mini_apparatus'::regclass
           AND cardinality(constraint_row.conkey) = 1
         ORDER BY namespace.nspname, relation.relname, attribute.attname",
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)
}

async fn text_references(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Vec<CutoverTextReference>, CanonicalApparatusError> {
    let columns = sqlx::query_as::<_, (String, String, String)>(
        "SELECT table_schema, table_name, column_name
         FROM information_schema.columns
         WHERE table_schema = 'public'
           AND data_type IN ('text', 'character varying')
           AND column_name LIKE '%apparatus%'
         ORDER BY table_schema, table_name, column_name",
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    let mut output = Vec::new();
    for (schema, table, column) in columns {
        let sql = format!(
            "SELECT {column}, count(*)::bigint FROM {table}
             WHERE btrim({column}) <> '' GROUP BY {column} ORDER BY {column}",
            column = quote_identifier(&column),
            table = quoted_table(&schema, &table),
        );
        for (observed_value, count) in sqlx::query_as::<_, (String, i64)>(&sql)
            .fetch_all(&mut **tx)
            .await
            .map_err(|_| CanonicalApparatusError::Persistence)?
        {
            output.push(CutoverTextReference {
                source_table: format!("{schema}.{table}"),
                source_column: column.clone(),
                observed_value,
                row_count: count_u64(count)?,
            });
        }
    }
    Ok(output)
}

async fn diagnostics(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Vec<CutoverDiagnostic>, CanonicalApparatusError> {
    let rows = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT source_table, unresolved_rows::bigint, orphan_rows::bigint
         FROM mini_canonical_apparatus_cutover_diagnostics
         UNION ALL
         SELECT 'mini_warehouse_assignments',
                count(*) FILTER (WHERE assignment_kind = 'apparatus' AND apparatus_id IS NULL),
                count(*) FILTER (WHERE apparatus_id IS NOT NULL AND NOT EXISTS (
                    SELECT 1 FROM mini_apparatus WHERE id = apparatus_id
                ))
         FROM mini_warehouse_assignments
         ORDER BY 1",
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    let mut seen = BTreeSet::new();
    rows.into_iter()
        .filter(|(source, _, _)| seen.insert(source.clone()))
        .map(|(source, unresolved, orphan)| {
            Ok(CutoverDiagnostic {
                source,
                unresolved_rows: count_u64(unresolved)?,
                orphan_rows: count_u64(orphan)?,
            })
        })
        .collect()
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn quoted_table(schema: &str, table: &str) -> String {
    format!("{}.{}", quote_identifier(schema), quote_identifier(table))
}

fn count_u64(value: i64) -> Result<u64, CanonicalApparatusError> {
    u64::try_from(value).map_err(|_| CanonicalApparatusError::Persistence)
}
