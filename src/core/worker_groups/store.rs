use async_trait::async_trait;

#[cfg(test)]
use super::normalize::sort_groups;
use super::{WorkerGroupError, WorkerGroupRecord, WorkerGroupStorePort};

pub(super) struct UnavailableWorkerGroupStore;

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
