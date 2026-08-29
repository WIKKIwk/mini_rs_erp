impl ProductionMapService {

    pub(in crate::core::production_map) async fn queue_action_controls_for_snapshot(
        &self,
        maps: &[ProductionMapDefinition],
        sequences: &BTreeMap<String, Vec<String>>,
        all_states: &ApparatusQueueStateMap,
        order_controls: &OrderControlMap,
    ) -> Result<
        BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
        ProductionMapError,
    > {
        let material_assignments = self.store.raw_material_assignments().await?;
        let order_ids = maps
            .iter()
            .map(|map| map.id.trim())
            .filter(|order_id| !order_id.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        let mut maps_by_order_id = HashMap::new();
        for map in maps {
            let order_id = map.id.trim();
            if !order_id.is_empty() {
                maps_by_order_id.entry(order_id).or_insert(map);
            }
        }
        let mut material_assignments_by_order =
            HashMap::<String, Vec<RawMaterialAssignment>>::new();
        for assignment in material_assignments {
            material_assignments_by_order
                .entry(assignment.order_id.trim().to_string())
                .or_default()
                .push(assignment);
        }
        let active_sessions_by_order = self
            .store
            .active_order_run_sessions_for_orders(&order_ids)
            .await?;
        let progress_batches_by_order = self.store.progress_batches_for_orders(&order_ids).await?;
        let mut opening_wip_by_order = HashMap::<String, Vec<OpeningWipRecord>>::new();
        for record in self
            .store
            .opening_wip_records(OpeningWipQuery {
                order_id: String::new(),
                wip_status: None,
                limit: 100_000,
            })
            .await?
        {
            opening_wip_by_order
                .entry(record.intake.order_id.trim().to_string())
                .or_default()
                .push(record);
        }
        let visible_by_apparatus = visible_order_ids_by_apparatus(maps);
        let frozen_order_ids = order_controls
            .iter()
            .filter_map(|(order_id, control)| {
                (control.state == OrderControlState::Frozen).then_some(order_id.clone())
            })
            .collect::<BTreeSet<_>>();
        let canonical_apparatuses = self.active_canonical_apparatuses().await?;
        let known_keys = sequences
            .keys()
            .map(String::as_str)
            .chain(all_states.keys().map(String::as_str))
            .chain(
                canonical_apparatuses
                    .iter()
                    .map(|configuration| configuration.runtime.apparatus_id.as_str()),
            )
            .chain(visible_by_apparatus.keys().map(String::as_str))
            .filter(|key| !queue_state::apparatus_search_key(key).is_empty())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let apparatuses = known_keys.iter().cloned().collect::<BTreeSet<_>>();
        let mut result = BTreeMap::new();

        for apparatus in apparatuses {
            let storage_key = queue_state::resolve_apparatus_storage_key(&apparatus, &known_keys);
            let canonical = self.resolve_canonical_apparatus_text(&storage_key).await?;
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
            let mut effective_states = parsed_queue_states(stored_states);
            for order_id in &sequence {
                if effective_states.get(order_id.trim())
                    != Some(&queue_state::ApparatusQueueOrderState::Completed)
                {
                    continue;
                }
                let Some(order_map) = maps_by_order_id.get(order_id.trim()).copied() else {
                    continue;
                };
                let batches = progress_batches_by_order
                    .get(order_id.trim())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let has_waiting_progress_reentry = waiting_reentry_stage_node_id(
                    order_map,
                    batches,
                    order_id,
                    &storage_key,
                )
                .is_some();
                let has_waiting_opening_wip_reentry = opening_wip_by_order
                    .get(order_id.trim())
                    .into_iter()
                    .flatten()
                    .any(|record| {
                        record.intake.status == OpeningWipIntakeStatus::Confirmed
                            && record
                                .batches
                                .iter()
                                .any(|batch| batch.wip_status == OpeningWipBatchStatus::Waiting)
                            && Self::opening_wip_target_stage(
                                order_map,
                                &record.intake,
                                &storage_key,
                                "",
                            )
                            .is_some()
                    });
                if has_waiting_progress_reentry || has_waiting_opening_wip_reentry {
                    effective_states.insert(
                        order_id.trim().to_string(),
                        queue_state::ApparatusQueueOrderState::Pending,
                    );
                }
            }
            let policy = queue_policy_for_apparatus(canonical.as_ref());
            let active_order_id = effective_states.iter().find_map(|(order_id, state)| {
                (*state == queue_state::ApparatusQueueOrderState::InProgress)
                    .then_some(order_id.as_str())
            });
            let actionable_order_id =
                queue_state::first_actionable_order_id(&sequence, &effective_states);
            let mut apparatus_controls = BTreeMap::new();

            for order_id in sequence {
                let Some(order_map) = maps_by_order_id.get(order_id.trim()).copied() else {
                    continue;
                };
                let batches = progress_batches_by_order
                    .get(order_id.trim())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let active_session = active_sessions_by_order
                    .get(order_id.trim())
                    .and_then(|sessions| {
                        sessions.iter().find(|session| {
                            queue_state::apparatus_ids_match(&session.apparatus, &storage_key)
                        })
                    });
                let active_stage_node_id = active_session
                    .and_then(|session| session.payload_json.get("stage_node_id"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .trim();
                let waiting_reentry_stage_node_id = waiting_reentry_stage_node_id(
                    order_map,
                    batches,
                    order_id.trim(),
                    &storage_key,
                );
                let opening_wip_stage_node_id = opening_wip_by_order
                    .get(order_id.trim())
                    .into_iter()
                    .flatten()
                    .find_map(|record| {
                        (record.intake.status == OpeningWipIntakeStatus::Confirmed
                            && record
                                .batches
                                .iter()
                                .any(|batch| batch.wip_status == OpeningWipBatchStatus::Waiting))
                        .then(|| {
                            Self::opening_wip_target_stage(
                                order_map,
                                &record.intake,
                                &storage_key,
                                "",
                            )
                            .map(|stage| stage.node_id.trim().to_string())
                        })
                        .flatten()
                    });
                let preferred_stage_node_id = if !active_stage_node_id.is_empty() {
                    active_stage_node_id.to_string()
                } else if let Some(stage_node_id) = waiting_reentry_stage_node_id {
                    stage_node_id
                } else {
                    opening_wip_stage_node_id.unwrap_or_default()
                };
                let stage = chain::work_stage_for_station(
                    order_map,
                    &storage_key,
                    &preferred_stage_node_id,
                )
                .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
                let stage_node_id = stage.node_id.clone();
                let state = effective_states
                    .get(order_id.trim())
                    .copied()
                    .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
                let order_control = order_controls.get(order_id.trim());
                let control = order_control
                    .map(|control| control.state)
                    .unwrap_or(OrderControlState::Active);
                let previous_stage = chain::previous_work_stage_for_node(order_map, &stage_node_id)
                    .and_then(|stage| stage.apparatus_id);
                let previous_stage_not_configured = apparatus::requires_previous_stage(&canonical)
                    && previous_stage.is_none()
                    && chain::previous_stage_resolution_is_unavailable(order_map, &apparatus);
                let previous_stage_ready = chain::order_ready_for_stage_node(
                    order_map,
                    order_id.trim(),
                    &stage_node_id,
                    all_states,
                );
                let mut previous_wip_mode = previous_stage
                    .as_deref()
                    .map(|previous_stage| {
                        if has_waiting_previous_stage_wip(
                            order_map,
                            batches,
                            order_id.trim(),
                            previous_stage,
                            &storage_key,
                            &stage_node_id,
                        ) {
                            ApparatusQueuePreviousWipMode::ScanRequired
                        } else {
                            ApparatusQueuePreviousWipMode::Waiting
                        }
                    })
                    .unwrap_or(ApparatusQueuePreviousWipMode::NotRequired);
                let opening_wip_mode = if opening_wip_by_order
                    .get(order_id.trim())
                    .into_iter()
                    .flatten()
                    .any(|record| {
                        record.intake.status == OpeningWipIntakeStatus::Confirmed
                            && Self::opening_wip_target_stage(
                                order_map,
                                &record.intake,
                                &storage_key,
                                &stage_node_id,
                            )
                            .is_some()
                            && record
                                .batches
                                .iter()
                                .any(|batch| batch.wip_status == OpeningWipBatchStatus::Waiting)
                    })
                {
                    ApparatusQueuePreviousWipMode::ScanRequired
                } else {
                    ApparatusQueuePreviousWipMode::NotRequired
                };
                if opening_wip_mode == ApparatusQueuePreviousWipMode::ScanRequired {
                    previous_wip_mode = ApparatusQueuePreviousWipMode::NotRequired;
                }
                let active_order_is_this = active_order_id
                    .is_none_or(|active_order_id| active_order_id == order_id.trim());
                let requeued_session = active_session
                    .is_some_and(order_run_session_was_requeued);
                let queue_actionable = state.is_active()
                    || actionable_order_id.as_deref() == Some(order_id.trim())
                    || (state == queue_state::ApparatusQueueOrderState::Pending
                        && !previous_stage_not_configured
                        && (opening_wip_mode == ApparatusQueuePreviousWipMode::ScanRequired
                            || (previous_stage.is_some()
                                && (previous_stage_ready
                                    || (apparatus::is_laminatsiya_apparatus(&canonical)
                                        && previous_wip_mode
                                            == ApparatusQueuePreviousWipMode::ScanRequired))))
                        && active_order_is_this);
                let mut allowed_actions = Vec::new();
                let mut complete_requires_full_report = false;
                let mut complete_requires_rezka_total_waste_only = false;
                let mut interaction = ApparatusQueueWorkerInteraction {
                    assigned_materials_display_only: !matches!(
                        state,
                        queue_state::ApparatusQueueOrderState::InProgress
                            | queue_state::ApparatusQueueOrderState::Paused
                    ),
                    opening_wip_mode,
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
                        if previous_stage_not_configured {
                            interaction.mode = ApparatusQueueInteractionMode::FreshStartBlocked;
                            interaction.blocking_reason_code =
                                "previous_stage_not_configured".to_string();
                        } else {
                            let assignments = material_assignments_by_order
                                .get(order_id.trim())
                                .into_iter()
                                .flatten()
                                .filter(|assignment| {
                                    assignment.apparatus_id == canonical.runtime.apparatus_id
                                })
                                .cloned()
                            .collect::<Vec<_>>();
                            let rule = live_material_rule(canonical.as_ref());
                            let material_requirements = build_raw_material_start_requirements(
                                rule.as_ref(),
                                &assignments,
                                &[],
                                "",
                            );
                            let material_scan_required = material_requirements.requires_material
                                || !material_requirements.assigned_barcodes.is_empty();
                            let start_materials_mode =
                                if opening_wip_mode
                                    == ApparatusQueuePreviousWipMode::ScanRequired
                                    || (apparatus::is_laminatsiya_apparatus(&canonical)
                                        && previous_wip_mode
                                            == ApparatusQueuePreviousWipMode::ScanRequired)
                                {
                                    ApparatusQueueStartMaterialsMode::Hidden
                                } else if material_scan_required {
                                    ApparatusQueueStartMaterialsMode::ScanRequired
                                } else {
                                    ApparatusQueueStartMaterialsMode::Hidden
                                };

                            if opening_wip_mode == ApparatusQueuePreviousWipMode::Waiting {
                                interaction.mode =
                                    ApparatusQueueInteractionMode::WaitingPreviousStage;
                                interaction.opening_wip_mode = opening_wip_mode;
                                interaction.blocking_reason_code =
                                    "waiting_opening_wip".to_string();
                            } else if previous_wip_mode == ApparatusQueuePreviousWipMode::Waiting {
                                interaction.mode =
                                    ApparatusQueueInteractionMode::WaitingPreviousStage;
                                interaction.previous_wip_mode = previous_wip_mode;
                                interaction.opening_wip_mode = opening_wip_mode;
                                interaction.blocking_reason_code =
                                    "waiting_previous_stage".to_string();
                            } else if !pending_actionable {
                                interaction.mode = ApparatusQueueInteractionMode::FreshStartBlocked;
                                interaction.blocking_reason_code = "waiting_sequence".to_string();
                            } else {
                                interaction.start_materials_mode = start_materials_mode;
                                interaction.material_scan_required = start_materials_mode
                                    == ApparatusQueueStartMaterialsMode::ScanRequired;
                                interaction.assigned_materials_display_only = false;
                                interaction.previous_wip_mode = previous_wip_mode;
                                interaction.qolip_mode =
                                    if apparatus::requires_qolip_scan(&canonical) {
                                        ApparatusQueueQolipMode::ScanRequired
                                    } else {
                                        ApparatusQueueQolipMode::NotRequired
                                    };
                                if material_requirements.assignments_satisfied {
                                    interaction.mode = ApparatusQueueInteractionMode::FreshStart;
                                    allowed_actions.push(queue_state::ApparatusQueueAction::Start);
                                } else {
                                    interaction.mode =
                                        ApparatusQueueInteractionMode::FreshStartBlocked;
                                    interaction.blocking_reason_code =
                                        "raw_material_assignment_required".to_string();
                                }
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
                            let current_input_batch_id = active_session
                                .map(session_progress_links)
                                .map(|links| links.batch_id)
                                .unwrap_or_default();
                            let batches = progress_batches_by_order
                                .get(order_id.trim())
                                .map(Vec::as_slice)
                                .unwrap_or_default();
                            let has_unprocessed_previous_wips =
                                has_unprocessed_previous_wips_from_sources(
                                    order_id.trim(),
                                    order_map,
                                    &storage_key,
                                    canonical.as_ref(),
                                    all_states,
                                    batches,
                                    &[],
                                    opening_wip_by_order
                                        .get(order_id.trim())
                                        .map(Vec::as_slice)
                                        .unwrap_or_default(),
                                    &[],
                                    &current_input_batch_id,
                                    &stage_node_id,
                                );
                            if apparatus::is_rezka_apparatus(&canonical) {
                                allowed_actions
                                    .push(queue_state::ApparatusQueueAction::RollComplete);
                            }
                            allowed_actions.push(queue_state::ApparatusQueueAction::Complete);
                            if apparatus::is_rezka_apparatus(&canonical) {
                                let is_final_stage = if stage_node_id.trim().is_empty() {
                                    chain::is_final_work_stage_station(order_map, &storage_key)
                                } else {
                                    chain::is_final_work_stage_node(order_map, &stage_node_id)
                                };
                                complete_requires_full_report =
                                    is_final_stage && !has_unprocessed_previous_wips;
                                complete_requires_rezka_total_waste_only = !is_final_stage;
                            } else {
                                complete_requires_full_report =
                                    !apparatus::is_laminatsiya_apparatus(&canonical)
                                        || !has_unprocessed_previous_wips;
                            }
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

                let input_contained_kadr_count = active_session
                    .and_then(|session| session.payload_json.get("contained_kadr_count"))
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok())
                    .filter(|value| *value > 0)
                    .or_else(|| {
                        batches.iter().find_map(|batch| {
                            (progress_batch_next_stage_node_id(batch) == stage_node_id)
                                .then(|| {
                                    batch
                                        .payload_json
                                        .get("contained_kadr_count")
                                        .and_then(serde_json::Value::as_u64)
                                        .and_then(|value| usize::try_from(value).ok())
                                        .filter(|value| *value > 0)
                                })
                                .flatten()
                        })
                    });
                let rezka_output_kadr_counts = if apparatus::is_rezka_apparatus(&canonical) {
                    service_progress_support::rezka_output_kadr_counts(
                        order_map,
                        &storage_key,
                        &stage_node_id,
                        input_contained_kadr_count,
                    )?
                    .into_iter()
                    .map(|value| {
                        i64::try_from(value)
                            .map_err(|_| ProductionMapError::InvalidRezkaFrameGroups)
                    })
                    .collect::<Result<Vec<_>, _>>()?
                } else {
                    Vec::new()
                };

                apparatus_controls.insert(
                    order_id.trim().to_string(),
                    ApparatusQueueOrderActionControl {
                        state,
                        allowed_actions,
                        interaction,
                        previous_stage: previous_stage.unwrap_or_default(),
                        stage_node_id,
                        previous_stage_ready,
                        rezka_output_kadr_counts,
                        complete_requires_full_report,
                        complete_requires_rezka_total_waste_only,
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
        self.enforce_qolip_start_boundary(apparatus, order_id, action, None)
            .await?;
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

    /// Core queue callers must not be able to enter a Qolip-protected start
    /// without the trusted handler validation bound to the same canonical
    /// apparatus and order. Untrusted core callers therefore remain
    /// fail-closed.
    pub(crate) async fn enforce_qolip_start_boundary(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        qolip_validation: Option<&TrustedQolipStartValidation>,
    ) -> Result<(), ProductionMapError> {
        if action != queue_state::ApparatusQueueAction::Start {
            return Ok(());
        }
        let canonical = self.resolve_canonical_apparatus_text(apparatus).await?;
        if apparatus::requires_qolip_scan(&canonical)
            && !qolip_validation.is_some_and(|validation| {
                validation.matches(&canonical.runtime.apparatus_id, order_id)
            })
        {
            return Err(ProductionMapError::QolipCodeMismatch);
        }
        Ok(())
    }
}
