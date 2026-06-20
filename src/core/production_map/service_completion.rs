use std::collections::BTreeSet;

use super::*;

use super::apparatus::visible_order_ids_for_apparatus;
use super::progress::{
    actor_display_name, completion_request_decision_event_id, effective_apparatus_queue_policy,
    queue_action_event_id, unix_seconds,
};

impl ProductionMapService {
    pub async fn request_completion_without_output(
        &self,
        apparatus: &str,
        order_id: &str,
        assigned_apparatus: &[String],
        actor: QueueActionActor,
        description: &str,
    ) -> Result<CompletionRequestResult, ProductionMapError> {
        let apparatus = apparatus.trim();
        let order_id = order_id.trim();
        let description = description.trim();
        if apparatus.is_empty() || order_id.is_empty() || description.is_empty() {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        if !queue_state::apparatus_matches_assigned(apparatus, assigned_apparatus) {
            return Err(ProductionMapError::ApparatusNotAssigned);
        }

        let sequences = self.store.apparatus_sequences().await?;
        let all_states = self.store.apparatus_queue_states().await?;
        let policies = self.store.apparatus_queue_policies().await?;
        let known_keys = sequences
            .keys()
            .chain(all_states.keys())
            .map(|key| key.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|key| key.to_string())
            .collect::<Vec<_>>();
        let storage_key = queue_state::resolve_apparatus_storage_key(apparatus, &known_keys);
        let policy = effective_apparatus_queue_policy(
            apparatus,
            policies
                .get(&storage_key)
                .copied()
                .or_else(|| policies.get(apparatus).copied())
                .or_else(|| {
                    policies.iter().find_map(|(key, policy)| {
                        queue_state::apparatus_titles_match(key, apparatus).then_some(*policy)
                    })
                }),
        );
        let stored_sequence = sequences.get(&storage_key).cloned().unwrap_or_default();
        let all_maps = self.store.maps().await?;
        let visible_order_ids = visible_order_ids_for_apparatus(&all_maps, apparatus);
        let sequence =
            queue_state::effective_apparatus_sequence(&stored_sequence, &visible_order_ids);
        if !sequence.iter().any(|id| id.trim() == order_id) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let order_map = all_maps
            .iter()
            .find(|map| map.id.trim() == order_id)
            .ok_or(ProductionMapError::MapNotFound)?;
        let states = all_states.get(&storage_key).cloned().unwrap_or_default();
        let from_state = states
            .get(order_id)
            .and_then(|value| queue_state::ApparatusQueueOrderState::parse(value))
            .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
        if from_state != queue_state::ApparatusQueueOrderState::InProgress {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }

        let now = unix_seconds();
        let event_id = queue_action_event_id(
            &storage_key,
            order_id,
            queue_state::ApparatusQueueAction::Complete,
        );
        let request = CompletionRequestNotification {
            event_id: event_id.clone(),
            apparatus: storage_key.clone(),
            order_id: order_id.to_string(),
            order_number: order_map.order_number.trim().to_string(),
            order_title: order_map.title.trim().to_string(),
            product_code: order_map.product_code.trim().to_string(),
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor_display_name(&actor),
            description: description.to_string(),
            notice_kind: "completion_request".to_string(),
            decision_required: true,
            created_at_unix: now,
        };
        let event = ApparatusQueueActionEvent {
            event_id,
            apparatus: storage_key.clone(),
            order_id: order_id.to_string(),
            action: queue_state::ApparatusQueueAction::Complete,
            from_state,
            to_state: from_state,
            policy,
            actor: actor.clone(),
            assigned_apparatus: assigned_apparatus
                .iter()
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect(),
            payload_json: serde_json::json!({
                "completion_request": true,
                "description": description,
                "requested_apparatus": apparatus,
                "storage_key": storage_key,
                "order_number": request.order_number.clone(),
                "order_title": request.order_title.clone(),
                "product_code": request.product_code.clone(),
                "worker_role": request.worker_role.clone(),
                "worker_ref": request.worker_ref.clone(),
                "worker_display_name": request.worker_display_name.clone(),
                "created_at_unix": now,
            }),
        };
        self.store
            .append_apparatus_queue_action_event(event)
            .await?;
        self.notify_live();
        Ok(CompletionRequestResult {
            states,
            completion_request: request,
        })
    }

    pub async fn decide_completion_request(
        &self,
        request_event_id: &str,
        decision: CompletionRequestDecision,
        actor: QueueActionActor,
    ) -> Result<CompletionRequestDecisionResult, ProductionMapError> {
        let request_event_id = request_event_id.trim();
        if request_event_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let _guard = self.queue_action_guard().await;
        let request = self
            .store
            .completion_request_by_event_id(request_event_id)
            .await?
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        let all_states = self.store.apparatus_queue_states().await?;
        let mut states = all_states
            .get(&request.apparatus)
            .cloned()
            .unwrap_or_default();
        let from_state = states
            .get(&request.order_id)
            .and_then(|value| queue_state::ApparatusQueueOrderState::parse(value))
            .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
        if from_state != queue_state::ApparatusQueueOrderState::InProgress {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let now = unix_seconds();
        let message = match decision {
            CompletionRequestDecision::Approved => "Muammo bilan yopildi",
            CompletionRequestDecision::Rejected => "Sizni so'rovingiz rad etildi",
        };
        let notification = CompletionRequestDecisionNotification {
            event_id: completion_request_decision_event_id(request_event_id, decision),
            request_event_id: request_event_id.to_string(),
            decision: decision.as_str().to_string(),
            apparatus: request.apparatus.clone(),
            order_id: request.order_id.clone(),
            order_number: request.order_number.clone(),
            order_title: request.order_title.clone(),
            product_code: request.product_code.clone(),
            worker_role: request.worker_role.clone(),
            worker_ref: request.worker_ref.clone(),
            worker_display_name: request.worker_display_name.clone(),
            decided_by_role: actor.role.trim().to_string(),
            decided_by_ref: actor.ref_.trim().to_string(),
            decided_by_display_name: actor_display_name(&actor),
            description: request.description.clone(),
            message: message.to_string(),
            created_at_unix: now,
        };
        let state_resolution = if decision == CompletionRequestDecision::Approved {
            states.insert(
                request.order_id.clone(),
                queue_state::ApparatusQueueOrderState::Completed
                    .as_str()
                    .to_string(),
            );
            let session = self
                .store
                .active_order_run_session(&request.apparatus, &request.order_id)
                .await?
                .map(|mut session| {
                    session.status = OrderRunStatus::Completed;
                    session.updated_at_unix = now;
                    session.payload_json["completed_with_issue"] = serde_json::Value::Bool(true);
                    session.payload_json["issue_note"] =
                        serde_json::Value::String(message.to_string());
                    session
                });
            Some(CompletionRequestStateResolution {
                apparatus: request.apparatus.clone(),
                states: states.clone(),
                event: ApparatusQueueActionEvent {
                    event_id: notification.event_id.clone(),
                    apparatus: request.apparatus.clone(),
                    order_id: request.order_id.clone(),
                    action: queue_state::ApparatusQueueAction::Complete,
                    from_state,
                    to_state: queue_state::ApparatusQueueOrderState::Completed,
                    policy: ApparatusQueuePolicy::StrictSequence,
                    actor: actor.clone(),
                    assigned_apparatus: vec![request.apparatus.clone()],
                    payload_json: serde_json::json!({
                        "completion_request_decision": "approved",
                        "completion_request_event_id": request_event_id,
                        "completed_with_issue": true,
                        "issue_note": message,
                        "description": request.description,
                        "worker_ref": request.worker_ref,
                        "worker_display_name": request.worker_display_name,
                    }),
                },
                session,
            })
        } else {
            None
        };
        self.store
            .resolve_completion_request_decision(
                request_event_id,
                decision,
                &actor,
                &notification,
                state_resolution,
            )
            .await?;
        self.notify_live();
        Ok(CompletionRequestDecisionResult {
            states,
            decision: notification,
        })
    }
}
