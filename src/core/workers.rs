use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::core::auth::ports::{AuthPortError, WorkerLookup, WorkerRecord};

pub const WORKER_LEVELS: [&str; 5] = [
    "Brigader",
    "Master",
    "1 - darajali",
    "2 - darajali",
    "3 - darajali",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worker {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub phone: String,
    pub level: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerUpsert {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub level: String,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("worker name is required")]
    MissingName,
    #[error("worker id is required")]
    MissingId,
    #[error("worker level is invalid")]
    InvalidLevel,
    #[error("worker phone already exists")]
    DuplicatePhone,
    #[error("worker not found")]
    NotFound,
    #[error("worker store failed")]
    StoreFailed,
}

#[async_trait]
pub trait WorkerStorePort: Send + Sync {
    async fn workers(&self, query: &str, limit: usize) -> Result<Vec<Worker>, WorkerError>;
    async fn workers_by_ids(&self, ids: &[String]) -> Result<Vec<Worker>, WorkerError>;
    async fn upsert_worker(&self, worker: Worker) -> Result<Worker, WorkerError>;
    async fn update_worker_level(&self, id: &str, level: &str) -> Result<Worker, WorkerError>;
    async fn update_worker_phone(&self, id: &str, phone: &str) -> Result<Worker, WorkerError>;
    async fn delete_worker(&self, id: &str) -> Result<(), WorkerError>;
}

#[derive(Clone)]
pub struct WorkerService {
    store: Arc<dyn WorkerStorePort>,
}

impl WorkerService {
    pub fn new(store: Arc<dyn WorkerStorePort>) -> Self {
        Self { store }
    }

    pub fn unavailable() -> Self {
        Self::new(Arc::new(UnavailableWorkerStore))
    }

    pub async fn workers(&self, query: &str, limit: usize) -> Result<Vec<Worker>, WorkerError> {
        self.store.workers(query, limit.clamp(1, 500)).await
    }

    pub async fn workers_by_ids(&self, ids: &[String]) -> Result<Vec<Worker>, WorkerError> {
        self.store.workers_by_ids(ids).await
    }

    pub async fn upsert_worker(&self, input: WorkerUpsert) -> Result<Worker, WorkerError> {
        let name = input.name.trim();
        if name.is_empty() {
            return Err(WorkerError::MissingName);
        }
        let level = normalize_level(&input.level)?;
        let id = if input.id.trim().is_empty() {
            new_worker_id()
        } else {
            input.id.trim().to_string()
        };
        self.store
            .upsert_worker(Worker {
                id,
                name: name.to_string(),
                phone: input.phone.trim().to_string(),
                level,
            })
            .await
    }

    pub async fn update_worker_level(&self, input: WorkerUpsert) -> Result<Worker, WorkerError> {
        let id = input.id.trim();
        if id.is_empty() {
            return Err(WorkerError::MissingId);
        }
        let level = normalize_level(&input.level)?;
        self.store.update_worker_level(id, &level).await
    }

    pub async fn update_worker_phone(&self, input: WorkerUpsert) -> Result<Worker, WorkerError> {
        let id = input.id.trim();
        if id.is_empty() {
            return Err(WorkerError::MissingId);
        }
        self.store.update_worker_phone(id, input.phone.trim()).await
    }

    pub async fn delete_worker(&self, id: &str) -> Result<(), WorkerError> {
        let id = id.trim();
        if id.is_empty() {
            return Err(WorkerError::MissingId);
        }
        self.store.delete_worker(id).await
    }
}

#[async_trait]
impl WorkerLookup for WorkerService {
    async fn search_workers(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<WorkerRecord>, AuthPortError> {
        self.workers(query, limit)
            .await
            .map(|workers| {
                workers
                    .into_iter()
                    .map(|worker| WorkerRecord {
                        id: worker.id,
                        name: worker.name,
                        phone: worker.phone,
                    })
                    .collect()
            })
            .map_err(|_| AuthPortError::LookupFailed)
    }
}

pub fn normalize_level(value: &str) -> Result<String, WorkerError> {
    let trimmed = value.trim();
    WORKER_LEVELS
        .iter()
        .find(|level| level.eq_ignore_ascii_case(trimmed))
        .map(|level| (*level).to_string())
        .ok_or(WorkerError::InvalidLevel)
}

fn new_worker_id() -> String {
    let bytes: [u8; 12] = rand::random();
    format!("worker_{}", data_encoding::HEXLOWER.encode(&bytes))
}

struct UnavailableWorkerStore;

#[async_trait]
impl WorkerStorePort for UnavailableWorkerStore {
    async fn workers(&self, _query: &str, _limit: usize) -> Result<Vec<Worker>, WorkerError> {
        Err(WorkerError::StoreFailed)
    }

    async fn workers_by_ids(&self, _ids: &[String]) -> Result<Vec<Worker>, WorkerError> {
        Err(WorkerError::StoreFailed)
    }

    async fn upsert_worker(&self, _worker: Worker) -> Result<Worker, WorkerError> {
        Err(WorkerError::StoreFailed)
    }

    async fn update_worker_level(&self, _id: &str, _level: &str) -> Result<Worker, WorkerError> {
        Err(WorkerError::StoreFailed)
    }

    async fn update_worker_phone(&self, _id: &str, _phone: &str) -> Result<Worker, WorkerError> {
        Err(WorkerError::StoreFailed)
    }

    async fn delete_worker(&self, _id: &str) -> Result<(), WorkerError> {
        Err(WorkerError::StoreFailed)
    }
}

#[derive(Default)]
pub struct MemoryWorkerStore {
    workers: RwLock<Vec<Worker>>,
}

impl MemoryWorkerStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl WorkerStorePort for MemoryWorkerStore {
    async fn workers(&self, query: &str, limit: usize) -> Result<Vec<Worker>, WorkerError> {
        let needle = query.trim().to_lowercase();
        let mut workers = self.workers.read().await.clone();
        workers.sort_by_key(|worker| worker.name.to_lowercase());
        Ok(workers
            .into_iter()
            .filter(|worker| {
                needle.is_empty()
                    || worker.name.to_lowercase().contains(&needle)
                    || worker.level.to_lowercase().contains(&needle)
            })
            .take(limit)
            .collect())
    }

    async fn workers_by_ids(&self, ids: &[String]) -> Result<Vec<Worker>, WorkerError> {
        let requested = ids
            .iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        let workers = self.workers.read().await;
        Ok(requested
            .into_iter()
            .filter_map(|id| workers.iter().find(|worker| worker.id == id).cloned())
            .collect())
    }

    async fn upsert_worker(&self, worker: Worker) -> Result<Worker, WorkerError> {
        let mut workers = self.workers.write().await;
        let key = worker.name.to_lowercase();
        if let Some(existing) = workers
            .iter_mut()
            .find(|item| item.id == worker.id || item.name.to_lowercase() == key)
        {
            existing.name = worker.name;
            existing.phone = worker.phone;
            existing.level = worker.level;
            return Ok(existing.clone());
        }
        workers.push(worker.clone());
        Ok(worker)
    }

    async fn update_worker_level(&self, id: &str, level: &str) -> Result<Worker, WorkerError> {
        let mut workers = self.workers.write().await;
        let Some(worker) = workers.iter_mut().find(|item| item.id == id.trim()) else {
            return Err(WorkerError::NotFound);
        };
        worker.level = level.to_string();
        Ok(worker.clone())
    }

    async fn update_worker_phone(&self, id: &str, phone: &str) -> Result<Worker, WorkerError> {
        let mut workers = self.workers.write().await;
        let Some(worker) = workers.iter_mut().find(|item| item.id == id.trim()) else {
            return Err(WorkerError::NotFound);
        };
        worker.phone = phone.trim().to_string();
        Ok(worker.clone())
    }

    async fn delete_worker(&self, id: &str) -> Result<(), WorkerError> {
        let mut workers = self.workers.write().await;
        let previous_len = workers.len();
        workers.retain(|worker| worker.id != id.trim());
        if workers.len() == previous_len {
            return Err(WorkerError::NotFound);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_level_allows_only_configured_levels() {
        assert_eq!(normalize_level("brigader").unwrap(), "Brigader");
        assert_eq!(normalize_level("Master").unwrap(), "Master");
        assert!(matches!(
            normalize_level("operator"),
            Err(WorkerError::InvalidLevel)
        ));
    }
}
