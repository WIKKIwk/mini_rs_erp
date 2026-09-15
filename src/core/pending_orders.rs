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
            || !matches!(t.status.as_str(), "rulon" | "paket")
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

    /// The completion form cannot silently replace the Telegram order identity.
    pub fn merge_completion(&self, mut t: CalculateOrderTemplate) -> CalculateOrderTemplate {
        let source = &self.template;
        t.id = source.id.clone();
        t.code = source.code.clone();
        t.order_number = source.order_number.clone();
        t.name = source.product.clone();
        t.customer_ref = source.customer_ref.clone();
        t.customer = source.customer.clone();
        t.item_code = source.item_code.clone();
        t.product = source.product.clone();
        t.status = source.status.clone();
        t.kg = source.kg;
        t.frame_product_size_mm = source.frame_product_size_mm;
        t.frame_count = source.frame_count;
        t.edge_allowance_mm = source.edge_allowance_mm;
        t.roll_count = source.roll_count.or(t.roll_count);
        t.print_val_size_mm = source.print_val_size_mm.or(t.print_val_size_mm);
        if !source.color.trim().is_empty() {
            t.color = source.color.clone();
        }
        if !source.note.trim().is_empty() {
            t.note = source.note.clone();
        }
        t.layers = source.layers.clone();
        t.image_id = source.image_id.clone();
        t.image_name = source.image_name.clone();
        t.image_mime = source.image_mime.clone();
        t.image_size_bytes = source.image_size_bytes;
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
    fn pending_completion_preserves_telegram_data_and_accepts_missing_fields() {
        let (order, _) = test_pending_order("9011");
        let incoming = CalculateOrderTemplate {
            order_number: "9999".into(),
            kg: 1.0,
            product: "Replaced".into(),
            waste_percent: 7.0,
            roll_count: Some(10),
            print_val_size_mm: Some(600.0),
            color: "Qizil".into(),
            note: "Tezroq".into(),
            ..Default::default()
        };
        let merged = order.merge_completion(incoming);
        assert_eq!(merged.order_number, "9011");
        assert_eq!(merged.kg, 500.0);
        assert_eq!(merged.product, "Mahsulot");
        assert_eq!(merged.customer_ref, "CUST-1");
        assert_eq!(merged.frame_product_size_mm, 300.0);
        assert_eq!(merged.frame_count, 2.0);
        assert_eq!(merged.layers.len(), 1);
        assert_eq!(merged.image_id, order.template.image_id);
        assert_eq!(merged.waste_percent, 7.0);
        assert_eq!(merged.roll_count, Some(10));
        assert_eq!(merged.print_val_size_mm, Some(600.0));
        assert_eq!(merged.color, "Qizil");
        assert_eq!(merged.note, "Tezroq");
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
