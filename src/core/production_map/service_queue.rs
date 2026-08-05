use std::collections::{BTreeMap, BTreeSet};

use super::*;

use super::apparatus::{
    claim_unassigned_alternative_apparatus_assignment, visible_order_ids_by_apparatus,
    visible_order_ids_for_apparatus,
};
use super::chain;
use super::progress::{
    effective_apparatus_queue_policy_record, order_completed_on_apparatus,
    required_apparatus_for_closed_order,
};
use super::service::ClaimedAlternativeMapUpdate;
use super::service_progress_support::session_progress_links;
use super::service_queue_support::*;
use super::store_port::ApparatusQueueStateMap;

impl ProductionMapService {
    pub async fn apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        self.store.apparatus_sequences().await
    }

    pub async fn effective_apparatus_sequences(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        let maps = self.store.maps().await?;
        let sequences = self.store.apparatus_sequences().await?;
        let visible_by_apparatus = visible_order_ids_by_apparatus(&maps);
        let apparatuses = sequences
            .keys()
            .chain(visible_by_apparatus.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        Ok(apparatuses
            .into_iter()
            .map(|apparatus| {
                let stored_sequence = sequences.get(&apparatus).cloned().unwrap_or_default();
                let visible_order_ids = visible_by_apparatus
                    .get(&apparatus)
                    .cloned()
                    .unwrap_or_default();
                let sequence = if visible_order_ids.is_empty() {
                    stored_sequence
                } else {
                    queue_state::effective_apparatus_sequence(&stored_sequence, &visible_order_ids)
                };
                (apparatus, sequence)
            })
            .collect())
    }

    pub async fn visible_order_ids_by_apparatus(
        &self,
    ) -> Result<BTreeMap<String, Vec<String>>, ProductionMapError> {
        let maps = self.store.maps().await?;
        Ok(visible_order_ids_by_apparatus(&maps))
    }

    pub async fn set_apparatus_sequence(
        &self,
        apparatus: &str,
        order_ids: Vec<String>,
    ) -> Result<(), ProductionMapError> {
        let _guard = self.queue_action_guard().await;
        let apparatus = apparatus.trim();
        if apparatus.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let order_ids = order_ids
            .into_iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        let sequences = self.store.apparatus_sequences().await?;
        let all_states = self.store.apparatus_queue_states().await?;
        let known_keys = sequences
            .keys()
            .chain(all_states.keys())
            .map(|key| key.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|key| key.to_string())
            .collect::<Vec<_>>();
        let storage_key = queue_state::resolve_apparatus_storage_key(apparatus, &known_keys);
        let current_sequence = sequences
            .get(&storage_key)
            .or_else(|| sequences.get(apparatus))
            .cloned()
            .unwrap_or_default();
        let states = all_states
            .get(&storage_key)
            .or_else(|| all_states.get(apparatus))
            .cloned()
            .unwrap_or_default();
        let frozen_order_ids = self
            .store
            .order_control_states()
            .await?
            .into_iter()
            .filter_map(|(order_id, control)| {
                (control.state == OrderControlState::Frozen).then_some(order_id)
            })
            .collect::<BTreeSet<_>>();
        validate_active_sequence_barrier(
            &current_sequence,
            &order_ids,
            &states,
            &frozen_order_ids,
        )?;
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
        let mut completed_orders = self
            .store
            .completed_queue_orders_for_actor(actor_ref, limit)
            .await?;
        let maps = self.store.maps().await?;
        let queue_states = self.store.apparatus_queue_states().await?;
        let maps_by_id = maps
            .into_iter()
            .map(|map| (map.id.trim().to_string(), map))
            .collect::<BTreeMap<_, _>>();

        for completed_order in &mut completed_orders {
            if completed_order.status != CompletedQueueOrderStatus::Completed {
                continue;
            }
            let Some(map) = maps_by_id.get(completed_order.order_id.trim()) else {
                continue;
            };
            let required_apparatus = required_apparatus_for_closed_order(map);
            let order_is_fully_completed = !required_apparatus.is_empty()
                && required_apparatus.iter().all(|apparatus| {
                    order_completed_on_apparatus(
                        &queue_states,
                        completed_order.order_id.trim(),
                        apparatus,
                    )
                });
            if !order_is_fully_completed {
                completed_order.status = CompletedQueueOrderStatus::InProgress;
            }
        }
        Ok(completed_orders)
    }

    pub async fn queue_action_logs_for_order(
        &self,
        order_id: &str,
    ) -> Result<Vec<ProductionOrderLogEntry>, ProductionMapError> {
        let order_id = order_id.trim().to_string();
        if order_id.is_empty() {
            return Ok(Vec::new());
        }
        let logs_by_order = self
            .store
            .queue_action_logs_for_orders(std::slice::from_ref(&order_id))
            .await?;
        Ok(logs_by_order.get(&order_id).cloned().unwrap_or_default())
    }

    pub async fn active_order_run_sessions_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<OrderRunSession>, ProductionMapError> {
        self.store
            .active_order_run_sessions_for_worker(worker_refs, worker_display_name, limit)
            .await
    }

    pub async fn progress_batches_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<OrderProgressBatch>, ProductionMapError> {
        self.store
            .progress_batches_for_worker(worker_refs, worker_display_name, limit)
            .await
    }

    pub async fn order_status_detail(
        &self,
        order_id: &str,
    ) -> Result<ProductionOrderStatusDetail, ProductionMapError> {
        let order_id = order_id.trim();
        if order_id.is_empty() {
            return Err(ProductionMapError::MissingId);
        }
        let queue_states =
            queue_states_for_order(self.store.apparatus_queue_states().await?, order_id);
        let progress_batches = self.store.progress_batches_for_order(order_id).await?;
        let run_sessions = self.store.order_run_sessions_for_order(order_id).await?;
        let logs_by_order = self
            .store
            .queue_action_logs_for_orders(&[order_id.to_string()])
            .await?;
        let logs = logs_by_order.get(order_id).cloned().unwrap_or_default();
        Ok(ProductionOrderStatusDetail::from_order_flow(
            &progress_batches,
            &run_sessions,
            &queue_states,
            &logs,
        ))
    }

    pub async fn order_status_details(
        &self,
    ) -> Result<BTreeMap<String, ProductionOrderStatusDetail>, ProductionMapError> {
        let mut statuses = BTreeMap::new();
        for saved in self.maps().await? {
            let order_id = saved.map.id.trim();
            if order_id.is_empty() {
                continue;
            }
            statuses.insert(
                order_id.to_string(),
                self.order_status_detail(order_id).await?,
            );
        }
        Ok(statuses)
    }

    pub async fn queue_action_logs_for_worker(
        &self,
        worker_refs: &[String],
        worker_display_name: &str,
        limit: usize,
    ) -> Result<Vec<ProductionOrderLogEntry>, ProductionMapError> {
        self.store
            .queue_action_logs_for_worker(worker_refs, worker_display_name, limit)
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

    pub async fn queue_action_controls(
        &self,
    ) -> Result<
        BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
        ProductionMapError,
    > {
        let maps = self.store.maps().await?;
        let sequences = self.store.apparatus_sequences().await?;
        let all_states = self.store.apparatus_queue_states().await?;
        let policies = self.store.apparatus_queue_policies().await?;
        let order_controls = self.order_control_states().await?;
        let visible_by_apparatus = visible_order_ids_by_apparatus(&maps);
        let known_keys = sequences
            .keys()
            .chain(all_states.keys())
            .chain(policies.keys())
            .chain(visible_by_apparatus.keys())
            .map(|key| key.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let apparatuses = known_keys.iter().cloned().collect::<BTreeSet<_>>();
        let mut result = BTreeMap::new();

        for apparatus in apparatuses {
            let storage_key =
                queue_state::resolve_apparatus_storage_key(&apparatus, &known_keys);
            let stored_sequence = sequences
                .get(&storage_key)
                .or_else(|| sequences.get(&apparatus))
                .cloned()
                .unwrap_or_default();
            let visible_order_ids = visible_order_ids_for_apparatus(&maps, &storage_key);
            let sequence = queue_state::effective_apparatus_sequence(
                &stored_sequence,
                &visible_order_ids,
            );
            let stored_states = all_states
                .get(&storage_key)
                .or_else(|| all_states.get(&apparatus))
                .cloned()
                .unwrap_or_default();
            let mut effective_states = parsed_queue_states(stored_states);
            effective_states.retain(|order_id, _| {
                order_controls
                    .get(order_id)
                    .is_none_or(|control| control.state != OrderControlState::Frozen)
            });
            let policy = queue_policy_for_apparatus(&apparatus, &storage_key, &policies);
            let active_order_id = effective_states
                .iter()
                .find_map(|(order_id, state)| {
                    (*state == queue_state::ApparatusQueueOrderState::InProgress)
                        .then_some(order_id.as_str())
                });
            let actionable_order_id =
                queue_state::first_actionable_order_id(&sequence, &effective_states);
            let mut apparatus_controls = BTreeMap::new();

            for order_id in sequence {
                let Some(order_map) = maps.iter().find(|map| map.id.trim() == order_id.trim())
                else {
                    continue;
                };
                let state = effective_states
                    .get(order_id.trim())
                    .copied()
                    .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
                let control = order_controls
                    .get(order_id.trim())
                    .map(|control| control.state)
                    .unwrap_or(OrderControlState::Active);
                let previous_stage = chain::previous_work_stage_station(order_map, &apparatus);
                let previous_stage_ready = chain::order_ready_for_station(
                    order_map,
                    order_id.trim(),
                    &apparatus,
                    &all_states,
                    &known_keys,
                );
                let active_order_is_this = active_order_id
                    .is_none_or(|active_order_id| active_order_id == order_id.trim());
                let queue_actionable = state.is_active()
                    || actionable_order_id.as_deref() == Some(order_id.trim())
                    || (state == queue_state::ApparatusQueueOrderState::Pending
                        && previous_stage.is_some()
                        && previous_stage_ready
                        && active_order_is_this);
                let mut allowed_actions = Vec::new();
                let mut complete_requires_full_report = false;

                if queue_actionable {
                    match state {
                        queue_state::ApparatusQueueOrderState::Pending
                            if control == OrderControlState::Active
                                && (policy == ApparatusQueuePolicy::FreePick
                                    || active_order_is_this) => {
                            allowed_actions.push(queue_state::ApparatusQueueAction::Start);
                        }
                        queue_state::ApparatusQueueOrderState::InProgress => {
                            if matches!(
                                control,
                                OrderControlState::Active | OrderControlState::FreezeRequested
                            ) {
                                allowed_actions.push(queue_state::ApparatusQueueAction::Pause);
                            }
                            if control == OrderControlState::Active {
                                let has_unprocessed_previous_wips =
                                    self.has_unprocessed_previous_wips(
                                        order_id.trim(),
                                        order_map,
                                        &storage_key,
                                        &all_states,
                                        &[],
                                        "",
                                    )
                                    .await?;
                                if apparatus::is_rezka_title(&storage_key)
                                    && has_unprocessed_previous_wips
                                {
                                    allowed_actions
                                        .push(queue_state::ApparatusQueueAction::RollComplete);
                                } else {
                                    allowed_actions
                                        .push(queue_state::ApparatusQueueAction::Complete);
                                }
                                complete_requires_full_report =
                                    !apparatus::is_laminatsiya_title(&storage_key)
                                        || !has_unprocessed_previous_wips;
                            }
                        }
                        queue_state::ApparatusQueueOrderState::Paused
                            if control == OrderControlState::Active => {
                            allowed_actions.push(queue_state::ApparatusQueueAction::Resume);
                        }
                        queue_state::ApparatusQueueOrderState::Completed => {}
                        _ => {}
                    }
                }

                apparatus_controls.insert(
                    order_id.trim().to_string(),
                    ApparatusQueueOrderActionControl {
                        state,
                        allowed_actions,
                        previous_stage: previous_stage.unwrap_or_default(),
                        previous_stage_ready,
                        complete_requires_full_report,
                    },
                );
            }
            if !apparatus_controls.is_empty() {
                result.insert(storage_key, apparatus_controls);
            }
        }
        Ok(result)
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
}

include!("service_queue_execution.rs");
