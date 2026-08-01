mod normalize;
mod store;

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use normalize::{ensure_workers_not_duplicated, normalize_input, sort_groups};
#[cfg(test)]
pub use store::MemoryWorkerGroupStore;
use store::UnavailableWorkerGroupStore;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerGroupRecord {
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
    pub apparatus: String,
    pub group_code: String,
    #[serde(default)]
    pub previous_apparatus: Option<String>,
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

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WorkerGroupError {
    #[error("apparatus is required")]
    MissingApparatus,
    #[error("worker group is invalid")]
    InvalidGroup,
    #[error("worker shift is invalid")]
    InvalidShift,
    #[error("worker schedule is invalid")]
    InvalidSchedule,
    #[error("worker is duplicated in apparatus groups")]
    DuplicateWorker,
    #[error("worker group was not found")]
    GroupNotFound,
    #[error("worker group name already exists")]
    DuplicateGroup,
    #[error("worker group store failed")]
    StoreFailed,
}

#[async_trait]
pub trait WorkerGroupStorePort: Send + Sync {
    async fn worker_groups(
        &self,
        apparatus: Option<&str>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError>;
    async fn put_apparatus_worker_groups(
        &self,
        apparatus: &str,
        groups: Vec<WorkerGroupRecord>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError>;
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
        apparatus: Option<&str>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError> {
        self.store.worker_groups(apparatus).await
    }

    pub async fn upsert_group(
        &self,
        input: WorkerGroupUpsert,
    ) -> Result<WorkerGroupRecord, WorkerGroupError> {
        let has_previous_identity =
            input.previous_apparatus.is_some() || input.previous_group_code.is_some();
        let previous_apparatus = input
            .previous_apparatus
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let previous_group_code = input
            .previous_group_code
            .as_deref()
            .map(normalize::normalize_group_code)
            .transpose()?;
        let next = normalize_input(input)?;
        let all_groups = self.store.worker_groups(None).await?;

        let previous_apparatus = previous_apparatus.unwrap_or_else(|| next.apparatus.clone());
        let previous_group_code = previous_group_code.unwrap_or_else(|| next.group_code.clone());
        if has_previous_identity
            && !all_groups.iter().any(|group| {
                group
                    .apparatus
                    .eq_ignore_ascii_case(&previous_apparatus)
                    && group.group_code == previous_group_code
            })
        {
            return Err(WorkerGroupError::GroupNotFound);
        }

        let source_apparatuses = if has_previous_identity {
            vec![previous_apparatus.clone()]
        } else {
            all_groups
                .iter()
                .filter(|group| {
                    group.group_code == next.group_code
                        && !group.apparatus.eq_ignore_ascii_case(&next.apparatus)
                })
                .map(|group| group.apparatus.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        };

        let mut groups = self
            .store
            .worker_groups(Some(&next.apparatus))
            .await?
            .into_iter()
            .filter(|group| group.apparatus.eq_ignore_ascii_case(&next.apparatus))
            .collect::<Vec<_>>();

        if has_previous_identity
            && groups.iter().any(|group| {
                group.group_code == next.group_code
                    && !(group
                        .apparatus
                        .eq_ignore_ascii_case(&previous_apparatus)
                        && group.group_code == previous_group_code)
            })
        {
            return Err(WorkerGroupError::DuplicateGroup);
        }

        if groups.iter().any(|group| {
            group.group_code != next.group_code
                && !(has_previous_identity
                    && group
                        .apparatus
                        .eq_ignore_ascii_case(&previous_apparatus)
                    && group.group_code == previous_group_code)
                && group.worker_ids.iter().any(|worker_id| {
                    next.worker_ids
                        .iter()
                        .any(|next_id| next_id.eq_ignore_ascii_case(worker_id))
            })
        }) {
            return Err(WorkerGroupError::DuplicateWorker);
        }

        if has_previous_identity {
            groups.retain(|group| {
                !(group
                    .apparatus
                    .eq_ignore_ascii_case(&previous_apparatus)
                    && group.group_code == previous_group_code)
            });
        }

        if let Some(existing) = groups
            .iter_mut()
            .find(|group| group.group_code == next.group_code)
        {
            *existing = next.clone();
        } else {
            groups.push(next.clone());
        }

        ensure_workers_not_duplicated(&groups)?;

        for source_apparatus in source_apparatuses {
            if source_apparatus.eq_ignore_ascii_case(&next.apparatus) {
                continue;
            }
            let remaining = all_groups
                .iter()
                .filter(|group| {
                    if !group.apparatus.eq_ignore_ascii_case(&source_apparatus) {
                        return false;
                    }
                    let code = if has_previous_identity {
                        &previous_group_code
                    } else {
                        &next.group_code
                    };
                    group.group_code != code.as_str()
                })
                .cloned()
                .collect::<Vec<_>>();
            self.store
                .put_apparatus_worker_groups(&source_apparatus, sort_groups(remaining))
                .await?;
        }

        let saved = self
            .store
            .put_apparatus_worker_groups(&next.apparatus, sort_groups(groups))
            .await?;
        saved
            .into_iter()
            .find(|group| group.group_code == next.group_code)
            .ok_or(WorkerGroupError::StoreFailed)
    }

    pub async fn remove_worker(&self, worker_id: &str) -> Result<(), WorkerGroupError> {
        let worker_id = worker_id.trim();
        if worker_id.is_empty() {
            return Ok(());
        }
        let all_groups = self.store.worker_groups(None).await?;
        let apparatuses = all_groups
            .iter()
            .filter(|group| {
                group
                    .worker_ids
                    .iter()
                    .any(|id| id.trim().eq_ignore_ascii_case(worker_id))
            })
            .map(|group| group.apparatus.clone())
            .collect::<BTreeSet<_>>();

        for apparatus in apparatuses {
            let mut groups = all_groups
                .iter()
                .filter(|group| group.apparatus.eq_ignore_ascii_case(&apparatus))
                .cloned()
                .collect::<Vec<_>>();
            for group in &mut groups {
                group
                    .worker_ids
                    .retain(|id| !id.trim().eq_ignore_ascii_case(worker_id));
            }
            self.store
                .put_apparatus_worker_groups(&apparatus, sort_groups(groups))
                .await?;
        }
        Ok(())
    }
}

fn default_work_days_per_week() -> i32 {
    6
}

#[cfg(test)]
mod tests;
