use crate::{app::AppState, core::formula::planned_material_kg};
use serde_json::Value;

/// Estimates are order-specific display data, never part of a shared recipe.
pub(super) async fn enrich(state: &AppState, order_id: &str, materials: &mut [Value]) {
    if materials.is_empty() {
        return;
    }
    // Use the opened order's saved calculation, not a reusable product template.
    // Legacy orders with incomplete inputs still retain their formula picker.
    let Ok(template) = state.production_orders.order_calculation(order_id).await else {
        return;
    };
    let Ok(Some(map)) = state.production_maps.raw_map(order_id).await else {
        return;
    };
    let (Some(width_mm), Some(length_m)) = (map.width_mm, map.base_length) else {
        return;
    };
    let Ok(catalog) = state.calculate_materials.list().await else {
        return;
    };
    let layers = template.effective_layers();
    for material in materials {
        let id = material["material_id"].as_str().unwrap_or_default();
        if let Ok(kg) = planned_material_kg(&layers, id, width_mm, length_m, &catalog) {
            material["estimated_kg"] = Value::String(format!("{kg:.6}"));
        }
    }
}
