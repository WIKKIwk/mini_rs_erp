use super::*;

use std::collections::{BTreeMap, BTreeSet};

use super::super::progress::{
    actor_display_name, completion_request_decision_notification_from_event,
    completion_request_notification_from_event, json_string_field,
};
use super::super::queue_state;
use crate::core::apparatus_standard::ApparatusId;

pub(super) async fn apparatus_queue_states(
    store: &MemoryProductionMapStore,
) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
    let queue_states = store.queue_states.read().await;
    let mut result = BTreeMap::new();
    for (apparatus, states) in queue_states.iter() {
        let apparatus = match ApparatusId::new(apparatus.trim().to_string()) {
            Ok(apparatus) => apparatus.to_string(),
            Err(error) => {
                tracing::warn!(?error, "skipping stored queue state with invalid apparatus");
                continue;
            }
        };
        if result.insert(apparatus, states.clone()).is_some() {
            return Err(ProductionMapError::StoreFailed);
        }
    }
    Ok(result)
}

pub(super) async fn put_apparatus_queue_states(
    store: &MemoryProductionMapStore,
    apparatus: &str,
    states: BTreeMap<String, String>,
) -> Result<(), ProductionMapError> {
    let apparatus = ApparatusId::new(apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::StoreFailed)?;
    let order_ids = states.keys().cloned().collect::<Vec<_>>();
    store
        .queue_states
        .write()
        .await
        .insert(apparatus.to_string(), states);
    refresh_production_order_lifecycles(store, &order_ids).await?;
    Ok(())
}

pub(crate) async fn refresh_production_order_lifecycles(
    store: &MemoryProductionMapStore,
    order_ids: &[String],
) -> Result<(), ProductionMapError> {
    let completed_with_issue_counts = store
        .queue_events
        .read()
        .await
        .iter()
        .filter(|event| {
            event
                .payload_json
                .get("completed_with_issue")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        })
        .fold(BTreeMap::<String, usize>::new(), |mut counts, event| {
            *counts.entry(event.order_id.trim().to_string()).or_default() += 1;
            counts
        });
    let completed_stage_nodes_by_order = store
        .queue_events
        .read()
        .await
        .iter()
        .filter(|event| event.action == queue_state::ApparatusQueueAction::Complete)
        .fold(
            BTreeMap::<String, BTreeSet<String>>::new(),
            |mut nodes, event| {
                let stage_node_id = event.stage_node_id.trim();
                if !stage_node_id.is_empty() {
                    nodes
                        .entry(event.order_id.trim().to_string())
                        .or_default()
                        .insert(stage_node_id.to_string());
                }
                nodes
            },
        );
    let maps = store.maps.read().await;
    let queue_states = store.queue_states.read().await;
    let roll_detached_orders = store
        .order_run_sessions
        .read()
        .await
        .values()
        .filter(|session| session.status == super::super::OrderRunStatus::RollDetached)
        .map(|session| session.order_id.trim().to_string())
        .collect::<BTreeSet<_>>();
    let mut lifecycles = store.production_order_lifecycles.write().await;
    for order_id in order_ids {
        let order_id = order_id.trim();
        let Some(map) = maps.get(order_id) else {
            continue;
        };
        let completed_stage_nodes = completed_stage_nodes_by_order
            .get(order_id)
            .cloned()
            .unwrap_or_default();
        let Some(status) =
            super::super::progress::derive_production_order_lifecycle_with_completed_stage_nodes(
                map,
                &queue_states,
                &completed_stage_nodes,
            )
        else {
            return Err(ProductionMapError::StoreFailed);
        };
        let record = lifecycles
            .entry(order_id.to_string())
            .or_insert_with(|| ProductionOrderLifecycleRecord::released(order_id));
        if record.status.can_automatically_transition_to(status) {
            let changed_at_unix = record.lifecycle_version + 1;
            record.transition_to(status, changed_at_unix);
        }
        let completed_with_issue_count = completed_with_issue_counts
            .get(order_id)
            .copied()
            .unwrap_or_default();
        record.completed_with_issue_count = completed_with_issue_count;
        if record.status == ProductionOrderLifecycleStatus::ProductionCompleted {
            record.completion_outcome = if completed_with_issue_count > 0 {
                "with_issue".to_string()
            } else {
                "normal".to_string()
            };
        }
        let mut free_wip_count = 0;
        let mut waiting_next_stage_count = 0;
        let mut in_use_wip_count = 0;
        let mut accepted_wip_count = 0;
        for batch in store.order_progress_batches.read().await.values() {
            if batch.order_id.trim() != order_id {
                continue;
            }
            match super::super::types::OrderProgressBatchStatusDetail::flow_status_for_batch(batch)
            {
                "waiting_next_stage" => waiting_next_stage_count += 1,
                "in_progress" => in_use_wip_count += 1,
                "free_wip" => free_wip_count += 1,
                "accepted_to_stock" => accepted_wip_count += 1,
                _ => {}
            }
        }
        let has_roll_detached = roll_detached_orders.contains(order_id);
        let operational_status = super::super::progress::derive_production_order_operational_status(
            record.status,
            &queue_states,
            order_id,
            completed_with_issue_count,
            has_roll_detached,
            waiting_next_stage_count,
        );
        if record.operational_status != operational_status {
            record.operational_status = operational_status;
            record.operational_status_changed_at_unix += 1;
        }
        let (flow_status, stock_status) = super::super::types::derive_order_flow_and_stock_status(
            operational_status.as_str(),
            free_wip_count,
            waiting_next_stage_count,
            in_use_wip_count,
            accepted_wip_count,
        );
        record.flow_status = flow_status.to_string();
        record.stock_status = stock_status.to_string();
    }
    Ok(())
}

