use std::collections::{BTreeMap, BTreeSet};

use super::apparatus::visible_order_ids_for_apparatus;
use super::progress::{effective_apparatus_queue_policy, queue_action_event_id, unix_seconds};
use super::service_queue_support::{
    QueueActionEventInput, known_apparatus_storage_keys, order_has_frozen_queue_state,
    parsed_queue_states, queue_action_event, sequence_updates_for_frozen_transition,
    serialized_queue_states,
};
use super::*;


impl ProductionMapService {
    pub async fn order_control_states(
        &self,
    ) -> Result<BTreeMap<String, OrderControlRecord>, ProductionMapError> {
        self.store.order_control_states().await
    }

    pub async fn order_control_state(
        &self,
        order_id: &str,
    ) -> Result<OrderControlRecord, ProductionMapError> {
        let order_id = required_existing_order_id(self, order_id).await?;
        Ok(self
            .store
            .order_control_states()
            .await?
            .remove(&order_id)
            .unwrap_or_else(|| OrderControlRecord::active(&order_id)))
    }

    pub async fn request_order_freeze(
        &self,
        order_id: &str,
        actor: QueueActionActor,
    ) -> Result<OrderControlRecord, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let order_id = required_existing_order_id(self, order_id).await?;
        let current = current_order_control(self, &order_id).await?;
        if current.state != OrderControlState::Active {
            return Err(ProductionMapError::OrderControlActionNotAllowed);
        }
        let queue_states = self.store.apparatus_queue_states().await?;
        if order_has_frozen_queue_state(&queue_states, &order_id) {
            return Err(ProductionMapError::OrderFrozen);
        }

        let evidence = order_flow_evidence(self, &order_id).await?;
        if evidence.completed {
            return Err(ProductionMapError::OrderAlreadyCompleted);
        }
        if !evidence.started {
            return Err(ProductionMapError::OrderNotStarted);
        }

