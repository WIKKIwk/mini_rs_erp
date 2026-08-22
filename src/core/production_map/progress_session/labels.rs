use super::super::queue_state;
use super::super::types::{ProductionMapDefinition, ProductionMapError, QueueActionActor};
use super::ids::queue_action_str;
use crate::core::quantity::positive_erp_quantity;

pub(in crate::core::production_map) fn valid_progress_qty(
    value: Option<f64>,
) -> Result<f64, ProductionMapError> {
    let value = value.ok_or(ProductionMapError::ProgressInputInvalid)?;
    positive_erp_quantity(value).ok_or(ProductionMapError::ProgressInputInvalid)
}

pub(in crate::core::production_map) fn non_empty_or(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.trim().to_string()
    } else {
        value.to_string()
    }
}

pub(crate) fn progress_label_item_name(
    order_map: &ProductionMapDefinition,
    apparatus: &str,
    action: queue_state::ApparatusQueueAction,
) -> String {
    let order_title = non_empty_or(&order_map.title, &order_map.product_code);
    let state_label = match action {
        queue_state::ApparatusQueueAction::Pause => "chiqarildi",
        queue_state::ApparatusQueueAction::Freeze => "muzlatildi",
        queue_state::ApparatusQueueAction::DetachRoll => "rulon yechildi",
        queue_state::ApparatusQueueAction::RollComplete => "rulon tugatildi",
        queue_state::ApparatusQueueAction::Complete => "ish tugatildi",
        _ => queue_action_str(action),
    };
    let product_kind = if super::super::chain::is_final_work_stage_station(order_map, apparatus) {
        "tayyor mahsulot"
    } else {
        "yarim tayyor mahsulot"
    };
    format!(
        "{order_title} {product_kind}, apparat: {}, {state_label}",
        apparatus.trim()
    )
}

pub(in crate::core::production_map) fn actor_display_name(actor: &QueueActionActor) -> String {
    non_empty_or(&actor.display_name, &actor.ref_)
}
