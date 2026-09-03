impl ProductionMapService {
    pub(in crate::core::production_map) async fn queue_action_controls_for_snapshot(
        &self,
        maps: &[ProductionMapDefinition],
        sequences: &BTreeMap<String, Vec<String>>,
        all_states: &ApparatusQueueStateMap,
        order_controls: &OrderControlMap,
        canonical_apparatuses: &[
            std::sync::Arc<crate::core::apparatus_standard::RuntimeApparatusConfiguration>
        ],
    ) -> Result<
        BTreeMap<String, BTreeMap<String, ApparatusQueueOrderActionControl>>,
        ProductionMapError,
    > {
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
        let opening_wip_query = OpeningWipQuery {
            order_id: String::new(),
            wip_status: None,
            limit: 100_000,
        };
        let (
            material_assignments,
            active_sessions_by_order,
            progress_batches_by_order,
            opening_wip_records,
        ) = tokio::join!(
            self.store.raw_material_assignments(),
            self.store.active_order_run_sessions_for_orders(&order_ids),
            self.store.progress_batches_for_orders(&order_ids),
            self.store.opening_wip_records(opening_wip_query),
        );
        let material_assignments = material_assignments?;
        let active_sessions_by_order = active_sessions_by_order?;
        let progress_batches_by_order = progress_batches_by_order?;
        let opening_wip_records = opening_wip_records?;
        let mut active_sessions_by_order_apparatus =
            HashMap::<(&str, &str), &OrderRunSession>::new();
        for sessions in active_sessions_by_order.values() {
            for session in sessions {
                if !queue_state::is_canonical_apparatus_id(&session.apparatus) {
                    continue;
                }
                active_sessions_by_order_apparatus
                    .entry((session.order_id.trim(), session.apparatus.trim()))
                    .or_insert(session);
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
        let mut opening_wip_by_order = HashMap::<String, Vec<OpeningWipRecord>>::new();
        for record in opening_wip_records {
            opening_wip_by_order
                .entry(record.intake.order_id.trim().to_string())
                .or_default()
                .push(record);
        }
        let queue_orders_by_apparatus = queue_order_ids_by_apparatus(maps);
        let frozen_order_ids = order_controls
            .iter()
            .filter_map(|(order_id, control)| {
                (control.state == OrderControlState::Frozen).then_some(order_id.clone())
            })
            .collect::<BTreeSet<_>>();
        let canonical_by_id = canonical_apparatuses
            .iter()
            .map(|configuration| {
                (
                    configuration.runtime.apparatus_id.as_str(),
                    configuration.as_ref(),
                )
            })
            .collect::<HashMap<_, _>>();
        let known_keys = sequences
            .keys()
            .map(String::as_str)
            .chain(all_states.keys().map(String::as_str))
            .chain(
                canonical_apparatuses
                    .iter()
                    .map(|configuration| configuration.runtime.apparatus_id.as_str()),
            )
            .chain(queue_orders_by_apparatus.keys().map(String::as_str))
            .filter(|key| queue_state::is_canonical_apparatus_id(key))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let mut result = BTreeMap::new();

        for apparatus in &known_keys {
            let storage_key = queue_state::resolve_apparatus_storage_key(apparatus, &known_keys);
            // Snapshot reads must stay fail-soft: one map referencing a deleted or
            // deactivated apparatus must not take the whole live stream down for
            // every operator. Write paths keep their strict validation.
            let Some(canonical) = canonical_by_id.get(storage_key.as_str()).copied() else {
                warn_skipped_snapshot_apparatus(&storage_key);
                continue;
            };
            let is_rezka = apparatus::is_rezka_apparatus(canonical);
            let is_laminatsiya = apparatus::is_laminatsiya_apparatus(canonical);
            let stored_sequence = sequences
                .get(&storage_key)
                .or_else(|| sequences.get(apparatus))
                .map(Vec::as_slice)
                .unwrap_or_default();
            let visible_order_ids = queue_orders_by_apparatus
                .get(&storage_key)
                .or_else(|| queue_orders_by_apparatus.get(apparatus))
                .map(Vec::as_slice)
                .unwrap_or_default();
            let sequence = queue_state::effective_apparatus_sequence_excluding(
                stored_sequence,
                visible_order_ids,
                &frozen_order_ids,
            );
            let mut order_inputs = Vec::with_capacity(sequence.len());
            for order_id in &sequence {
                let Some(order_map) = maps_by_order_id.get(order_id.trim()).copied() else {
                    continue;
                };
                let batches = progress_batches_by_order
                    .get(order_id.trim())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let opening_wip_records = opening_wip_by_order
                    .get(order_id.trim())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let active_session = active_sessions_by_order_apparatus
                    .get(&(order_id.trim(), storage_key.as_str()))
                    .copied();
                order_inputs.push(queue_control_order_input(
                    order_id.trim(),
                    order_map,
                    &storage_key,
                    batches,
                    opening_wip_records,
                    active_session,
                ));
            }
            let mut effective_states = all_states
                .get(&storage_key)
                .or_else(|| all_states.get(apparatus))
                .map(parsed_queue_states)
                .unwrap_or_default();
            for input in &order_inputs {
                if effective_states.get(input.order_id)
                    != Some(&queue_state::ApparatusQueueOrderState::Completed)
                {
                    continue;
                }
                if input.waiting_reentry_stage_node_id.is_some()
                    || input.opening_wip_stage_node_id.is_some()
                {
                    effective_states.insert(
                        input.order_id.to_string(),
                        queue_state::ApparatusQueueOrderState::Pending,
                    );
                }
            }
            let policy = effective_apparatus_queue_policy(canonical);
            let active_order_id = effective_states.iter().find_map(|(order_id, state)| {
                (*state == queue_state::ApparatusQueueOrderState::InProgress)
                    .then_some(order_id.as_str())
            });
            let actionable_order_id =
                queue_state::first_actionable_order_id(&sequence, &effective_states);
            let mut apparatus_controls = BTreeMap::new();

            for input in &order_inputs {
                let order_id = input.order_id;
                let order_map = input.order_map;
                let batches = input.batches;
                let active_session = input.active_session;
                let active_stage_node_id = active_session
                    .map(|session| session.stage_node_id.trim())
                    .unwrap_or_default();
                let preferred_stage_node_id = if !active_stage_node_id.is_empty() {
                    active_stage_node_id.to_string()
                } else if let Some(stage_node_id) = input.waiting_reentry_stage_node_id.clone() {
                    stage_node_id
                } else {
                    input
                        .opening_wip_stage_node_id
                        .clone()
                        .unwrap_or_default()
                };
                // A map edited after its sequence was saved may no longer resolve
                // a stage for this apparatus. Skip the single order instead of
                // failing the snapshot for everyone.
                let Some(stage) = chain::work_stage_for_station(
                    order_map,
                    &storage_key,
                    &preferred_stage_node_id,
                ) else {
                    warn_skipped_snapshot_order(
                        order_id.trim(),
                        &storage_key,
                        "unresolvable work stage",
                    );
                    continue;
                };
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
                let previous_stage_not_configured = apparatus::requires_previous_stage(canonical)
                    && previous_stage.is_none()
                    && chain::previous_stage_resolution_is_unavailable(order_map, apparatus);
                let previous_stage_ready = chain::order_ready_for_stage_node(
                    order_map,
                    order_id.trim(),
                    &stage_node_id,
                    all_states,
                );
                let stage_projection = queue_control_stage_projection(
                    order_map,
                    batches,
                    order_id,
                    previous_stage.as_deref(),
                    &storage_key,
                    &stage_node_id,
                );
                let mut previous_wip_mode = previous_stage
                    .as_ref()
                    .map(|_| {
                        if stage_projection.has_waiting_previous_stage_wip {
                            ApparatusQueuePreviousWipMode::ScanRequired
                        } else {
                            ApparatusQueuePreviousWipMode::Waiting
                        }
                    })
                    .unwrap_or(ApparatusQueuePreviousWipMode::NotRequired);
                let opening_wip_mode = if input.has_waiting_opening_wip_for_stage(&stage_node_id) {
                    ApparatusQueuePreviousWipMode::ScanRequired
                } else {
                    ApparatusQueuePreviousWipMode::NotRequired
                };
                if opening_wip_mode == ApparatusQueuePreviousWipMode::ScanRequired {
                    previous_wip_mode = ApparatusQueuePreviousWipMode::NotRequired;
                }
                let active_order_is_this = active_order_id
                    .is_none_or(|active_order_id| active_order_id == order_id.trim());
                let requeued_session = active_session.is_some_and(order_run_session_was_requeued);
                let queue_actionable = state.is_active()
                    || actionable_order_id == Some(order_id.trim())
                    || (state == queue_state::ApparatusQueueOrderState::Pending
                        && !previous_stage_not_configured
                        && (opening_wip_mode == ApparatusQueuePreviousWipMode::ScanRequired
                            || (previous_stage.is_some()
                                && (previous_stage_ready
                                    || previous_wip_mode
                                        == ApparatusQueuePreviousWipMode::ScanRequired)))
                        && active_order_is_this);
                let mut complete_requires_full_report = false;
                let mut complete_requires_rezka_total_waste_only = false;
                let mut start_ready = false;
                let mut merge_ready = false;
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
                                .collect::<Vec<_>>();
                            let rule = live_material_rule(canonical);
                            let material_requirements = build_raw_material_start_requirements_refs(
                                rule.as_ref(),
                                &assignments,
                                &[],
                                "",
                            );
                            let material_scan_required = material_requirements.requires_material
                                || !material_requirements.assigned_barcodes.is_empty();
                            let start_materials_mode = if opening_wip_mode
                                == ApparatusQueuePreviousWipMode::ScanRequired
                                || (is_laminatsiya && previous_wip_mode
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
                                    if apparatus::requires_qolip_scan(canonical) {
                                        ApparatusQueueQolipMode::ScanRequired
                                    } else {
                                        ApparatusQueueQolipMode::NotRequired
                                    };
                                if material_requirements.assignments_satisfied {
                                    interaction.mode = ApparatusQueueInteractionMode::FreshStart;
                                    start_ready = true;
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
                        if control == OrderControlState::FreezeRequested {
                            interaction.blocking_reason_code = "order_freeze_requested".to_string();
                        }
                        if control == OrderControlState::Active {
                            let current_input_batch_id = active_session
                                .map(session_progress_links)
                                .map(|links| links.batch_id)
                                .unwrap_or_default();
                            let has_unprocessed_previous_wips =
                                has_unprocessed_previous_wips_from_sources(
                                    order_id.trim(),
                                    order_map,
                                    &storage_key,
                                    canonical,
                                    all_states,
                                    batches,
                                    &[],
                                    input.opening_wip_records,
                                    &[],
                                    &current_input_batch_id,
                                    &stage_node_id,
                                );
                            if is_rezka || is_laminatsiya {
                                merge_ready = active_session.is_some_and(|session| {
                                    !session_progress_links(session).batch_id.trim().is_empty()
                                });
                            }
                            if is_rezka {
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
                                    !is_laminatsiya || !has_unprocessed_previous_wips;
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
                    }
                    queue_state::ApparatusQueueOrderState::Frozen => {
                        interaction.mode = ApparatusQueueInteractionMode::Frozen;
                        interaction.blocking_reason_code = "order_frozen".to_string();
                    }
                    queue_state::ApparatusQueueOrderState::Completed => {
                        interaction.mode = ApparatusQueueInteractionMode::Completed;
                    }
                }

                let allowed_actions = allowed_actions_for_control(QueueActionPolicyInput {
                    state,
                    profile: QueueActionPolicyProfile::Live {
                        order_control: control,
                        is_rezka,
                        merge_ready,
                    },
                    requeued_session,
                    pending_actionable,
                    queue_actionable,
                    start_ready,
                });

                // Corrupt session payloads must only hide one order's controls.
                // The write path revalidates everything, so the snapshot keeps
                // serving the remaining orders instead of erroring globally.
                let (rezka_input_lineage, rezka_active_partial_rolls) =
                    if is_rezka || is_laminatsiya {
                        match snapshot_session_lineage(
                            active_session,
                            is_rezka,
                            order_id.trim(),
                            &storage_key,
                        ) {
                            Some(lineage) => lineage,
                            None => continue,
                        }
                    } else {
                        (Vec::new(), Vec::new())
                    };
                let input_contained_kadr_count = active_session
                    .and_then(|session| session.payload_json.get("contained_kadr_count"))
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok())
                    .filter(|value| *value > 0)
                    .or(stage_projection.input_contained_kadr_count);
                let rezka_output_kadr_counts = if !rezka_active_partial_rolls.is_empty() {
                    rezka_active_partial_rolls
                        .iter()
                        .map(|roll| i64::from(roll.contained_kadr_count))
                        .collect()
                } else if is_rezka {
                    match snapshot_rezka_output_kadr_counts(
                        order_map,
                        &storage_key,
                        &stage_node_id,
                        input_contained_kadr_count,
                        order_id.trim(),
                    ) {
                        Some(counts) => counts,
                        None => continue,
                    }
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
                        rezka_input_lineage,
                        rezka_active_partial_rolls,
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
