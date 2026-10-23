//! Holds on a physical route occurrence never change the order lifecycle.
use super::*;

impl ProductionMapService {
    pub async fn stage_order_control_states(
        &self,
    ) -> Result<Vec<OrderControlRecord>, ProductionMapError> {
        self.store.stage_order_control_states().await
    }

    pub async fn request_stage_freeze(
        &self,
        order_id: &str,
        apparatus: &str,
        stage_node_id: &str,
        actor: QueueActionActor,
    ) -> Result<OrderControlRecord, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let order_id = required_existing_order_id(self, order_id).await?;
        let global = current_order_control(self, &order_id).await?;
        if global.state != OrderControlState::Active || global.early_close.is_some() {
            return Err(ProductionMapError::OrderControlActionNotAllowed);
        }
        if self
            .production_order_lifecycle(&order_id)
            .await?
            .status
            .is_terminal_for_material_assignment()
        {
            return Err(ProductionMapError::OrderAlreadyCompleted);
        }
        let canonical = self.resolve_canonical_apparatus_text(apparatus).await?;
        let apparatus = canonical.runtime.apparatus_id.as_str();
        let stage_node_id = self
            .resolve_hold_stage(&order_id, apparatus, stage_node_id)
            .await?;
        if self
            .store
            .stage_order_control_by_id(&order_id, apparatus, &stage_node_id)
            .await?
            .is_some_and(|c| c.state != OrderControlState::Active)
        {
            return Err(ProductionMapError::OrderControlActionNotAllowed);
        }
        let sessions = self.store.order_run_sessions_for_order(&order_id).await?;
        let target_session = sessions.iter().find(|s| {
            s.apparatus == apparatus && s.stage_node_id == stage_node_id && s.status.is_open()
        });
        let holds = self.store.active_print_preflight_holds().await?;
        let target_preflight = holds.iter().find(|h| {
            h.order_id == order_id && h.apparatus == apparatus && h.stage_node_id == stage_node_id
        });
        let running = target_session.is_some_and(|s| s.status == OrderRunStatus::Active)
            || target_preflight.is_some_and(|h| {
                matches!(
                    h.status,
                    PrintPreflightStatus::Held | PrintPreflightStatus::Running
                )
            });
        let now = unix_seconds();
        let record = OrderControlRecord {
            order_id,
            stage_node_id: stage_node_id.clone(),
            actor: actor.clone(),
            state: if running {
                OrderControlState::FreezeRequested
            } else {
                OrderControlState::Frozen
            },
            requested_at_unix: now,
            frozen_at_unix: (!running).then_some(now),
            early_close: None,
            freeze_request: Some(OrderFreezeRequest {
                scope: "stage".to_string(),
                stage_node_id: stage_node_id.clone(),
                request_id: new_freeze_request_id(),
                status: if running {
                    OrderFreezeRequestStatus::Pending
                } else {
                    OrderFreezeRequestStatus::Frozen
                },
                target_session_id: target_session
                    .map(|s| s.session_id.clone())
                    .unwrap_or_default(),
                target_apparatus: apparatus.to_string(),
                target_worker_role: target_session
                    .map(|s| s.worker_role.clone())
                    .or_else(|| target_preflight.map(|h| h.actor.role.clone()))
                    .unwrap_or_default(),
                target_worker_ref: target_session
                    .map(|s| s.worker_ref.clone())
                    .or_else(|| target_preflight.map(|h| h.actor.ref_.clone()))
                    .unwrap_or_default(),
                target_worker_display_name: target_session
                    .map(|s| s.worker_display_name.clone())
                    .or_else(|| target_preflight.map(|h| h.actor.display_name.clone()))
                    .unwrap_or_default(),
                requested_at_unix: now,
                transitioned_at_unix: now,
            }),
        };
        let shared_apparatus_occurrence = target_session.is_none()
            && sessions
                .iter()
                .any(|s| s.apparatus == apparatus && s.stage_node_id != record.stage_node_id);
        if running || shared_apparatus_occurrence {
            self.store.put_order_control_state(record.clone()).await?;
        } else if let Some(write) =
            prepare_direct_freeze_queue_write(self, &record, target_session, target_preflight)
                .await?
        {
            self.store
                .put_apparatus_queue_states_with_event_and_progress(&write)
                .await?;
        } else {
            return Err(ProductionMapError::OrderControlActionNotAllowed);
        }
        self.notify_live();
        Ok(record)
    }

    pub async fn cancel_stage_freeze_request(
        &self,
        order_id: &str,
        apparatus: &str,
        stage_node_id: &str,
        actor: QueueActionActor,
    ) -> Result<OrderControlRecord, ProductionMapError> {
        self.transition_stage_control(
            order_id,
            apparatus,
            stage_node_id,
            actor,
            OrderControlState::FreezeRequested,
        )
        .await
    }

    pub async fn unfreeze_stage(
        &self,
        order_id: &str,
        apparatus: &str,
        stage_node_id: &str,
        actor: QueueActionActor,
    ) -> Result<OrderControlRecord, ProductionMapError> {
        self.transition_stage_control(
            order_id,
            apparatus,
            stage_node_id,
            actor,
            OrderControlState::Frozen,
        )
        .await
    }

    async fn transition_stage_control(
        &self,
        order_id: &str,
        apparatus: &str,
        stage_node_id: &str,
        actor: QueueActionActor,
        expected: OrderControlState,
    ) -> Result<OrderControlRecord, ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let order_id = required_existing_order_id(self, order_id).await?;
        let global = current_order_control(self, &order_id).await?;
        if global.state != OrderControlState::Active || global.early_close.is_some() {
            return Err(ProductionMapError::OrderControlActionNotAllowed);
        }
        let canonical = self.resolve_canonical_apparatus_text(apparatus).await?;
        let apparatus = canonical.runtime.apparatus_id.as_str();
        let stage = self
            .resolve_hold_stage(&order_id, apparatus, stage_node_id)
            .await?;
        let mut record = self
            .store
            .stage_order_control_by_id(&order_id, apparatus, &stage)
            .await?
            .filter(|c| c.state == expected)
            .ok_or(ProductionMapError::OrderControlActionNotAllowed)?;
        let request = record
            .freeze_request
            .as_mut()
            .ok_or(ProductionMapError::OrderFreezeRequestMismatch)?;
        request.status = if expected == OrderControlState::Frozen {
            OrderFreezeRequestStatus::Unfrozen
        } else {
            OrderFreezeRequestStatus::Cancelled
        };
        request.transitioned_at_unix = unix_seconds();
        record.state = OrderControlState::Active;
        record.actor = actor;
        record.frozen_at_unix = None;
        let queue_frozen = self
            .store
            .apparatus_queue_states()
            .await?
            .get(apparatus)
            .and_then(|s| s.get(&order_id))
            .is_some_and(|s| s == "frozen");
        let owns_frozen_session = self
            .store
            .order_run_sessions_for_order(&order_id)
            .await?
            .iter()
            .any(|s| {
                s.apparatus == apparatus
                    && s.stage_node_id == record.stage_node_id
                    && s.status == OrderRunStatus::Frozen
            });
        let another_occurrence_open = self
            .store
            .order_run_sessions_for_order(&order_id)
            .await?
            .iter()
            .any(|s| {
                s.apparatus == apparatus
                    && s.stage_node_id != record.stage_node_id
                    && s.status.is_open()
            });
        if expected == OrderControlState::Frozen
            && (owns_frozen_session || (queue_frozen && !another_occurrence_open))
        {
            restore_frozen_queue_after_unfreeze(self, &record).await?;
        } else {
            self.store.put_order_control_state(record.clone()).await?;
        }
        self.notify_live();
        Ok(record)
    }

    pub(crate) async fn resolve_hold_stage(
        &self,
        order_id: &str,
        apparatus: &str,
        stage_node_id: &str,
    ) -> Result<String, ProductionMapError> {
        let map = self
            .store
            .map_by_id(order_id)
            .await?
            .ok_or(ProductionMapError::MapNotFound)?;
        let matches = super::super::chain::linear_work_stages(&map)
            .into_iter()
            .filter(|s| {
                s.apparatus_id.as_deref() == Some(apparatus.trim())
                    && (stage_node_id.trim().is_empty() || s.node_id == stage_node_id.trim())
            })
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [stage] => Ok(stage.node_id.clone()),
            [] => Err(ProductionMapError::OrderFreezeTargetNotFound),
            _ => Err(ProductionMapError::OrderFreezeTargetAmbiguous),
        }
    }

    pub(crate) async fn effective_order_control_for_apparatus(
        &self,
        order_id: &str,
        apparatus: &str,
    ) -> Result<OrderControlRecord, ProductionMapError> {
        let session = self
            .store
            .active_order_run_session(apparatus, order_id)
            .await?;
        let stage = if let Some(session) = session {
            session.stage_node_id
        } else {
            let map = self
                .store
                .map_by_id(order_id)
                .await?
                .ok_or(ProductionMapError::MapNotFound)?;
            super::super::chain::work_stage_for_station(&map, apparatus, "")
                .map(|s| s.node_id.clone())
                .ok_or(ProductionMapError::OrderFreezeTargetNotFound)?
        };
        self.effective_order_control_for_stage(order_id, apparatus, &stage)
            .await
    }

    /// The order overlay always wins. Local holds apply only to this occurrence.
    pub(crate) async fn effective_order_control_for_stage(
        &self,
        order_id: &str,
        apparatus: &str,
        stage_node_id: &str,
    ) -> Result<OrderControlRecord, ProductionMapError> {
        let global = self.order_control_state(order_id).await?;
        if global.state != OrderControlState::Active || global.early_close.is_some() {
            return Ok(global);
        }
        let map = self
            .store
            .map_by_id(order_id)
            .await?
            .ok_or(ProductionMapError::MapNotFound)?;
        let stage = super::super::chain::work_stage_for_station(&map, apparatus, stage_node_id)
            .ok_or(ProductionMapError::OrderFreezeTargetNotFound)?;
        Ok(self
            .store
            .stage_order_control_by_id(order_id, apparatus, &stage.node_id)
            .await?
            .filter(|c| c.state != OrderControlState::Active)
            .unwrap_or(global))
    }
}
