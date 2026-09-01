use sqlx::{Postgres, Transaction};

use super::collect_in_transaction;
use crate::core::apparatus_standard::{CanonicalApparatusError, cutover::PreparedCutoverPlan};

pub(super) async fn commit(
    pool: &sqlx::PgPool,
    plan: PreparedCutoverPlan,
) -> Result<(), CanonicalApparatusError> {
    let expected_count =
        i64::try_from(plan.entries.len()).map_err(|_| CanonicalApparatusError::Persistence)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| CanonicalApparatusError::Persistence)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *tx)
        .await
        .map_err(|_| CanonicalApparatusError::Persistence)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind("canonical-apparatus:legacy-cutover")
        .execute(&mut *tx)
        .await
        .map_err(|_| CanonicalApparatusError::Persistence)?;
    let current = collect_in_transaction(&mut tx).await?;
    if current.fingerprint != plan.preflight_fingerprint || !current.blocking_issues.is_empty() {
        return Err(CanonicalApparatusError::CutoverBlocked(
            "database changed after manifest preflight".to_string(),
        ));
    }
    sqlx::query("SELECT set_config('mini_rs_erp.canonical_writer', 'on', true)")
        .execute(&mut *tx)
        .await
        .map_err(|_| CanonicalApparatusError::Persistence)?;
    for entry in &plan.entries {
        assert_legacy_row(&mut tx, entry).await?;
        let artifact_sha256_hex = entry.artifact.sha256().to_hex();
        super::super::mutations::insert_identity(&mut tx, &entry.revision).await?;
        super::super::mutations::insert_revision(
            &mut tx,
            &entry.revision,
            entry.artifact.bytes(),
            &artifact_sha256_hex,
        )
        .await?;
        super::super::mutations::cas_head(&mut tx, &entry.revision, None, &artifact_sha256_hex)
            .await?;
        super::legacy_projections::write(
            &mut tx,
            &entry.revision,
            &entry.projections,
            &artifact_sha256_hex,
        )
        .await?;
        super::super::mutations::insert_outbox(
            &mut tx,
            &entry.revision,
            "apparatus_created",
            &entry.projections.runtime,
        )
        .await?;
    }
    reconcile(&mut tx, expected_count).await?;
    tx.commit()
        .await
        .map_err(|_| CanonicalApparatusError::Persistence)
}

async fn assert_legacy_row(
    tx: &mut Transaction<'_, Postgres>,
    entry: &crate::core::apparatus_standard::cutover::PreparedCutoverEntry,
) -> Result<(), CanonicalApparatusError> {
    let matches: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM mini_apparatus
             WHERE id = $1 AND source_revision IS NULL AND source_aasx_sha256 IS NULL
         )",
    )
    .bind(&entry.legacy_apparatus_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    if !matches || entry.revision.apparatus_id.as_str() != entry.legacy_apparatus_id {
        return Err(CanonicalApparatusError::CutoverBlocked(
            "stable legacy apparatus identity changed during cutover".to_string(),
        ));
    }
    Ok(())
}

async fn reconcile(
    tx: &mut Transaction<'_, Postgres>,
    expected_count: i64,
) -> Result<(), CanonicalApparatusError> {
    let counts = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
        "SELECT
             (SELECT count(*) FROM mini_apparatus),
             (SELECT count(*) FROM mini_canonical_apparatus_heads),
             (SELECT count(*) FROM mini_canonical_apparatus_revisions),
             (SELECT count(*) FROM mini_canonical_apparatus_change_outbox),
             (SELECT count(*) FROM mini_canonical_apparatus_projection_drift)",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    if counts
        != (
            expected_count,
            expected_count,
            expected_count,
            expected_count,
            0,
        )
    {
        return Err(CanonicalApparatusError::CutoverBlocked(
            "canonical source/target counts or projection reconciliation differ".to_string(),
        ));
    }
    let incomplete: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM mini_apparatus WHERE source_revision IS NULL
             UNION ALL SELECT 1 FROM mini_apparatus_queue_policies WHERE source_revision IS NULL
             UNION ALL SELECT 1 FROM mini_apparatus_material_rules WHERE source_revision IS NULL
             UNION ALL SELECT 1 FROM mini_apparatus_capacity_profiles WHERE source_revision IS NULL
         )",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| CanonicalApparatusError::Persistence)?;
    if incomplete {
        return Err(CanonicalApparatusError::CutoverBlocked(
            "legacy projection authority remains after cutover".to_string(),
        ));
    }
    Ok(())
}
