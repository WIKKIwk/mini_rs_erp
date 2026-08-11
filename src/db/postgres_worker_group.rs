use std::collections::BTreeSet;

use async_trait::async_trait;
use sqlx::{Executor, PgPool, Postgres, Transaction};

use crate::core::worker_groups::{
    WorkerGroupError, WorkerGroupMutation, WorkerGroupRecord, WorkerGroupStorePort,
    apply_worker_group_mutation,
};

// Shared by group writes and worker deactivation so validation and JSON references
// cannot be changed by separate transactions at the same time.
pub(crate) const WORKER_GROUP_MUTATION_LOCK_KEY: i64 = 0x4D49_4E49_5747_5250;

#[derive(Clone)]
pub struct PostgresWorkerGroupStore {
    pool: PgPool,
}

impl PostgresWorkerGroupStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WorkerGroupStorePort for PostgresWorkerGroupStore {
    async fn worker_groups(
        &self,
        apparatus: Option<&str>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError> {
        load_worker_groups(&self.pool, apparatus).await
    }

    async fn upsert_group(
        &self,
        mutation: WorkerGroupMutation,
    ) -> Result<WorkerGroupRecord, WorkerGroupError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| WorkerGroupError::StoreFailed)?;
        lock_worker_group_mutations(&mut tx).await?;

        let before = load_worker_groups(&mut *tx, None).await?;
        let mut after = before.clone();
        let saved = apply_worker_group_mutation(&mut after, &mutation)?;
        ensure_workers_active(&mut tx, &saved.worker_ids).await?;
        persist_group_delta(&mut tx, &before, &after).await?;

        tx.commit()
            .await
            .map_err(|_| WorkerGroupError::StoreFailed)?;
        Ok(saved)
    }

    async fn remove_worker(&self, worker_id: &str) -> Result<(), WorkerGroupError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| WorkerGroupError::StoreFailed)?;
        lock_worker_group_mutations(&mut tx).await?;
        remove_worker_from_groups(&mut tx, worker_id).await?;
        tx.commit().await.map_err(|_| WorkerGroupError::StoreFailed)
    }
}

pub(crate) async fn lock_worker_group_mutations(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), WorkerGroupError> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(WORKER_GROUP_MUTATION_LOCK_KEY)
        .execute(&mut **tx)
        .await
        .map_err(|_| WorkerGroupError::StoreFailed)?;
    Ok(())
}

pub(crate) async fn remove_worker_from_groups(
    tx: &mut Transaction<'_, Postgres>,
    worker_id: &str,
) -> Result<(), WorkerGroupError> {
    let worker_id = worker_id.trim();
    if worker_id.is_empty() {
        return Ok(());
    }
    let groups = load_worker_groups(&mut **tx, None).await?;
    for mut group in groups {
        let previous_len = group.worker_ids.len();
        group
            .worker_ids
            .retain(|id| !id.trim().eq_ignore_ascii_case(worker_id));
        if group.worker_ids.len() != previous_len {
            save_group(&mut *tx, &group).await?;
        }
    }
    Ok(())
}

