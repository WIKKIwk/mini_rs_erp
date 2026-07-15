use super::*;

use std::collections::{BTreeMap, BTreeSet};

use super::super::progress::{
    actor_display_name, completion_request_decision_notification_from_event,
    completion_request_notification_from_event, json_string_field,
    laminatsiya_metric_notice_from_event,
};
use super::super::queue_state;

pub(super) async fn apparatus_queue_states(
    store: &MemoryProductionMapStore,
) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
    Ok(store.queue_states.read().await.clone())
}

pub(super) async fn put_apparatus_queue_states(
    store: &MemoryProductionMapStore,
    apparatus: &str,
    states: BTreeMap<String, String>,
) -> Result<(), ProductionMapError> {
    store
        .queue_states
        .write()
        .await
        .insert(apparatus.trim().to_string(), states);
    Ok(())
}

pub(super) async fn apparatus_queue_policies(
    store: &MemoryProductionMapStore,
) -> Result<BTreeMap<String, ApparatusQueuePolicy>, ProductionMapError> {
    Ok(store.queue_policies.read().await.clone())
}

pub(super) async fn put_apparatus_queue_policy(
    store: &MemoryProductionMapStore,
    apparatus: &str,
    policy: ApparatusQueuePolicy,
    _actor: &QueueActionActor,
) -> Result<(), ProductionMapError> {
    store
        .queue_policies
        .write()
        .await
        .insert(apparatus.trim().to_string(), policy);
    Ok(())
}

pub(super) async fn append_apparatus_queue_action_event(
    store: &MemoryProductionMapStore,
    event: ApparatusQueueActionEvent,
) -> Result<(), ProductionMapError> {
    store.queue_events.write().await.push(event);
    Ok(())
}

pub(super) async fn completed_queue_orders_for_actor(
    store: &MemoryProductionMapStore,
    actor_ref: &str,
    limit: usize,
) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
    let actor_ref = actor_ref.trim();
    if actor_ref.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let events = store.queue_events.read().await;
    let mut seen = BTreeSet::new();
    let mut completed = Vec::new();
    for (index, event) in events.iter().enumerate().rev() {
        if event.actor.ref_.trim() != actor_ref
            || event.action != queue_state::ApparatusQueueAction::Complete
            || event.to_state != queue_state::ApparatusQueueOrderState::Completed
        {
            continue;
        }
        let order_id = event.order_id.trim();
        if order_id.is_empty() || !seen.insert(order_id.to_string()) {
            continue;
        }
        completed.push(CompletedQueueOrder {
            apparatus: event.apparatus.trim().to_string(),
            order_id: order_id.to_string(),
            completed_at_unix: index as i64 + 1,
        });
        if completed.len() >= limit {
            break;
        }
    }
    Ok(completed)
}

pub(super) async fn completion_requests(
    store: &MemoryProductionMapStore,
    limit: usize,
) -> Result<Vec<CompletionRequestNotification>, ProductionMapError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let events = store.queue_events.read().await;
    let mut requests = Vec::new();
    for (index, event) in events.iter().enumerate().rev() {
        let request = completion_request_notification_from_event(event, index as i64 + 1)
            .or_else(|| laminatsiya_metric_notice_from_event(event, index as i64 + 1));
        let Some(request) = request else { continue };
        requests.push(request);
        if requests.len() >= limit {
            break;
        }
    }
    Ok(requests)
}

pub(super) async fn completion_request_by_event_id(
    store: &MemoryProductionMapStore,
    event_id: &str,
) -> Result<Option<CompletionRequestNotification>, ProductionMapError> {
    let event_id = event_id.trim();
    if event_id.is_empty() {
        return Ok(None);
    }
    let events = store.queue_events.read().await;
    Ok(events.iter().enumerate().find_map(|(index, event)| {
        if event.event_id.trim() != event_id {
            return None;
        }
        completion_request_notification_from_event(event, index as i64 + 1)
    }))
}

pub(super) async fn completion_request_decisions_for_actor(
    store: &MemoryProductionMapStore,
    actor_ref: &str,
    limit: usize,
) -> Result<Vec<CompletionRequestDecisionNotification>, ProductionMapError> {
    let actor_ref = actor_ref.trim();
    if actor_ref.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let events = store.queue_events.read().await;
    let mut decisions = Vec::new();
    for (index, event) in events.iter().enumerate().rev() {
        let Some(decision) =
            completion_request_decision_notification_from_event(event, index as i64 + 1)
        else {
            continue;
        };
        if decision.worker_ref.trim() != actor_ref {
            continue;
        }
        decisions.push(decision);
        if decisions.len() >= limit {
            break;
        }
    }
    Ok(decisions)
}

