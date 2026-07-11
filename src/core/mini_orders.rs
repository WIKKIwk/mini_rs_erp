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
    fn enabled(&self) -> bool {
        false
    }

    async fn save_order(
        &self,
        map: &ProductionMapDefinition,
        template: &CalculateOrderTemplate,
    ) -> Result<(), MiniOrderError>;

    async fn sync_orders(
        &self,
        maps: &[ProductionMapDefinition],
        templates: &[CalculateOrderTemplate],
    ) -> Result<usize, MiniOrderError> {
        let mut synced = 0;
        for map in maps.iter().filter(|map| is_order_map(map)) {
            let Some(template) = templates.iter().find(|template| {
                template.source_map_id.trim() == map.id.trim()
                    || template.order_number.trim() == map.order_number.trim()
                    || template.code.trim() == map.code.trim()
            }) else {
                continue;
            };
            self.save_order(map, template).await?;
            synced += 1;
        }
        Ok(synced)
    }
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

fn is_order_map(map: &ProductionMapDefinition) -> bool {
    let order_number = map.order_number.trim();
    map.id.trim().starts_with("zakaz-")
        && order_number.len() == 4
        && order_number.chars().all(|ch| ch.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct RecordingSink {
        saved_map_ids: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl MiniOrderSink for RecordingSink {
        async fn save_order(
            &self,
            map: &ProductionMapDefinition,
            _template: &CalculateOrderTemplate,
        ) -> Result<(), MiniOrderError> {
            self.saved_map_ids
                .lock()
                .expect("recording sink lock")
                .push(map.id.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn sync_orders_reconciles_only_matching_order_maps() {
        let sink = RecordingSink::default();
        let maps = vec![test_map("zakaz-1111", "1111"), test_map("template-a", "")];
        let templates = vec![CalculateOrderTemplate {
            source_map_id: "zakaz-1111".to_string(),
            ..CalculateOrderTemplate::default()
        }];

        let synced = sink.sync_orders(&maps, &templates).await.expect("sync orders");

        assert_eq!(synced, 1);
        assert_eq!(
            sink.saved_map_ids.lock().expect("recording sink lock").as_slice(),
            ["zakaz-1111"]
        );
    }

    fn test_map(id: &str, order_number: &str) -> ProductionMapDefinition {
        ProductionMapDefinition {
            id: id.to_string(),
            product_code: "ITEM-1".to_string(),
            title: "Test map".to_string(),
            code: order_number.to_string(),
            order_number: order_number.to_string(),
            roll_count: None,
            width_mm: None,
            order_kg: None,
            base_length: None,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}
