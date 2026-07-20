use async_trait::async_trait;

use super::models::RpsBatchSession;

#[async_trait]
pub trait RpsBatchStorePort: Send + Sync {
    async fn get(&self, owner_key: &str) -> Result<Option<RpsBatchSession>, RpsBatchStoreError>;

    async fn put(&self, batch: RpsBatchSession) -> Result<(), RpsBatchStoreError>;

    async fn complete(&self, batch: RpsBatchSession) -> Result<(), RpsBatchStoreError>;

    async fn list_completed(
        &self,
        owner_key: &str,
        limit: usize,
    ) -> Result<Vec<RpsBatchSession>, RpsBatchStoreError>;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RpsBatchStoreError {
    #[error("batch store failed")]
    StoreFailed,
}
