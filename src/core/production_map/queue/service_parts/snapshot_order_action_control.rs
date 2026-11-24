impl ProductionMapService {
    async fn snapshot_order_action_control(
        &self,
        station: &StationQueueSnapshot<'_>,
        input: &QueueControlOrderInput<'_>,
    ) -> Result<Option<ApparatusQueueOrderActionControl>, ProductionMapError> {
        let order_id = input.order_id;
        let order_map = input.order_map;
        let projection = input.projection;
        let batches = input.batches;
        let active_session = input.active_session;
        let preferred_stage_node_id = station.preferred_stage_node_id(input);
        // A map edited after its sequence was saved may no longer resolve
        // a stage for this apparatus. Skip the single order instead of
        // failing the snapshot for everyone.
        let Some(stage) = projection.work_stage(station.storage_key, &preferred_stage_node_id)
        else {
            warn_skipped_snapshot_order(
                order_id.trim(),
                station.storage_key,
                "unresolvable work stage",
            );
            return Ok(None);
        };
        let stage_node_id = stage.node_id.clone();
        let rezka_total_waste_only = station.is_rezka
            && !chain::is_final_work_stage_node(order_map, &stage_node_id);
        let stage_work = super::super::stage_execution::work_control_projected(
            projection,
            station.storage_key,
            &stage_node_id,
            station.stage_work_by_order
                .get(order_id)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            station.active_sessions_by_order
                .get(order_id)
                .map(Vec::as_slice)
                .unwrap_or_default(),
        );
        let stage_control = station.stage_controls.iter().find(|c| c.order_id == order_id.trim()
            && c.stage_node_id == stage_node_id
            && c.freeze_request.as_ref().is_some_and(|r| r.target_apparatus == station.storage_key));
        let order_control = station.order_controls.get(order_id.trim())
            .filter(|c| c.state != OrderControlState::Active || c.early_close.is_some())
            .or(stage_control.filter(|c| c.state != OrderControlState::Active))
            .or_else(|| station.order_controls.get(order_id.trim()));
        let control = order_control
            .map(|control| control.state)
            .unwrap_or(OrderControlState::Active);
        let state = if control == OrderControlState::Frozen {
            queue_state::ApparatusQueueOrderState::Frozen
        } else {
            station.effective_states
                .get(order_id.trim())
                .copied()
                .unwrap_or(queue_state::ApparatusQueueOrderState::Pending)
        };
        let previous_stage = projection
            .previous_stages(&stage_node_id)
            .into_iter()
            .next()
            .and_then(|stage| stage.apparatus_id);
        let previous_stage_not_configured = apparatus::requires_previous_stage(station.canonical)
            && previous_stage.is_none()
            && projection.previous_unavailable(station.apparatus);
        let previous_stage_ready =
            projection.ready_for_stage(order_id.trim(), &stage_node_id, station.all_states);
        let stage_projection = queue_control_stage_projection(
            projection,
            batches,
            order_id,
            previous_stage.as_deref(),
            station.storage_key,
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
        // Opening stock and newly produced rolls are alternative inputs.
        // Only suppress a waiting gate; keep available production WIP scannable.
        if opening_wip_mode == ApparatusQueuePreviousWipMode::ScanRequired
            && previous_wip_mode == ApparatusQueuePreviousWipMode::Waiting
        {
            previous_wip_mode = ApparatusQueuePreviousWipMode::NotRequired;
        }
        let active_order_is_this = station.active_order_id
            .is_none_or(|active_id| active_id == order_id.trim());
        let requeued_session = active_session.is_some_and(order_run_session_was_requeued);
        let print_preflight = station.apparatus_preflight
            .filter(|hold| hold.order_id.trim() == order_id.trim())
            .cloned();
        let preflight_blocks_this_order = state
            == queue_state::ApparatusQueueOrderState::PrintPreflight
            && !print_preflight
                .as_ref()
                .is_some_and(|hold| hold.status == PrintPreflightStatus::Passed);
        let preflight_blocks_other_order =
            station.apparatus_preflight.is_some_and(|hold| hold.order_id.trim() != order_id.trim());
        let queue_actionable = active_order_is_this
            && (state.is_active()
                || station.policy == ApparatusQueuePolicy::FreePick
                || station.actionable_order_id == Some(order_id.trim()));
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
            opening_wip_mode: if control == OrderControlState::Frozen {
                ApparatusQueuePreviousWipMode::NotRequired
            } else {
                opening_wip_mode
            },
            ..ApparatusQueueWorkerInteraction::default()
        };
        let pending_actionable = queue_actionable
            && control == OrderControlState::Active
            && !preflight_blocks_this_order
            && !preflight_blocks_other_order;
        let queue_blocking_reason = if preflight_blocks_other_order {
            "print_preflight_other_order_active"
        } else if active_order_is_this {
            if preflight_blocks_this_order {
                "print_preflight_active"
            } else {
                "waiting_sequence"
            }
        } else if preflight_blocks_this_order {
            "print_preflight_active"
        } else {
            "apparatus_busy"
        };

        match state {
            queue_state::ApparatusQueueOrderState::Pending if requeued_session => {
                if pending_actionable {
                    interaction.mode = ApparatusQueueInteractionMode::RequeuedReady;
                } else {
                    interaction.mode = ApparatusQueueInteractionMode::RequeuedWaiting;
                    interaction.blocking_reason_code = queue_blocking_reason.to_string();
                }
            }
            queue_state::ApparatusQueueOrderState::PrintPreflight
                if control == OrderControlState::FreezeRequested
                    && print_preflight.as_ref().is_some_and(|hold| {
                        matches!(
                            hold.status,
                            PrintPreflightStatus::Held | PrintPreflightStatus::Running
                        )
                    }) =>
            {
                interaction.mode = ApparatusQueueInteractionMode::FreezeRequested;
                interaction.blocking_reason_code = "order_freeze_requested".to_string();
            }
            queue_state::ApparatusQueueOrderState::Pending
            | queue_state::ApparatusQueueOrderState::PrintPreflight => {
                if preflight_blocks_this_order || preflight_blocks_other_order {
                    interaction.mode = ApparatusQueueInteractionMode::FreshStartBlocked;
                    interaction.blocking_reason_code = queue_blocking_reason.to_string();
                } else if previous_stage_not_configured {
                    interaction.mode = ApparatusQueueInteractionMode::FreshStartBlocked;
                    interaction.blocking_reason_code =
                        "previous_stage_not_configured".to_string();
                } else {
                    start_ready = station.configure_fresh_start(
                        order_id, &mut interaction, pending_actionable, opening_wip_mode,
                        previous_wip_mode, queue_blocking_reason,
                    );
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
                        has_available_previous_wips_for_local_report(
                            order_id.trim(),
                            order_map,
                            station.storage_key,
                            station.canonical,
                            station.all_states,
                            batches,
                            &[],
                            input.opening_wip_records,
                            &[],
                            &current_input_batch_id,
                            &stage_node_id,
                            stage_work.as_ref().map(|s| s.upstream_closed),
                        );
                    if station.is_rezka || station.is_laminatsiya {
                        // A mounted roll alone does not provide another input to splice.
                        merge_ready = (stage_projection.has_waiting_previous_stage_wip
                            || input.has_waiting_opening_wip_for_stage(&stage_node_id))
                            && active_session.is_some_and(|session| {
                                !session_progress_links(session).batch_id.trim().is_empty()
                            });
                    }
                    if station.is_rezka {
                        let is_final_stage = if stage_node_id.trim().is_empty() {
                            chain::is_final_work_stage_station(order_map, station.storage_key)
                        } else {
                            chain::is_final_work_stage_node(order_map, &stage_node_id)
                        };
                        complete_requires_full_report =
                            is_final_stage && !has_unprocessed_previous_wips;
                        complete_requires_rezka_total_waste_only =
                            !is_final_stage && !has_unprocessed_previous_wips;
                    } else {
                        complete_requires_full_report =
                            !station.is_laminatsiya || !has_unprocessed_previous_wips;
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
                if station.is_laminatsiya && control == OrderControlState::Active {
                    interaction.resume_input_required = previous_stage.is_some()
                        && active_session.is_some_and(lamination_input_was_released);
                    if interaction.resume_input_required {
                        interaction.previous_wip_mode = previous_wip_mode;
                        interaction.opening_wip_mode = ApparatusQueuePreviousWipMode::NotRequired;
                    }
                    if active_session.is_some_and(|session| session.payload_json
                        .get("lamination_astatka_raw_released")
                        .and_then(serde_json::Value::as_bool) == Some(true))
                    {
                        interaction.start_materials_mode = ApparatusQueueStartMaterialsMode::ScanRequired;
                        interaction.material_scan_required = true;
                    }
                }
                if !active_order_is_this {
                    interaction.blocking_reason_code = "apparatus_busy".to_string();
                } else if !queue_actionable {
                    interaction.blocking_reason_code = "waiting_sequence".to_string();
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

        if control == OrderControlState::Active
            && (state == queue_state::ApparatusQueueOrderState::Paused
                || (state == queue_state::ApparatusQueueOrderState::Pending && requeued_session))
            && let Some(session) = active_session.filter(|session| session.needs_print_transfer_material_check())
            && let Some(rule) = live_material_rule(station.canonical).filter(|rule| rule.requires_material)
            && !self.print_transfer_can_reuse_materials(
                session, &rule, station.material_assignments_by_order.get(order_id).map(Vec::as_slice).unwrap_or_default(),
            ).await?
        {
            interaction.start_materials_mode = ApparatusQueueStartMaterialsMode::ScanRequired;
            interaction.material_scan_required = true;
        }

        if control == OrderControlState::Active
            && apparatus::requires_qolip_scan(station.canonical)
            && active_session.is_some_and(|session| session.qolip_reacquisition_required())
            && matches!(state, queue_state::ApparatusQueueOrderState::Pending
                | queue_state::ApparatusQueueOrderState::Paused)
        {
            interaction.qolip_mode = ApparatusQueueQolipMode::ScanRequired;
            interaction.qolip_automatic_confirmation = station.qolip_start_settings.automatic_confirmation_enabled;
            interaction.qolip_policy_revision = station.qolip_start_settings.revision;
        }

        let mut allowed_actions = allowed_actions_for_control(QueueActionPolicyInput {
            state,
            profile: QueueActionPolicyProfile::Live {
                order_control: control,
                is_rezka: station.is_rezka,
                merge_ready,
            },
            requeued_session,
            pending_actionable,
            queue_actionable,
            start_ready,
        });
        let closing_output_batch_id = if state
            == queue_state::ApparatusQueueOrderState::Paused
            && control == OrderControlState::Active
            && pechat::is_pechat_apparatus(station.canonical)
        {
            active_session
                .and_then(|session| bosma_closing_output(session, batches))
                .map(|batch| batch.batch_id.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
        if !closing_output_batch_id.is_empty() {
            allowed_actions.push(queue_state::ApparatusQueueAction::Complete);
        }

        if active_session.is_some_and(|session| {
            session
                .payload_json
                .get("rezka_output_report")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|frames| !frames.is_empty())
        }) {
            allowed_actions
                .retain(|action| *action != queue_state::ApparatusQueueAction::Merge);
        }

        let Some(SnapshotOrderOutput {
            rezka_input_lineage, rezka_active_partial_rolls, rezka_output_kadr_counts,
        }) = station.order_output(input, &stage_node_id, stage_projection.input_contained_kadr_count)
        else {
            return Ok(None);
        };
        Ok(Some(ApparatusQueueOrderActionControl {
            reclose: if state == queue_state::ApparatusQueueOrderState::Paused
                && control == OrderControlState::Active {
                active_session.and_then(super::super::reactivation::reclose_control)
            } else { None },
            re_pause: if state == queue_state::ApparatusQueueOrderState::InProgress
                && control == OrderControlState::Active {
                active_session.and_then(super::super::re_pause::control)
            } else { None },
            cancel_start: if self.store.supports_cancel_start()
                && (station.is_laminatsiya || station.is_rezka)
                && state == queue_state::ApparatusQueueOrderState::InProgress
                && control == OrderControlState::Active {
                active_session.and_then(super::super::cancel_start::control)
            } else { None },
            temporary_pause: if self.store.supports_temporary_pause()
                && state == queue_state::ApparatusQueueOrderState::InProgress
                && control == OrderControlState::Active {
                active_session.and_then(super::super::temporary_pause::control)
            } else { None },
            temporarily_paused: state == queue_state::ApparatusQueueOrderState::Paused
                && active_session.is_some_and(super::super::temporary_pause::is_temporarily_paused),
            stage_work,
            work_activity: ApparatusQueueWorkActivity::from_session(
                active_session,
                state,
                &stage_node_id,
            ),
            last_worked_at_unix: station.active_sessions_by_order
                .get(order_id)
                .into_iter()
                .flatten()
                .filter(|session| {
                    session.apparatus == station.storage_key
                        && (session.stage_node_id.trim().is_empty()
                            || session.stage_node_id.trim() == stage_node_id.trim())
                })
                .map(|session| session.updated_at_unix.max(session.started_at_unix))
                .max()
                .unwrap_or(0)
                .max(0),
            state,
            allowed_actions,
            interaction,
            previous_stage: previous_stage.unwrap_or_default(),
            stage_node_id,
            previous_stage_ready,
            rezka_total_waste_only,
            rezka_output_kadr_counts: rezka_output_kadr_counts.clone(),
            rezka_output_report: if station.is_rezka {
                active_session
                    .map(|session| {
                        serde_json::json!({
                            "layout_outputs": super::super::layout::output_layout(order_map, session, &rezka_output_kadr_counts),
                            "peremotka_supported": true,
                            "cycle_id": session.payload_json.get("rezka_output_cycle")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or(&session.session_id),
                            "frames": session.payload_json.get("rezka_output_report")
                                .cloned().unwrap_or_else(|| serde_json::json!([])),
                            "reference_frame_index": super::super::rezka_reference_frame_index(&session.payload_json),
                        })
                    })
                    .unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            },
            rezka_input_lineage,
            rezka_active_partial_rolls,
            complete_requires_full_report,
            closing_output_batch_id,
            complete_requires_rezka_total_waste_only,
            freeze_request: order_control.and_then(|control| control.freeze_request.clone().map(|mut request| {
                request.scope = if control.stage_node_id.is_empty() { "order" } else { "stage" }.to_string();
                request.stage_node_id = control.stage_node_id.clone();
                request
            })),
            stage_control_state: stage_control.map(|c| c.state.as_str().to_string()).unwrap_or_default(),
            print_preflight_allowed: station.is_bosma
                && state == queue_state::ApparatusQueueOrderState::Pending
                && pending_actionable
                && !requeued_session,
            print_preflight,
        }))
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
}