pub(super) async fn append_apparatus_queue_action_event(
    store: &MemoryProductionMapStore,
    event: ApparatusQueueActionEvent,
) -> Result<(), ProductionMapError> {
    validate_queue_event(&event)?;
    let order_id = event.order_id.trim().to_string();
    store.queue_events.write().await.push(event);
    refresh_production_order_lifecycles(store, &[order_id]).await
}

fn validate_queue_event(event: &ApparatusQueueActionEvent) -> Result<(), ProductionMapError> {
    ApparatusId::new(event.apparatus.trim().to_string())
        .map_err(|_| ProductionMapError::StoreFailed)?;
    for apparatus in &event.assigned_apparatus {
        ApparatusId::new(apparatus.trim().to_string())
            .map_err(|_| ProductionMapError::StoreFailed)?;
    }
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
            || event
                .payload_json
                .get("completion_request")
                .and_then(|value| value.as_bool())
                == Some(true)
        {
            continue;
        }
        if event.action != queue_state::ApparatusQueueAction::Freeze
            && !event.action.records_progress_output()
        {
            continue;
        }
        let order_id = event.order_id.trim();
        if order_id.is_empty() || !seen.insert(order_id.to_string()) {
            continue;
        }
        let status = match event.action {
            // A freeze is not a successful worker completion. It still
            // suppresses older history for the same order until the order is
            // explicitly resumed/unfrozen.
            queue_state::ApparatusQueueAction::Freeze => continue,
            queue_state::ApparatusQueueAction::Complete
                if event.to_state == queue_state::ApparatusQueueOrderState::Completed =>
            {
                CompletedQueueOrderStatus::Completed
            }
            action if action.records_progress_output() => CompletedQueueOrderStatus::InProgress,
            _ => continue,
        };
        completed.push(CompletedQueueOrder {
            apparatus: event.apparatus.trim().to_string(),
            order_id: order_id.to_string(),
            completed_at_unix: index as i64 + 1,
            status,
            issue_note: String::new(),
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
        let request = completion_request_notification_from_event(event, index as i64 + 1);
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
        let apparatus = ApparatusId::new(resolution.apparatus.trim().to_string())
            .map_err(|_| ProductionMapError::StoreFailed)?;
        validate_queue_event(&resolution.event)?;
        let event_apparatus = ApparatusId::new(resolution.event.apparatus.trim().to_string())
            .map_err(|_| ProductionMapError::StoreFailed)?;
        if event_apparatus != apparatus {
            return Err(ProductionMapError::StoreFailed);
        }
        if let Some(session) = resolution.session.as_ref() {
            ApparatusId::new(session.apparatus.trim().to_string())
                .map_err(|_| ProductionMapError::StoreFailed)?;
        }
        let order_id = resolution.event.order_id.trim().to_string();
        store
            .queue_states
            .write()
            .await
            .insert(apparatus.to_string(), resolution.states);
        store.queue_events.write().await.push(resolution.event);
        refresh_production_order_lifecycles(store, &[order_id]).await?;
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
    _worker_display_name: &str,
    limit: usize,
) -> Result<Vec<ProductionOrderLogEntry>, ProductionMapError> {
    let worker_refs = worker_refs
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<BTreeSet<_>>();
    if worker_refs.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let events = store.queue_events.read().await;
    let mut logs = Vec::new();
    for (index, event) in events.iter().enumerate().rev() {
        let matches_ref = worker_refs.contains(event.actor.ref_.trim());
        if !matches_ref {
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
        stage_node_id: event.stage_node_id.trim().to_string(),
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
        transfer: None,
        freeze: None,
    }
}

#[cfg(test)]
mod typed_stage_identity_tests {
    use super::*;

    #[test]
    fn queue_log_uses_typed_stage_identity_not_payload_metadata() {
        let event = ApparatusQueueActionEvent {
            event_id: "event-typed-stage".to_string(),
            apparatus: "apparatus:test:stage".to_string(),
            order_id: "order-typed-stage".to_string(),
            stage_node_id: "typed-stage".to_string(),
            action: queue_state::ApparatusQueueAction::Start,
            from_state: queue_state::ApparatusQueueOrderState::Pending,
            to_state: queue_state::ApparatusQueueOrderState::InProgress,
            policy: ApparatusQueuePolicy::FreePick,
            actor: QueueActionActor::default(),
            assigned_apparatus: Vec::new(),
            payload_json: serde_json::json!({"stage_node_id": "legacy-stage"}),
        };

        let log = production_order_log_entry(&event, 0, String::new());

        assert_eq!(log.stage_node_id, "typed-stage");
    }

    #[tokio::test]
    async fn repeated_apparatus_lifecycle_uses_typed_stage_occurrences() {
        let store = MemoryProductionMapStore::new();
        let order_id = "order-typed-repeated-stage";
        let apparatus = "apparatus:test:typed-rezka";
        let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
            "id": order_id,
            "product_code": "TYPED-STAGE",
            "title": "Typed repeated stage",
            "nodes": [
                {"id": "start", "kind": "start", "title": "Start"},
                {"id": "rezka_first", "kind": "apparatus", "title": "Rezka first", "apparatus_id": apparatus},
                {"id": "rezka_final", "kind": "apparatus", "title": "Rezka final", "apparatus_id": apparatus},
                {"id": "end", "kind": "end", "title": "End"}
            ],
            "edges": [
                {"from": "start", "to": "rezka_first"},
                {"from": "rezka_first", "to": "rezka_final"},
                {"from": "rezka_final", "to": "end"}
            ]
        }))
        .expect("repeated-stage map");
        store.put_map(map).await.expect("store repeated-stage map");

        let event = |event_id: &str, stage_node_id: &str| ApparatusQueueActionEvent {
            event_id: event_id.to_string(),
            apparatus: apparatus.to_string(),
            order_id: order_id.to_string(),
            stage_node_id: stage_node_id.to_string(),
            action: queue_state::ApparatusQueueAction::Complete,
            from_state: queue_state::ApparatusQueueOrderState::InProgress,
            to_state: queue_state::ApparatusQueueOrderState::Completed,
            policy: ApparatusQueuePolicy::FreePick,
            actor: QueueActionActor::default(),
            assigned_apparatus: vec![apparatus.to_string()],
            payload_json: serde_json::json!({}),
        };

        store
            .append_apparatus_queue_action_event(event("event-first", "rezka_first"))
            .await
            .expect("first occurrence completion");
        let after_first = store
            .production_order_lifecycles(&[order_id.to_string()])
            .await
            .expect("lifecycle after first occurrence");
        assert_ne!(
            after_first.get(order_id).expect("first lifecycle").status,
            ProductionOrderLifecycleStatus::ProductionCompleted
        );

        store
            .append_apparatus_queue_action_event(event("event-final", "rezka_final"))
            .await
            .expect("final occurrence completion");
        let after_final = store
            .production_order_lifecycles(&[order_id.to_string()])
            .await
            .expect("lifecycle after final occurrence");
        assert_eq!(
            after_final.get(order_id).expect("final lifecycle").status,
            ProductionOrderLifecycleStatus::ProductionCompleted
        );
    }
}
