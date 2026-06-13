use async_trait::async_trait;

use super::models::{CreateRezkaRepackDraftInput, RezkaRepackDraft};

#[async_trait]
pub trait RezkaRepackStorePort: Send + Sync {
    async fn create_rezka_repack_draft(
        &self,
        input: CreateRezkaRepackDraftInput,
    ) -> Result<RezkaRepackDraft, RezkaPortError>;

    async fn submit_rezka_repack_draft(&self, name: &str) -> Result<(), RezkaPortError>;

    async fn delete_rezka_repack_draft(&self, name: &str) -> Result<(), RezkaPortError>;
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct RezkaPortError(String);

impl RezkaPortError {
    pub fn message(&self) -> String {
        self.to_string()
    }
}