        let now = unix_seconds();
        let state = if evidence.has_active_work {
            OrderControlState::FreezeRequested
        } else {
            OrderControlState::Frozen
        };
        let target_session = if state == OrderControlState::FreezeRequested {
            match evidence.active_sessions.as_slice() {
                [session] => Some(session),
                [] => return Err(ProductionMapError::OrderFreezeTargetNotFound),
                _ => return Err(ProductionMapError::OrderFreezeTargetAmbiguous),
            }
        } else {
            match evidence.paused_sessions.as_slice() {
                [session] => Some(session),
                _ => None,
            }
        };
        let request_status = if state == OrderControlState::FreezeRequested {
            OrderFreezeRequestStatus::Pending
        } else {
            OrderFreezeRequestStatus::Frozen
        };
        let freeze_request = OrderFreezeRequest {
            request_id: new_freeze_request_id(),
            status: request_status,
            target_session_id: target_session
                .map(|session| session.session_id.trim().to_string())
                .unwrap_or_default(),
            target_apparatus: target_session
                .map(|session| session.apparatus.trim().to_string())
                .unwrap_or_default(),
            target_worker_role: target_session
                .map(|session| session.worker_role.trim().to_string())
                .unwrap_or_default(),
            target_worker_ref: target_session
                .map(|session| session.worker_ref.trim().to_string())
                .unwrap_or_default(),
            target_worker_display_name: target_session
                .map(|session| session.worker_display_name.trim().to_string())
                .unwrap_or_default(),
            requested_at_unix: now,
            transitioned_at_unix: now,
        };
        let record = OrderControlRecord {
            order_id,
            state,
            actor,
            requested_at_unix: now,
            frozen_at_unix: (state == OrderControlState::Frozen).then_some(now),
            freeze_request: Some(freeze_request),
        };
        if let Some(write) =
            prepare_direct_freeze_queue_write(self, &record, target_session).await?
        {
            self.store
                .put_apparatus_queue_states_with_event_and_progress(&write)
                .await?;
        } else if state == OrderControlState::FreezeRequested {
            self.store.put_order_control_state(record.clone()).await?;
        } else {
            return Err(ProductionMapError::OrderFreezeTargetNotFound);
        }
        self.notify_live();
        Ok(record)
    }

    pub async fn cancel_order_freeze_request(
        &self,
        order_id: &str,
        actor: QueueActionActor,
    ) -> Result<OrderControlRecord, ProductionMapError> {
        self.transition_order_control(
            order_id,
            OrderControlState::FreezeRequested,
            OrderControlState::Active,
            actor,
        )
        .await
    }

    pub async fn unfreeze_order(
        &self,
        order_id: &str,
        actor: QueueActionActor,
    ) -> Result<OrderControlRecord, ProductionMapError> {
        self.transition_order_control(
            order_id,
            OrderControlState::Frozen,
            OrderControlState::Active,
            actor,
        )
        .await
    }

    async fn transition_order_control(
        &self,
        order_id: &str,
        expected: OrderControlState,
        next: OrderControlState,
        actor: QueueActionActor,
    ) -> Result<OrderControlRecord, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let order_id = required_existing_order_id(self, order_id).await?;
        let current = current_order_control(self, &order_id).await?;
        if current.state != expected {
            return Err(ProductionMapError::OrderControlActionNotAllowed);
        }
        let now = unix_seconds();
        let mut freeze_request = match current.freeze_request {
            Some(request) => request,
            None if expected == OrderControlState::Frozen && next == OrderControlState::Active => {
                recovery_freeze_request(self, &current, &order_id, now).await?
            }
            None => return Err(ProductionMapError::OrderControlActionNotAllowed),
        };
        freeze_request.status = match (expected, next) {
            (OrderControlState::FreezeRequested, OrderControlState::Active) => {
                OrderFreezeRequestStatus::Cancelled
            }
            (OrderControlState::Frozen, OrderControlState::Active) => {
                OrderFreezeRequestStatus::Unfrozen
            }
            _ => return Err(ProductionMapError::OrderControlActionNotAllowed),
        };
        freeze_request.transitioned_at_unix = now;
        let record = OrderControlRecord {
            order_id,
            state: next,
            actor,
            requested_at_unix: freeze_request.requested_at_unix,
            frozen_at_unix: None,
            freeze_request: Some(freeze_request),
        };
        if expected == OrderControlState::Frozen && next == OrderControlState::Active {
            restore_frozen_queue_after_unfreeze(self, &record).await?;
        } else {
            self.store.put_order_control_state(record.clone()).await?;
        }
        self.notify_live();
        Ok(record)
    }

    pub async fn delete_order(
        &self,
        order_id: &str,
    ) -> Result<OrderDeleteResult, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let order_id = required_existing_order_id(self, order_id).await?;
        let mut blockers = Vec::new();

        for (apparatus, sequence) in self.effective_apparatus_sequences().await? {
            if sequence
                .first()
                .is_some_and(|first| first.trim() == order_id)
            {
                blockers.push(OrderDeleteBlocker::new(
                    "first_in_sequence",
                    format!("Buyurtma {apparatus} ketma-ketligida 1-o‘rinda turibdi"),
                ));
            }
        }

        let evidence = order_flow_evidence(self, &order_id).await?;
        if evidence.started {
            let stages = evidence.started_apparatuses.into_iter().collect::<Vec<_>>();
            let message = if stages.is_empty() {
                "Buyurtmada ish jarayoni allaqachon boshlangan".to_string()
            } else {
                format!("Buyurtmada ish jarayoni boshlangan: {}", stages.join(", "))
            };
            blockers.push(OrderDeleteBlocker::new("work_started", message));
        }

        let assignments = self
            .store
            .raw_material_assignments()
            .await?
            .into_iter()
            .filter(|assignment| assignment.order_id.trim() == order_id)
            .collect::<Vec<_>>();
        if !assignments.is_empty() {
            blockers.push(OrderDeleteBlocker::new(
                "raw_material_attached",
                format!("Buyurtmaga {} ta homashyo biriktirilgan", assignments.len()),
            ));
        }

        if !blockers.is_empty() {
            return Err(ProductionMapError::OrderDeleteBlocked(blockers));
        }

        self.store.delete_map(&order_id).await?;
        self.notify_live();
        Ok(OrderDeleteResult {
            order_id,
            deleted: true,
        })
    }
}

