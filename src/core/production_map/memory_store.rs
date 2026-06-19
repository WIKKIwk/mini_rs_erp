use super::*;

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::compiler::reject_order_number_immutable;
use super::progress::{
    actor_display_name, completion_request_decision_notification_from_event,
    completion_request_notification_from_event, json_string_field,
};

#[cfg(test)]
pub struct MemoryProductionMapStore {
    maps: RwLock<BTreeMap<String, ProductionMapDefinition>>,
    sequences: RwLock<BTreeMap<String, Vec<String>>>,
    queue_states: RwLock<BTreeMap<String, BTreeMap<String, String>>>,
    queue_policies: RwLock<BTreeMap<String, ApparatusQueuePolicy>>,
    queue_events: RwLock<Vec<ApparatusQueueActionEvent>>,
    order_run_sessions: RwLock<BTreeMap<String, OrderRunSession>>,
    order_progress_events: RwLock<Vec<OrderProgressEvent>>,
    order_progress_batches: RwLock<BTreeMap<String, OrderProgressBatch>>,
    material_rules: RwLock<BTreeMap<String, ApparatusMaterialRule>>,
    material_assignments: RwLock<BTreeMap<String, RawMaterialAssignment>>,
}

#[cfg(test)]
impl MemoryProductionMapStore {
    pub fn new() -> Self {
        Self {
            maps: RwLock::new(BTreeMap::new()),
            sequences: RwLock::new(BTreeMap::new()),
            queue_states: RwLock::new(BTreeMap::new()),
            queue_policies: RwLock::new(BTreeMap::new()),
            queue_events: RwLock::new(Vec::new()),
            order_run_sessions: RwLock::new(BTreeMap::new()),
            order_progress_events: RwLock::new(Vec::new()),
            order_progress_batches: RwLock::new(BTreeMap::new()),
            material_rules: RwLock::new(BTreeMap::new()),
            material_assignments: RwLock::new(BTreeMap::new()),
        }
    }
}

#[cfg(test)]
impl Default for MemoryProductionMapStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
#[cfg(test)]
impl ProductionMapStorePort for MemoryProductionMapStore {
    async fn maps(&self) -> Result<Vec<ProductionMapDefinition>, ProductionMapError> {
        Ok(self.maps.read().await.values().cloned().collect())
    }

    async fn put_map(&self, map: ProductionMapDefinition) -> Result<(), ProductionMapError> {
        let mut maps = self.maps.write().await;
        reject_order_number_immutable(&maps, &map)?;
        let order_number = map.order_number.trim();
        if !order_number.is_empty() {
            let duplicate = maps.values().any(|existing| {
                existing.order_number.trim() == order_number && existing.id.trim() != map.id.trim()
            });
            if duplicate {
                return Err(ProductionMapError::DuplicateOrderNumber);
            }
        }
        maps.insert(map.id.clone(), map);
        Ok(())
    }

    async fn put_maps_batch(
        &self,
        maps: &[ProductionMapDefinition],
    ) -> Result<(), ProductionMapError> {
        let mut store = self.maps.write().await;
        for map in maps {
            reject_order_number_immutable(&store, map)?;
            let order_number = map.order_number.trim();
            if !order_number.is_empty() {
                let duplicate = store.values().any(|existing| {
                    existing.order_number.trim() == order_number
                        && existing.id.trim() != map.id.trim()
                });
                if duplicate {
                    return Err(ProductionMapError::DuplicateOrderNumber);
                }
            }
        }
        for map in maps {
            store.insert(map.id.clone(), map.clone());
        }
        Ok(())
    }

    async fn delete_map(&self, map_id: &str) -> Result<(), ProductionMapError> {
        self.maps.write().await.remove(map_id.trim());
        Ok(())
    }

