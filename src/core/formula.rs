use serde::{Deserialize, Serialize};

mod calculation;
mod materials;
mod request_layers;

pub use self::calculation::{calculate, calculate_with_material_catalog, derive_width_mm};
use self::calculation::{close, normalize, parse_micron_parts, split_parts};

pub const DEFAULT_EDGE_ALLOWANCE_MM: f64 = 15.0;
pub const MIN_MOLD_EXTRA_MM: f64 = 50.0;
pub const DEFAULT_ADHESIVE_GSM_PER_BOND: f64 = 2.5;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CalculateRequest {
    #[serde(default)]
    pub order_number: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub customer: Option<String>,
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub material_display: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub kg: Option<f64>,
    #[serde(default)]
    pub frame_product_size_mm: Option<f64>,
    #[serde(default)]
    pub frame_count: Option<f64>,
    #[serde(default = "default_edge_allowance_option")]
    pub edge_allowance_mm: Option<f64>,
    #[serde(default)]
    pub waste_percent: Option<f64>,
    #[serde(default)]
    pub roll_count: Option<i64>,
    #[serde(default)]
    pub layers: Vec<LayerInput>,
    #[serde(default)]
    pub first_layer: LayerInput,
    #[serde(default)]
    pub second_layer: LayerInput,
    #[serde(default)]
    pub third_layer: LayerInput,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct LayerInput {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material_id: String,
    #[serde(default)]
    pub material: String,
    #[serde(default)]
    pub micron: String,
}

impl LayerInput {
    pub fn new(material: impl Into<String>, micron: impl Into<String>) -> Self {
        Self {
            material_id: String::new(),
            material: material.into(),
            micron: micron.into(),
        }
    }

    pub fn with_material_id(
        material_id: impl Into<String>,
        material: impl Into<String>,
        micron: impl Into<String>,
    ) -> Self {
        Self {
            material_id: material_id.into(),
            material: material.into(),
            micron: micron.into(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.material.trim().is_empty() && self.micron.trim().is_empty()
    }
}

impl CalculateRequest {
    pub fn effective_layers(&self) -> Vec<LayerInput> {
        if !self.layers.is_empty() {
            return self.layers.clone();
        }
        [
            self.first_layer.clone(),
            self.second_layer.clone(),
            self.third_layer.clone(),
        ]
        .into_iter()
        .filter(|layer| !layer.is_empty())
        .collect()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CalculateResponse {
    pub ok: bool,
    pub order_number: Option<String>,
    pub date: Option<String>,
    pub customer: Option<String>,
    pub product: Option<String>,
    pub status: Option<String>,
    pub material_display: Option<String>,
    pub color: Option<String>,
    pub kg: f64,
    pub frame_product_size_mm: f64,
    pub frame_count: f64,
    pub edge_allowance_mm: f64,
    pub width_mm: f64,
    pub min_mold_size_mm: f64,
    pub rubber_size_mm: u32,
    pub waste_percent: f64,
    pub roll_count: Option<i64>,
    pub layers: Vec<LayerInput>,
    pub results: Vec<CalcResult>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalcResult {
    pub film_gsm: f64,
    pub adhesive_gsm: f64,
    pub total_gsm: f64,
    pub first_coeff: f64,
    pub other_coeff: f64,
    pub coeff_sum: f64,
    pub width_sm: f64,
    pub base_length: f64,
    pub waste_length: f64,
    pub rounded_length: f64,
}

fn default_edge_allowance_option() -> Option<f64> {
    Some(DEFAULT_EDGE_ALLOWANCE_MM)
}
