use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::formula::{DEFAULT_EDGE_ALLOWANCE_MM, LayerInput, derive_width_mm};
use crate::core::quantity::deserialize_optional_integer_count;

mod image_reference;
pub use image_reference::OrderImageLookup;

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct CalculateOrderTemplate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub combo: Option<super::combo::ComboReference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub products: Vec<crate::core::order_layout::OrderProduct>,
    /// Explicit independent variants opt out of legacy business-key merging.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub variant_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub production_options: Option<super::production_map::automatic::OrderProductionOptions>,
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
    /// Overrides only print-apparatus compatibility; frame/material dimensions stay intact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print_val_size_mm: Option<f64>,
    #[serde(default = "default_edge_allowance")]
    pub edge_allowance_mm: f64,
    #[serde(default)]
    pub width_mm: f64,
    #[serde(default = "default_waste_percent")]
    pub waste_percent: f64,
    #[serde(default, deserialize_with = "deserialize_optional_integer_count")]
    pub roll_count: Option<i64>,
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
    #[error(
        "Shablon identitysi boshqa mahsulot yoki egaga tegishli. Yangi variant uchun mustaqil ID va kod ishlating."
    )]
    IdentityConflict,
    #[error("store failed")]
    StoreFailed,
}

/// ID and owner/code are independent lookup paths. Both must name the same SKU.
/// Display-name edits are deliberately not identity changes. Legacy empty SKUs
/// may be hydrated, but an established SKU may never be silently replaced.
pub(crate) struct TemplateIdentity {
    pub id: String,
    pub owner: String,
    pub item_code: String,
    pub variant_id: String,
}

// Brigaders share the existing operational template workspace while each saved
// row retains the identity of its original author.
fn template_owner_can_write(owner: &str, stored_owner: &str) -> bool {
    let owner = owner.trim();
    owner == stored_owner
        || (owner.starts_with("brigader:")
            && ["admin:", "telegram:", "brigader:"]
                .iter()
                .any(|prefix| stored_owner.starts_with(prefix)))
        || (owner.starts_with("admin:") && stored_owner.starts_with("brigader:"))
}

pub(crate) fn checked_template_identity(
    owner: &str,
    incoming: &CalculateOrderTemplate,
    by_id: Option<TemplateIdentity>,
    by_code: Option<TemplateIdentity>,
) -> Result<Option<TemplateIdentity>, CalculateOrderError> {
    for existing in by_id.iter().chain(by_code.iter()) {
        if !template_owner_can_write(owner, &existing.owner)
            || (!existing.item_code.trim().is_empty()
                && existing.item_code.trim() != incoming.item_code.trim())
        {
            return Err(CalculateOrderError::IdentityConflict);
        }
    }
    if let (Some(id), Some(code)) = (&by_id, &by_code)
        && id.id != code.id
    {
        return Err(CalculateOrderError::IdentityConflict);
    }
    Ok(by_code.or(by_id))
}

pub(crate) fn checked_template_read_identity(
    owner: &str, incoming: &CalculateOrderTemplate, existing: Option<TemplateIdentity>,
) -> Result<Option<TemplateIdentity>, CalculateOrderError> {
    if let Some(identity)=&existing {
        let readable = template_owner_can_write(owner, &identity.owner)
            || (owner.starts_with("admin:") && identity.owner.starts_with("telegram:"))
            || (owner.starts_with("sales_manager:") && identity.owner.starts_with("admin:"));
        if !readable || (!identity.item_code.trim().is_empty() && identity.item_code.trim()!=incoming.item_code.trim()) {
            return Err(CalculateOrderError::IdentityConflict);
        }
    }
    Ok(existing)
}

#[async_trait]
pub trait CalculateOrderStorePort: Send + Sync {
    async fn validate_read_identity(&self, owner: &str, template: &CalculateOrderTemplate) -> Result<(), CalculateOrderError> {
        self.validate_identity(owner,template).await
    }
    async fn validate_identity(
        &self,
        _owner: &str,
        _template: &CalculateOrderTemplate,
    ) -> Result<(), CalculateOrderError> {
        Ok(())
    }
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

