use super::*;

use std::collections::BTreeMap;

use serde::Serialize;
use tokio::sync::{Mutex, OwnedMutexGuard, broadcast};

const LIVE_NOTIFY_CAPACITY: usize = 256;

#[derive(Debug, Clone, Serialize)]
pub struct ProductionMapLiveSnapshot {
    pub maps: Vec<ProductionMapSaved>,
    pub sequences: BTreeMap<String, Vec<String>>,
    pub queue_states: BTreeMap<String, BTreeMap<String, String>>,
    pub queue_policies: Vec<ApparatusQueuePolicyRecord>,
}

#[derive(Clone)]
pub struct ProductionMapService {
    pub(super) store: std::sync::Arc<dyn ProductionMapStorePort>,
    live_notify: broadcast::Sender<()>,
    queue_action_lock: std::sync::Arc<Mutex<()>>,
}

struct QueueProgressRecords {
    session: Option<OrderRunSession>,
    progress_event: Option<OrderProgressEvent>,
    progress_batch: Option<OrderProgressBatch>,
}

pub struct PreparedApparatusQueueAction {
    apparatus: String,
    states: BTreeMap<String, String>,
    event: ApparatusQueueActionEvent,
    session: Option<OrderRunSession>,
    progress_event: Option<OrderProgressEvent>,
    progress_batch: Option<OrderProgressBatch>,
}

impl PreparedApparatusQueueAction {
    pub fn progress_batch(&self) -> Option<&OrderProgressBatch> {
        self.progress_batch.as_ref()
    }
}

impl ProductionMapService {
    pub fn new(store: std::sync::Arc<dyn ProductionMapStorePort>) -> Self {
        let (live_notify, _) = broadcast::channel(LIVE_NOTIFY_CAPACITY);
        Self {
            store,
            live_notify,
            queue_action_lock: std::sync::Arc::new(Mutex::new(())),
        }
    }

    pub(crate) async fn queue_action_guard(&self) -> OwnedMutexGuard<()> {
        self.queue_action_lock.clone().lock_owned().await
    }

    pub fn subscribe_live(&self) -> broadcast::Receiver<()> {
        self.live_notify.subscribe()
    }

    pub(super) fn notify_live(&self) {
        let _ = self.live_notify.send(());
    }

    pub async fn live_snapshot(&self) -> Result<ProductionMapLiveSnapshot, ProductionMapError> {
        Ok(ProductionMapLiveSnapshot {
            maps: self.maps().await?,
            sequences: self.apparatus_sequences().await?,
            queue_states: self.apparatus_queue_states().await?,
            queue_policies: self.apparatus_queue_policy_records().await?,
        })
    }

    pub async fn maps(&self) -> Result<Vec<ProductionMapSaved>, ProductionMapError> {
        let maps = self.store.maps().await?;
        let mut saved = Vec::with_capacity(maps.len());
        for mut map in maps {
            // Legacy maps saved before `code` existed: expose the order
            // number as the code so clients never need a fallback.
            if map.code.trim().is_empty() && !map.order_number.trim().is_empty() {
                map.code = map.order_number.trim().to_string();
            }
            match compile_map(&map) {
                Ok(program) => saved.push(ProductionMapSaved { map, program }),
                Err(error) => {
                    tracing::warn!(
                        map_id = %map.id,
                        error = ?error,
                        "skipping invalid production map in list response"
                    );
                }
            }
        }
        Ok(saved)
    }

