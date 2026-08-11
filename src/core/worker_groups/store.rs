use async_trait::async_trait;

use super::{WorkerGroupError, WorkerGroupMutation, WorkerGroupRecord, WorkerGroupStorePort};
#[cfg(test)]
use super::{apply_worker_group_mutation, normalize::sort_groups};

pub(super) struct UnavailableWorkerGroupStore;

#[async_trait]
impl WorkerGroupStorePort for UnavailableWorkerGroupStore {
    async fn worker_groups(
        &self,
        _apparatus: Option<&str>,
    ) -> Result<Vec<WorkerGroupRecord>, WorkerGroupError> {
        Err(WorkerGroupError::StoreFailed)
    }

    async fn upsert_group(
        &self,
        _mutation: WorkerGroupMutation,
    ) -> Result<WorkerGroupRecord, WorkerGroupError> {
        Err(WorkerGroupError::StoreFailed)
    }

    async fn remove_worker(&self, _worker_id: &str) -> Result<(), WorkerGroupError> {
        Err(WorkerGroupError::StoreFailed)
    }
}

#[derive(Default)]
#[cfg(test)]
pub struct MemoryWorkerGroupStore {
    groups: tokio::sync::RwLock<Vec<WorkerGroupRecord>>,
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

    async fn upsert_group(
        &self,
        mutation: WorkerGroupMutation,
    ) -> Result<WorkerGroupRecord, WorkerGroupError> {
        let mut stored = self.groups.write().await;
        apply_worker_group_mutation(&mut stored, &mutation)
    }

    async fn remove_worker(&self, worker_id: &str) -> Result<(), WorkerGroupError> {
        let mut stored = self.groups.write().await;
        for group in stored.iter_mut() {
            group
                .worker_ids
                .retain(|id| !id.trim().eq_ignore_ascii_case(worker_id.trim()));
        }
        *stored = sort_groups(std::mem::take(&mut *stored));
        Ok(())
    }
}
