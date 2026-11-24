#[path = "scoped.rs"]
mod scoped;

use std::collections::{BTreeMap, BTreeSet};

use super::apparatus::visible_order_ids_for_apparatus;
use super::progress::{effective_apparatus_queue_policy, queue_action_event_id, unix_seconds};
use super::service_queue_support::{
    QueueActionEventInput,
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
            .order_control_by_id(&order_id)
            .await?
            .unwrap_or_else(|| OrderControlRecord::active(&order_id)))
    }

    pub async fn request_order_freeze(
        &self,
        order_id: &str,
        actor: QueueActionActor,
    ) -> Result<OrderControlRecord, ProductionMapError> {
        self.request_order_stop(order_id, actor, None).await
    }

    pub async fn request_order_early_close(
        &self,
        order_id: &str,
        actor: QueueActionActor,
        comment: &str,
    ) -> Result<OrderControlRecord, ProductionMapError> {
        let comment = comment.trim();
        if comment.is_empty() || comment.chars().count() > 2000 {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        self.request_order_stop(order_id, actor, Some(comment.to_string())).await
    }

    async fn request_order_stop(
        &self,
        order_id: &str,
        actor: QueueActionActor,
        comment: Option<String>,
    ) -> Result<OrderControlRecord, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let order_id = required_existing_order_id(self, order_id).await?;
        let mut current = current_order_control(self, &order_id).await?;
        if let Some(close) = &current.early_close {
            return if comment.as_deref() == Some(close.comment.as_str()) {
                Ok(current)
            } else {
                Err(ProductionMapError::OrderControlActionNotAllowed)
            };
        }
        if self.production_order_lifecycle(&order_id).await?.status.is_terminal_for_material_assignment() {
            return Err(ProductionMapError::OrderAlreadyCompleted);
        }
        // A pending freeze can be upgraded without changing its worker/request identity.
        // An already frozen order has no running roll to wait for.
        if comment.is_some() && current.state != OrderControlState::Active {
            let now = unix_seconds();
            current.early_close = Some(OrderEarlyClose {
                comment: comment.unwrap(), actor, requested_at_unix: now,
                closed_at_unix: (current.state == OrderControlState::Frozen).then_some(now),
            });
            self.store.put_order_control_state(current.clone()).await?;
            self.notify_live();
            return Ok(current);
        }
        if current.state != OrderControlState::Active {
            return Err(ProductionMapError::OrderControlActionNotAllowed);
        }
        let queue_states = self.store.apparatus_queue_states().await?;
        let stage_controls = self.store.stage_order_control_states().await?;
        if queue_states.iter().any(|(apparatus, states)|
            states.get(&order_id).is_some_and(|state| state == "frozen")
                && !stage_controls.iter().any(|control| control.order_id == order_id
                    && control.state == OrderControlState::Frozen
                    && control.freeze_request.as_ref().is_some_and(|request| request.target_apparatus == *apparatus))) {
            // Queue-only records from before order controls existed retain
            // their original whole-order hold meaning.
            return Err(ProductionMapError::OrderFrozen);
        }

        let evidence = order_flow_evidence(self, &order_id).await?;
        if evidence.completed {
            return Err(ProductionMapError::OrderAlreadyCompleted);
        }
        // An untouched order has no running roll to wait for and can be held
        // immediately. Started operations retain the safe-stop protocol.

        let now = unix_seconds();
        let mut state = if evidence.has_active_work {
            OrderControlState::FreezeRequested
        } else {
            OrderControlState::Frozen
        };
        let print_preflight_targets = evidence
            .print_preflight_holds
            .iter()
            .filter(|hold| {
                queue_states.iter().any(|(apparatus, states)| {
                    queue_state::apparatus_ids_match(apparatus, &hold.apparatus)
                        && states
                            .get(&order_id)
                            .and_then(|state| {
                                queue_state::ApparatusQueueOrderState::parse(state)
                            })
                            == Some(queue_state::ApparatusQueueOrderState::PrintPreflight)
                })
            })
            .collect::<Vec<_>>();
        let passed_preflights = print_preflight_targets.iter().copied()
            .filter(|hold| hold.status == PrintPreflightStatus::Passed).collect::<Vec<_>>();
        if !passed_preflights.is_empty()
            && (print_preflight_targets.len() > 1 || !evidence.paused_sessions.is_empty()) {
            state = OrderControlState::FreezeRequested;
        }
        let target_session = if state == OrderControlState::FreezeRequested {
            match (evidence.active_sessions.as_slice(), print_preflight_targets.as_slice()) {
                ([session], []) => Some(session),
                ([], [_]) => None,
                ([], []) => return Err(ProductionMapError::OrderFreezeTargetNotFound),
                _ => None, // One global request safely stops every active occurrence.
            }
        } else {
            match evidence.paused_sessions.as_slice() {
                [session] => Some(session),
                _ => None,
            }
        };
        let target_preflight = if target_session.is_none()
            && (state != OrderControlState::FreezeRequested || evidence.active_sessions.is_empty()) {
            match print_preflight_targets.as_slice() {
                [hold] => Some(*hold),
                [] => None,
                _ => None,
            }
        } else {
            None
        };
        let target_worker = target_session
            .map(|session| QueueActionActor {
                role: session.worker_role.clone(),
                ref_: session.worker_ref.clone(),
                display_name: session.worker_display_name.clone(),
            })
            .or_else(|| target_preflight.map(|hold| hold.actor.clone()));
        let request_status = if state == OrderControlState::FreezeRequested {
            OrderFreezeRequestStatus::Pending
        } else {
            OrderFreezeRequestStatus::Frozen
        };
        let freeze_request = OrderFreezeRequest {
            scope: "order".to_string(),
            stage_node_id: String::new(),
            request_id: new_freeze_request_id(),
            status: request_status,
            target_session_id: target_session
                .map(|session| session.session_id.trim().to_string())
                .unwrap_or_default(),
            target_apparatus: target_session
                .map(|session| session.apparatus.trim().to_string())
                .or_else(|| {
                    target_preflight.map(|hold| hold.apparatus.trim().to_string())
                })
                .unwrap_or_default(),
            target_worker_role: target_worker
                .as_ref()
                .map(|worker| worker.role.trim().to_string())
                .unwrap_or_else(|| if state == OrderControlState::FreezeRequested { "aparatchi".to_string() } else { String::new() }),
            target_worker_ref: target_worker
                .as_ref()
                .map(|worker| worker.ref_.trim().to_string())
                .unwrap_or_default(),
            target_worker_display_name: target_worker
                .as_ref()
                .map(|worker| worker.display_name.trim().to_string())
                .unwrap_or_default(),
            requested_at_unix: now,
            transitioned_at_unix: now,
        };
        let early_close_without_work = comment.is_some() && !evidence.started;
        let mut record = OrderControlRecord {
            stage_node_id: String::new(),
            order_id,
            state,
            early_close: comment.map(|comment| OrderEarlyClose {
                comment, actor: actor.clone(), requested_at_unix: now,
                closed_at_unix: (state == OrderControlState::Frozen).then_some(now),
            }),
            actor,
            requested_at_unix: now,
            frozen_at_unix: (state == OrderControlState::Frozen).then_some(now),
            freeze_request: (!early_close_without_work).then_some(freeze_request),
        };
        if state == OrderControlState::FreezeRequested && !passed_preflights.is_empty() {
            // Passed colour trials have no running roll to acknowledge. Stop
            // them directly while the global request blocks every new start.
            self.store.put_order_control_state(record.clone()).await?;
            for hold in passed_preflights {
                let write = prepare_direct_freeze_queue_write(self, &record, None, Some(hold)).await?
                    .ok_or(ProductionMapError::OrderFreezeTargetNotFound)?;
                self.store.put_apparatus_queue_states_with_event_and_progress(&write).await?;
            }
            let durable = current_order_control(self, &record.order_id).await?;
            if durable.state == OrderControlState::Frozen {
                // PostgreSQL may finalize the wildcard request atomically with
                // its last preflight cancellation. Preserve that close time and
                // leave already-paused work safely behind the frozen overlay.
                self.notify_live();
                return Ok(durable);
            }
            for session in &evidence.paused_sessions {
                if let Some(write) = prepare_direct_freeze_queue_write(self, &record, Some(session), None).await? {
                    self.store.put_apparatus_queue_states_with_event_and_progress(&write).await?;
                }
            }
            if !evidence.has_active_work {
                record.state = OrderControlState::Frozen;
                record.frozen_at_unix = Some(now);
                if let Some(request) = record.freeze_request.as_mut() {
                    request.status = OrderFreezeRequestStatus::Frozen;
                }
                if let Some(close) = record.early_close.as_mut() { close.closed_at_unix = Some(now); }
                self.store.put_order_control_state(record.clone()).await?;
            }
            self.notify_live();
            return current_order_control(self, &record.order_id).await;
        }
        if let Some(write) =
            prepare_direct_freeze_queue_write(self, &record, target_session, target_preflight)
                .await?
        {
            self.store
                .put_apparatus_queue_states_with_event_and_progress(&write)
                .await?;
        } else if state == OrderControlState::FreezeRequested {
            self.store.put_order_control_state(record.clone()).await?;
        } else if !evidence.has_active_work {
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
        if current.state != expected || current.early_close.is_some() {
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
            stage_node_id: String::new(),
            order_id,
            state: next,
            actor,
            requested_at_unix: freeze_request.requested_at_unix,
            frozen_at_unix: None,
            freeze_request: Some(freeze_request),
            early_close: None,
        };
        // A multi-stage request may already have safely stopped some machines
        // when it is cancelled. Restore those without releasing local holds.
        if next == OrderControlState::Active {
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
        let map = self
            .store
            .map_by_id(&order_id)
            .await?
            .ok_or(ProductionMapError::MapNotFound)?;
        // A downstream queue head still waits for preceding work. Only the
        // initial physical stage (including its alternatives) locks deletion
        // by queue position; work history and material guards remain global.
        let initial_apparatuses = super::chain::linear_work_stages(&map)
            .into_iter()
            .filter(|stage| {
                super::chain::previous_work_stages_for_node(&map, &stage.node_id).is_empty()
            })
            .filter_map(|stage| stage.apparatus_id)
            .collect::<BTreeSet<_>>();
        let mut blockers = Vec::new();

        if current_order_control(self, &order_id).await?.early_close.is_some() {
            blockers.push(OrderDeleteBlocker::new(
                "early_close_history",
                "Erta yopilgan buyurtmaning sababi va tarixi saqlanishi shart; uni o‘chirib bo‘lmaydi",
            ));
        }

        if self
            .store
            .active_print_preflight_holds()
            .await?
            .into_iter()
            .any(|hold| hold.order_id.trim() == order_id)
        {
            blockers.push(OrderDeleteBlocker::new(
                "print_preflight_active",
                "Buyurtmada rang chiqarish davom etmoqda; avval rang sinovi natijasini belgilang",
            ));
        }

        for (apparatus, sequence) in self.effective_apparatus_sequences().await? {
            if initial_apparatuses.contains(&apparatus)
                && sequence
                    .first()
                    .is_some_and(|first| first.trim() == order_id)
            {
                let apparatus_name = deletion_apparatus_name(self, &apparatus).await;
                blockers.push(OrderDeleteBlocker::new(
                    "first_in_sequence",
                    format!("Buyurtma «{apparatus_name}» navbatida 1-o‘rinda turibdi"),
                ));
            }
        }

        let evidence = order_flow_evidence(self, &order_id).await?;
        if evidence.started {
            let mut stages = Vec::new();
            for apparatus in evidence.started_apparatuses {
                stages.push(deletion_apparatus_name(self, &apparatus).await);
            }
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

        if !self
            .store
            .opening_wip_records(OpeningWipQuery {
                order_id: order_id.clone(),
                limit: 1,
                ..OpeningWipQuery::default()
            })
            .await?
            .is_empty()
        {
            blockers.push(OrderDeleteBlocker::new(
                "opening_wip_exists",
                "Buyurtmaga Opening WIP ochilgan",
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

// Labels are presentation only: missing/retired catalog entries must neither
// expose internal IDs nor weaken the work-history and queue deletion guards.
async fn deletion_apparatus_name(service: &ProductionMapService, value: &str) -> String {
    if let Ok(id) = crate::core::apparatus_standard::ApparatusId::new(value.trim()) {
        if let Ok(Some(configuration)) = service.apparatus_resolver.resolve(&id).await {
            let name = configuration.runtime.display.display_name.trim();
            if configuration.runtime.apparatus_id == id
                && configuration.has_coherent_source()
                && !name.is_empty()
            {
                return name.to_string();
            }
        }
    }
    "Nomi aniqlanmagan apparat".to_string()
}

struct OrderFlowEvidence {
    started: bool,
    completed: bool,
    has_active_work: bool,
    started_apparatuses: BTreeSet<String>,
    active_sessions: Vec<OrderRunSession>,
    paused_sessions: Vec<OrderRunSession>,
    print_preflight_holds: Vec<PrintPreflightHold>,
}

include!("service_parts/required_existing_order_id.rs");

include!("service_parts/restore_one_frozen_queue.rs");