    /// Finds an image by id across all owners. Order sheets are viewed by
    /// operators other than the uploader, so order-image serving cannot stay
    /// owner-scoped. Defaults to `None` for stores without global lookup.
    async fn get_image_global(
        &self,
        _image_id: &str,
    ) -> Result<Option<CalculateOrderImage>, CalculateOrderError> {
        Ok(None)
    }

    /// Compatibility lookup for old orders without an image_id. Production
    /// stores can resolve one id without fetching the entire template archive.
    async fn image_id_for_order(
        &self, lookup: &OrderImageLookup,
    ) -> Result<Option<String>, CalculateOrderError> {
        Ok(lookup.resolve(&self.list_all().await?))
    }
}

pub fn validate_template(template: &CalculateOrderTemplate) -> Result<(), CalculateOrderError> {
    super::combo::validate_reference(template)
        .map_err(|error| CalculateOrderError::InvalidInput(error.to_string()))?;
    if !template.variant_id.is_empty() && template.variant_id.trim() != template.id.trim() {
        return Err(CalculateOrderError::IdentityConflict);
    }
    if template
        .production_options
        .as_ref()
        .and_then(|o| o.diameter_mm)
        .is_some_and(|diameter| !diameter.is_finite() || diameter <= 0.0)
    {
        return Err(CalculateOrderError::InvalidInput("diametr noto'g'ri".into()));
    }
    if template
        .print_val_size_mm
        .is_some_and(|size| !size.is_finite() || size <= 0.0)
    {
        return Err(CalculateOrderError::InvalidInput(
            "val razmeri noto'g'ri".to_string(),
        ));
    }
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
    if !template.products.is_empty() {
        crate::core::order_layout::validate_products(&template.products)
            .map_err(CalculateOrderError::InvalidInput)?;
        if template.customer_ref.trim().is_empty() {
            return Err(CalculateOrderError::InvalidInput("Ko‘p mahsulotli buyurtma uchun mijozni tanlang".into()));
        }
    }
    if template.products.is_empty() && template.frame_product_size_mm <= 0.0 {
        return Err(CalculateOrderError::InvalidInput(
            "kadrdagi mahsulot o'lchami noto'g'ri".to_string(),
        ));
    }
    if template.products.is_empty() && template.frame_count <= 0.0 {
        return Err(CalculateOrderError::InvalidInput(
            "kadr soni noto'g'ri".to_string(),
        ));
    }
    if !template.edge_allowance_mm.is_finite() || template.edge_allowance_mm < 0.0 {
        return Err(CalculateOrderError::InvalidInput(
            "qo'shimcha razmer noto'g'ri".to_string(),
        ));
    }
    if !template.products.is_empty()
        && !(crate::core::order_layout::useful_width(&template.products) + template.edge_allowance_mm).is_finite()
    {
        return Err(CalculateOrderError::InvalidInput("razmer noto'g'ri".to_string()));
    }
    if template.products.is_empty() && derive_width_mm(
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
    crate::core::formula::validate_micron_options(&layers)
        .map_err(CalculateOrderError::InvalidInput)?;
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
        .map(|layer| LayerInput {
            material_id: layer.material_id.trim().to_string(),
            material: layer.material.trim().to_string(),
            micron: layer.micron.trim().to_string(),
            ..layer
        })
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
#[allow(clippy::items_after_test_module)]
#[path = "calculate_orders_tests.rs"]
mod tests;

pub fn hydrate_template_dimensions(mut template: CalculateOrderTemplate) -> CalculateOrderTemplate {
    template = hydrate_template_layers(template);
    if !template.edge_allowance_mm.is_finite() || template.edge_allowance_mm < 0.0 {
        template.edge_allowance_mm = DEFAULT_EDGE_ALLOWANCE_MM;
    }
    if !template.products.is_empty() {
        template.frame_product_size_mm = 0.0;
        template.frame_count = crate::core::order_layout::frame_count(&template.products) as f64;
        template.width_mm = crate::core::order_layout::useful_width(&template.products) + template.edge_allowance_mm;
        template.item_code = template.products[0].item_code.trim().to_string();
        template.product = crate::core::order_layout::title(&template.products);
        template.name = template.product.clone();
        if let Some(combo) = &template.combo {
            template.item_code = combo.item_code.clone();
            template.product = combo.name.clone();
            template.name = combo.name.clone();
        }
        return template;
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
