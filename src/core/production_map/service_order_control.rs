use std::collections::{BTreeMap, BTreeSet};

use super::progress::unix_seconds;
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
        self.store.put_order_control_state(record.clone()).await?;
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
        let mut freeze_request = current
            .freeze_request
            .ok_or(ProductionMapError::OrderControlActionNotAllowed)?;
        let now = unix_seconds();
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
        self.store.put_order_control_state(record.clone()).await?;
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
        .filter(|session| session.status == OrderRunStatus::Paused)
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
