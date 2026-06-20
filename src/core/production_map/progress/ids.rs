use std::time::{SystemTime, UNIX_EPOCH};

use super::super::queue_state;
use super::super::types::{CompletionRequestDecision, QueueActionActor};

pub(in crate::core::production_map) fn queue_action_event_id(
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

pub(in crate::core::production_map) fn completion_request_decision_event_id(
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

pub(in crate::core::production_map) fn queue_action_str(
    action: queue_state::ApparatusQueueAction,
) -> &'static str {
    match action {
        queue_state::ApparatusQueueAction::Start => "start",
        queue_state::ApparatusQueueAction::Pause => "pause",
        queue_state::ApparatusQueueAction::Resume => "resume",
        queue_state::ApparatusQueueAction::Complete => "complete",
    }
}

pub(in crate::core::production_map) fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or_default()
}

pub(in crate::core::production_map) fn progress_session_id(
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

pub(in crate::core::production_map) fn progress_event_id(
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

pub(in crate::core::production_map) fn progress_batch_id(
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

pub(in crate::core::production_map) fn progress_qr_payload(batch_id: &str) -> String {
    let stamp = batch_id
        .split(':')
        .nth(1)
        .and_then(|value| value.parse::<u128>().ok())
        .unwrap_or_else(unix_nanos);
    let stamp = (stamp & u128::from(u64::MAX)) as u64;
    let hash = progress_qr_hash(batch_id);
    format!("4001{stamp:016X}{hash:04X}")
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default()
}

fn progress_qr_hash(value: &str) -> u16 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.trim().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash & 0xffff) as u16
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
