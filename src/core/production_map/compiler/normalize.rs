use std::collections::BTreeMap;

use super::super::types::*;

#[cfg(test)]
pub(in crate::core::production_map) fn reject_order_number_immutable(
    maps: &BTreeMap<String, ProductionMapDefinition>,
    next: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let id = next.id.trim();
    if !id.starts_with("zakaz-") {
        return Ok(());
    }
    let order_number = next.order_number.trim();
    if order_number.is_empty() {
        return Ok(());
    }
    let Some(existing) = maps.get(id) else {
        return Ok(());
    };
    let existing_number = existing.order_number.trim();
    if !existing_number.is_empty() && existing_number != order_number {
        return Err(ProductionMapError::OrderNumberImmutable);
    }
    Ok(())
}

pub(in crate::core::production_map) fn normalize_map(map: &mut ProductionMapDefinition) {
    map.id = map.id.trim().to_ascii_lowercase();
    map.product_code = map.product_code.trim().to_string();
    map.title = map.title.trim().to_string();
    map.code = map.code.trim().to_string();
    map.order_number = map.order_number.trim().to_string();
    if map
        .roll_count
        .is_some_and(|value| !value.is_finite() || value <= 0.0)
    {
        map.roll_count = None;
    }
    if map
        .width_mm
        .is_some_and(|value| !value.is_finite() || value <= 0.0)
    {
        map.width_mm = None;
    }
    for node in &mut map.nodes {
        node.id = node.id.trim().to_ascii_lowercase();
        node.title = node.title.trim().to_string();
        node.role_code = node.role_code.trim().to_string();
        node.item_code = node.item_code.trim().to_string();
        node.qty_formula = node.qty_formula.trim().to_string();
        node.from_location = node.from_location.trim().to_string();
        node.to_location = node.to_location.trim().to_string();
        node.alternative_group_id = node.alternative_group_id.trim().to_string();
        node.alternative_group_label = node.alternative_group_label.trim().to_string();
        node.alternative_assigned_title = node.alternative_assigned_title.trim().to_string();
        if !node.x.is_finite() {
            node.x = 0.0;
        }
        if !node.y.is_finite() {
            node.y = 0.0;
        }
        if let Some(formula) = &mut node.formula {
            formula.target = formula.target.trim().to_string();
            formula.expression = formula.expression.trim().to_string();
        }
    }
    for edge in &mut map.edges {
        edge.from = edge.from.trim().to_ascii_lowercase();
        edge.to = edge.to.trim().to_ascii_lowercase();
        edge.branch = normalize_branch(&edge.branch);
    }
}

pub(super) fn normalize_branch(branch: &str) -> String {
    match branch.trim().to_ascii_lowercase().as_str() {
        "ha" | "yes" | "true" | "1" => "true".to_string(),
        "yo'q" | "yoq" | "no" | "false" | "0" => "false".to_string(),
        value => value.to_string(),
    }
}
