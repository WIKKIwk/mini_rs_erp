use std::collections::{BTreeMap, BTreeSet};

use super::super::*;

use super::super::apparatus::{
    claim_unassigned_alternative_apparatus_assignment, visible_order_ids_by_apparatus,
    visible_order_ids_for_apparatus,
};
use super::super::chain;
use super::super::materials::build_raw_material_start_requirements;
use super::super::progress::effective_apparatus_queue_policy_record;
use super::super::service::{ClaimedAlternativeMapUpdate, QueueProgressRecords};
use super::super::service_progress_support::{session_progress_links, wip_batch_was_consumed_by_producer};
use super::super::service_queue_support::*;
use super::super::store_port::{ApparatusQueuePolicyMap, ApparatusQueueStateMap, OrderControlMap};

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
        let frozen_order_ids = self
            .store
            .order_control_states()
            .await?
            .into_iter()
            .filter_map(|(id, control)| (control.state == OrderControlState::Frozen).then_some(id))
            .collect::<BTreeSet<_>>();
        Ok(Self::effective_apparatus_sequences_for_maps(
            &maps,
            &sequences,
            &frozen_order_ids,
        ))
    }

    pub(in crate::core::production_map) fn effective_apparatus_sequences_for_maps(
        maps: &[ProductionMapDefinition],
        sequences: &BTreeMap<String, Vec<String>>,
        frozen_order_ids: &BTreeSet<String>,
    ) -> BTreeMap<String, Vec<String>> {
        let visible_by_apparatus = visible_order_ids_by_apparatus(maps);
        let apparatuses = sequences
            .keys()
            .chain(visible_by_apparatus.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        apparatuses
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
                    queue_state::effective_apparatus_sequence_excluding(
                        &stored_sequence,
                        &visible_order_ids,
                        frozen_order_ids,
                    )
                };
                (apparatus, sequence)
            })
            .collect()
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
        let maps = self.store.maps().await?;
        let known_order_ids = maps
            .iter()
            .map(|map| map.id.trim())
            .filter(|id| !id.is_empty())
            .collect::<BTreeSet<_>>();
        let visible_order_ids = visible_order_ids_for_apparatus(&maps, apparatus)
            .into_iter()
            .collect::<BTreeSet<_>>();
        for order_id in &order_ids {
            if !known_order_ids.contains(order_id.as_str()) {
                return Err(ProductionMapError::QueueSequenceOrderNotFound(
                    order_id.clone(),
                ));
            }
            if !visible_order_ids.contains(order_id) {
                return Err(ProductionMapError::QueueSequenceApparatusMismatch(
                    order_id.clone(),
                ));
            }
        }
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
        // This endpoint is the actor-scoped stage history consumed by the
        // worker "Tugatish" tab. Global order closure belongs to
        // `fully_completed_orders`, which is exposed separately through the
        // closed-orders endpoint. Do not project the global closure invariant
        // onto this worker history.
        self.store
            .completed_queue_orders_for_actor(actor_ref, limit)
            .await
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
            queue_states_for_order(&self.store.apparatus_queue_states().await?, order_id);
        let progress_batches = self.store.progress_batches_for_order(order_id).await?;
        let run_sessions = self.store.order_run_sessions_for_order(order_id).await?;
        let logs_by_order = self
            .store
            .queue_action_logs_for_orders(&[order_id.to_string()])
            .await?;
        let logs = logs_by_order.get(order_id).cloned().unwrap_or_default();
        let mut status = ProductionOrderStatusDetail::from_order_flow(
            &progress_batches,
            &run_sessions,
            &queue_states,
            &logs,
        );
        if self
            .store
            .order_control_states()
            .await?
            .get(order_id)
            .is_some_and(|control| control.state == OrderControlState::Frozen)
        {
            status.force_frozen();
        }
        Ok(status)
    }

    pub async fn order_status_details(
        &self,
    ) -> Result<BTreeMap<String, ProductionOrderStatusDetail>, ProductionMapError> {
        let maps = self.maps().await?;
        let queue_states = self.store.apparatus_queue_states().await?;
        let order_controls = self.store.order_control_states().await?;
        self.order_status_details_for_snapshot(&maps, &queue_states, &order_controls)
            .await
    }

    pub(in crate::core::production_map) async fn order_status_details_for_snapshot(
        &self,
        maps: &[ProductionMapSaved],
        queue_states: &ApparatusQueueStateMap,
        order_controls: &OrderControlMap,
    ) -> Result<BTreeMap<String, ProductionOrderStatusDetail>, ProductionMapError> {
        let order_ids = maps
            .iter()
            .filter_map(|saved| {
                let order_id = saved.map.id.trim();
                (!order_id.is_empty()).then(|| order_id.to_string())
            })
            .collect::<Vec<_>>();
        let progress_batches = self.store.progress_batches_for_orders(&order_ids).await?;
        let run_sessions = self.store.order_run_sessions_for_orders(&order_ids).await?;
        let logs_by_order = self.store.queue_action_logs_for_orders(&order_ids).await?;
        let mut statuses = BTreeMap::new();
        for order_id in order_ids {
            let order_queue_states = queue_states_for_order(queue_states, &order_id);
            let order_progress_batches =
                progress_batches.get(&order_id).cloned().unwrap_or_default();
            let order_run_sessions = run_sessions.get(&order_id).cloned().unwrap_or_default();
            let order_logs = logs_by_order.get(&order_id).cloned().unwrap_or_default();
            let mut status = ProductionOrderStatusDetail::from_order_flow(
                &order_progress_batches,
                &order_run_sessions,
                &order_queue_states,
                &order_logs,
            );
            if order_controls
                .get(&order_id)
                .is_some_and(|control| control.state == OrderControlState::Frozen)
            {
                status.force_frozen();
            }
            statuses.insert(order_id.to_string(), status);
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
        self.queue_action_controls_for_snapshot(
            &maps,
            &sequences,
            &all_states,
            &policies,
            &order_controls,
        )
        .await
    }

    pub(in crate::core::production_map) async fn queue_action_controls_for_snapshot(
        &self,
        maps: &[ProductionMapDefinition],
        sequences: &BTreeMap<String, Vec<String>>,
        all_states: &ApparatusQueueStateMap,
        policies: &ApparatusQueuePolicyMap,
        order_controls: &OrderControlMap,
    ) -> Result<
        BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
        ProductionMapError,
    > {
        let material_rules = self.store.apparatus_material_rules().await?;
        let material_assignments = self.store.raw_material_assignments().await?;
        let order_ids = maps
            .iter()
            .map(|map| map.id.trim())
            .filter(|order_id| !order_id.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        let progress_batches_by_order = self.store.progress_batches_for_orders(&order_ids).await?;
        let visible_by_apparatus = visible_order_ids_by_apparatus(maps);
        let frozen_order_ids = order_controls
            .iter()
            .filter_map(|(order_id, control)| {
                (control.state == OrderControlState::Frozen).then_some(order_id.clone())
            })
            .collect::<BTreeSet<_>>();
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
            let storage_key = queue_state::resolve_apparatus_storage_key(&apparatus, &known_keys);
            let stored_sequence = sequences
                .get(&storage_key)
                .or_else(|| sequences.get(&apparatus))
                .cloned()
                .unwrap_or_default();
            let visible_order_ids = visible_order_ids_for_apparatus(maps, &storage_key);
            let sequence = queue_state::effective_apparatus_sequence_excluding(
                &stored_sequence,
                &visible_order_ids,
                &frozen_order_ids,
            );
            let stored_states = all_states
                .get(&storage_key)
                .or_else(|| all_states.get(&apparatus))
                .cloned()
                .unwrap_or_default();
            let effective_states = parsed_queue_states(stored_states);
            let policy = queue_policy_for_apparatus(&apparatus, &storage_key, policies);
            let active_order_id = effective_states.iter().find_map(|(order_id, state)| {
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
                let order_control = order_controls.get(order_id.trim());
                let control = order_control
                    .map(|control| control.state)
                    .unwrap_or(OrderControlState::Active);
                let previous_stage = chain::previous_work_stage_station(order_map, &apparatus);
                let previous_stage_ready = chain::order_ready_for_station(
                    order_map,
                    order_id.trim(),
                    &apparatus,
                    all_states,
                    &known_keys,
                );
                let active_order_is_this = active_order_id
                    .is_none_or(|active_order_id| active_order_id == order_id.trim());
                let active_session = self
                    .store
                    .active_order_run_session(&storage_key, order_id.trim())
                    .await?;
                let requeued_session = active_session
                    .as_ref()
                    .is_some_and(order_run_session_was_requeued);
                let queue_actionable = state.is_active()
                    || actionable_order_id.as_deref() == Some(order_id.trim())
                    || (state == queue_state::ApparatusQueueOrderState::Pending
                        && previous_stage.is_some()
                        && previous_stage_ready
                        && active_order_is_this);
                let mut allowed_actions = Vec::new();
                let mut complete_requires_full_report = false;
                let mut interaction = ApparatusQueueWorkerInteraction {
                    assigned_materials_display_only: !matches!(
                        state,
                        queue_state::ApparatusQueueOrderState::InProgress
                            | queue_state::ApparatusQueueOrderState::Paused
                    ),
                    ..ApparatusQueueWorkerInteraction::default()
                };
                let pending_actionable = queue_actionable
                    && control == OrderControlState::Active
                    && (policy == ApparatusQueuePolicy::FreePick || active_order_is_this);

                match state {
                    queue_state::ApparatusQueueOrderState::Pending if requeued_session => {
                        if pending_actionable {
                            interaction.mode = ApparatusQueueInteractionMode::RequeuedReady;
                            allowed_actions.push(queue_state::ApparatusQueueAction::Resume);
                        } else {
                            interaction.mode = ApparatusQueueInteractionMode::RequeuedWaiting;
                            interaction.blocking_reason_code = "waiting_sequence".to_string();
                        }
                    }
                    queue_state::ApparatusQueueOrderState::Pending => {
                        let previous_wip_mode = previous_stage
                            .as_deref()
                            .map(|previous_stage| {
                                let batches = progress_batches_by_order
                                    .get(order_id.trim())
                                    .map(Vec::as_slice)
                                    .unwrap_or_default();
                                if has_waiting_previous_stage_wip(
                                    batches,
                                    order_id.trim(),
                                    previous_stage,
                                    &storage_key,
                                ) {
                                    ApparatusQueuePreviousWipMode::ScanRequired
                                } else {
                                    ApparatusQueuePreviousWipMode::Waiting
                                }
                            })
                            .unwrap_or(ApparatusQueuePreviousWipMode::NotRequired);
                        let assignments = material_assignments
                            .iter()
                            .filter(|assignment| {
                                assignment.order_id.trim() == order_id.trim()
                                    && queue_state::apparatus_titles_match(
                                        &assignment.apparatus,
                                        &storage_key,
                                    )
                            })
                            .cloned()
                            .collect::<Vec<_>>();
                        let rule = material_rules.iter().find(|rule| {
                            queue_state::apparatus_titles_match(&rule.apparatus, &storage_key)
                        });
                        let material_requirements =
                            build_raw_material_start_requirements(rule, &assignments, &[], "");
                        let material_scan_required = material_requirements.requires_material
                            || !material_requirements.assigned_barcodes.is_empty();
                        let start_materials_mode = if apparatus::is_laminatsiya_title(&storage_key)
                            && previous_wip_mode == ApparatusQueuePreviousWipMode::ScanRequired
                        {
                            ApparatusQueueStartMaterialsMode::Hidden
                        } else if material_scan_required {
                            ApparatusQueueStartMaterialsMode::ScanRequired
                        } else {
                            ApparatusQueueStartMaterialsMode::Hidden
                        };

                        if previous_wip_mode == ApparatusQueuePreviousWipMode::Waiting {
                            interaction.mode = ApparatusQueueInteractionMode::WaitingPreviousStage;
                            interaction.previous_wip_mode = previous_wip_mode;
                            interaction.blocking_reason_code = "waiting_previous_stage".to_string();
                        } else if !pending_actionable {
                            interaction.mode = ApparatusQueueInteractionMode::FreshStartBlocked;
                            interaction.blocking_reason_code = "waiting_sequence".to_string();
                        } else {
                            interaction.start_materials_mode = start_materials_mode;
                            interaction.material_scan_required = start_materials_mode
                                == ApparatusQueueStartMaterialsMode::ScanRequired;
                            interaction.assigned_materials_display_only = false;
                            interaction.previous_wip_mode = previous_wip_mode;
                            interaction.qolip_mode = if pechat::is_pechat_apparatus(&storage_key) {
                                ApparatusQueueQolipMode::ScanRequired
                            } else {
                                ApparatusQueueQolipMode::NotRequired
                            };
                            if material_requirements.assignments_satisfied {
                                interaction.mode = ApparatusQueueInteractionMode::FreshStart;
                                allowed_actions.push(queue_state::ApparatusQueueAction::Start);
                            } else {
                                interaction.mode = ApparatusQueueInteractionMode::FreshStartBlocked;
                                interaction.blocking_reason_code =
                                    "raw_material_assignment_required".to_string();
                            }
                        }
                    }
                    queue_state::ApparatusQueueOrderState::InProgress => {
                        interaction.mode = if control == OrderControlState::FreezeRequested {
                            ApparatusQueueInteractionMode::FreezeRequested
                        } else {
                            ApparatusQueueInteractionMode::InProgress
                        };
                        interaction.material_intake_allowed = control == OrderControlState::Active;
                        interaction.assigned_materials_display_only =
                            control != OrderControlState::Active;
                        if matches!(
                            control,
                            OrderControlState::Active | OrderControlState::FreezeRequested
                        ) {
                            allowed_actions.push(queue_state::ApparatusQueueAction::Pause);
                        }
                        if control == OrderControlState::FreezeRequested {
                            interaction.blocking_reason_code = "order_freeze_requested".to_string();
                        }
                        if control == OrderControlState::Active {
                            // A worker may finish with an issue note, which is
                            // persisted as the explicit frozen transition.
                            allowed_actions.push(queue_state::ApparatusQueueAction::Freeze);
                            let current_input_batch_id = self
                                .completion_input_batch_id(
                                    &storage_key,
                                    order_id.trim(),
                                    &QueueProgressInput::default(),
                                )
                                .await?;
                            let has_unprocessed_previous_wips = self
                                .has_unprocessed_previous_wips(
                                    order_id.trim(),
                                    order_map,
                                    &storage_key,
                                    &all_states,
                                    &[],
                                    &current_input_batch_id,
                                )
                                .await?;
                            if apparatus::is_rezka_title(&storage_key) {
                                allowed_actions
                                    .push(queue_state::ApparatusQueueAction::RollComplete);
                            }
                            allowed_actions.push(queue_state::ApparatusQueueAction::Complete);
                            complete_requires_full_report =
                                !(apparatus::is_laminatsiya_title(&storage_key)
                                    || apparatus::is_rezka_title(&storage_key))
                                    || !has_unprocessed_previous_wips;
                        }
                    }
                    queue_state::ApparatusQueueOrderState::Paused => {
                        interaction.mode = if control == OrderControlState::FreezeRequested {
                            ApparatusQueueInteractionMode::FreezeRequested
                        } else {
                            ApparatusQueueInteractionMode::Paused
                        };
                        interaction.material_intake_allowed = control == OrderControlState::Active;
                        interaction.assigned_materials_display_only =
                            control != OrderControlState::Active;
                        if queue_actionable && control == OrderControlState::Active {
                            allowed_actions.push(queue_state::ApparatusQueueAction::Resume);
                        }
                    }
                    queue_state::ApparatusQueueOrderState::Frozen => {
                        interaction.mode = ApparatusQueueInteractionMode::Frozen;
                        interaction.blocking_reason_code = "order_frozen".to_string();
                    }
                    queue_state::ApparatusQueueOrderState::Completed => {
                        interaction.mode = ApparatusQueueInteractionMode::Completed;
                    }
                }

                apparatus_controls.insert(
                    order_id.trim().to_string(),
                    ApparatusQueueOrderActionControl {
                        state,
                        allowed_actions,
                        interaction,
                        previous_stage: previous_stage.unwrap_or_default(),
                        previous_stage_ready,
                        complete_requires_full_report,
                        freeze_request: order_control
                            .and_then(|control| control.freeze_request.clone()),
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

include!("execution.rs");
