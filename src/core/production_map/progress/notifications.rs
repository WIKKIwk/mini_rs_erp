use super::super::queue_state;
use super::super::types::{
    ApparatusQueueActionEvent, CompletionRequestDecisionNotification, CompletionRequestNotification,
};
use super::labels::actor_display_name;

pub(in crate::core::production_map) fn completion_request_notification_from_event(
    event: &ApparatusQueueActionEvent,
    created_at_unix: i64,
) -> Option<CompletionRequestNotification> {
    if event.action != queue_state::ApparatusQueueAction::Complete
        || event.payload_json.get("completion_request")?.as_bool() != Some(true)
    {
        return None;
    }
    let description = event.payload_json.get("description")?.as_str()?.trim();
    if description.is_empty() {
        return None;
    }
    let status = json_string_field(&event.payload_json, "completion_request_status");
    if !status.is_empty() && status != "pending" {
        return None;
    }
    let notice_kind = json_string_field(&event.payload_json, "notice_kind");
    Some(CompletionRequestNotification {
        event_id: event.event_id.trim().to_string(),
        apparatus: event.apparatus.trim().to_string(),
        order_id: event.order_id.trim().to_string(),
        order_number: json_string_field(&event.payload_json, "order_number"),
        order_title: json_string_field(&event.payload_json, "order_title"),
        product_code: json_string_field(&event.payload_json, "product_code"),
        worker_role: event.actor.role.trim().to_string(),
        worker_ref: event.actor.ref_.trim().to_string(),
        worker_display_name: actor_display_name(&event.actor),
        description: description.to_string(),
        zero_metric_codes: json_string_array(&event.payload_json, "zero_metric_codes"),
        notice_kind: if notice_kind.is_empty() {
            "completion_request".to_string()
        } else {
            notice_kind
        },
        decision_required: true,
        created_at_unix,
    })
}

pub(in crate::core::production_map) fn laminatsiya_metric_notice_from_event(
    event: &ApparatusQueueActionEvent,
    created_at_unix: i64,
) -> Option<CompletionRequestNotification> {
    if event.action != queue_state::ApparatusQueueAction::Complete
        || json_string_field(&event.payload_json, "notice_kind") != "laminatsiya_double_leftover"
    {
        return None;
    }
    let description = event.payload_json.get("description")?.as_str()?.trim();
    if description.is_empty() {
        return None;
    }
    Some(CompletionRequestNotification {
        event_id: event.event_id.trim().to_string(),
        apparatus: event.apparatus.trim().to_string(),
        order_id: event.order_id.trim().to_string(),
        order_number: json_string_field(&event.payload_json, "order_number"),
        order_title: json_string_field(&event.payload_json, "order_title"),
        product_code: json_string_field(&event.payload_json, "product_code"),
        worker_role: event.actor.role.trim().to_string(),
        worker_ref: event.actor.ref_.trim().to_string(),
        worker_display_name: actor_display_name(&event.actor),
        description: description.to_string(),
        zero_metric_codes: Vec::new(),
        notice_kind: "laminatsiya_double_leftover".to_string(),
        decision_required: false,
        created_at_unix,
    })
}

fn json_string_array(payload: &serde_json::Value, key: &str) -> Vec<String> {
    payload
        .get(key)
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub(in crate::core::production_map) fn completion_request_decision_notification_from_event(
    event: &ApparatusQueueActionEvent,
    created_at_unix: i64,
) -> Option<CompletionRequestDecisionNotification> {
    if event.action != queue_state::ApparatusQueueAction::Complete
        || event.payload_json.get("completion_request")?.as_bool() != Some(true)
    {
        return None;
    }
    let decision = json_string_field(&event.payload_json, "completion_request_status");
    if decision != "approved" && decision != "rejected" {
        return None;
    }
    let decision_at_unix = event
        .payload_json
        .get("decision_at_unix")
        .and_then(|value| value.as_i64())
        .unwrap_or(created_at_unix);
    Some(CompletionRequestDecisionNotification {
        event_id: json_string_field(&event.payload_json, "decision_event_id"),
        request_event_id: event.event_id.trim().to_string(),
        decision,
        apparatus: event.apparatus.trim().to_string(),
        order_id: event.order_id.trim().to_string(),
        order_number: json_string_field(&event.payload_json, "order_number"),
        order_title: json_string_field(&event.payload_json, "order_title"),
        product_code: json_string_field(&event.payload_json, "product_code"),
        worker_role: event.actor.role.trim().to_string(),
        worker_ref: event.actor.ref_.trim().to_string(),
        worker_display_name: actor_display_name(&event.actor),
        decided_by_role: json_string_field(&event.payload_json, "decided_by_role"),
        decided_by_ref: json_string_field(&event.payload_json, "decided_by_ref"),
        decided_by_display_name: json_string_field(&event.payload_json, "decided_by_display_name"),
        description: json_string_field(&event.payload_json, "description"),
        message: json_string_field(&event.payload_json, "decision_message"),
        created_at_unix: decision_at_unix,
    })
}

pub(in crate::core::production_map) fn json_string_field(
    payload: &serde_json::Value,
    key: &str,
) -> String {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}