    pub async fn fully_completed_orders(
        &self,
        limit: usize,
    ) -> Result<Vec<FullyCompletedProductionOrder>, ProductionMapError> {
        let maps = self.store.maps().await?;
        let queue_states = self.store.apparatus_queue_states().await?;
        let mut candidates = Vec::new();
        for map in maps {
            let order_id = map.id.trim();
            if order_id.is_empty() || !order_id.starts_with("zakaz-") {
                continue;
            }
            let required_apparatus = required_apparatus_for_closed_order(&map);
            if required_apparatus.is_empty() {
                continue;
            }
            if !required_apparatus
                .iter()
                .all(|apparatus| order_completed_on_apparatus(&queue_states, order_id, apparatus))
            {
                continue;
            }
            candidates.push((map, required_apparatus));
        }
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        let order_ids = candidates
            .iter()
            .map(|(map, _)| map.id.trim().to_string())
            .collect::<Vec<_>>();
        let logs_by_order = self.store.queue_action_logs_for_orders(&order_ids).await?;
        let mut closed = Vec::new();
        for (map, required_apparatus) in candidates {
            let order_id = map.id.trim().to_string();
            let logs = logs_by_order.get(&order_id).cloned().unwrap_or_default();
            let Some(closed_event) = latest_required_complete_event(&logs, &required_apparatus)
            else {
                continue;
            };
            closed.push(FullyCompletedProductionOrder {
                order_id,
                order_number: map.order_number.trim().to_string(),
                title: map.title.trim().to_string(),
                product_code: map.product_code.trim().to_string(),
                completed_at_unix: closed_event.created_at_unix,
                closed_by_role: closed_event.actor_role.clone(),
                closed_by_ref: closed_event.actor_ref.clone(),
                closed_by_display_name: closed_event.actor_display_name.clone(),
                logs,
            });
        }
        closed.sort_by(|left, right| {
            right
                .completed_at_unix
                .cmp(&left.completed_at_unix)
                .then_with(|| left.order_id.cmp(&right.order_id))
        });
        closed.truncate(limit.clamp(1, 500));
        Ok(closed)
    }

    pub async fn map(
        &self,
        map_id: &str,
    ) -> Result<Option<ProductionMapSaved>, ProductionMapError> {
        let map_id = map_id.trim();
        if map_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let Some(mut map) = self.raw_map(map_id).await? else {
            return Ok(None);
        };
        if map.code.trim().is_empty() && !map.order_number.trim().is_empty() {
            map.code = map.order_number.trim().to_string();
        }
        let program = compile_map(&map)?;
        Ok(Some(ProductionMapSaved { map, program }))
    }

