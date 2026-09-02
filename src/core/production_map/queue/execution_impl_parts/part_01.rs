impl ProductionMapService {
    pub(crate) async fn prepare_apparatus_queue_action_with_progress(
        &self,
        apparatus: &str,
        order_id: &str,
        action: queue_state::ApparatusQueueAction,
        assigned_apparatus: &[String],
        actor: QueueActionActor,
        progress: QueueProgressInput,
    ) -> Result<PreparedApparatusQueueAction, ProductionMapError> {
        let apparatus = apparatus.trim();
        let order_id = order_id.trim();
        validate_queue_action_request(apparatus, order_id, assigned_apparatus)?;
        let control = self.order_control_state(order_id).await?;
        let requested_action = action;
        let freeze_request_finalization = control.state == OrderControlState::FreezeRequested
            && (requested_action.creates_resumable_output()
                || requested_action == queue_state::ApparatusQueueAction::Freeze);
        let freeze_request_safe_stop = control.state == OrderControlState::FreezeRequested
            && requested_action.creates_resumable_output();
        let freeze_request_safe_stop_has_output = progress.has_reported_output();
        let freeze_request_safe_stop_with_issue = freeze_request_safe_stop
            && !freeze_request_safe_stop_has_output
            && !progress.description.trim().is_empty();
        let freeze_with_issue = progress.freeze_with_issue
            || (requested_action == queue_state::ApparatusQueueAction::Freeze
                && control.state == OrderControlState::Active);
        let action = if freeze_with_issue || freeze_request_finalization {
            queue_state::ApparatusQueueAction::Freeze
        } else {
            requested_action
        };
        let mut progress = progress;
        progress.freeze_with_issue = freeze_with_issue;
        if freeze_with_issue {
            if action != queue_state::ApparatusQueueAction::Freeze
                || !actor.role.trim().eq_ignore_ascii_case("aparatchi")
            {
                return Err(ProductionMapError::OrderControlActionNotAllowed);
            }
            if progress.description.trim().is_empty()
                || !progress.freeze_request_id.trim().is_empty()
                || progress.worker_handoff
                || progress.remove_roll_from_apparatus
            {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
            match control.state {
                OrderControlState::Active => {}
                OrderControlState::FreezeRequested => {
                    return Err(ProductionMapError::OrderFreezeRequested);
                }
                OrderControlState::Frozen => return Err(ProductionMapError::OrderFrozen),
            }
        }
        validate_freeze_request_pause(
            &control,
            apparatus,
            requested_action,
            &actor,
            &progress.freeze_request_id,
        )?;
        match control.state {
            OrderControlState::Active => {}
            OrderControlState::FreezeRequested
                if action == queue_state::ApparatusQueueAction::Freeze => {}
            OrderControlState::FreezeRequested => {
                return Err(ProductionMapError::OrderFreezeRequested);
            }
            OrderControlState::Frozen => return Err(ProductionMapError::OrderFrozen),
        }
        let sequences = self.store.apparatus_sequences().await?;
        let all_states = self.store.apparatus_queue_states().await?;
        let order_controls = self.store.order_control_states().await?;
        if action == queue_state::ApparatusQueueAction::Freeze
            && control.state == OrderControlState::Active
            && order_has_frozen_queue_state(&all_states, order_id)
        {
            return Err(ProductionMapError::OrderFrozen);
        }
        let known_keys = known_apparatus_storage_keys(&sequences, &all_states);
        let storage_key = queue_state::resolve_apparatus_storage_key(apparatus, &known_keys);
        let canonical = self.resolve_canonical_apparatus_text(&storage_key).await?;
        let policy = effective_apparatus_queue_policy(canonical.as_ref());
        let stored_sequence = sequences
            .get(&storage_key)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let all_maps = self.store.maps().await?;
        let visible_order_ids = visible_order_ids_for_apparatus(&all_maps, apparatus);
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
        let stored_states = all_states.get(&storage_key);
        if let Some(stored_states) = stored_states {
            validate_requested_queue_state(stored_states, order_id)?;
        }
        let order_map = all_maps
            .iter()
            .find(|map| map.id.trim() == order_id)
            .ok_or(ProductionMapError::MapNotFound)?;
        let mut effective_order_map = order_map.clone();
        let claimed_alternative_map = if action == queue_state::ApparatusQueueAction::Start
            && claim_unassigned_alternative_apparatus_assignment(
                &mut effective_order_map,
                apparatus,
            ) {
            Some(effective_order_map.clone())
        } else {
            None
        };
        let order_map = &effective_order_map;
        ensure_previous_stage_is_configured(action, order_map, apparatus, canonical.as_ref())?;
        let previous_progress_ready = self
            .previous_progress_ready_for_action(action, order_id, order_map, apparatus, &progress)
            .await?;
        let mut parsed = stored_states.map(parsed_queue_states).unwrap_or_default();
        let stage_reentry = action == queue_state::ApparatusQueueAction::Start
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
        let active_stage_node_id = active_session
            .as_ref()
            .and_then(|session| session.payload_json.get("stage_node_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let completion_read_snapshot = if action == queue_state::ApparatusQueueAction::Complete {
            Some(
                self.completion_progress_build_snapshot(order_id, &progress, active_session.clone())
                    .await?,
            )
        } else {
            None
        };
        if freeze_request_finalization {
            validate_freeze_request_target_session(&control, active_session.as_ref())?;
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
        if requeued_session && action == queue_state::ApparatusQueueAction::Start {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let requeued_resume = requeued_session
            && action == queue_state::ApparatusQueueAction::Resume
            && from_state == queue_state::ApparatusQueueOrderState::Pending;
        let remove_roll_from_apparatus = action == queue_state::ApparatusQueueAction::DetachRoll
            && progress.remove_roll_from_apparatus;
        if remove_roll_from_apparatus {
            if !apparatus::is_laminatsiya_apparatus(&canonical)
                || from_state != queue_state::ApparatusQueueOrderState::Paused
            {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
            if policy == ApparatusQueuePolicy::StrictSequence
                && queue_state::first_actionable_order_id(&sequence, &parsed) != Some(order_id)
            {
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
                    previous_progress_ready,
                    &sequence,
                    &mut parsed,
                    order_id,
                    action,
                )?;
            }
        }
        if matches!(
            action,
            queue_state::ApparatusQueueAction::Start | queue_state::ApparatusQueueAction::Resume
        ) {
            self.ensure_apparatus_execution_capacity(&storage_key, order_id, &all_states)
                .await?;
        }
        let to_state = parsed
            .get(order_id)
            .copied()
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        let mut saved = serialized_queue_states(parsed);
        let mut event = queue_action_event(QueueActionEventInput {
            requested_apparatus: apparatus,
            storage_key: &storage_key,
            order_id,
            stage_node_id: &active_stage_node_id,
            action,
            from_state,
            to_state,
            policy,
            actor: &actor,
            assigned_apparatus,
            sequence: &sequence,
            visible_order_ids: &visible_order_ids,
        });
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
        if freeze_with_issue {
            let issue_note = progress.description.trim();
            event.payload_json["freeze_with_issue"] = serde_json::json!(true);
            event.payload_json["issue_note"] = serde_json::json!(issue_note);
            event.payload_json["description"] = serde_json::json!(issue_note);
        }
        if freeze_request_finalization {
            event.payload_json["admin_freeze_finalization"] = serde_json::json!(true);
        }
        if freeze_request_safe_stop {
            event.payload_json["freeze_request_safe_stop"] = serde_json::json!(true);
            event.payload_json["freeze_request_id"] =
                serde_json::json!(progress.freeze_request_id.trim());
            if freeze_request_safe_stop_with_issue {
                let issue_note = progress.description.trim();
                event.payload_json["freeze_with_issue"] = serde_json::json!(true);
                event.payload_json["issue_note"] = serde_json::json!(issue_note);
                event.payload_json["description"] = serde_json::json!(issue_note);
            }
        }
        if action == queue_state::ApparatusQueueAction::Complete
            && (apparatus::is_laminatsiya_apparatus(&canonical)
                || apparatus::is_rezka_apparatus(&canonical))
            && !progress.force_full_completion_metrics
        {
            let read_snapshot = completion_read_snapshot
                .as_ref()
                .ok_or(ProductionMapError::StoreFailed)?;
            let input_batch_id = Self::completion_input_batch_id_from_snapshot(
                &progress,
                read_snapshot,
            );
            progress.allow_partial_station_completion = has_unprocessed_previous_wips_from_sources(
                order_id,
                order_map,
                &storage_key,
                canonical.as_ref(),
                &all_states,
                &read_snapshot.progress_batches,
                &[],
                &read_snapshot.opening_wip_records,
                &[],
                &input_batch_id,
                &active_stage_node_id,
            );
        }
        let progress_action = if freeze_request_safe_stop && !freeze_request_safe_stop_with_issue {
            queue_state::ApparatusQueueAction::DetachRoll
        } else {
            action
        };
        let mut progress = self
            .build_progress_records_with_snapshot(
                &storage_key,
                order_id,
                order_map,
                progress_action,
                &actor,
                progress,
                canonical.as_ref(),
                completion_read_snapshot.as_ref(),
            )
            .await?;
        if event.stage_node_id.is_empty()
            && let Some(stage_node_id) = progress
                .session
                .as_ref()
                .and_then(|session| session.payload_json.get("stage_node_id"))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        {
            event.stage_node_id = stage_node_id.to_string();
        }
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
        let has_unprocessed_previous_wips = if action
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
            downgrade_completed_state_to_pending(order_id, &mut saved, &mut event);
        }
        let order_control_update = if freeze_with_issue {
            let session = progress
                .session
                .as_ref()
                .ok_or(ProductionMapError::OrderFreezeTargetNotFound)?;
            let now = progress::unix_seconds();
            Some(OrderControlRecord {
                order_id: order_id.to_string(),
                state: OrderControlState::Frozen,
                actor: actor.clone(),
                requested_at_unix: now,
                frozen_at_unix: Some(now),
                freeze_request: Some(OrderFreezeRequest {
                    request_id: format!("order-freeze-issue:{}", event.event_id),
                    status: OrderFreezeRequestStatus::Frozen,
                    target_session_id: session.session_id.clone(),
                    target_apparatus: storage_key.clone(),
                    target_worker_role: actor.role.trim().to_string(),
                    target_worker_ref: actor.ref_.trim().to_string(),
                    target_worker_display_name: actor.display_name.trim().to_string(),
                    requested_at_unix: now,
                    transitioned_at_unix: now,
                }),
            })
        } else if control.state == OrderControlState::FreezeRequested
            && action == queue_state::ApparatusQueueAction::Freeze
        {
            let now = progress::unix_seconds();
            let mut freeze_request = control
                .freeze_request
                .ok_or(ProductionMapError::OrderFreezeRequestMismatch)?;
            freeze_request.status = OrderFreezeRequestStatus::Frozen;
            freeze_request.transitioned_at_unix = now;
            Some(OrderControlRecord {
                order_id: order_id.to_string(),
                state: OrderControlState::Frozen,
                actor: control.actor,
                requested_at_unix: control.requested_at_unix,
                frozen_at_unix: Some(now),
                freeze_request: Some(freeze_request),
            })
        } else {
            None
        };
        let sequence_updates = if action == queue_state::ApparatusQueueAction::Freeze {
            let mut excluded_order_ids = frozen_order_ids;
            excluded_order_ids.insert(order_id.to_string());
            sequence_updates_for_frozen_transition(&all_maps, &sequences, &excluded_order_ids, None)
        } else {
            BTreeMap::new()
        };
        Ok(PreparedApparatusQueueAction {
            apparatus: storage_key,
            states: saved,
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
        })
    }

    async fn previous_progress_ready_for_action(
        &self,
        action: queue_state::ApparatusQueueAction,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        progress: &QueueProgressInput,
    ) -> Result<bool, ProductionMapError> {
        if action != queue_state::ApparatusQueueAction::Start {
            return Ok(false);
        }
        if self
            .opening_wip_start_batch(order_id, order_map, apparatus, progress)
            .await?
            .is_some()
        {
            return Ok(true);
        }
        Ok(self
            .previous_stage_start_progress_batch(order_id, order_map, apparatus, progress)
            .await?
            .is_some())
    }
}

fn ensure_previous_stage_is_configured(
    action: queue_state::ApparatusQueueAction,
    order_map: &ProductionMapDefinition,
    apparatus: &str,
    canonical: &crate::core::apparatus_standard::RuntimeApparatusConfiguration,
) -> Result<(), ProductionMapError> {
    if action != queue_state::ApparatusQueueAction::Freeze
        && apparatus::requires_previous_stage(canonical)
        && chain::previous_stage_resolution_is_unavailable(order_map, apparatus)
    {
        return Err(ProductionMapError::ProgressQrRequired);
    }
    Ok(())
}
