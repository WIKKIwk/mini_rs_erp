use std::collections::BTreeSet;

use serde_json::Value;

use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::formula::{CalculateRequest, LayerInput, calculate};
use crate::core::production_map::{ProductionMapDefinition, ProductionMapNodeKind};

pub fn is_sheet_order_map(map: &ProductionMapDefinition) -> bool {
    let id = map.id.trim();
    let order_number = map.order_number.trim();
    id.starts_with("zakaz-")
        && order_number.len() == 4
        && order_number.chars().all(|ch| ch.is_ascii_digit())
}

pub(super) fn order_sheet_row(
    map: &ProductionMapDefinition,
    template: &CalculateOrderTemplate,
) -> Option<Vec<Value>> {
    if !is_sheet_order_map(map) || template.kg <= 0.0 {
        return None;
    }
    let calculation = calculate(CalculateRequest {
        order_number: Some(map.order_number.trim().to_string()),
        product: Some(template.product.trim().to_string()),
        kg: Some(template.kg),
        frame_product_size_mm: Some(template.frame_product_size_mm),
        frame_count: Some(template.frame_count),
        edge_allowance_mm: Some(template.edge_allowance_mm),
        waste_percent: Some(template.waste_percent),
        roll_count: template.roll_count,
        first_layer: LayerInput::new(
            template.first_layer_material.trim(),
            template.first_layer_micron.trim(),
        ),
        second_layer: LayerInput::new(
            template.second_layer_material.trim(),
            template.second_layer_micron.trim(),
        ),
        third_layer: LayerInput::new(
            template.third_layer_material.trim(),
            template.third_layer_micron.trim(),
        ),
        ..CalculateRequest::default()
    })
    .ok()?;
    let first_result = calculation.results.first()?;
    let now = time::OffsetDateTime::now_utc()
        .to_offset(time::UtcOffset::from_hms(5, 0, 0).expect("valid tashkent offset"));
    Some(vec![
        Value::String(sheet_press_marker(map, template)),
        Value::String(format!("{:02}/{:02}", now.day(), u8::from(now.month()))),
        Value::String(format!("{:02}:{:02}", now.hour(), now.minute())),
        Value::String(sheet_order_code(map, template)),
        Value::String(first_non_empty([
            &template.product,
            &template.name,
            &map.title,
        ])),
        json_number(template.kg),
        Value::String(template.first_layer_material.trim().to_string()),
        Value::String(template.second_layer_material.trim().to_string()),
        Value::String(template.third_layer_material.trim().to_string()),
        json_number(template.width_mm),
        Value::String(template.first_layer_micron.trim().to_string()),
        Value::String(template.second_layer_micron.trim().to_string()),
        Value::String(template.third_layer_micron.trim().to_string()),
        json_number(first_result.rounded_length),
        template
            .roll_count
            .map(json_number)
            .unwrap_or_else(|| Value::String(String::new())),
        json_number(f64::from(calculation.rubber_size_mm)),
    ])
}

pub(super) fn missing_order_rows(
    maps: &[ProductionMapDefinition],
    templates: &[CalculateOrderTemplate],
    existing_codes: &BTreeSet<String>,
) -> Vec<Vec<Value>> {
    let mut seen = existing_codes.clone();
    let mut rows = Vec::new();
    for map in maps {
        if !is_sheet_order_map(map) {
            continue;
        }
        let Some(template) = templates.iter().find(|template| {
            template.source_map_id.trim() == map.id.trim()
                || template.order_number.trim() == map.order_number.trim()
                || template.code.trim() == map.code.trim()
        }) else {
            continue;
        };
        let Some(row) = order_sheet_row(map, template) else {
            continue;
        };
        let code = row_code(&row);
        if seen.insert(code) {
            rows.push(row);
        }
    }
    rows
}

pub(super) fn sheet_codes(values: Vec<Vec<Value>>) -> BTreeSet<String> {
    values
        .into_iter()
        .filter_map(|row| row.get(3).and_then(value_text).map(normalize_sheet_code))
        .filter(|code| !code.is_empty())
        .collect()
}

pub(super) fn row_code(row: &[Value]) -> String {
    row.get(3)
        .and_then(value_text)
        .map(normalize_sheet_code)
        .unwrap_or_default()
}

pub(super) fn json_number(value: f64) -> Value {
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or_else(|| Value::String(String::new()))
}

fn value_text(value: &Value) -> Option<&str> {
    match value {
        Value::String(value) => Some(value.as_str()),
        _ => None,
    }
}

fn normalize_sheet_code(value: &str) -> String {
    value.trim().trim_start_matches('/').trim().to_string()
}

fn sheet_press_marker(map: &ProductionMapDefinition, template: &CalculateOrderTemplate) -> String {
    let product = format!("{} {}", template.product, template.name).to_lowercase();
    if product.contains("flex") || product.contains("fleks") || product.contains("flekso") {
        return "F".to_string();
    }
    let mut titles = Vec::new();
    for node in &map.nodes {
        if node.kind != ProductionMapNodeKind::Apparatus {
            continue;
        }
        let assigned = node.alternative_assigned_title.trim();
        if !assigned.is_empty() {
            titles.insert(0, assigned.to_string());
        }
        titles.push(node.title.trim().to_string());
    }
    for title in titles {
        let lower = title.to_lowercase();
        if lower.contains("flex") || lower.contains("fleks") || lower.contains("flekso") {
            return "F".to_string();
        }
        for marker in ["9", "8", "7"] {
            if lower.contains(&format!("{marker} ta rangli"))
                || lower.contains(&format!("{marker} rangli"))
            {
                return marker.to_string();
            }
        }
    }
    String::new()
}

fn sheet_order_code(map: &ProductionMapDefinition, template: &CalculateOrderTemplate) -> String {
    let code = first_non_empty([
        map.order_number.as_str(),
        map.code.as_str(),
        template.order_number.as_str(),
        template.code.as_str(),
    ]);
    if code.starts_with('/') {
        code
    } else {
        format!("/{code}")
    }
}

fn first_non_empty<const N: usize>(values: [&str; N]) -> String {
    values
        .into_iter()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or("")
        .to_string()
}
