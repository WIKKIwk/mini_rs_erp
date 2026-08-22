mod normalize;
mod store;

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::apparatus_standard::ApparatusId;

use normalize::{ensure_workers_not_duplicated, normalize_input, sort_groups};
#[cfg(test)]
pub use store::MemoryWorkerGroupStore;
use store::UnavailableWorkerGroupStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerGroupRecord {
    pub apparatus_id: ApparatusId,
    /// Historical/display-only label. Authorization and identity use `apparatus_id`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub apparatus: String,
    pub group_code: String,
    pub shift: String,
    #[serde(default)]
    pub start_time: String,
    #[serde(default)]
    pub end_time: String,
    #[serde(default = "default_work_days_per_week")]
    pub work_days_per_week: i32,
    #[serde(default)]
    pub start_day: String,
    #[serde(default)]
    pub accounting_enabled: bool,
    #[serde(default)]
    pub worker_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerGroupUpsert {
    #[serde(default)]
    pub apparatus_id: Option<ApparatusId>,
    /// Historical/display-only label. It is never used to locate a group.
    #[serde(default)]
    pub apparatus: String,
    pub group_code: String,
    #[serde(default)]
    pub previous_apparatus: Option<String>,
    #[serde(default)]
    pub previous_apparatus_id: Option<ApparatusId>,
    #[serde(default)]
    pub previous_group_code: Option<String>,
    #[serde(default)]
    pub shift: String,
    #[serde(default)]
    pub start_time: String,
    #[serde(default)]
    pub end_time: String,
    #[serde(default = "default_work_days_per_week")]
    pub work_days_per_week: i32,
    #[serde(default)]
    pub start_day: String,
    #[serde(default)]
    pub accounting_enabled: bool,
    #[serde(default)]
    pub worker_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerGroupMutation {
    pub next: WorkerGroupRecord,
    pub previous_apparatus_id: ApparatusId,
    pub previous_group_code: String,
    pub has_previous_identity: bool,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WorkerGroupError {
    #[error("apparatus id is required")]
    MissingApparatus,
    #[error("apparatus id is invalid")]
    InvalidApparatusId,
    #[error("worker group is invalid")]
    InvalidGroup,
    #[error("worker shift is invalid")]
    InvalidShift,
    #[error("worker schedule is invalid")]
    InvalidSchedule,
    #[error("worker is duplicated in worker groups")]
    DuplicateWorker,
    #[error("worker group was not found")]
    GroupNotFound,
    #[error("worker group name already exists")]
    DuplicateGroup,
    #[error("worker was not found or is inactive")]
    WorkerNotFound,
    #[error("worker group store failed")]
    StoreFailed,
}

#[async_trait]
pub trait WorkerGroupStorePort: Send + Sync {
    async fn worker_groups(
        &self,
        apparatus_id: Option<&ApparatusId>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError>;
    async fn upsert_group(
        &self,
        mutation: WorkerGroupMutation,
    ) -> Result<WorkerGroupRecord, WorkerGroupError>;
    async fn remove_worker(&self, worker_id: &str) -> Result<(), WorkerGroupError>;
}

#[derive(Clone)]
pub struct WorkerGroupService {
    store: Arc<dyn WorkerGroupStorePort>,
}

impl WorkerGroupService {
    pub fn new(store: Arc<dyn WorkerGroupStorePort>) -> Self {
        Self { store }
    }

    pub fn unavailable() -> Self {
        Self::new(Arc::new(UnavailableWorkerGroupStore))
    }

    pub async fn worker_groups(
        &self,
        apparatus_id: Option<&ApparatusId>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError> {
        self.store.worker_groups(apparatus_id).await
    }

    pub async fn upsert_group(
        &self,
        input: WorkerGroupUpsert,
    ) -> Result<WorkerGroupRecord, WorkerGroupError> {
        let has_previous_identity =
            input.previous_apparatus_id.is_some() || input.previous_group_code.is_some();
        let previous_apparatus_id = input.previous_apparatus_id.clone();
        let previous_group_code = input
            .previous_group_code
            .as_deref()
            .map(normalize::normalize_group_code)
            .transpose()?;
        let next = normalize_input(input)?;
        // A missing previous ID means an edit within the current immutable
        // apparatus scope. The legacy display label is deliberately ignored.
        let previous_apparatus_id =
            previous_apparatus_id.unwrap_or_else(|| next.apparatus_id.clone());
        let previous_group_code = previous_group_code.unwrap_or_else(|| next.group_code.clone());
        self.store
            .upsert_group(WorkerGroupMutation {
                next,
                previous_apparatus_id,
                previous_group_code,
                has_previous_identity,
            })
            .await
    }

    pub async fn remove_worker(&self, worker_id: &str) -> Result<(), WorkerGroupError> {
        let worker_id = worker_id.trim();
        if worker_id.is_empty() {
            return Ok(());
        }
        self.store.remove_worker(worker_id).await
    }
}

pub(crate) fn apply_worker_group_mutation(
    groups: &mut Vec<WorkerGroupRecord>,
    mutation: &WorkerGroupMutation,
) -> Result<WorkerGroupRecord, WorkerGroupError> {
    let next = &mutation.next;
    if mutation.has_previous_identity
        && !groups.iter().any(|group| {
            group.apparatus_id == mutation.previous_apparatus_id
                && group.group_code == mutation.previous_group_code
        })
    {
        return Err(WorkerGroupError::GroupNotFound);
    }

    if mutation.has_previous_identity
        && groups.iter().any(|group| {
            group.apparatus_id == next.apparatus_id
                && group.group_code == next.group_code
                && !(group.apparatus_id == mutation.previous_apparatus_id
                    && group.group_code == mutation.previous_group_code)
        })
    {
        return Err(WorkerGroupError::DuplicateGroup);
    }

    let mut updated = groups.clone();
    if mutation.has_previous_identity {
        updated.retain(|group| {
            !(group.apparatus_id == mutation.previous_apparatus_id
                && group.group_code == mutation.previous_group_code)
        });
    } else {
        updated.retain(|group| {
            !(group.apparatus_id == next.apparatus_id && group.group_code == next.group_code)
        });
    }

    let mut all_groups = updated.clone();
    all_groups.push(next.clone());
    ensure_workers_not_duplicated(&all_groups)?;

    updated.push(next.clone());
    *groups = sort_groups(updated);
    Ok(next.clone())
}

fn default_work_days_per_week() -> i32 {
    6
}

#[cfg(test)]
mod tests;
