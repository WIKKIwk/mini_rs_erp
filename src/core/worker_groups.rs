use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
#[cfg(test)]
use tokio::sync::RwLock;

pub const WORKER_GROUP_CODES: [&str; 2] = ["A", "B"];
pub const WORKER_GROUP_SHIFTS: [&str; 2] = ["day", "night"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerGroupRecord {
    pub apparatus: String,
    pub group_code: String,
    pub shift: String,
    #[serde(default)]
    pub worker_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerGroupUpsert {
    pub apparatus: String,
    pub group_code: String,
    pub shift: String,
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
    #[error("worker is duplicated in apparatus groups")]
    DuplicateWorker,
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
        let next = normalize_input(input)?;
        let mut groups = self
            .store
            .worker_groups(Some(&next.apparatus))
            .await?
            .into_iter()
            .filter(|group| group.apparatus.eq_ignore_ascii_case(&next.apparatus))
            .collect::<Vec<_>>();

        for group_code in WORKER_GROUP_CODES {
            if !groups
                .iter()
                .any(|group| group.group_code.eq_ignore_ascii_case(group_code))
            {
                groups.push(WorkerGroupRecord {
                    apparatus: next.apparatus.clone(),
                    group_code: group_code.to_string(),
                    shift: if group_code == next.group_code {
                        next.shift.clone()
                    } else {
                        opposite_shift(&next.shift).to_string()
                    },
                    worker_ids: Vec::new(),
                });
            }
        }

        for group in &mut groups {
            if group.group_code == next.group_code {
                *group = next.clone();
            } else {
                group.apparatus = next.apparatus.clone();
                group.shift = opposite_shift(&next.shift).to_string();
                group.worker_ids.retain(|worker_id| {
                    !next
                        .worker_ids
                        .iter()
                        .any(|next_id| next_id.eq_ignore_ascii_case(worker_id))
                });
            }
        }

        ensure_workers_not_duplicated(&groups)?;

        let saved = self
            .store
            .put_apparatus_worker_groups(&next.apparatus, sort_groups(groups))
            .await?;
        saved
            .into_iter()
            .find(|group| group.group_code == next.group_code)
            .ok_or(WorkerGroupError::StoreFailed)
    }
}

fn normalize_input(input: WorkerGroupUpsert) -> Result<WorkerGroupRecord, WorkerGroupError> {
    let apparatus = input.apparatus.trim().to_string();
    if apparatus.is_empty() {
        return Err(WorkerGroupError::MissingApparatus);
    }
    let group_code = normalize_group_code(&input.group_code)?;
    let shift = normalize_shift(&input.shift)?;
    let mut seen = BTreeSet::new();
    let worker_ids = input
        .worker_ids
        .into_iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .filter(|id| seen.insert(id.to_lowercase()))
        .collect();

    Ok(WorkerGroupRecord {
        apparatus,
        group_code,
        shift,
        worker_ids,
    })
}

fn normalize_group_code(value: &str) -> Result<String, WorkerGroupError> {
    let upper = value.trim().to_uppercase();
    WORKER_GROUP_CODES
        .iter()
        .find(|code| **code == upper)
        .map(|code| (*code).to_string())
        .ok_or(WorkerGroupError::InvalidGroup)
}

fn normalize_shift(value: &str) -> Result<String, WorkerGroupError> {
    let lower = value.trim().to_lowercase();
    WORKER_GROUP_SHIFTS
        .iter()
        .find(|shift| **shift == lower)
        .map(|shift| (*shift).to_string())
        .ok_or(WorkerGroupError::InvalidShift)
}

fn opposite_shift(shift: &str) -> &'static str {
    if shift == "day" { "night" } else { "day" }
}

fn ensure_workers_not_duplicated(groups: &[WorkerGroupRecord]) -> Result<(), WorkerGroupError> {
    let mut seen = BTreeSet::new();
    for worker_id in groups.iter().flat_map(|group| group.worker_ids.iter()) {
        if !seen.insert(worker_id.to_lowercase()) {
            return Err(WorkerGroupError::DuplicateWorker);
        }
    }
    Ok(())
}

fn sort_groups(mut groups: Vec<WorkerGroupRecord>) -> Vec<WorkerGroupRecord> {
    groups.sort_by(|left, right| {
        left.apparatus
            .to_lowercase()
            .cmp(&right.apparatus.to_lowercase())
            .then(left.group_code.cmp(&right.group_code))
    });
    groups
}

struct UnavailableWorkerGroupStore;

#[async_trait]
impl WorkerGroupStorePort for UnavailableWorkerGroupStore {
    async fn worker_groups(
        &self,
        _apparatus: Option<&str>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError> {
        Err(WorkerGroupError::StoreFailed)
    }

    async fn put_apparatus_worker_groups(
        &self,
        _apparatus: &str,
        _groups: Vec<WorkerGroupRecord>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError> {
        Err(WorkerGroupError::StoreFailed)
    }
}

#[derive(Default)]
#[cfg(test)]
pub struct MemoryWorkerGroupStore {
    groups: RwLock<Vec<WorkerGroupRecord>>,
}

#[cfg(test)]
impl MemoryWorkerGroupStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
#[cfg(test)]
impl WorkerGroupStorePort for MemoryWorkerGroupStore {
    async fn worker_groups(
        &self,
        apparatus: Option<&str>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError> {
        let apparatus = apparatus.unwrap_or("").trim().to_lowercase();
        let mut groups = self
            .groups
            .read()
            .await
            .iter()
            .filter(|group| apparatus.is_empty() || group.apparatus.to_lowercase() == apparatus)
            .cloned()
            .collect::<Vec<_>>();
        groups = sort_groups(groups);
        Ok(groups)
    }

    async fn put_apparatus_worker_groups(
        &self,
        apparatus: &str,
        groups: Vec<WorkerGroupRecord>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError> {
        let key = apparatus.trim().to_lowercase();
        let mut stored = self.groups.write().await;
        stored.retain(|group| group.apparatus.to_lowercase() != key);
        stored.extend(groups.clone());
        *stored = sort_groups(stored.clone());
        Ok(sort_groups(groups))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn worker_group_swap_keeps_one_day_and_one_night_group() {
        let service = WorkerGroupService::new(Arc::new(MemoryWorkerGroupStore::new()));
        service
            .upsert_group(WorkerGroupUpsert {
                apparatus: "Laminatsiya 1".to_string(),
                group_code: "A".to_string(),
                shift: "day".to_string(),
                worker_ids: vec!["w1".to_string()],
            })
            .await
            .expect("save a");

        service
            .upsert_group(WorkerGroupUpsert {
                apparatus: "Laminatsiya 1".to_string(),
                group_code: "B".to_string(),
                shift: "day".to_string(),
                worker_ids: vec!["w2".to_string()],
            })
            .await
            .expect("save b");

        let groups = service
            .worker_groups(Some("Laminatsiya 1"))
            .await
            .expect("groups");
        assert_eq!(groups[0].group_code, "A");
        assert_eq!(groups[0].shift, "night");
        assert_eq!(groups[1].group_code, "B");
        assert_eq!(groups[1].shift, "day");
    }
}
