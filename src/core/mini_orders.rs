use async_trait::async_trait;
use thiserror::Error;
use sha2::{Digest, Sha256};

use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::production_map::{ProductionMapDefinition, ProductionMapError, QueueActionActor};
use crate::core::werka::models::SupplierItem;

/// Catalog material and thicknesses from an opened order's saved layers.
#[derive(Debug, Clone)]
pub struct OrderMaterial {
    pub item: SupplierItem,
    pub microns: Vec<f64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OrderMaterialTask {
    pub order_id: String,
    pub item_code: String,
    pub material: String,
    pub micron: f64,
    /// Whether this order has any material linked, regardless of stock usage.
    pub assigned: bool,
}

/// Prepared maps and the exact calculation must become visible together.
pub struct NewProductionOrder {
    pub creation: Option<OrderCreationIdentity>,
    pub map: ProductionMapDefinition,
    pub template_map: Option<ProductionMapDefinition>,
    pub template: CalculateOrderTemplate,
    pub quick_template: Option<CalculateOrderTemplate>,
    pub owner_key: String,
    pub created_by: QueueActionActor,
}

/// Original creation metadata, independent of reusable templates and later edits.
#[derive(Debug, Clone, Default)]
pub struct OrderCreationMetadata {
    pub created_at_unix: Option<i64>,
    pub created_by: Option<QueueActionActor>,
}

#[derive(Clone)]
pub struct OrderCreationIdentity {
    pub key: String,
    pub fingerprint: String,
}

#[derive(Debug, Error)]
pub enum MiniOrderError {
    #[error("mini order store failed")]
    StoreFailed,
}

/// Match repeat orders by customer, SKU and physical print specification.
pub(crate) fn print_side_layout_key(template: &CalculateOrderTemplate) -> Option<String> {
    let customer = template.customer_ref.trim();
    let item = template.item_code.trim();
    if customer.is_empty() || (item.is_empty() && template.products.is_empty()) {
        return None;
    }
    let products = template.products.iter().map(|product| {
        (product.item_code.trim(), product.frame_product_size_mm, product.frame_count)
    }).collect::<Vec<_>>();
    if products.iter().any(|(sku, _, _)| sku.is_empty()) {
        return None;
    }
    let flexo = template.production_options.as_ref().map_or_else(
        || template.status.trim().eq_ignore_ascii_case("flexo"),
        |options| options.print_method == super::production_map::automatic::PrintMethod::Flexo,
    );
    let layout = serde_json::json!({
        "version": 1,
        "customer": customer,
        "item": if products.is_empty() { item } else {
            template.combo.as_ref().map(|combo| combo.item_code.as_str()).unwrap_or("")
        },
        "products": products,
        "frame_width": if products.is_empty() { template.frame_product_size_mm } else { 0.0 },
        "frame_count": if products.is_empty() { template.frame_count } else { 0.0 },
        "print_method": if flexo { "flexo" } else { "metal" },
        "print_val": template.print_val_size_mm,
        "color": template.color.trim(),
        "artwork": template.image_id.trim(),
    });
    Some(format!("{:x}", Sha256::digest(serde_json::to_vec(&layout).ok()?)))
}

#[async_trait]
pub trait MiniOrderSink: Send + Sync {
    async fn save_template_map_atomic(
        &self, _map: &ProductionMapDefinition, _template: &CalculateOrderTemplate, _owner: &str,
    ) -> Result<CalculateOrderTemplate, ProductionMapError> { Err(ProductionMapError::StoreFailed) }
    async fn print_side_preference(
        &self,
        _template: &CalculateOrderTemplate,
    ) -> Result<Option<bool>, MiniOrderError> {
        Ok(None)
    }

    async fn order_creation_result(
        &self,
        _owner: &str,
        _key: &str,
    ) -> Result<Option<(String, serde_json::Value)>, ProductionMapError> {
        Ok(None)
    }
    async fn order_material_tasks(
        &self,
        _order_ids: &[String],
        _item_groups: &[String],
    ) -> Result<Vec<OrderMaterialTask>, MiniOrderError> {
        Err(MiniOrderError::StoreFailed)
    }

    async fn order_materials(&self, _order_id: &str) -> Result<Vec<OrderMaterial>, MiniOrderError> {
        Err(MiniOrderError::StoreFailed)
    }

    async fn order_creation_metadata(
        &self,
        _order_id: &str,
    ) -> Result<OrderCreationMetadata, MiniOrderError> {
        Ok(OrderCreationMetadata::default())
    }

    async fn create_order_atomic(
        &self,
        _order: &NewProductionOrder,
    ) -> Result<Option<CalculateOrderTemplate>, ProductionMapError> {
        // A persistence-enabled sink must explicitly support atomic creation.
        Err(ProductionMapError::StoreFailed)
    }

    /// Reads the opened order's calculation even after production has started.
    async fn order_calculation(
        &self,
        _order_id: &str,
    ) -> Result<CalculateOrderTemplate, super::order_edit::OrderEditError> {
        Err(super::order_edit::OrderEditError::Store)
    }

    async fn order_edit_source(
        &self,
        _order_id: &str,
    ) -> Result<super::order_edit::OrderEditSource, super::order_edit::OrderEditError> {
        Err(super::order_edit::OrderEditError::Store)
    }

    async fn save_order_edit(
        &self,
        _original: &super::order_edit::OrderEditSource,
        _map: &ProductionMapDefinition,
        _template: &CalculateOrderTemplate,
        _actor: &crate::core::production_map::QueueActionActor,
    ) -> Result<super::order_edit::OrderEditSource, super::order_edit::OrderEditError> {
        Err(super::order_edit::OrderEditError::Store)
    }

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

    #[test]
    fn print_side_preferences_follow_customer_and_print_layout() {
        let mut order = CalculateOrderTemplate {
            customer_ref: "customer-a".into(),
            item_code: "SKU".into(),
            frame_product_size_mm: 515.0,
            frame_count: 2.0,
            ..Default::default()
        };
        let original = print_side_layout_key(&order).unwrap();
        order.kg = 1000.0;
        order.order_number = "0123".into();
        assert_eq!(print_side_layout_key(&order).as_deref(), Some(original.as_str()));
        order.customer_ref = "customer-b".into();
        assert_ne!(print_side_layout_key(&order).unwrap(), original);
        order.customer_ref = "customer-a".into();
        order.frame_count = 3.0;
        assert_ne!(print_side_layout_key(&order).unwrap(), original);
    }

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

        let synced = sink
            .sync_orders(&maps, &templates)
            .await
            .expect("sync orders");

        assert_eq!(synced, 1);
        assert_eq!(
            sink.saved_map_ids
                .lock()
                .expect("recording sink lock")
                .as_slice(),
            ["zakaz-1111"]
        );
    }

    fn test_map(id: &str, order_number: &str) -> ProductionMapDefinition {
        ProductionMapDefinition {
            combo: None,
            products: Vec::new(),
            print_val_size_mm: None,
            id: id.to_string(),
            product_code: "ITEM-1".to_string(),
            title: "Test map".to_string(),
            code: order_number.to_string(),
            order_number: order_number.to_string(),
            customer_name: String::new(),
            image_id: String::new(),
            roll_count: None,
            width_mm: None,
            order_kg: None,
            base_length: None,
            duplicate_rezka_labels: None,
            include_bobina_in_net: None,
            standard_print_side: None,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}
