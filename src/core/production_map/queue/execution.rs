struct QueueActionIntent {
    hold_stage: String,
    control: OrderControlRecord,
    queue_action: queue_state::ApparatusQueueAction,
    progress_action: queue_state::ApparatusQueueAction,
    freeze_request_finalization: bool,
    freeze_request_safe_stop: bool,
    freeze_request_safe_stop_has_output: bool,
    freeze_request_safe_stop_with_issue: bool,
    freeze_with_issue: bool,
    resume_pause_allowed: bool,
}

impl ProductionMapService {
    pub(crate) async fn prepare_apparatus_queue_action_with_progress(
        &self,
        apparatus: &str,
        order_id: &str,
        requested_action: queue_state::ApparatusQueueAction,
        assigned_apparatus: &[String],
        actor: QueueActionActor,
        mut progress: QueueProgressInput,
    ) -> Result<PreparedApparatusQueueAction, ProductionMapError> {
        let apparatus = apparatus.trim();
        let order_id = order_id.trim();
        validate_queue_action_request(apparatus, order_id, assigned_apparatus)?;
        let QueueActionIntent {
            hold_stage, control, queue_action, progress_action, freeze_request_finalization,
            freeze_request_safe_stop, freeze_request_safe_stop_has_output,
            freeze_request_safe_stop_with_issue, freeze_with_issue, resume_pause_allowed,
        } = self.queue_action_intent(apparatus, order_id, requested_action, &actor, &mut progress).await?;
        // Independent reads under the same mutation guard. Keep every domain
        // check and the transaction's final state validation, without adding
        // four database round trips to the worker's critical path in series.
        let (sequences, all_states, order_controls, all_maps) = tokio::try_join!(
            self.store.apparatus_sequences(),
            self.store.apparatus_queue_states(),
            self.store.order_control_states(),
            async {
                // Freeze edits sequences across stations; it retains the full
                // map set. Normal actions need only candidate station maps.
                if queue_action == queue_state::ApparatusQueueAction::Freeze {
                    self.store.maps().await
                } else {
                    self.store.maps_for_apparatus(apparatus).await
                }
            },
        )?;
        let storage_key = apparatus.to_string();
        let canonical = self.resolve_canonical_apparatus_text(&storage_key).await?;
        if progress.rezka_record_frame_index.is_some()
            && (!apparatus::is_rezka_apparatus(&canonical)
                || queue_action != queue_state::ApparatusQueueAction::RollComplete
                || progress.worker_handoff || progress.remove_roll_from_apparatus
                || progress.freeze_with_issue || !progress.freeze_request_id.is_empty())
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let policy = effective_apparatus_queue_policy(canonical.as_ref());
        let stored_sequence = sequences
            .get(&storage_key)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let visible_order_ids = apparatus::selected_order_ids_for_apparatus(&all_maps, &canonical);
        let frozen_order_ids = order_controls
            .iter()
            .filter_map(|(id, control)| {
                (control.state == OrderControlState::Frozen).then_some(id.clone())
            })
            .collect::<BTreeSet<_>>();
        let sequence = queue_state::effective_apparatus_sequence_excluding(
            stored_sequence,
            &visible_order_ids,
            &frozen_order_ids,
        );
        if !sequence.iter().any(|id| id.trim() == order_id) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let mut occurrence_states = all_states.get(&storage_key).cloned().unwrap_or_default();
        if occurrence_states.get(order_id).is_some_and(|s| s == "frozen")
            && control.state == OrderControlState::Active
            && self.store.stage_order_control_states().await?.iter().any(|c| c.order_id == order_id
                && c.stage_node_id != hold_stage && c.state == OrderControlState::Frozen
                && c.freeze_request.as_ref().is_some_and(|r| r.target_apparatus == storage_key)) {
            occurrence_states.insert(order_id.to_string(), "pending".to_string());
        }
        let stored_states = Some(&occurrence_states);
        validate_requested_queue_state(&occurrence_states, order_id)?;
        let order_map = all_maps
            .iter()
            .find(|map| map.id.trim() == order_id)
            .ok_or(ProductionMapError::MapNotFound)?;
        if !apparatus::print_assignment_allows_order(order_map, &canonical) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        // Alternative candidates share the operation, not its input rolls.
        // The transaction claims the scanned WIP; never claim the whole map.
        let claimed_alternative_map = None;
        ensure_previous_stage_is_configured(queue_action, order_map, apparatus, canonical.as_ref())?;
        let previous_progress_ready = self
            .previous_progress_ready_for_action(queue_action, order_id, order_map, apparatus, &progress)
            .await?;
        let mut parsed = stored_states.map(parsed_queue_states).unwrap_or_default();
        let stage_reentry = queue_action == queue_state::ApparatusQueueAction::Start
            && previous_progress_ready
            && parsed.get(order_id)
                == Some(&queue_state::ApparatusQueueOrderState::Completed);
        if stage_reentry {
            parsed.insert(
                order_id.to_string(),
                queue_state::ApparatusQueueOrderState::Pending,
            );
        }
        let from_state = parsed
            .get(order_id)
            .copied()
            .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
        let active_session = self
            .store
            .active_order_run_session(&storage_key, order_id)
            .await?;
        let print_preflight_hold = if control.state == OrderControlState::FreezeRequested
            && queue_action == queue_state::ApparatusQueueAction::Freeze
            && from_state == queue_state::ApparatusQueueOrderState::PrintPreflight
        {
            self.store
                .active_print_preflight_holds()
                .await?
                .into_iter()
                .find(|hold| {
                    hold.order_id.trim() == order_id
                        && queue_state::apparatus_ids_match(&hold.apparatus, &storage_key)
                        && matches!(
                            hold.status,
                            PrintPreflightStatus::Held | PrintPreflightStatus::Running
                        )
                })
        } else {
            None
        };
        let print_preflight_freeze = print_preflight_hold.is_some();
        if control.state == OrderControlState::FreezeRequested
            && queue_action == queue_state::ApparatusQueueAction::Freeze
            && from_state == queue_state::ApparatusQueueOrderState::PrintPreflight
            && !print_preflight_freeze
        {
            return Err(ProductionMapError::OrderFreezeRequestMismatch);
        }
        let active_stage_node_id = active_session
            .as_ref()
            .map(|session| session.stage_node_id.trim().to_string())
            .unwrap_or_default();
        let closing_output_batch_id = if progress.complete_without_output {
            if queue_action != queue_state::ApparatusQueueAction::Complete
                || from_state != queue_state::ApparatusQueueOrderState::Paused
                || !pechat::is_pechat_apparatus(&canonical)
            {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
            validate_bosma_closing_metrics(&progress)?;
            let session = active_session.as_ref().ok_or(ProductionMapError::QueueActionNotAllowed)?;
            let batches = self.store.progress_batches_for_order(order_id).await?;
            let output = bosma_closing_output(session, &batches)
                .ok_or(ProductionMapError::QueueActionNotAllowed)?;
            if progress.progress_batch_id.trim() != output.batch_id {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
            let id = output.batch_id.clone();
            // This id is an optimistic closing anchor, not a new input roll.
            progress.progress_batch_id.clear();
            Some(id)
        } else { None };
        if queue_action == queue_state::ApparatusQueueAction::Merge
            && active_session.as_ref().is_some_and(|session| session.payload_json
                .get("rezka_output_report").and_then(serde_json::Value::as_array)
                .is_some_and(|frames| !frames.is_empty()))
        {
            return Err(ProductionMapError::RezkaOutputCycleConflict);
        }
        let completion_read_snapshot = if queue_action == queue_state::ApparatusQueueAction::Complete {
            Some(
                self.completion_progress_build_snapshot(order_id, &progress, active_session.clone())
                    .await?,
            )
        } else {
            None
        };
        if freeze_request_finalization {
            if !print_preflight_freeze {
                validate_freeze_request_target_session(&control, active_session.as_ref())?;
            }
        }
        if freeze_request_safe_stop
            && !freeze_request_safe_stop_has_output
            && !freeze_request_safe_stop_with_issue
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        if freeze_request_safe_stop
            && freeze_request_safe_stop_has_output
            && !progress.has_complete_freeze_safe_stop_output(apparatus::is_rezka_apparatus(
                &canonical,
            ))
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let requeued_session = active_session
            .as_ref()
            .is_some_and(order_run_session_was_requeued);
        if requeued_session && queue_action == queue_state::ApparatusQueueAction::Start {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let requeued_resume = requeued_session
            && queue_action == queue_state::ApparatusQueueAction::Resume
            && from_state == queue_state::ApparatusQueueOrderState::Pending;
        let remove_roll_from_apparatus = queue_action == queue_state::ApparatusQueueAction::DetachRoll
            && progress.remove_roll_from_apparatus;
        if closing_output_batch_id.is_some() {
            // This releases only this paused order; another running order is untouched.
            parsed.insert(order_id.to_string(), queue_state::ApparatusQueueOrderState::Completed);
        } else if remove_roll_from_apparatus {
            if !apparatus::uses_lamination_workflow(&canonical)
                || from_state != queue_state::ApparatusQueueOrderState::Paused
            {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
            if parsed.iter().any(|(id, state)| id != order_id && state.is_active()) {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
            parsed.insert(
                order_id.to_string(),
                queue_state::ApparatusQueueOrderState::Paused,
            );
        } else {
            if requeued_resume {
                apply_requeued_resume(policy, &sequence, &mut parsed, order_id)?;
            } else {
                apply_queue_policy(
                    policy,
                    &sequence,
                    &mut parsed,
                    order_id,
                    queue_action,
                )?;
            }
        }
        if matches!(
            queue_action,
            queue_state::ApparatusQueueAction::Start | queue_state::ApparatusQueueAction::Resume
        ) {
            self.ensure_apparatus_execution_capacity(&storage_key, order_id, &all_states)
                .await?;
        }
        let to_state = parsed
            .get(order_id)
            .copied()
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        let mut event = queue_action_event(QueueActionEventInput {
            requested_apparatus: apparatus,
            storage_key: &storage_key,
            order_id,
            stage_node_id: &active_stage_node_id,
            action: queue_action,
            from_state,
            to_state,
            policy,
            actor: &actor,
            assigned_apparatus,
            sequence: &sequence,
            visible_order_ids: &visible_order_ids,
        });
        if matches!(queue_action, queue_state::ApparatusQueueAction::Start | queue_state::ApparatusQueueAction::Resume) {
            event.payload_json["queue_expected_sequence"] = serde_json::json!(stored_sequence);
            event.payload_json["queue_expected_states"] = serde_json::json!(all_states.get(&storage_key).cloned().unwrap_or_default());
            event.payload_json["queue_expected_policy_revision"] = serde_json::json!(canonical.queue.source_revision);
        }
        if let Some(batch_id) = &closing_output_batch_id {
            let session = active_session.as_ref().ok_or(ProductionMapError::QueueActionNotAllowed)?;
            event.payload_json["complete_without_output"] = serde_json::json!(true);
            event.payload_json["closing_output_batch_id"] = serde_json::json!(batch_id);
            event.payload_json["bosma_closing_session_id"] = serde_json::json!(session.session_id);
            event.payload_json["bosma_closing_expected_payload"] = session.payload_json.clone();
        }
        if apparatus::is_rezka_apparatus(&canonical)
            && let Some(session) = &active_session
        {
            event.payload_json["rezka_expected_session_id"] = serde_json::json!(session.session_id);
            event.payload_json["rezka_expected_output_revision"] = session.payload_json
                .get("rezka_output_revision").cloned().unwrap_or_else(|| serde_json::json!(0));
        }
        if stage_reentry {
            event.from_state = queue_state::ApparatusQueueOrderState::Completed;
            event.payload_json["stage_reentry"] = serde_json::json!(true);
        }
        if progress.worker_handoff {
            event.payload_json["worker_handoff"] = serde_json::json!(true);
        }
        if progress.remove_roll_from_apparatus {
            event.payload_json["roll_removed_from_apparatus"] = serde_json::json!(true);
        }
        if freeze_with_issue || freeze_request_safe_stop_with_issue {
            let issue_note = progress.description.trim();
            event.payload_json["freeze_with_issue"] = serde_json::json!(true);
            event.payload_json["issue_note"] = serde_json::json!(issue_note);
            event.payload_json["description"] = serde_json::json!(issue_note);
        }
        if freeze_request_finalization {
            event.payload_json["admin_freeze_finalization"] = serde_json::json!(true);
        }
        if print_preflight_freeze {
            event.payload_json["print_preflight_freeze"] = serde_json::json!(true);
        }
        if freeze_request_safe_stop {
            event.payload_json["freeze_request_safe_stop"] = serde_json::json!(true);
            event.payload_json["freeze_request_id"] =
                serde_json::json!(progress.freeze_request_id.trim());
        }
        self.set_partial_station_completion(
            &event, order_map, canonical.as_ref(), &all_states,
            completion_read_snapshot.as_ref(), &mut progress,
        ).await?;
        let local_report_submitted = queue_action == queue_state::ApparatusQueueAction::Complete
            && !progress.allow_partial_station_completion;
        let framed_rezka_output = !progress.rezka_frames.is_empty()
            && apparatus::is_rezka_apparatus(&canonical);
        let mut progress = if print_preflight_freeze {
            QueueProgressRecords {
                session: None,
                progress_event: None,
                progress_batch: None,
                progress_batches: Vec::new(),
                progress_batch_updates: Vec::new(),
                opening_wip_batch_updates: Vec::new(),
            }
        } else {
            self.build_progress_records_with_snapshot(
                &storage_key,
                order_id,
                order_map,
                progress_action,
                &actor,
                progress,
                canonical.as_ref(),
                completion_read_snapshot.as_ref(),
            )
            .await?
        };
        attach_queue_progress_witnesses(
            &mut event, &mut progress, framed_rezka_output, order_map.include_bobina_in_net,
            local_report_submitted, closing_output_batch_id.as_deref(),
        );
        if freeze_request_safe_stop {
            mark_freeze_request_safe_stop_progress(
                &mut progress,
                control
                    .freeze_request
                    .as_ref()
                    .map(|request| request.request_id.as_str())
                    .unwrap_or_default(),
                freeze_request_safe_stop_with_issue,
            );
        }
        if (freeze_request_finalization || freeze_with_issue) && !print_preflight_freeze {
            let original = active_session.as_ref()
                .ok_or(ProductionMapError::OrderFreezeTargetNotFound)?;
            // Safe stopping another shift's work is not a worker handoff.
            // Keep production ownership; the queue/progress events record
            // the actual operator who stopped it.
            let owner = serde_json::json!({
                "role": original.worker_role,
                "ref": original.worker_ref,
                "display_name": original.worker_display_name,
            });
            event.payload_json["freeze_session_owner"] = owner.clone();
            preserve_freeze_work_owner(&mut progress, original, &actor, &owner);
        }
        let has_unprocessed_previous_wips = if queue_action
            == queue_state::ApparatusQueueAction::Complete
            && to_state == queue_state::ApparatusQueueOrderState::Completed
        {
            let read_snapshot = completion_read_snapshot
                .as_ref()
                .ok_or(ProductionMapError::StoreFailed)?;
            has_unprocessed_previous_wips_from_sources(
                order_id,
                order_map,
                &storage_key,
                canonical.as_ref(),
                &all_states,
                &read_snapshot.progress_batches,
                &progress.progress_batch_updates,
                &read_snapshot.opening_wip_records,
                &progress.opening_wip_batch_updates,
                "",
                &active_stage_node_id,
            )
        } else {
            false
        };
        if has_unprocessed_previous_wips {
            let pending = queue_state::ApparatusQueueOrderState::Pending;
            parsed.insert(order_id.to_string(), pending);
            event.to_state = pending;
            event.payload_json["to_state"] = serde_json::json!(pending.as_str());
            event.payload_json["batch_complete_order_state"] = serde_json::json!("pending");
        }
        let order_control_update = self.freeze_control_update_for_action(
            &event, progress.session.as_ref(), control, freeze_with_issue,
        ).await?;
        let sequence_updates = if queue_action == queue_state::ApparatusQueueAction::Freeze {
            let mut excluded_order_ids = frozen_order_ids;
            excluded_order_ids.insert(order_id.to_string());
            let mut updates = sequence_updates_for_frozen_transition(&all_maps, &sequences, &excluded_order_ids, None);
            if order_control_update.as_ref().is_some_and(|c| !c.stage_node_id.is_empty()) {
                updates.retain(|apparatus, _| apparatus == &storage_key);
            }
            updates
        } else {
            BTreeMap::new()
        };
        let mut prepared = PreparedApparatusQueueAction {
            apparatus: storage_key,
            states: serialized_queue_states(parsed),
            sequence_updates,
            event,
            session: progress.session,
            progress_event: progress.progress_event,
            progress_batch: progress.progress_batch,
            progress_batches: progress.progress_batches,
            progress_batch_updates: progress.progress_batch_updates,
            opening_wip_batch_updates: progress.opening_wip_batch_updates,
            material_scan_skipped: false,
            claimed_alternative_map,
            order_control_update,
            print_preflight_hold_id: None,
            print_preflight_cancel_hold_id: print_preflight_hold
                .map(|hold| hold.hold_id.trim().to_string()),
        };
        if queue_action == queue_state::ApparatusQueueAction::Start
            && apparatus::requires_previous_stage(&canonical)
        {
            prepared.event.payload_json["cancel_start_supported"] = serde_json::json!(true);
        }
        if resume_pause_allowed && let Some(previous) = active_session.as_ref() {
            // Mark the continuation as started before re-pause fingerprints it.
            // The persistence layer also enforces this latch for other writers.
            if let Some(session) = &mut prepared.session {
                if session.status == OrderRunStatus::Active && session.payload_json.get("_reactivation_reclose").is_some() {
                    session.payload_json["_reactivation_started"] = serde_json::json!(true);
                    session.payload_json.as_object_mut().map(|p| p.remove("_reactivation_reclose"));
                }
            }
            super::super::re_pause::attach_checkpoint(previous, &mut prepared);
        }
        if let Some(session) = &prepared.session {
            if session.stage_node_id != hold_stage {
                match self.effective_order_control_for_stage(order_id, apparatus, &session.stage_node_id).await?.state {
                    OrderControlState::Active => {},
                    OrderControlState::FreezeRequested => return Err(ProductionMapError::OrderFreezeRequested),
                    OrderControlState::Frozen => return Err(ProductionMapError::OrderFrozen),
                }
            }
        }
        Ok(prepared)
    }
}

include!("execution_parts/queue_action_intent.rs");

include!("execution_parts/completion_progress_build_snapshot.rs");

#[cfg(test)]
#[path = "execution_completion_tests.rs"]
mod execution_completion_tests;

include!("execution_parts/schedule_reservation_status_for_action.rs");
