use async_trait::async_trait;

use crate::core::auth::models::Principal;
use crate::error::AppError;

#[async_trait]
pub trait SessionAccessValidator: Send + Sync {
    async fn is_active(&self, principal: &Principal) -> Result<bool, AppError>;
}
