use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

use super::types::*;
use super::{pechat, queue_state};

pub(super) fn required_apparatus_for_closed_order(map: &ProductionMapDefinition) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut apparatus = Vec::new();
    for node in &map.nodes {
        if node.kind != ProductionMapNodeKind::Apparatus {
            continue;
        }
        let title = if node.alternative_assigned_title.trim().is_empty() {
            node.title.trim()
        } else {
            node.alternative_assigned_title.trim()
        };
        if title.is_empty() || !seen.insert(title.to_ascii_lowercase()) {
            continue;
        }
        apparatus.push(title.to_string());
    }
    apparatus
}

pub(super) fn order_completed_on_apparatus(
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    order_id: &str,
    apparatus: &str,
) -> bool {
    queue_states.iter().any(|(state_apparatus, states)| {
        queue_state::apparatus_titles_match(state_apparatus, apparatus)
            && matches!(
                states
                    .get(order_id.trim())
                    .map(|value| value.trim().to_ascii_lowercase()),
                Some(state) if state == "completed"
            )
    })
}

pub(super) fn latest_required_complete_event<'a>(
    logs: &'a [ProductionOrderLogEntry],
    required_apparatus: &[String],
) -> Option<&'a ProductionOrderLogEntry> {
    logs.iter()
        .filter(|entry| {
            entry.action == queue_state::ApparatusQueueAction::Complete
                && entry.to_state == queue_state::ApparatusQueueOrderState::Completed
                && required_apparatus.iter().any(|apparatus| {
                    queue_state::apparatus_titles_match(&entry.apparatus, apparatus)
                })
        })
        .max_by_key(|entry| entry.created_at_unix)
}

#[cfg(test)]
pub(super) fn completion_request_notification_from_event(
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
        created_at_unix,
    })
}

#[cfg(test)]
pub(super) fn completion_request_decision_notification_from_event(
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

#[cfg(test)]
pub(super) fn json_string_field(payload: &serde_json::Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

pub(super) fn effective_apparatus_queue_policy(
    apparatus: &str,
    stored: Option<ApparatusQueuePolicy>,
) -> ApparatusQueuePolicy {
    if pechat::pechat_color_count(apparatus).is_some() {
        ApparatusQueuePolicy::StrictSequence
    } else {
        stored.unwrap_or(ApparatusQueuePolicy::StrictSequence)
    }
}

pub(super) fn effective_apparatus_queue_policy_record(
    apparatus: &str,
    stored: ApparatusQueuePolicy,
) -> ApparatusQueuePolicyRecord {
    let locked = pechat::pechat_color_count(apparatus).is_some();
    ApparatusQueuePolicyRecord {
        apparatus: apparatus.trim().to_string(),
        policy: if locked {
            ApparatusQueuePolicy::StrictSequence
        } else {
            stored
        },
        locked,
        reason: if locked {
            "pechat_always_strict".to_string()
        } else {
            String::new()
        },
    }
}

pub(super) fn queue_action_event_id(
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    format!(
        "production-map-queue:{nanos}:{}:{}:{}",
        apparatus.trim(),
        order_id.trim(),
        queue_action_str(action)
    )
}

pub(super) fn completion_request_decision_event_id(
    request_event_id: &str,
    decision: CompletionRequestDecision,
) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    format!(
        "production-map-completion-request:{nanos}:{}:{}",
        request_event_id.trim(),
        decision.as_str()
    )
}

pub(super) fn queue_action_str(action: queue_state::ApparatusQueueAction) -> &'static str {
    match action {
        queue_state::ApparatusQueueAction::Start => "start",
        queue_state::ApparatusQueueAction::Pause => "pause",
        queue_state::ApparatusQueueAction::Resume => "resume",
        queue_state::ApparatusQueueAction::Complete => "complete",
    }
}

pub(super) fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or_default()
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default()
}

pub(super) fn progress_session_id(
    apparatus: &str,
    order_id: &str,
    actor: &QueueActionActor,
    _now: i64,
) -> String {
    let stamp = unix_nanos();
    format!(
        "order-run:{stamp}:{}:{}:{}",
        sanitize_id(apparatus),
        sanitize_id(order_id),
        sanitize_id(&actor.ref_)
    )
}

pub(super) fn progress_event_id(
    session_id: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    _now: i64,
) -> String {
    let stamp = unix_nanos();
    format!(
        "order-progress:{stamp}:{}:{}:{}",
        sanitize_id(session_id),
        sanitize_id(order_id),
        queue_action_str(action)
    )
}

pub(super) fn progress_batch_id(
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    _now: i64,
) -> String {
    let stamp = unix_nanos();
    format!(
        "progress-batch:{stamp}:{}:{}:{}",
        sanitize_id(apparatus),
        sanitize_id(order_id),
        queue_action_str(action)
    )
}

pub(super) fn progress_qr_payload(batch_id: &str) -> String {
    let stamp = batch_id
        .split(':')
        .nth(1)
        .and_then(|value| value.parse::<u128>().ok())
        .unwrap_or_else(unix_nanos);
    let stamp = (stamp & u128::from(u64::MAX)) as u64;
    let hash = progress_qr_hash(batch_id);
    format!("4001{stamp:016X}{hash:04X}")
}

fn progress_qr_hash(value: &str) -> u16 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.trim().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash & 0xffff) as u16
}

pub(super) fn valid_progress_qty(value: Option<f64>) -> Result<f64, ProductionMapError> {
    let value = value.ok_or(ProductionMapError::ProgressInputInvalid)?;
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(ProductionMapError::ProgressInputInvalid)
    }
}

pub(super) fn non_empty_or(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.trim().to_string()
    } else {
        value.to_string()
    }
}

pub(super) fn progress_label_item_name(
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

pub(super) fn actor_display_name(actor: &QueueActionActor) -> String {
    non_empty_or(&actor.display_name, &actor.ref_)
}

pub(super) fn legacy_order_run_session(
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

fn sanitize_id(value: &str) -> String {
    let sanitized = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "blank".to_string()
    } else {
        sanitized
    }
}