struct OrderFlowEvidence {
    started: bool,
    completed: bool,
    has_active_work: bool,
    started_apparatuses: BTreeSet<String>,
    active_sessions: Vec<OrderRunSession>,
    paused_sessions: Vec<OrderRunSession>,
}

async fn required_existing_order_id(
    service: &ProductionMapService,
    order_id: &str,
) -> Result<String, ProductionMapError> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Err(ProductionMapError::MissingId);
    }
    if !order_id.starts_with("zakaz-") {
        return Err(ProductionMapError::MapNotFound);
    }
    if !service
        .store
        .maps()
        .await?
        .iter()
        .any(|map| map.id.trim() == order_id)
    {
        return Err(ProductionMapError::MapNotFound);
    }
    Ok(order_id.to_string())
}

async fn current_order_control(
    service: &ProductionMapService,
    order_id: &str,
) -> Result<OrderControlRecord, ProductionMapError> {
    Ok(service
        .store
        .order_control_states()
        .await?
        .remove(order_id)
        .unwrap_or_else(|| OrderControlRecord::active(order_id)))
}

async fn recovery_freeze_request(
    service: &ProductionMapService,
    current: &OrderControlRecord,
    order_id: &str,
    now: i64,
) -> Result<OrderFreezeRequest, ProductionMapError> {
    let queue_states = service.store.apparatus_queue_states().await?;
    let sessions = service.store.order_run_sessions_for_order(order_id).await?;
    let session = sessions
        .iter()
        .find(|session| session.status == OrderRunStatus::Frozen);
    let target_apparatus = session
        .map(|session| session.apparatus.trim().to_string())
        .or_else(|| {
            queue_states.iter().find_map(|(apparatus, states)| {
                (states
                    .get(order_id)
                    .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                    == Some(queue_state::ApparatusQueueOrderState::Frozen))
                .then(|| apparatus.trim().to_string())
            })
        })
        .filter(|apparatus| !apparatus.is_empty())
        .ok_or(ProductionMapError::OrderFreezeTargetNotFound)?;
    let actor = session
        .map(|session| QueueActionActor {
            role: session.worker_role.clone(),
            ref_: session.worker_ref.clone(),
            display_name: session.worker_display_name.clone(),
        })
        .unwrap_or_else(|| current.actor.clone());
    Ok(OrderFreezeRequest {
        request_id: format!("order-freeze-recovery:{order_id}"),
        status: OrderFreezeRequestStatus::Unfrozen,
        target_session_id: session
            .map(|session| session.session_id.trim().to_string())
            .unwrap_or_default(),
        target_apparatus,
        target_worker_role: actor.role,
        target_worker_ref: actor.ref_,
        target_worker_display_name: actor.display_name,
        requested_at_unix: current.requested_at_unix.max(0),
        transitioned_at_unix: now,
    })
}

