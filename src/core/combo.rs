//! Stable combo identity and the physical layout revision used by its tooling.
//! Component SKUs remain the finished outputs; the combo is a planning identity.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::calculate_orders::CalculateOrderTemplate;

pub const COMBO_ITEM_GROUP: &str = "Kombo";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComboReference {
    pub id: String,
    pub item_code: String,
    pub name: String,
    pub revision: i64,
    pub layout_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComboCatalogEntry {
    pub combo: ComboReference,
    pub template: CalculateOrderTemplate,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ComboError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("combo_name_conflict")]
    NameConflict,
    #[error("combo_not_found")]
    NotFound,
    #[error("combo_reference_mismatch")]
    ReferenceMismatch,
    #[error("combo_store_failed")]
    StoreFailed,
}

pub fn normalize_name(name: &str) -> Result<String, ComboError> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > 160 || name.chars().any(char::is_control) {
        return Err(ComboError::InvalidInput("combo_name_invalid".into()));
    }
    Ok(name)
}

/// Quantity, waste, notes and transient row IDs do not change physical tooling.
/// Ordered components, artwork and printing/material specifications do.
pub fn layout_fingerprint(template: &CalculateOrderTemplate) -> Result<String, ComboError> {
    if template.products.is_empty() || template.customer_ref.trim().is_empty() {
        return Err(ComboError::InvalidInput("combo_layout_required".into()));
    }
    if !template.edge_allowance_mm.is_finite() || template.edge_allowance_mm < 0.0 {
        return Err(ComboError::InvalidInput(
            "combo_edge_allowance_invalid".into(),
        ));
    }
    let mut ordinary = template.clone();
    ordinary.combo = None;
    ordinary = super::calculate_orders::hydrate_template_dimensions(ordinary);
    super::calculate_orders::validate_template(&ordinary)
        .map_err(|error| ComboError::InvalidInput(error.to_string()))?;
    let products = ordinary
        .products
        .iter()
        .map(|product| {
            serde_json::json!({
                "item_code": product.item_code.trim().to_lowercase(),
                "width_mm": product.frame_product_size_mm,
                "frame_count": product.frame_count,
            })
        })
        .collect::<Vec<_>>();
    let layers = ordinary
        .effective_layers()
        .into_iter()
        .map(|layer| {
            serde_json::json!({
                "material_id": layer.material_id.trim(),
                "material": layer.material.trim(),
                "micron": layer.micron.trim(),
            })
        })
        .collect::<Vec<_>>();
    let layout = serde_json::json!({
        "version": 1,
        "customer_ref": ordinary.customer_ref.trim(),
        "products": products,
        "layers": layers,
        "edge_allowance_mm": ordinary.edge_allowance_mm,
        "print_val_size_mm": ordinary.print_val_size_mm,
        "production_options": ordinary.production_options,
        "roll_count": ordinary.roll_count,
        "status": ordinary.status.trim().to_lowercase(),
        "image_id": ordinary.image_id.trim(),
        "color": ordinary.color.trim(),
    });
    let bytes = serde_json::to_vec(&layout).map_err(|_| ComboError::ReferenceMismatch)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// Local integrity check, supplemented by the stored immutable revision at HTTP boundaries.
pub fn validate_reference(template: &CalculateOrderTemplate) -> Result<(), ComboError> {
    let Some(combo) = &template.combo else {
        return Ok(());
    };
    if combo.id.is_empty()
        || combo.item_code.is_empty()
        || combo.revision < 1
        || normalize_name(&combo.name)? != combo.name
        || template.item_code != combo.item_code
        || template.name != combo.name
        || template.product != combo.name
        || layout_fingerprint(template)? != combo.layout_fingerprint
    {
        return Err(ComboError::ReferenceMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
