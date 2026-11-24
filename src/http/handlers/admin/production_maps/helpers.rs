use std::collections::{BTreeMap, BTreeSet};

use crate::core::auth::models::PrincipalRole;
use crate::core::calculate_orders::CalculateOrderTemplate;
use crate::core::production_map::{ProductionMapDefinition, ProductionMapSaved};

use super::*;

pub(super) fn production_map_print_order_code(map: &ProductionMapDefinition) -> Option<String> {
    [map.code.trim(), map.order_number.trim()]
        .into_iter()
        .find(|value| !value.is_empty())
        .or_else(|| {
            map.id.trim().strip_prefix("zakaz-").map(str::trim).filter(|value| !value.is_empty())
        })
        .map(str::to_owned)
}

pub(super) fn automatic_rezka_label_count(
    map: Option<&ProductionMapDefinition>,
    batch: &crate::core::production_map::OrderProgressBatch,
) -> u32 {
    // The production service assigns this kind from the resolved stage occurrence.
    let is_final_rezka_frame = batch.payload_json.get("rezka_output_kind")
        .and_then(|value| value.as_str()) == Some("frame");
    if is_final_rezka_frame
        && map.is_some_and(|map| map.duplicate_rezka_labels == Some(true))
    {
        2
    } else {
        1
    }
}

pub(super) fn effective_progress_qr_payload<'a>(
    qr_payload: &'a str,
    legacy_progress_qr: &'a str,
) -> &'a str {
    if qr_payload.trim().is_empty() {
        legacy_progress_qr
    } else {
        qr_payload
    }
}

pub(super) async fn production_map_order_customers(
    state: &AppState,
    maps: &[ProductionMapSaved],
) -> BTreeMap<String, String> {
    production_map_definition_customers(
        state,
        &maps.iter().map(|saved| &saved.map).collect::<Vec<_>>(),
    )
    .await
}

pub(super) async fn production_map_definition_customers(
    state: &AppState,
    maps: &[&ProductionMapDefinition],
) -> BTreeMap<String, String> {
    let mut order_map_count = 0;
    let mut customers = BTreeMap::new();
    for map in maps.iter().filter(|map| is_customer_order_map(map)) {
        order_map_count += 1;
        let map_id = map.id.trim();
        let customer = map.customer_name.trim();
        if !map_id.is_empty() && !customer.is_empty() {
            customers.insert(map_id.to_string(), customer.to_string());
        }
    }

    if customers.len() == order_map_count {
        return customers;
    }
    let templates = match state.calculate_orders.list_all().await {
        Ok(templates) => templates,
        Err(error) => {
            tracing::warn!(%error, "production map customer fallback load failed");
            return customers;
        }
    };
    for map in maps.iter().filter(|map| is_customer_order_map(map)) {
        let map_id = map.id.trim();
        if map_id.is_empty() || customers.contains_key(map_id) {
            continue;
        }
        if let Some(customer) = resolve_production_map_customer(map, &templates) {
            customers.insert(map_id.to_string(), customer);
        }
    }
    customers
}

fn is_customer_order_map(map: &ProductionMapDefinition) -> bool {
    let map_id = map.id.trim();
    !map_id.is_empty()
        && !map_id.starts_with("template-")
        && (map_id.starts_with("zakaz-")
            || !map.order_number.trim().is_empty()
            || !map.code.trim().is_empty())
}

fn resolve_production_map_customer(
    map: &ProductionMapDefinition,
    templates: &[CalculateOrderTemplate],
) -> Option<String> {
    let map_id = map.id.trim();
    let mut source_matches = templates
        .iter()
        .filter(|template| {
            let source_map_id = template.source_map_id.trim();
            source_map_id == map_id
                || (!map_id.is_empty() && source_map_id.strip_prefix("template-") == Some(map_id))
        })
        .peekable();
    if source_matches.peek().is_some() {
        return unique_template_customer(source_matches);
    }

    let id_suffix = map_id.strip_prefix("zakaz-").unwrap_or("").trim();
    let order_keys = [map.order_number.trim(), map.code.trim(), id_suffix]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    let mut order_matches = templates
        .iter()
        .filter(|template| {
            !order_keys.is_empty()
                && (order_keys.contains(template.order_number.trim())
                    || order_keys.contains(template.code.trim()))
        })
        .peekable();
    if order_matches.peek().is_some() {
        return unique_template_customer(order_matches);
    }

    let mut product_keys = [map.product_code.as_str(), map.title.as_str()]
        .into_iter()
        .map(normalized_customer_match_key)
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    product_keys.extend(
        map.nodes
            .iter()
            .map(|node| normalized_customer_match_key(&node.item_code))
            .filter(|value| !value.is_empty()),
    );
    if product_keys.is_empty() {
        return None;
    }
    unique_template_customer(templates.iter().filter(|template| {
        let product_matches = product_keys
            .contains(&normalized_customer_match_key(&template.product))
            || product_keys.contains(&normalized_customer_match_key(&template.item_code));
        if !product_matches {
            return false;
        }
        match (map.width_mm, template.width_mm) {
            (Some(map_width), template_width) if map_width > 0.0 && template_width > 0.0 => {
                (map_width - template_width).abs() <= 0.5
            }
            _ => true,
        }
    }))
}

fn unique_template_customer<'a>(
    templates: impl Iterator<Item = &'a CalculateOrderTemplate>,
) -> Option<String> {
    let mut unique_key: Option<String> = None;
    let mut unique_customer: Option<String> = None;
    for template in templates {
        let customer = template.customer.trim();
        if customer.is_empty() {
            continue;
        }
        let key = normalized_customer_match_key(customer);
        if unique_key.as_ref().is_some_and(|existing| existing != &key) {
            return None;
        }
        unique_key = Some(key);
        unique_customer = Some(customer.to_string());
    }
    unique_customer
}

fn normalized_customer_match_key(value: &str) -> String {
    value.trim().to_lowercase()
}

#[cfg(test)]
#[path = "customer_resolution_tests.rs"]
mod customer_resolution_tests;

pub(super) fn raw_material_stock_status_error(
    error: crate::core::gscale::GscaleServiceError,
) -> AdminError {
    match error {
        crate::core::gscale::GscaleServiceError::InvalidInput(detail) => bad_request(detail),
        _ => server_error("raw material stock status update failed"),
    }
}

pub(super) fn calculate_order_error(error: CalculateOrderError) -> AdminError {
    match error {
        CalculateOrderError::InvalidInput(detail) => bad_request(detail),
        CalculateOrderError::IdentityConflict => conflict("template_identity_conflict"),
        CalculateOrderError::StoreFailed => server_error("calculate order save failed"),
    }
}

include!("helpers_parts/production_map_error.rs");

#[cfg(test)]
#[path = "production_map_error_tests.rs"]
mod production_map_error_tests;

include!("helpers_parts/ambiguous_raw_material_apparatuses.rs");

#[cfg(test)]
#[path = "progress_qr_payload_tests.rs"]
mod progress_qr_payload_tests;