async fn order_flow_evidence(
    service: &ProductionMapService,
    order_id: &str,
) -> Result<OrderFlowEvidence, ProductionMapError> {
    let all_states = service.store.apparatus_queue_states().await?;
    let sessions = service.store.order_run_sessions_for_order(order_id).await?;
    let batches = service.store.progress_batches_for_order(order_id).await?;
    let logs = service
        .store
        .queue_action_logs_for_orders(&[order_id.to_string()])
        .await?
        .remove(order_id)
        .unwrap_or_default();

    let mut started_apparatuses = BTreeSet::new();
    let mut non_pending_state = false;
    let mut has_active_work = false;
    for (apparatus, states) in &all_states {
        let Some(state) = states
            .get(order_id)
            .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
        else {
            continue;
        };
        if state != queue_state::ApparatusQueueOrderState::Pending {
            non_pending_state = true;
            started_apparatuses.insert(apparatus.clone());
        }
        if state == queue_state::ApparatusQueueOrderState::InProgress {
            has_active_work = true;
        }
    }
    let active_sessions = sessions
        .iter()
        .filter(|session| session.status == OrderRunStatus::Active)
        .cloned()
        .collect::<Vec<_>>();
    let paused_sessions = sessions
        .iter()
        .filter(|session| {
            matches!(
                session.status,
                OrderRunStatus::Paused | OrderRunStatus::RollDetached
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    for session in &sessions {
        started_apparatuses.insert(session.apparatus.trim().to_string());
        if session.status == OrderRunStatus::Active {
            has_active_work = true;
        }
    }
    for batch in &batches {
        started_apparatuses.insert(batch.apparatus.trim().to_string());
    }
    for log in &logs {
        started_apparatuses.insert(log.apparatus.trim().to_string());
    }
    started_apparatuses.retain(|value| !value.is_empty());

    let status = service.order_status_detail(order_id).await?;
    let completed = matches!(
        status.order_status.as_str(),
        "completed" | "completed_with_issue"
    );
    let started =
        non_pending_state || !sessions.is_empty() || !batches.is_empty() || !logs.is_empty();
    Ok(OrderFlowEvidence {
        started,
        completed,
        has_active_work,
        started_apparatuses,
        active_sessions,
        paused_sessions,
    })
}

fn new_freeze_request_id() -> String {
    let bytes: [u8; 12] = rand::random();
    format!(
        "order-freeze-request_{}",
        data_encoding::HEXLOWER.encode(&bytes)
    )
}


async fn prepare_direct_freeze_queue_write(
    service: &ProductionMapService,
    record: &OrderControlRecord,
    target_session: Option<&OrderRunSession>,
) -> Result<Option<QueueActionProgressWrite>, ProductionMapError> {
    let all_states = service.store.apparatus_queue_states().await?;
    let sequences = service.store.apparatus_sequences().await?;
    let order_controls = service.store.order_control_states().await?;
    let maps = service.store.maps().await?;
    let known_keys = known_apparatus_storage_keys(&sequences, &all_states);
    let target_apparatus = target_session
        .map(|session| session.apparatus.trim().to_string())
        .or_else(|| {
            all_states.iter().find_map(|(apparatus, states)| {
                (states
                    .get(record.order_id.trim())
                    .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                    == Some(queue_state::ApparatusQueueOrderState::Paused))
                .then(|| apparatus.clone())
            })
        });
    let Some(target_apparatus) = target_apparatus else {
        return Ok(None);
    };
    let storage_key = queue_state::resolve_apparatus_storage_key(&target_apparatus, &known_keys);
    let canonical = service
        .resolve_canonical_apparatus_text(&storage_key)
        .await?;
    let mut parsed = all_states
        .get(&storage_key)
        .or_else(|| all_states.get(&target_apparatus))
        .map(parsed_queue_states)
        .unwrap_or_default();
    let from_state = parsed
        .get(record.order_id.trim())
        .copied()
        .unwrap_or(queue_state::ApparatusQueueOrderState::Paused);
    let requeued_paused_session = from_state == queue_state::ApparatusQueueOrderState::Pending
        && target_session.is_some_and(|session| {
            matches!(
                session.status,
                OrderRunStatus::Paused | OrderRunStatus::RollDetached
            )
        });
    if !matches!(
        from_state,
        queue_state::ApparatusQueueOrderState::Paused
            | queue_state::ApparatusQueueOrderState::Frozen
    ) && !requeued_paused_session
    {
        return Ok(None);
    }
    parsed.insert(
        record.order_id.clone(),
        queue_state::ApparatusQueueOrderState::Frozen,
    );
    let visible_order_ids = visible_order_ids_for_apparatus(&maps, &target_apparatus);
    let stored_sequence = sequences
        .get(&storage_key)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut excluded_order_ids = order_controls
        .iter()
        .filter_map(|(id, control)| {
            (control.state == OrderControlState::Frozen).then_some(id.clone())
        })
        .collect::<BTreeSet<_>>();
    excluded_order_ids.insert(record.order_id.clone());
    let sequence = queue_state::effective_apparatus_sequence_excluding(
        stored_sequence,
        &visible_order_ids,
        &excluded_order_ids,
    );
    let sequence_updates =
        sequence_updates_for_frozen_transition(&maps, &sequences, &excluded_order_ids, None);
    let policy = effective_apparatus_queue_policy(canonical.as_ref());
    let actor = record.actor.clone();
    let mut event = queue_action_event(QueueActionEventInput {
        requested_apparatus: &target_apparatus,
        storage_key: &storage_key,
        order_id: &record.order_id,
        stage_node_id: target_session
            .map(|session| session.stage_node_id.trim())
            .unwrap_or_default(),
        action: queue_state::ApparatusQueueAction::Freeze,
        from_state,
        to_state: queue_state::ApparatusQueueOrderState::Frozen,
        policy,
        actor: &actor,
        assigned_apparatus: &[],
        sequence: &sequence,
        visible_order_ids: &visible_order_ids,
    });
    event.event_id = queue_action_event_id(
        &storage_key,
        &record.order_id,
        queue_state::ApparatusQueueAction::Freeze,
    );
    event.payload_json["admin_freeze"] = serde_json::json!(true);
    event.payload_json["order_control_state"] = serde_json::json!(record.state.as_str());
    let session = target_session.map(|session| {
        let mut payload_json = session.payload_json.clone();
        if !payload_json.is_object() {
            payload_json = serde_json::json!({});
        }
        payload_json["frozen_order"] = serde_json::json!(true);
        payload_json["admin_freeze"] = serde_json::json!(true);
        OrderRunSession {
            status: OrderRunStatus::Frozen,
            updated_at_unix: unix_seconds(),
            payload_json,
            ..session.clone()
        }
    });
    Ok(Some(QueueActionProgressWrite {
        apparatus: storage_key,
        map_update: None,
        states: serialized_queue_states(parsed),
        sequence_updates,
        event,
        session,
        progress_event: None,
        progress_batch: None,
        progress_batches: Vec::new(),
        progress_batch_updates: Vec::new(),
        opening_wip_batch_updates: Vec::new(),
        raw_material_stock_transitions: Vec::new(),
        qolip_checkouts: Vec::new(),
        returned_paint_report: None,
        order_control_update: Some(record.clone()),
        schedule_reservation_status: Some(ApparatusScheduleStatus::Paused),
    }))
}

async fn restore_frozen_queue_after_unfreeze(
    service: &ProductionMapService,
    record: &OrderControlRecord,
) -> Result<(), ProductionMapError> {
    let all_states = service.store.apparatus_queue_states().await?;
    let sequences = service.store.apparatus_sequences().await?;
    let sessions = service
        .store
        .order_run_sessions_for_order(&record.order_id)
        .await?;
    let known_keys = known_apparatus_storage_keys(&sequences, &all_states);
    let maps = service.store.maps().await?;
    let target_apparatus = record
        .freeze_request
        .as_ref()
        .map(|request| request.target_apparatus.trim())
        .filter(|apparatus| !apparatus.is_empty())
        .map(str::to_string)
        .or_else(|| {
            all_states.iter().find_map(|(apparatus, states)| {
                (states
                    .get(record.order_id.trim())
                    .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                    == Some(queue_state::ApparatusQueueOrderState::Frozen))
                .then(|| apparatus.clone())
            })
        })
        .or_else(|| {
            sessions.iter().find_map(|session| {
                (session.status == OrderRunStatus::Frozen).then(|| session.apparatus.clone())
            })
        })
        .ok_or(ProductionMapError::OrderFreezeTargetNotFound)?;
    let storage_key = queue_state::resolve_apparatus_storage_key(&target_apparatus, &known_keys);
    let canonical = service
        .resolve_canonical_apparatus_text(&storage_key)
        .await?;
    let mut parsed = all_states
        .get(&storage_key)
        .or_else(|| all_states.get(&target_apparatus))
        .map(parsed_queue_states)
        .unwrap_or_default();
    let from_state = parsed
        .get(record.order_id.trim())
        .copied()
        .unwrap_or(queue_state::ApparatusQueueOrderState::Frozen);
    let requested_session = record.freeze_request.as_ref().and_then(|request| {
        let target_session_id = request.target_session_id.trim();
        if target_session_id.is_empty() {
            return None;
        }
        sessions.iter().find(|session| {
            session.session_id.trim() == target_session_id
                && queue_state::apparatus_ids_match(&session.apparatus, &storage_key)
        })
    });
    let already_requeued_recovery = from_state == queue_state::ApparatusQueueOrderState::Pending
        && requested_session.is_some_and(|session| session.status == OrderRunStatus::Paused);
    if from_state != queue_state::ApparatusQueueOrderState::Frozen && !already_requeued_recovery {
        return Err(ProductionMapError::OrderControlActionNotAllowed);
    }
    parsed.insert(
        record.order_id.clone(),
        queue_state::ApparatusQueueOrderState::Pending,
    );

    let visible_order_ids = visible_order_ids_for_apparatus(&maps, &target_apparatus);
    let stored_sequence = sequences
        .get(&storage_key)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let frozen_order_ids = service
        .store
        .order_control_states()
        .await?
        .into_iter()
        .filter_map(|(id, control)| {
            (control.state == OrderControlState::Frozen && id != record.order_id).then_some(id)
        })
        .collect::<BTreeSet<_>>();
    let sequence = queue_state::effective_apparatus_sequence_excluding(
        stored_sequence,
        &visible_order_ids,
        &frozen_order_ids,
    );
    let sequence_updates = sequence_updates_for_frozen_transition(
        &maps,
        &sequences,
        &frozen_order_ids,
        Some(&record.order_id),
    );
    let policy = effective_apparatus_queue_policy(canonical.as_ref());
    let actor = record.actor.clone();
    let mut event = queue_action_event(QueueActionEventInput {
        requested_apparatus: &target_apparatus,
        storage_key: &storage_key,
        order_id: &record.order_id,
        stage_node_id: requested_session
            .map(|session| session.stage_node_id.trim())
            .unwrap_or_default(),
        action: queue_state::ApparatusQueueAction::Pause,
        from_state,
        to_state: queue_state::ApparatusQueueOrderState::Pending,
        policy,
        actor: &actor,
        assigned_apparatus: &[],
        sequence: &sequence,
        visible_order_ids: &visible_order_ids,
    });
    event.payload_json["admin_unfreeze"] = serde_json::json!(true);
    event.payload_json["requeued_at_tail"] = serde_json::json!(true);
    event.payload_json["order_control_state"] = serde_json::json!(record.state.as_str());
    if already_requeued_recovery {
        event.payload_json["recovered_control_only_freeze"] = serde_json::json!(true);
    }

    let session = requested_session
        .filter(|session| {
            session.status == OrderRunStatus::Frozen
                || (already_requeued_recovery && session.status == OrderRunStatus::Paused)
        })
        .or_else(|| {
            if already_requeued_recovery {
                None
            } else {
                sessions.iter().find(|session| {
                    session.status == OrderRunStatus::Frozen
                        && queue_state::apparatus_ids_match(&session.apparatus, &storage_key)
                })
            }
        })
        .map(unfrozen_order_run_session);

    let write = QueueActionProgressWrite {
        apparatus: storage_key,
        map_update: None,
        states: serialized_queue_states(parsed),
        sequence_updates,
        event,
        session,
        progress_event: None,
        progress_batch: None,
        progress_batches: Vec::new(),
        progress_batch_updates: Vec::new(),
        opening_wip_batch_updates: Vec::new(),
        raw_material_stock_transitions: Vec::new(),
        qolip_checkouts: Vec::new(),
        returned_paint_report: None,
        order_control_update: Some(record.clone()),
        schedule_reservation_status: Some(ApparatusScheduleStatus::Paused),
    };
    service
        .store
        .put_apparatus_queue_states_with_event_and_progress(&write)
        .await?;
    Ok(())
}

fn unfrozen_order_run_session(session: &OrderRunSession) -> OrderRunSession {
    let mut payload_json = session.payload_json.clone();
    if !payload_json.is_object() {
        payload_json = serde_json::json!({});
    }
    let payload = payload_json
        .as_object_mut()
        .expect("object payload initialized above");
    payload.remove("frozen_order");
    payload.remove("admin_freeze");
    payload.insert("requeued_at_tail".to_string(), serde_json::json!(true));
    payload.insert(
        "unfrozen_at_unix".to_string(),
        serde_json::json!(unix_seconds()),
    );
    OrderRunSession {
        status: OrderRunStatus::Paused,
        updated_at_unix: unix_seconds(),
        payload_json,
        ..session.clone()
    }
}
