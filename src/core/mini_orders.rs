use async_trait::async_trait;
use thiserror::Error;

use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::production_map::ProductionMapDefinition;

#[derive(Debug, Error)]
pub enum MiniOrderError {
    #[error("mini order store failed")]
    StoreFailed,
}

#[async_trait]
pub trait MiniOrderSink: Send + Sync {
    async fn save_order(
        &self,
        map: &ProductionMapDefinition,
        template: &CalculateOrderTemplate,
    ) -> Result<(), MiniOrderError>;
}

#[derive(Debug, Clone, Copy)]
pub struct NoopMiniOrderSink;

#[async_trait]
impl MiniOrderSink for NoopMiniOrderSink {
    async fn save_order(
        &self,
        _map: &ProductionMapDefinition,
        _template: &CalculateOrderTemplate,
    ) -> Result<(), MiniOrderError> {
        Ok(())
    }
}
