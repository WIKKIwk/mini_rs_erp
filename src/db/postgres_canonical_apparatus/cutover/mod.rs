mod commit;
mod inventory;
mod legacy_projections;

use sqlx::{Postgres, Transaction};

use crate::core::apparatus_standard::{CanonicalApparatusError, CutoverPreflightReport};

pub(super) async fn collect_from_pool(
    pool: &sqlx::PgPool,
) -> Result<CutoverPreflightReport, CanonicalApparatusError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| CanonicalApparatusError::Persistence)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(|_| CanonicalApparatusError::Persistence)?;
    let report = inventory::collect(&mut tx).await?;
    tx.rollback()
        .await
        .map_err(|_| CanonicalApparatusError::Persistence)?;
    Ok(report)
}

pub(super) async fn collect_in_transaction(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<CutoverPreflightReport, CanonicalApparatusError> {
    inventory::collect(tx).await
}

pub(super) async fn commit_plan(
    pool: &sqlx::PgPool,
    plan: crate::core::apparatus_standard::cutover::PreparedCutoverPlan,
) -> Result<(), CanonicalApparatusError> {
    commit::commit(pool, plan).await
}
