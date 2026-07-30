use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::formula::{DEFAULT_EDGE_ALLOWANCE_MM, LayerInput, derive_width_mm};

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
    pub frame_product_size_mm: f64,
    #[serde(default)]
    pub frame_count: f64,
    #[serde(default = "default_edge_allowance")]
    pub edge_allowance_mm: f64,
    #[serde(default)]
    pub width_mm: f64,
    #[serde(default = "default_waste_percent")]
    pub waste_percent: f64,
    #[serde(default)]
    pub roll_count: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<LayerInput>,
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
    if template.frame_product_size_mm <= 0.0 {
        return Err(CalculateOrderError::InvalidInput(
            "kadrdagi mahsulot o'lchami noto'g'ri".to_string(),
        ));
    }
    if template.frame_count <= 0.0 {
        return Err(CalculateOrderError::InvalidInput(
            "kadr soni noto'g'ri".to_string(),
        ));
    }
    if template.edge_allowance_mm < 0.0 {
        return Err(CalculateOrderError::InvalidInput(
            "qo'shimcha razmer noto'g'ri".to_string(),
        ));
    }
    if derive_width_mm(
        Some(template.frame_product_size_mm),
        Some(template.frame_count),
        Some(template.edge_allowance_mm),
    )
    .is_err()
    {
        return Err(CalculateOrderError::InvalidInput(
            "razmer noto'g'ri".to_string(),
        ));
    }
    if template.waste_percent < 0.0 {
        return Err(CalculateOrderError::InvalidInput(
            "atxod foiz noto'g'ri".to_string(),
        ));
    }
    let layers = template.effective_layers();
    if layers.is_empty() {
        return Err(CalculateOrderError::InvalidInput(
            "1-qavat kerak".to_string(),
        ));
    }
    for (index, layer) in layers.iter().enumerate() {
        validate_layer(layer, index + 1)?;
    }
    Ok(())
}

fn validate_layer(layer: &LayerInput, number: usize) -> Result<(), CalculateOrderError> {
    if layer.material.trim().is_empty() || layer.micron.trim().is_empty() {
        return Err(CalculateOrderError::InvalidInput(format!(
            "{number}-qavat materiali va mikroni birga kiritilishi kerak"
        )));
    }
    Ok(())
}

impl CalculateOrderTemplate {
    pub fn effective_layers(&self) -> Vec<LayerInput> {
        if !self.layers.is_empty() {
            return self.layers.clone();
        }
        [
            LayerInput::new(&self.first_layer_material, &self.first_layer_micron),
            LayerInput::new(&self.second_layer_material, &self.second_layer_micron),
            LayerInput::new(&self.third_layer_material, &self.third_layer_micron),
        ]
        .into_iter()
        .filter(|layer| !layer.is_empty())
        .collect()
    }
}

pub fn hydrate_template_layers(mut template: CalculateOrderTemplate) -> CalculateOrderTemplate {
    template.layers = template
        .effective_layers()
        .into_iter()
        .map(|layer| LayerInput::new(layer.material.trim(), layer.micron.trim()))
        .filter(|layer| !layer.is_empty())
        .collect();
    let layer = |index: usize| template.layers.get(index).cloned().unwrap_or_default();
    let first = layer(0);
    let second = layer(1);
    let third = layer(2);
    template.first_layer_material = first.material;
    template.first_layer_micron = first.micron;
    template.second_layer_material = second.material;
    template.second_layer_micron = second.micron;
    template.third_layer_material = third.material;
    template.third_layer_micron = third.micron;
    template
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_template() -> CalculateOrderTemplate {
        CalculateOrderTemplate {
            name: "Bir qavatli zakaz".to_string(),
            product: "Mahsulot".to_string(),
            frame_product_size_mm: 100.0,
            frame_count: 1.0,
            first_layer_material: "pet".to_string(),
            first_layer_micron: "12".to_string(),
            ..CalculateOrderTemplate::default()
        }
    }

    #[test]
    fn accepts_single_layer_template() {
        validate_template(&valid_template()).expect("single-layer template");
    }

    #[test]
    fn rejects_partially_filled_optional_layer() {
        let mut template = valid_template();
        template.second_layer_material = "pe oq".to_string();

        let error = validate_template(&template).expect_err("incomplete second layer");

        assert_eq!(
            error.to_string(),
            "invalid input: 2-qavat materiali va mikroni birga kiritilishi kerak"
        );
    }

    #[test]
    fn accepts_template_with_arbitrary_layer_count() {
        let mut template = valid_template();
        template.layers = (1..=8)
            .map(|number| LayerInput::new(format!("pet{number}"), "12"))
            .collect();

        validate_template(&template).expect("arbitrary layer template");
    }
}

pub fn hydrate_template_dimensions(mut template: CalculateOrderTemplate) -> CalculateOrderTemplate {
    template = hydrate_template_layers(template);
    if !template.edge_allowance_mm.is_finite() || template.edge_allowance_mm < 0.0 {
        template.edge_allowance_mm = DEFAULT_EDGE_ALLOWANCE_MM;
    }
    let frame_size_valid =
        template.frame_product_size_mm.is_finite() && template.frame_product_size_mm > 0.0;
    let frame_count_valid = template.frame_count.is_finite() && template.frame_count > 0.0;
    let width_valid =
        template.width_mm.is_finite() && template.width_mm > template.edge_allowance_mm;
    if !frame_size_valid && width_valid && frame_count_valid {
        template.frame_product_size_mm =
            (template.width_mm - template.edge_allowance_mm) / template.frame_count;
    } else if frame_size_valid && width_valid && !frame_count_valid {
        template.frame_count =
            (template.width_mm - template.edge_allowance_mm) / template.frame_product_size_mm;
    } else if (!frame_size_valid || !frame_count_valid) && width_valid {
        template.frame_product_size_mm = template.width_mm - template.edge_allowance_mm;
        template.frame_count = 1.0;
    }
    if template.frame_product_size_mm.is_finite()
        && template.frame_product_size_mm > 0.0
        && template.frame_count.is_finite()
        && template.frame_count > 0.0
    {
        template.width_mm = derive_width_mm(
            Some(template.frame_product_size_mm),
            Some(template.frame_count),
            Some(template.edge_allowance_mm),
        )
        .unwrap_or(template.width_mm);
    }
    template
}

pub fn owner_key(role: &str, ref_: &str) -> String {
    format!("{}:{}", role.trim(), ref_.trim())
}

fn default_waste_percent() -> f64 {
    5.0
}

fn default_edge_allowance() -> f64 {
    DEFAULT_EDGE_ALLOWANCE_MM
}

fn is_zero_f64(value: &f64) -> bool {
    *value == 0.0
}
