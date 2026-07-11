use super::super::queue_state;
use super::super::types::{
    OrderRunSession, OrderRunStatus, ProductionMapDefinition, ProductionMapError, QueueActionActor,
};
use super::ids::{progress_session_id, queue_action_str};
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

pub(in crate::core::production_map) fn progress_label_item_name(
    order_map: &ProductionMapDefinition,
    apparatus: &str,
    action: queue_state::ApparatusQueueAction,
) -> String {
    let order_title = non_empty_or(&order_map.title, &order_map.product_code);
    let state_label = match action {
        queue_state::ApparatusQueueAction::Pause => "pauza",
        queue_state::ApparatusQueueAction::Complete => "tugatildi",
        _ => queue_action_str(action),
    };
    format!(
        "{order_title} yarim tayyor, {} holatda, {state_label}",
        apparatus.trim()
    )
}

pub(in crate::core::production_map) fn actor_display_name(actor: &QueueActionActor) -> String {
    non_empty_or(&actor.display_name, &actor.ref_)
}

pub(in crate::core::production_map) fn legacy_order_run_session(
    apparatus: &str,
    order_id: &str,
    actor: &QueueActionActor,
    now: i64,
) -> OrderRunSession {
    OrderRunSession {
        session_id: progress_session_id(apparatus, order_id, actor, now),
        apparatus: apparatus.trim().to_string(),
        order_id: order_id.trim().to_string(),
        status: OrderRunStatus::Active,
        worker_role: actor.role.trim().to_string(),
        worker_ref: actor.ref_.trim().to_string(),
        worker_display_name: actor.display_name.trim().to_string(),
        started_at_unix: now,
        updated_at_unix: now,
        payload_json: serde_json::json!({"legacy_session": true}),
    }
}