pub(super) async fn resolve_completion_request_decision(
    store: &MemoryProductionMapStore,
    request_event_id: &str,
    decision: CompletionRequestDecision,
    actor: &QueueActionActor,
    notification: &CompletionRequestDecisionNotification,
    state_resolution: Option<CompletionRequestStateResolution>,
) -> Result<QueueActionProgressWriteResult, ProductionMapError> {
    let request_event_id = request_event_id.trim();
    if request_event_id.is_empty() {
        return Err(ProductionMapError::MissingId);
    }
    if let Some(resolution) = state_resolution {
        store
            .queue_states
            .write()
            .await
            .insert(resolution.apparatus.trim().to_string(), resolution.states);
        store.queue_events.write().await.push(resolution.event);
        if let Some(session) = resolution.session {
            store
                .order_run_sessions
                .write()
                .await
                .insert(session.session_id.clone(), session);
        }
        if let Some(report) = resolution.returned_paint_report {
            store
                .returned_paint_requests
                .write()
                .await
                .entry(report.id.clone())
                .or_insert(report);
        }
    }
    let mut events = store.queue_events.write().await;
    let Some(event) = events
        .iter_mut()
        .find(|event| event.event_id.trim() == request_event_id)
    else {
        return Err(ProductionMapError::QueueActionNotAllowed);
    };
    if event
        .payload_json
        .get("completion_request")
        .and_then(|value| value.as_bool())
        != Some(true)
    {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    event.payload_json["completion_request_status"] =
        serde_json::Value::String(decision.as_str().to_string());
    event.payload_json["decision_event_id"] =
        serde_json::Value::String(notification.event_id.clone());
    event.payload_json["decision_message"] =
        serde_json::Value::String(notification.message.clone());
    event.payload_json["decided_by_role"] = serde_json::Value::String(actor.role.clone());
    event.payload_json["decided_by_ref"] = serde_json::Value::String(actor.ref_.clone());
    event.payload_json["decided_by_display_name"] =
        serde_json::Value::String(actor_display_name(actor));
    event.payload_json["decision_at_unix"] =
        serde_json::Value::Number(serde_json::Number::from(notification.created_at_unix));
    Ok(QueueActionProgressWriteResult::default())
}

pub(super) async fn queue_action_logs_for_orders(
    store: &MemoryProductionMapStore,
    order_ids: &[String],
) -> Result<BTreeMap<String, Vec<ProductionOrderLogEntry>>, ProductionMapError> {
    let order_ids = order_ids
        .iter()
        .map(|order_id| order_id.trim().to_string())
        .filter(|order_id| !order_id.is_empty())
        .collect::<BTreeSet<_>>();
    if order_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let events = store.queue_events.read().await;
    let mut by_order: BTreeMap<String, Vec<ProductionOrderLogEntry>> = BTreeMap::new();
    for (index, event) in events.iter().enumerate() {
        if !order_ids.contains(event.order_id.trim()) {
            continue;
        }
        by_order
            .entry(event.order_id.trim().to_string())
            .or_default()
            .push(production_order_log_entry(
                event,
                index,
                event.actor.display_name.trim().to_string(),
            ));
    }
    Ok(by_order)
}

pub(super) async fn queue_action_logs_for_worker(
    store: &MemoryProductionMapStore,
    worker_refs: &[String],
    worker_display_name: &str,
    limit: usize,
) -> Result<Vec<ProductionOrderLogEntry>, ProductionMapError> {
    let worker_refs = worker_refs
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<BTreeSet<_>>();
    let worker_display_name = worker_display_name.trim().to_ascii_lowercase();
    if worker_refs.is_empty() && worker_display_name.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let events = store.queue_events.read().await;
    let mut logs = Vec::new();
    for (index, event) in events.iter().enumerate().rev() {
        let matches_ref = worker_refs.contains(event.actor.ref_.trim());
        let matches_name = !worker_display_name.is_empty()
            && event
                .actor
                .display_name
                .trim()
                .eq_ignore_ascii_case(&worker_display_name);
        if !matches_ref && !matches_name {
            continue;
        }
        logs.push(production_order_log_entry(
            event,
            index,
            actor_display_name(&event.actor),
        ));
        if logs.len() >= limit.min(500) {
            break;
        }
    }
    Ok(logs)
}

fn production_order_log_entry(
    event: &ApparatusQueueActionEvent,
    index: usize,
    actor_display_name: String,
) -> ProductionOrderLogEntry {
    ProductionOrderLogEntry {
        event_id: event.event_id.trim().to_string(),
        apparatus: event.apparatus.trim().to_string(),
        order_id: event.order_id.trim().to_string(),
        action: event.action,
        from_state: event.from_state,
        to_state: event.to_state,
        actor_role: event.actor.role.trim().to_string(),
        actor_ref: event.actor.ref_.trim().to_string(),
        actor_display_name,
        created_at_unix: index as i64 + 1,
        completed_with_issue: event
            .payload_json
            .get("completed_with_issue")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        issue_note: json_string_field(&event.payload_json, "issue_note"),
    }
}
