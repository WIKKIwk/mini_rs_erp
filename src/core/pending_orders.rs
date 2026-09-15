//! Telegram intake is durable, but has no production map until completion.
use super::calculate_orders::{CalculateOrderImage, CalculateOrderTemplate};
use super::production_map::ProductionMapSaved;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingOrder {
    pub id: String,
    pub telegram_user_id: String,
    pub manager_name: String,
    pub template: CalculateOrderTemplate,
    pub created_at: i64,
    #[serde(default)]
    pub completion: Option<PendingOrderCompletion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingOrderCompletion {
    pub saved: ProductionMapSaved,
    pub template: CalculateOrderTemplate,
}

#[derive(Debug, thiserror::Error)]
pub enum PendingOrderError {
    #[error("chala buyurtma topilmadi")]
    NotFound,
    #[error("buyurtma ma'lumotlari noto'g'ri: {0}")]
    Invalid(String),
    #[error("buyurtma raqami band")]
    Conflict,
    #[error("chala buyurtmani saqlashda xatolik")]
    Store,
}

impl PendingOrder {
    pub fn validate(&self) -> Result<(), PendingOrderError> {
        let t = &self.template;
        let positive = |v: f64| v.is_finite() && v > 0.0;
        if t.order_number.len() != 4
            || !t.order_number.bytes().all(|b| b.is_ascii_digit())
            || self.id != format!("zakaz-{}", t.order_number)
            || t.id.trim().is_empty()
            || t.code.trim().is_empty()
            || self.telegram_user_id.trim().is_empty()
            || t.customer_ref.trim().is_empty()
            || t.item_code.trim().is_empty()
            || t.product.trim().is_empty()
            || t.customer.trim().is_empty()
            || !matches!(t.status.as_str(), "rulon" | "paket" | "flexo")
            || !t.edge_allowance_mm.is_finite()
            || t.edge_allowance_mm < 0.0
            || !positive(t.kg)
            || !positive(t.frame_product_size_mm)
            || !positive(t.frame_count)
            || t.frame_count.fract() != 0.0
            || t.frame_count > i32::MAX as f64
            || t.layers.is_empty()
            || t.layers
                .iter()
                .any(|l| l.material.trim().is_empty() || l.micron.trim().is_empty())
        {
            return Err(PendingOrderError::Invalid("asosiy maydonlar kerak".into()));
        }
        Ok(())
    }

    /// Reuse the normal editor's values, but keep the reserved order identity.
    pub fn merge_completion(&self, mut t: CalculateOrderTemplate) -> CalculateOrderTemplate {
        let source = &self.template;
        t.id = source.id.clone();
        t.code = source.code.clone();
        t.order_number = source.order_number.clone();
        if t.production_options.is_none() {
            t.production_options = source.production_options.clone();
        }
        t.name = t.product.clone();
        if t.image_id == source.image_id {
            t.image_name = source.image_name.clone();
            t.image_mime = source.image_mime.clone();
            t.image_size_bytes = source.image_size_bytes;
        }
        t.image_url.clear();
        t
    }
}

#[cfg(test)]
pub(crate) fn test_pending_order(number: &str) -> (PendingOrder, CalculateOrderImage) {
    let image = CalculateOrderImage {
        image_id: format!("telegram-order-{number}-{:032x}", rand::random::<u128>()),
        image_name: "order.webp".into(),
        image_mime: "image/webp".into(),
        image_size_bytes: 4,
        body: vec![1, 2, 3, 4],
    };
    let order = PendingOrder {
        id: format!("zakaz-{number}"),
        telegram_user_id: "123".into(),
        manager_name: "Manager".into(),
        created_at: 1234,
        completion: None,
        template: CalculateOrderTemplate {
            id: format!("telegram-template-{:032x}", rand::random::<u128>()),
            code: format!("TG-{:032x}", rand::random::<u128>()),
            order_number: number.into(),
            customer_ref: "CUST-1".into(),
            customer: "Mijoz".into(),
            item_code: "ITEM-1".into(),
            product: "Mahsulot".into(),
            status: "rulon".into(),
            kg: 500.0,
            frame_product_size_mm: 300.0,
            frame_count: 2.0,
            edge_allowance_mm: 15.0,
            waste_percent: 5.0,
            layers: vec![crate::core::formula::LayerInput::new("pet", "12")],
            image_id: image.image_id.clone(),
            image_name: image.image_name.clone(),
            image_mime: image.image_mime.clone(),
            image_size_bytes: image.image_size_bytes,
            ..Default::default()
        },
    };
    (order, image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_order_requires_valid_frame_dimensions_and_identity() {
        let (order, _) = test_pending_order("9011");
        assert!(order.validate().is_ok());
        for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut invalid = order.clone();
            invalid.template.frame_product_size_mm = value;
            assert!(invalid.validate().is_err());
            invalid = order.clone();
            invalid.template.frame_count = value;
            assert!(invalid.validate().is_err());
        }
        let mut invalid = order.clone();
        invalid.template.frame_count = 1.5;
        assert!(invalid.validate().is_err());
        invalid = order;
        invalid.id = "zakaz-other".into();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn pending_completion_preserves_identity_and_accepts_normal_editor_values() {
        let (mut order, _) = test_pending_order("9011");
        order.template.production_options = Some(crate::core::production_map::automatic::OrderProductionOptions {
            print_method: crate::core::production_map::automatic::PrintMethod::Flexo,
            cold_glue: true,
            diameter_mm: Some(45.5),
        });
        let incoming = CalculateOrderTemplate {
            production_options: None, // An older Mobile must not erase Telegram routing choices.
            order_number: "9999".into(),
            kg: 1.0,
            product: "Replaced".into(),
            status: "Flexo".into(),
            frame_product_size_mm: 250.0,
            frame_count: 3.0,
            edge_allowance_mm: 55.0,
            waste_percent: 7.0,
            roll_count: Some(10),
            print_val_size_mm: Some(600.0),
            color: "Qizil".into(),
            note: "Tezroq".into(),
            ..order.template.clone()
        };
        let merged = order.merge_completion(incoming);
        assert_eq!(merged.id, order.template.id);
        assert_eq!(merged.code, order.template.code);
        assert_eq!(merged.order_number, "9011");
        assert_eq!(merged.production_options, order.template.production_options);
        assert_eq!(merged.kg, 1.0);
        assert_eq!(merged.product, "Replaced");
        assert_eq!(merged.name, "Replaced");
        assert_eq!(merged.status, "Flexo");
        assert_eq!(merged.customer_ref, "CUST-1");
        assert_eq!(merged.frame_product_size_mm, 250.0);
        assert_eq!(merged.frame_count, 3.0);
        assert_eq!(merged.edge_allowance_mm, 55.0);
        assert_eq!(merged.layers.len(), 1);
        assert_eq!(merged.image_id, order.template.image_id);
        assert_eq!(merged.waste_percent, 7.0);
        assert_eq!(merged.roll_count, Some(10));
        assert_eq!(merged.print_val_size_mm, Some(600.0));
        assert_eq!(merged.color, "Qizil");
        assert_eq!(merged.note, "Tezroq");
    }

    #[test]
    fn pending_flexo_accepts_custom_allowance_and_rejects_invalid_values() {
        let (mut order, _) = test_pending_order("9011");
        order.template.status = "flexo".into();
        for value in [0.0, 40.0, 40.5] {
            order.template.edge_allowance_mm = value;
            assert!(order.validate().is_ok());
        }
        for value in [-1.0, f64::NAN, f64::INFINITY] {
            order.template.edge_allowance_mm = value;
            assert!(order.validate().is_err());
        }
    }

    #[test]
    fn pending_completion_allows_replacing_or_clearing_the_prefilled_image() {
        let (order, _) = test_pending_order("9011");
        for image_id in ["replacement-upload", ""] {
            let mut incoming = order.template.clone();
            incoming.image_id = image_id.into();
            incoming.image_url = "old-url".into();
            let merged = order.merge_completion(incoming);
            assert_eq!(merged.image_id, image_id);
            assert!(merged.image_url.is_empty());
        }
    }
}

#[async_trait]
pub trait PendingOrderStore: Send + Sync {
    async fn create(
        &self,
        order: PendingOrder,
        image: CalculateOrderImage,
    ) -> Result<PendingOrder, PendingOrderError>;
    async fn list(&self) -> Result<Vec<PendingOrder>, PendingOrderError>;
    async fn get(&self, id: &str) -> Result<PendingOrder, PendingOrderError>;
    async fn image(&self, id: &str) -> Result<CalculateOrderImage, PendingOrderError>;
    /// Returns false on an already completed request. All writes must commit together.
    async fn complete(
        &self,
        id: &str,
        owner_key: &str,
        completion: PendingOrderCompletion,
        template_map: ProductionMapSaved,
    ) -> Result<(PendingOrderCompletion, bool), PendingOrderError>;
}
