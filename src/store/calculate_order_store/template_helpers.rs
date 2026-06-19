use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::core::calculate_orders::{
    CalculateOrderError, CalculateOrderImage, CalculateOrderTemplate,
};
use crate::core::formula::{DEFAULT_EDGE_ALLOWANCE_MM, derive_width_mm};

pub(super) fn existing_id_by_code(
    conn: &Connection,
    owner_key: &str,
    code: &str,
) -> Result<Option<String>, CalculateOrderError> {
    conn.query_row(
        "SELECT id
         FROM calculate_order_templates
         WHERE owner_key = ?1 AND lower_code = ?2
         ORDER BY saved_at DESC
         LIMIT 1",
        params![owner_key.trim(), normalize_key(code)],
        |row| row.get(0),
    )
    .optional()
    .map_err(|_| CalculateOrderError::StoreFailed)
}

pub(super) fn stamp_template(
    mut template: CalculateOrderTemplate,
    existing_id: Option<String>,
) -> CalculateOrderTemplate {
    template.id = existing_id
        .filter(|id| !id.trim().is_empty())
        .or_else(|| (!template.id.trim().is_empty()).then(|| template.id.trim().to_string()))
        .unwrap_or_else(new_id);
    template.code = template.code.trim().to_string();
    template.name = template.name.trim().to_string();
    template.order_number = template.order_number.trim().to_string();
    template.customer_ref = template.customer_ref.trim().to_string();
    template.customer = template.customer.trim().to_string();
    template.item_code = template.item_code.trim().to_string();
    template.product = template.product.trim().to_string();
    template.status = template.status.trim().to_string();
    template.material_display = template.material_display.trim().to_string();
    template.color = template.color.trim().to_string();
    template.image_id = template.image_id.trim().to_string();
    template.image_name = template.image_name.trim().to_string();
    template.image_mime = template.image_mime.trim().to_string();
    template.image_url = template.image_url.trim().to_string();
    template.edge_allowance_mm = if template.edge_allowance_mm.is_finite() {
        template.edge_allowance_mm
    } else {
        DEFAULT_EDGE_ALLOWANCE_MM
    };
    template.width_mm = derive_width_mm(
        Some(template.frame_product_size_mm),
        Some(template.frame_count),
        Some(template.edge_allowance_mm),
    )
    .unwrap_or_default();
    template.first_layer_material = template.first_layer_material.trim().to_string();
    template.first_layer_micron = template.first_layer_micron.trim().to_string();
    template.second_layer_material = template.second_layer_material.trim().to_string();
    template.second_layer_micron = template.second_layer_micron.trim().to_string();
    template.third_layer_material = template.third_layer_material.trim().to_string();
    template.third_layer_micron = template.third_layer_micron.trim().to_string();
    template.note = template.note.trim().to_string();
    template.source_map_id = template.source_map_id.trim().to_string();
    template.saved_at = unix_micros().to_string();
    template
}

pub(super) fn stamp_image(
    mut image: CalculateOrderImage,
) -> Result<CalculateOrderImage, CalculateOrderError> {
    image.image_id = image.image_id.trim().to_string();
    image.image_name = image.image_name.trim().to_string();
    image.image_mime = image.image_mime.trim().to_string();
    image.image_size_bytes = image.body.len() as u64;
    if image.image_id.is_empty() {
        return Err(CalculateOrderError::InvalidInput("id kerak".to_string()));
    }
    if image.image_name.is_empty() {
        image.image_name = "rang.jpg".to_string();
    }
    if image.image_mime.is_empty() {
        image.image_mime = "image/jpeg".to_string();
    }
    Ok(image)
}

pub(super) fn dedupe_templates(
    templates: Vec<CalculateOrderTemplate>,
) -> Vec<CalculateOrderTemplate> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::with_capacity(templates.len());
    for template in templates {
        let key = quick_template_key(&template);
        if key == "id:" || seen.insert(key) {
            result.push(template);
        }
    }
    result
}

pub(super) fn normalize_key(value: &str) -> String {
    value.trim().to_lowercase()
}

pub(super) fn new_id() -> String {
    unix_micros().to_string()
}

pub(super) fn unix_micros() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or_default()
}

fn quick_template_key(template: &CalculateOrderTemplate) -> String {
    let product_key = [
        template.item_code.as_str(),
        template.product.as_str(),
        template.name.as_str(),
    ]
    .into_iter()
    .map(normalize_key)
    .find(|value| !value.is_empty())
    .unwrap_or_default();
    if product_key.is_empty() {
        return legacy_template_key(template);
    }
    [
        "quick".to_string(),
        normalize_key(&template.customer_ref),
        normalize_key(&template.customer),
        product_key,
        normalize_key(&template.status),
        normalize_key(&template.material_display),
        normalize_key(&template.color),
        number_key(template.frame_product_size_mm),
        number_key(template.frame_count),
        number_key(template.edge_allowance_mm),
        number_key(template.waste_percent),
        option_number_key(template.roll_count),
        normalize_key(&template.first_layer_material),
        normalize_key(&template.first_layer_micron),
        normalize_key(&template.second_layer_material),
        normalize_key(&template.second_layer_micron),
        normalize_key(&template.third_layer_material),
        normalize_key(&template.third_layer_micron),
        normalize_key(&template.note),
    ]
    .join("|")
}

fn legacy_template_key(template: &CalculateOrderTemplate) -> String {
    let code = normalize_key(&template.code);
    if code.is_empty() {
        format!("id:{}", template.id.trim())
    } else {
        format!("code:{code}")
    }
}

fn number_key(value: f64) -> String {
    if value.is_finite() {
        format!("{value:.3}")
    } else {
        String::new()
    }
}

fn option_number_key(value: Option<f64>) -> String {
    value.map(number_key).unwrap_or_default()
}