async fn ensure_workers_active(
    tx: &mut Transaction<'_, Postgres>,
    worker_ids: &[String],
) -> Result<(), WorkerGroupError> {
    let requested = worker_ids
        .iter()
        .map(|id| id.trim().to_ascii_lowercase())
        .filter(|id| !id.is_empty())
        .collect::<BTreeSet<_>>();
    if requested.is_empty() {
        return Ok(());
    }
    let requested_ids = requested.iter().cloned().collect::<Vec<_>>();
    let active = sqlx::query_scalar::<_, String>(
        "SELECT lower(id)
         FROM mini_workers
         WHERE active AND lower(id) = ANY($1)
         FOR KEY SHARE",
    )
    .bind(&requested_ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(|_| WorkerGroupError::StoreFailed)?
    .into_iter()
    .collect::<BTreeSet<_>>();
    if active != requested {
        return Err(WorkerGroupError::WorkerNotFound);
    }
    Ok(())
}

async fn persist_group_delta(
    tx: &mut Transaction<'_, Postgres>,
    before: &[WorkerGroupRecord],
    after: &[WorkerGroupRecord],
) -> Result<(), WorkerGroupError> {
    for previous in before {
        let replacement = after
            .iter()
            .find(|candidate| same_group_identity(candidate, previous));
        if replacement.is_none_or(|replacement| {
            replacement.apparatus != previous.apparatus
                || replacement.group_code != previous.group_code
        }) {
            delete_group(tx, previous).await?;
        }
    }

    for next in after {
        let previous = before.iter().find(|candidate| {
            candidate.apparatus == next.apparatus && candidate.group_code == next.group_code
        });
        if previous != Some(next) {
            save_group(tx, next).await?;
        }
    }
    Ok(())
}

fn same_group_identity(left: &WorkerGroupRecord, right: &WorkerGroupRecord) -> bool {
    left.apparatus.eq_ignore_ascii_case(&right.apparatus) && left.group_code == right.group_code
}

async fn delete_group(
    tx: &mut Transaction<'_, Postgres>,
    group: &WorkerGroupRecord,
) -> Result<(), WorkerGroupError> {
    sqlx::query("DELETE FROM mini_worker_groups WHERE apparatus = $1 AND group_code = $2")
        .bind(&group.apparatus)
        .bind(&group.group_code)
        .execute(&mut **tx)
        .await
        .map_err(|_| WorkerGroupError::StoreFailed)?;
    Ok(())
}

async fn save_group(
    tx: &mut Transaction<'_, Postgres>,
    group: &WorkerGroupRecord,
) -> Result<(), WorkerGroupError> {
    let worker_ids =
        serde_json::to_value(&group.worker_ids).map_err(|_| WorkerGroupError::StoreFailed)?;
    let payload = serde_json::to_value(group).map_err(|_| WorkerGroupError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO mini_worker_groups
            (apparatus, group_code, shift, start_time, end_time,
             work_days_per_week, start_day, accounting_enabled, worker_ids, payload_json)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         ON CONFLICT (apparatus, group_code) DO UPDATE SET
            shift = EXCLUDED.shift,
            start_time = EXCLUDED.start_time,
            end_time = EXCLUDED.end_time,
            work_days_per_week = EXCLUDED.work_days_per_week,
            start_day = EXCLUDED.start_day,
            accounting_enabled = EXCLUDED.accounting_enabled,
            worker_ids = EXCLUDED.worker_ids,
            payload_json = EXCLUDED.payload_json,
            updated_at = now()",
    )
    .bind(&group.apparatus)
    .bind(&group.group_code)
    .bind(&group.shift)
    .bind(&group.start_time)
    .bind(&group.end_time)
    .bind(group.work_days_per_week)
    .bind(&group.start_day)
    .bind(group.accounting_enabled)
    .bind(worker_ids)
    .bind(payload)
    .execute(&mut **tx)
    .await
    .map_err(|_| WorkerGroupError::StoreFailed)?;
    Ok(())
}

async fn load_worker_groups<'e, E>(
    executor: E,
    apparatus: Option<&str>,
) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError>
where
    E: Executor<'e, Database = Postgres>,
{
    let apparatus = apparatus.unwrap_or("").trim().to_lowercase();
    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            String,
            i32,
            String,
            bool,
            serde_json::Value,
        ),
    >(
        "SELECT apparatus, group_code, shift, start_time, end_time, work_days_per_week,
                start_day, accounting_enabled, worker_ids
         FROM mini_worker_groups
         WHERE ($1 = '' OR lower(apparatus) = $1)
         ORDER BY lower(apparatus) ASC, group_code ASC",
    )
    .bind(apparatus)
    .fetch_all(executor)
    .await
    .map_err(|_| WorkerGroupError::StoreFailed)?;

    rows.into_iter()
        .map(
            |(
                apparatus,
                group_code,
                shift,
                start_time,
                end_time,
                work_days_per_week,
                start_day,
                accounting_enabled,
                worker_ids,
            )| {
                let worker_ids = serde_json::from_value::<Vec<String>>(worker_ids)
                    .map_err(|_| WorkerGroupError::StoreFailed)?;
                Ok(WorkerGroupRecord {
                    apparatus,
                    group_code,
                    shift,
                    start_time,
                    end_time,
                    work_days_per_week,
                    start_day,
                    accounting_enabled,
                    worker_ids,
                })
            },
        )
        .collect()
}