    async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        Ok(self.sequences.read().await.clone())
    }

    async fn put_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        self.sequences
            .write()
            .await
            .insert(apparatus.trim().to_string(), order_ids);
        Ok(())
    }

    async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        Ok(self.queue_states.read().await.clone())
    }

    async fn put_apparatus_queue_states(
        &self,
        apparatus: &str,
        states: BTreeMap<String, String>,
    ) -> Result<(), ProductionMapError> {
        self.queue_states
            .write()
            .await
            .insert(apparatus.trim().to_string(), states);
        Ok(())
    }

    async fn apparatus_queue_policies(
        &self,
    ) -> Result<BTreeMap<String, ApparatusQueuePolicy>, ProductionMapError> {
        Ok(self.queue_policies.read().await.clone())
    }

    async fn put_apparatus_queue_policy(
        &self,
        apparatus: &str,
        policy: ApparatusQueuePolicy,
        _actor: &QueueActionActor,
    ) -> Result<(), ProductionMapError> {
        self.queue_policies
            .write()
            .await
            .insert(apparatus.trim().to_string(), policy);
        Ok(())
    }

    async fn append_apparatus_queue_action_event(
        &self,
        event: ApparatusQueueActionEvent,
    ) -> Result<(), ProductionMapError> {
        self.queue_events.write().await.push(event);
        Ok(())
    }

    async fn completed_queue_orders_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
        let actor_ref = actor_ref.trim();
        if actor_ref.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let events = self.queue_events.read().await;
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

    async fn completion_requests(
        &self,
        limit: usize,
    ) -> Result<Vec<CompletionRequestNotification>, ProductionMapError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let events = self.queue_events.read().await;
        let mut requests = Vec::new();
        for (index, event) in events.iter().enumerate().rev() {
            let Some(request) = completion_request_notification_from_event(event, index as i64 + 1)
            else {
                continue;
            };
            requests.push(request);
            if requests.len() >= limit {
                break;
            }
        }
        Ok(requests)
    }

    async fn completion_request_by_event_id(
        &self,
        event_id: &str,
    ) -> Result<Option<CompletionRequestNotification>, ProductionMapError> {
        let event_id = event_id.trim();
        if event_id.is_empty() {
            return Ok(None);
        }
        let events = self.queue_events.read().await;
        Ok(events.iter().enumerate().find_map(|(index, event)| {
            if event.event_id.trim() != event_id {
                return None;
            }
            completion_request_notification_from_event(event, index as i64 + 1)
        }))
    }

    async fn completion_request_decisions_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletionRequestDecisionNotification>, ProductionMapError> {
        let actor_ref = actor_ref.trim();
        if actor_ref.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let events = self.queue_events.read().await;
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

    async fn resolve_completion_request_decision(
        &self,
        request_event_id: &str,
        decision: CompletionRequestDecision,
        actor: &QueueActionActor,
        notification: &CompletionRequestDecisionNotification,
        state_resolution: Option<CompletionRequestStateResolution>,
    ) -> Result<(), ProductionMapError> {
        let request_event_id = request_event_id.trim();
        if request_event_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        if let Some(resolution) = state_resolution {
            self.queue_states
                .write()
                .await
                .insert(resolution.apparatus.trim().to_string(), resolution.states);
            self.queue_events.write().await.push(resolution.event);
            if let Some(session) = resolution.session {
                self.order_run_sessions
                    .write()
                    .await
                    .insert(session.session_id.clone(), session);
            }
        }
        let mut events = self.queue_events.write().await;
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
        Ok(())
    }

    async fn queue_action_logs_for_orders(
        &self,
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
        let events = self.queue_events.read().await;
        let mut by_order: BTreeMap<String, Vec<ProductionOrderLogEntry>> = BTreeMap::new();
        for (index, event) in events.iter().enumerate() {
            if !order_ids.contains(event.order_id.trim()) {
                continue;
            }
            by_order
                .entry(event.order_id.trim().to_string())
                .or_default()
                .push(ProductionOrderLogEntry {
                    event_id: event.event_id.trim().to_string(),
                    apparatus: event.apparatus.trim().to_string(),
                    order_id: event.order_id.trim().to_string(),
                    action: event.action,
                    from_state: event.from_state,
                    to_state: event.to_state,
                    actor_role: event.actor.role.trim().to_string(),
                    actor_ref: event.actor.ref_.trim().to_string(),
                    actor_display_name: event.actor.display_name.trim().to_string(),
                    created_at_unix: index as i64 + 1,
                    completed_with_issue: event
                        .payload_json
                        .get("completed_with_issue")
                        .and_then(|value| value.as_bool())
                        == Some(true),
                    issue_note: json_string_field(&event.payload_json, "issue_note"),
                });
        }
        Ok(by_order)
    }

    async fn active_order_run_session(
        &self,
        apparatus: &str,
        order_id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        Ok(self
            .order_run_sessions
            .read()
            .await
            .values()
            .find(|session| {
                queue_state::apparatus_titles_match(&session.apparatus, apparatus)
                    && session.order_id.trim() == order_id.trim()
                    && matches!(
                        session.status,
                        OrderRunStatus::Active | OrderRunStatus::Paused
                    )
            })
            .cloned())
    }

    async fn order_run_session(
        &self,
        session_id: &str,
    ) -> Result<Option<OrderRunSession>, ProductionMapError> {
        Ok(self
            .order_run_sessions
            .read()
            .await
            .get(session_id.trim())
            .cloned())
    }

    async fn progress_batch(
        &self,
        batch_id: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        Ok(self
            .order_progress_batches
            .read()
            .await
            .get(batch_id.trim())
            .cloned())
    }

    async fn progress_batch_by_qr(
        &self,
        qr_payload: &str,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        let qr_payload = qr_payload.trim();
        Ok(self
            .order_progress_batches
            .read()
            .await
            .values()
            .find(|batch| batch.qr_payload.trim().eq_ignore_ascii_case(qr_payload))
            .cloned())
    }

    async fn put_order_run_session(
        &self,
        session: OrderRunSession,
    ) -> Result<(), ProductionMapError> {
        self.order_run_sessions
            .write()
            .await
            .insert(session.session_id.trim().to_string(), session);
        Ok(())
    }

    async fn put_order_progress_event(
        &self,
        event: OrderProgressEvent,
    ) -> Result<(), ProductionMapError> {
        self.order_progress_events.write().await.push(event);
        Ok(())
    }

    async fn put_order_progress_batch(
        &self,
        batch: OrderProgressBatch,
    ) -> Result<(), ProductionMapError> {
        self.order_progress_batches
            .write()
            .await
            .insert(batch.batch_id.trim().to_string(), batch);
        Ok(())
    }

    async fn apparatus_material_rules(
        &self,
    ) -> Result<Vec<ApparatusMaterialRule>, ProductionMapError> {
        Ok(self.material_rules.read().await.values().cloned().collect())
    }

    async fn put_apparatus_material_rule(
        &self,
        rule: ApparatusMaterialRule,
    ) -> Result<(), ProductionMapError> {
        self.material_rules
            .write()
            .await
            .insert(rule.apparatus.to_lowercase(), rule);
        Ok(())
    }

    async fn raw_material_assignments(
        &self,
    ) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
        Ok(self
            .material_assignments
            .read()
            .await
            .values()
            .cloned()
            .collect())
    }

    async fn put_raw_material_assignment(
        &self,
        assignment: RawMaterialAssignment,
    ) -> Result<(), ProductionMapError> {
        self.material_assignments
            .write()
            .await
            .insert(assignment.barcode.to_uppercase(), assignment);
        Ok(())
    }
}