    pub async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        self.store.apparatus_sequences().await
    }

    pub async fn set_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        let apparatus = apparatus.trim();
        if apparatus.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let order_ids = order_ids
            .into_iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect();
        self.store
            .put_apparatus_sequence(apparatus, order_ids)
            .await?;
        self.notify_live();
        Ok(())
    }

    pub async fn apparatus_queue_states(
        &self,
    ) -> Result<BTreeMap<String, BTreeMap<String, String>>, ProductionMapError> {
        self.store.apparatus_queue_states().await
    }

    pub async fn completed_queue_orders_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletedQueueOrder>, ProductionMapError> {
        self.store
            .completed_queue_orders_for_actor(actor_ref, limit)
            .await
    }

    pub async fn completion_requests(
        &self,
        limit: usize,
    ) -> Result<Vec<CompletionRequestNotification>, ProductionMapError> {
        self.store.completion_requests(limit).await
    }

    pub async fn completion_request_decisions_for_actor(
        &self,
        actor_ref: &str,
        limit: usize,
    ) -> Result<Vec<CompletionRequestDecisionNotification>, ProductionMapError> {
        self.store
            .completion_request_decisions_for_actor(actor_ref, limit)
            .await
    }

    pub async fn apparatus_queue_policy_records(
        &self,
    ) -> Result<Vec<ApparatusQueuePolicyRecord>, ProductionMapError> {
        Ok(self
            .store
            .apparatus_queue_policies()
            .await?
            .into_iter()
            .map(|(apparatus, policy)| effective_apparatus_queue_policy_record(&apparatus, policy))
            .collect())
    }

    pub async fn set_apparatus_queue_policy(
        &self,
        apparatus: &str,
        policy: ApparatusQueuePolicy,
        actor: &QueueActionActor,
    ) -> Result<ApparatusQueuePolicyRecord, ProductionMapError> {
        let apparatus = apparatus.trim();
        if apparatus.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let record = effective_apparatus_queue_policy_record(apparatus, policy);
        if record.locked && record.policy != policy {
            return Err(ProductionMapError::ApparatusQueuePolicyLocked);
        }
        self.store
            .put_apparatus_queue_policy(apparatus, record.policy, actor)
            .await?;
        self.notify_live();
        Ok(record)
    }

    pub async fn apply_apparatus_queue_action(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        assigned_apparatus: &[String],
        actor: QueueActionActor,
    ) -> Result<BTreeMap<String, String>, ProductionMapError> {
        Ok(self
            .apply_apparatus_queue_action_with_progress(
                apparatus,
                order_id,
                action,
                assigned_apparatus,
                actor,
                QueueProgressInput::default(),
            )
            .await?
            .states)
    }

    pub async fn apply_apparatus_queue_action_with_progress(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        assigned_apparatus: &[String],
        actor: QueueActionActor,
        progress: QueueProgressInput,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let prepared = self
            .prepare_apparatus_queue_action_with_progress(
                apparatus,
                order_id,
                action,
                assigned_apparatus,
                actor,
                progress,
            )
            .await?;
        self.commit_prepared_queue_action(prepared).await
    }

    pub(crate) async fn prepare_apparatus_queue_action_with_progress(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        assigned_apparatus: &[String],
        actor: QueueActionActor,
        progress: QueueProgressInput,
    ) -> Result<PreparedApparatusQueueAction, ProductionMapError> {
        let apparatus = apparatus.trim();
        let order_id = order_id.trim();
        if apparatus.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        if order_id.is_empty() {
            return Err(ProductionMapError::MissingId);
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
        if matches!(action, queue_state::ApparatusQueueAction::Start)
            && !chain::order_ready_for_station(
                order_map,
                order_id,
                apparatus,
                &all_states,
                &known_keys,
            )
        {
            return Err(ProductionMapError::PreviousStageNotCompleted);
        }
        let states = all_states.get(&storage_key).cloned().unwrap_or_default();
        let mut parsed = BTreeMap::new();
        for (id, value) in states {
            if let Some(state) = queue_state::ApparatusQueueOrderState::parse(&value) {
                parsed.insert(id, state);
            }
        }
        let from_state = parsed
            .get(order_id)
            .copied()
            .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
        match policy {
            ApparatusQueuePolicy::StrictSequence => {
                queue_state::apply_queue_action(&sequence, &mut parsed, order_id, action)?;
            }
            ApparatusQueuePolicy::FreePick => {
                queue_state::apply_unordered_queue_action(&mut parsed, order_id, action)?;
            }
        }
        let to_state = parsed
            .get(order_id)
            .copied()
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        let saved = parsed
            .into_iter()
            .map(|(id, state)| (id, state.as_str().to_string()))
            .collect::<BTreeMap<_, _>>();
        let event = ApparatusQueueActionEvent {
            event_id: queue_action_event_id(&storage_key, order_id, action),
            apparatus: storage_key.clone(),
            order_id: order_id.to_string(),
            action,
            from_state,
            to_state,
            policy,
            actor: actor.clone(),
            assigned_apparatus: assigned_apparatus
                .iter()
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect(),
            payload_json: serde_json::json!({
                "requested_apparatus": apparatus,
                "storage_key": storage_key,
                "sequence": sequence,
                "visible_order_ids": visible_order_ids,
                "from_state": from_state.as_str(),
                "to_state": to_state.as_str(),
                "policy": policy.as_str(),
            }),
        };
        let progress = self
            .build_progress_records(&storage_key, order_id, order_map, action, &actor, progress)
            .await?;
        Ok(PreparedApparatusQueueAction {
            apparatus: storage_key,
            states: saved,
            event,
            session: progress.session,
            progress_event: progress.progress_event,
            progress_batch: progress.progress_batch,
        })
    }

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

    pub(crate) async fn commit_prepared_queue_action(
        &self,
        prepared: PreparedApparatusQueueAction,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        self.store
            .put_apparatus_queue_states_with_event_and_progress(
                &prepared.apparatus,
                prepared.states.clone(),
                prepared.event,
                prepared.session.clone(),
                prepared.progress_event.clone(),
                prepared.progress_batch.clone(),
            )
            .await?;
        self.notify_live();
        Ok(ApparatusQueueActionResult {
            states: prepared.states,
            session: prepared.session,
            progress_event: prepared.progress_event,
            progress_batch: prepared.progress_batch,
        })
    }

    pub async fn progress_batch_for_qr(
        &self,
        progress_batch_id: &str,
        qr_payload: &str,
    ) -> Result<OrderProgressBatch, ProductionMapError> {
        let batch = if !progress_batch_id.trim().is_empty() {
            self.store.progress_batch(progress_batch_id).await?
        } else if !qr_payload.trim().is_empty() {
            self.store.progress_batch_by_qr(qr_payload).await?
        } else {
            return Err(ProductionMapError::ProgressInputInvalid);
        };
        batch.ok_or(ProductionMapError::ProgressBatchNotFound)
    }

    async fn build_progress_records(
        &self,
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        action: queue_state::ApparatusQueueAction,
        actor: &QueueActionActor,
        progress: QueueProgressInput,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let now = unix_seconds();
        match action {
            queue_state::ApparatusQueueAction::Start => {
                let session = OrderRunSession {
                    session_id: progress_session_id(apparatus, order_id, actor, now),
                    apparatus: apparatus.to_string(),
                    order_id: order_id.to_string(),
                    status: OrderRunStatus::Active,
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    started_at_unix: now,
                    updated_at_unix: now,
                    payload_json: serde_json::json!({
                        "started_by": actor,
                    }),
                };
                let event = OrderProgressEvent {
                    event_id: progress_event_id(&session.session_id, order_id, action, now),
                    session_id: session.session_id.clone(),
                    batch_id: String::new(),
                    apparatus: apparatus.to_string(),
                    order_id: order_id.to_string(),
                    action,
                    produced_qty: 0.0,
                    uom: String::new(),
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    qr_payload: String::new(),
                    payload_json: serde_json::json!({"event": "start"}),
                };
                Ok(QueueProgressRecords {
                    session: Some(session),
                    progress_event: Some(event),
                    progress_batch: None,
                })
            }
            queue_state::ApparatusQueueAction::Pause
            | queue_state::ApparatusQueueAction::Complete => {
                let produced_qty = valid_progress_qty(progress.produced_qty)?;
                let uom = non_empty_or(&progress.uom, "kg");
                let session = self
                    .store
                    .active_order_run_session(apparatus, order_id)
                    .await?
                    .unwrap_or_else(|| legacy_order_run_session(apparatus, order_id, actor, now));
                let status = match action {
                    queue_state::ApparatusQueueAction::Pause => OrderRunStatus::Paused,
                    queue_state::ApparatusQueueAction::Complete => OrderRunStatus::Completed,
                    _ => OrderRunStatus::Active,
                };
                let session = OrderRunSession {
                    status,
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    updated_at_unix: now,
                    payload_json: serde_json::json!({
                        "last_action": queue_action_str(action),
                        "last_qty": produced_qty,
                        "last_uom": uom,
                    }),
                    ..session
                };
                let batch_id = non_empty_or(
                    &progress.progress_batch_id,
                    &progress_batch_id(apparatus, order_id, action, now),
                );
                let qr_payload =
                    non_empty_or(&progress.qr_payload, &progress_qr_payload(&batch_id));
                let label_item_name = progress_label_item_name(order_map, apparatus, action);
                let batch = OrderProgressBatch {
                    batch_id: batch_id.clone(),
                    session_id: session.session_id.clone(),
                    apparatus: apparatus.to_string(),
                    order_id: order_id.to_string(),
                    action,
                    status: match action {
                        queue_state::ApparatusQueueAction::Pause => {
                            OrderProgressBatchStatus::Paused
                        }
                        queue_state::ApparatusQueueAction::Complete => {
                            OrderProgressBatchStatus::Completed
                        }
                        _ => return Err(ProductionMapError::ProgressInputInvalid),
                    },
                    produced_qty,
                    uom: uom.clone(),
                    qr_payload: qr_payload.clone(),
                    label_item_code: order_id.to_string(),
                    label_item_name,
                    executor_name: actor_display_name(actor),
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    payload_json: serde_json::json!({
                        "order_title": order_map.title.trim(),
                        "apparatus": apparatus,
                        "action": queue_action_str(action),
                    }),
                };
                let event = OrderProgressEvent {
                    event_id: progress_event_id(&session.session_id, order_id, action, now),
                    session_id: session.session_id.clone(),
                    batch_id,
                    apparatus: apparatus.to_string(),
                    order_id: order_id.to_string(),
                    action,
                    produced_qty,
                    uom,
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    qr_payload,
                    payload_json: serde_json::json!({"event": queue_action_str(action)}),
                };
                Ok(QueueProgressRecords {
                    session: Some(session),
                    progress_event: Some(event),
                    progress_batch: Some(batch),
                })
            }
            queue_state::ApparatusQueueAction::Resume => {
                if progress.progress_batch_id.trim().is_empty()
                    && progress.qr_payload.trim().is_empty()
                {
                    let session = self
                        .store
                        .active_order_run_session(apparatus, order_id)
                        .await?
                        .unwrap_or_else(|| {
                            legacy_order_run_session(apparatus, order_id, actor, now)
                        });
                    let session = OrderRunSession {
                        status: OrderRunStatus::Active,
                        worker_role: actor.role.trim().to_string(),
                        worker_ref: actor.ref_.trim().to_string(),
                        worker_display_name: actor.display_name.trim().to_string(),
                        updated_at_unix: now,
                        payload_json: serde_json::json!({
                            "resumed_without_progress_qr": true,
                        }),
                        ..session
                    };
                    let event = OrderProgressEvent {
                        event_id: progress_event_id(&session.session_id, order_id, action, now),
                        session_id: session.session_id.clone(),
                        batch_id: String::new(),
                        apparatus: apparatus.to_string(),
                        order_id: order_id.to_string(),
                        action,
                        produced_qty: 0.0,
                        uom: String::new(),
                        worker_role: actor.role.trim().to_string(),
                        worker_ref: actor.ref_.trim().to_string(),
                        worker_display_name: actor.display_name.trim().to_string(),
                        qr_payload: String::new(),
                        payload_json: serde_json::json!({"event": "resume"}),
                    };
                    return Ok(QueueProgressRecords {
                        session: Some(session),
                        progress_event: Some(event),
                        progress_batch: None,
                    });
                }
                let mut batch = self
                    .progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)
                    .await?;
                if batch.status != OrderProgressBatchStatus::Paused
                    || batch.action != queue_state::ApparatusQueueAction::Pause
                {
                    return Err(ProductionMapError::ProgressBatchNotResumable);
                }
                if batch.order_id.trim() != order_id
                    || !queue_state::apparatus_titles_match(&batch.apparatus, apparatus)
                {
                    return Err(ProductionMapError::ProgressBatchNotResumable);
                }
                batch.status = OrderProgressBatchStatus::Resumed;
                batch.payload_json = serde_json::json!({
                    "resumed_by": actor,
                    "resumed_at_unix": now,
                });
                let session = self
                    .store
                    .order_run_session(&batch.session_id)
                    .await?
                    .or_else(|| Some(legacy_order_run_session(apparatus, order_id, actor, now)))
                    .map(|session| OrderRunSession {
                        status: OrderRunStatus::Active,
                        worker_role: actor.role.trim().to_string(),
                        worker_ref: actor.ref_.trim().to_string(),
                        worker_display_name: actor.display_name.trim().to_string(),
                        updated_at_unix: now,
                        payload_json: serde_json::json!({
                            "resumed_batch_id": batch.batch_id,
                            "resumed_qr_payload": batch.qr_payload,
                        }),
                        ..session
                    })
                    .ok_or(ProductionMapError::ProgressBatchNotFound)?;
                let event = OrderProgressEvent {
                    event_id: progress_event_id(&session.session_id, order_id, action, now),
                    session_id: session.session_id.clone(),
                    batch_id: batch.batch_id.clone(),
                    apparatus: apparatus.to_string(),
                    order_id: order_id.to_string(),
                    action,
                    produced_qty: 0.0,
                    uom: String::new(),
                    worker_role: actor.role.trim().to_string(),
                    worker_ref: actor.ref_.trim().to_string(),
                    worker_display_name: actor.display_name.trim().to_string(),
                    qr_payload: batch.qr_payload.clone(),
                    payload_json: serde_json::json!({"event": "resume"}),
                };
                Ok(QueueProgressRecords {
                    session: Some(session),
                    progress_event: Some(event),
                    progress_batch: Some(batch),
                })
            }
        }
    }

    pub async fn upsert_map(
        &self,
        mut map: ProductionMapDefinition,
    ) -> Result<ProductionMapSaved, ProductionMapError> {
        normalize_map(&mut map);
        let program = compile_map(&map)?;
        self.store.put_map(map.clone()).await?;
        self.notify_live();
        Ok(ProductionMapSaved { map, program })
    }

    #[allow(dead_code)]
    pub async fn upsert_maps_batch(
        &self,
        maps: Vec<ProductionMapDefinition>,
    ) -> Result<Vec<ProductionMapSaved>, ProductionMapError> {
        let mut normalized = Vec::with_capacity(maps.len());
        let mut saved = Vec::with_capacity(maps.len());
        for mut map in maps {
            normalize_map(&mut map);
            let program = compile_map(&map)?;
            saved.push(ProductionMapSaved {
                map: map.clone(),
                program,
            });
            normalized.push(map);
        }
        self.store.put_maps_batch(&normalized).await?;
        self.notify_live();
        Ok(saved)
    }

    pub async fn raw_map(
        &self,
        map_id: &str,
    ) -> Result<Option<ProductionMapDefinition>, ProductionMapError> {
        let map_id = map_id.trim().to_ascii_lowercase();
        Ok(self
            .store
            .maps()
            .await?
            .into_iter()
            .find(|map| map.id.trim() == map_id))
    }

    pub async fn restore_map(
        &self,
        previous: Option<&ProductionMapDefinition>,
        map_id: &str,
    ) -> Result<(), ProductionMapError> {
        let result = match previous {
            Some(map) => self.store.put_map(map.clone()).await,
            None => self.store.delete_map(map_id).await,
        };
        if result.is_ok() {
            self.notify_live();
        }
        result
    }

    /// Moves multiple orders atomically: either every move succeeds or none
    /// are persisted.
    pub async fn move_apparatus_batch(
        &self,
        input: ProductionMapBatchMoveRequest,
    ) -> Result<Vec<ProductionMapSaved>, ProductionMapError> {
        let from = input.from_apparatus.trim();
        let to = input.to_apparatus.trim();
        if from.is_empty() || to.is_empty() || from == to {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        let map_ids: Vec<String> = input
            .map_ids
            .iter()
            .map(|id| id.trim().to_ascii_lowercase())
            .filter(|id| !id.is_empty())
            .collect();
        if map_ids.is_empty() {
            return Err(ProductionMapError::MissingId);
        }

        let maps = self.store.maps().await?;
        let mut updated = Vec::with_capacity(map_ids.len());
        for map_id in &map_ids {
            let Some(map) = maps.iter().find(|item| item.id.trim() == map_id).cloned() else {
                return Err(ProductionMapError::MapNotFound);
            };
            if !move_allowed(&map, from, to) {
                return Err(ProductionMapError::MoveNotAllowed);
            }
            let mut next = map;
            if !reassign_alternative_apparatus_assignment(&mut next, from, to)
                && !reassign_apparatus_nodes(&mut next, from, to)
            {
                return Err(ProductionMapError::MoveNotAllowed);
            }
            updated.push(next);
        }

        self.store.put_maps_batch(&updated).await?;
        self.notify_live();
        updated
            .into_iter()
            .map(|map| {
                let program = compile_map(&map)?;
                Ok(ProductionMapSaved { map, program })
            })
            .collect()
    }

    /// Moves an order between apparatus, validating pechat rules server-side.
    pub async fn move_apparatus(
        &self,
        input: ProductionMapMoveRequest,
    ) -> Result<ProductionMapSaved, ProductionMapError> {
        let map_id = input.map_id.trim().to_ascii_lowercase();
        let from = input.from_apparatus.trim();
        let to = input.to_apparatus.trim();
        if map_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        if to.is_empty() || from == to {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        let maps = self.store.maps().await?;
        let Some(map) = maps.into_iter().find(|map| map.id.trim() == map_id) else {
            return Err(ProductionMapError::MapNotFound);
        };
        if !move_allowed(&map, from, to) {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        let mut next = map;
        if !reassign_alternative_apparatus_assignment(&mut next, from, to)
            && !reassign_apparatus_nodes(&mut next, from, to)
        {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        self.upsert_map(next).await
    }

    pub async fn run_map(
        &self,
        input: ProductionMapRunRequest,
    ) -> Result<ProductionMapRunResult, ProductionMapError> {
        if input.order_qty <= 0.0 {
            return Err(ProductionMapError::InvalidOrderQty);
        }
        let map_id = input.map_id.trim().to_ascii_lowercase();
        let product_code = input.product_code.trim();
        let maps = self.store.maps().await?;
        let Some(map) = maps.into_iter().find(|map| {
            (!map_id.is_empty() && map.id == map_id)
                || (!product_code.is_empty() && map.product_code == product_code)
        }) else {
            return Err(ProductionMapError::MapNotFound);
        };
        run_map_with_variables(&map, input.order_qty, input.variables)
    }
}
