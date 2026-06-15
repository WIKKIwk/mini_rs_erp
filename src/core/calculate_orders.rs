use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct CalculateOrderTemplate {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub saved_at: String,
    #[serde(default)]
    pub order_number: String,
    #[serde(default)]
    pub customer_ref: String,
    #[serde(default)]
    pub customer: String,
    #[serde(default)]
    pub item_code: String,
    #[serde(default)]
    pub product: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub material_display: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub image_id: String,
    #[serde(default)]
    pub image_name: String,
    #[serde(default)]
    pub image_mime: String,
    #[serde(default)]
    pub image_size_bytes: u64,
    #[serde(default)]
    pub image_url: String,
    #[serde(default)]
    pub width_mm: f64,
    #[serde(default = "default_waste_percent")]
    pub waste_percent: f64,
    #[serde(default)]
    pub roll_count: Option<f64>,
    #[serde(default)]
    pub first_layer_material: String,
    #[serde(default)]
    pub first_layer_micron: String,
    #[serde(default)]
    pub second_layer_material: String,
    #[serde(default)]
    pub second_layer_micron: String,
    #[serde(default)]
    pub third_layer_material: String,
    #[serde(default)]
    pub third_layer_micron: String,
    #[serde(default)]
    pub note: String,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub kg: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_map_id: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CalculateOrderImage {
    pub image_id: String,
    pub image_name: String,
    pub image_mime: String,
    pub image_size_bytes: u64,
    pub body: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum CalculateOrderError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("store failed")]
    StoreFailed,
}

#[async_trait]
pub trait CalculateOrderStorePort: Send + Sync {
    async fn list(
        &self,
        owner_key: &str,
    ) -> Result<Vec<CalculateOrderTemplate>, CalculateOrderError>;
    async fn list_all(&self) -> Result<Vec<CalculateOrderTemplate>, CalculateOrderError> {
        Err(CalculateOrderError::StoreFailed)
    }
    async fn upsert(
        &self,
        owner_key: &str,
        template: CalculateOrderTemplate,
    ) -> Result<CalculateOrderTemplate, CalculateOrderError>;
    async fn delete(&self, owner_key: &str, id: &str) -> Result<(), CalculateOrderError>;
    async fn save_image(
        &self,
        owner_key: &str,
        image: CalculateOrderImage,
    ) -> Result<CalculateOrderImage, CalculateOrderError>;
    async fn get_image(
        &self,
        owner_key: &str,
        image_id: &str,
    ) -> Result<Option<CalculateOrderImage>, CalculateOrderError>;
}

pub fn validate_template(template: &CalculateOrderTemplate) -> Result<(), CalculateOrderError> {
    if template.name.trim().is_empty() {
        return Err(CalculateOrderError::InvalidInput(
            "zakaz nomi kerak".to_string(),
        ));
    }
    if template.product.trim().is_empty() {
        return Err(CalculateOrderError::InvalidInput(
            "mahsulot kerak".to_string(),
        ));
    }
    if template.width_mm <= 0.0 {
        return Err(CalculateOrderError::InvalidInput(
            "razmer noto'g'ri".to_string(),
        ));
    }
    if template.waste_percent < 0.0 {
        return Err(CalculateOrderError::InvalidInput(
            "atxod foiz noto'g'ri".to_string(),
        ));
    }
    if template.first_layer_material.trim().is_empty()
        || template.first_layer_micron.trim().is_empty()
    {
        return Err(CalculateOrderError::InvalidInput(
            "1-qavat kerak".to_string(),
        ));
    }
    if template.second_layer_material.trim().is_empty()
        || template.second_layer_micron.trim().is_empty()
    {
        return Err(CalculateOrderError::InvalidInput(
            "2-qavat kerak".to_string(),
        ));
    }
    Ok(())
}

pub fn owner_key(role: &str, ref_: &str) -> String {
    format!("{}:{}", role.trim(), ref_.trim())
}

fn default_waste_percent() -> f64 {
    5.0
}

fn is_zero_f64(value: &f64) -> bool {
    *value == 0.0
}
