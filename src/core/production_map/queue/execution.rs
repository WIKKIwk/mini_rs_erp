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
        if progress.rezka_record_frame_index.is_some()
            && (!apparatus::is_rezka_apparatus(&canonical)
                || action != queue_state::ApparatusQueueAction::RollComplete
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
            .map(|session| session.stage_node_id.trim().to_string())
            .unwrap_or_default();
        if action == queue_state::ApparatusQueueAction::Merge
            && active_session.as_ref().is_some_and(|session| session.payload_json
                .get("rezka_output_report").and_then(serde_json::Value::as_array)
                .is_some_and(|frames| !frames.is_empty()))
        {
            return Err(ProductionMapError::RezkaOutputCycleConflict);
        }
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
        if let Some(revision) = event.payload_json.get("rezka_expected_output_revision")
            .and_then(serde_json::Value::as_u64)
            && let Some(session) = &mut progress.session
        {
            session.payload_json["rezka_output_revision"] = serde_json::json!(revision + 1);
        }
        if event.stage_node_id.is_empty()
            && let Some(stage_node_id) = progress
                .session
                .as_ref()
                .map(|session| session.stage_node_id.trim())
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

impl ProductionMapService {

    async fn completion_progress_build_snapshot(
        &self,
        order_id: &str,
        progress: &QueueProgressInput,
        active_session: Option<OrderRunSession>,
    ) -> Result<ProgressBuildReadSnapshot, ProductionMapError> {
        let (progress_batches, opening_wip_records) = tokio::join!(
            self.store.progress_batches_for_order(order_id),
            self.store.opening_wip_records(OpeningWipQuery {
                order_id: order_id.trim().to_string(),
                wip_status: None,
                limit: 10_000,
            }),
        );
        let progress_batches = progress_batches?;
        let opening_wip_records = opening_wip_records?;
        let has_bulk_progress_batch = if !progress.progress_batch_id.trim().is_empty() {
            progress_batches
                .iter()
                .any(|batch| batch.batch_id.trim() == progress.progress_batch_id.trim())
        } else if !progress.qr_payload.trim().is_empty() {
            progress_batches
                .iter()
                .any(|batch| {
                    batch
                        .qr_payload
                        .trim()
                        .eq_ignore_ascii_case(progress.qr_payload.trim())
                })
        } else {
            false
        };
        let input_progress_batch = if has_bulk_progress_batch {
            None
        } else if !progress.progress_batch_id.trim().is_empty() {
            self
                .store
                .progress_batch(progress.progress_batch_id.trim())
                .await?
        } else if !progress.qr_payload.trim().is_empty() {
            self
                .store
                .progress_batch_by_qr(progress.qr_payload.trim())
                .await?
        } else {
            None
        };
        let has_progress_input = has_bulk_progress_batch || input_progress_batch.is_some();
        let has_bulk_opening_wip_batch = if !progress.progress_batch_id.trim().is_empty()
            || !progress.qr_payload.trim().is_empty()
        {
            opening_wip_records
                .iter()
                .any(|record| {
                    record.batches.iter().any(|batch| {
                        (!progress.progress_batch_id.trim().is_empty()
                            && batch.batch_id.trim() == progress.progress_batch_id.trim())
                            || (!progress.qr_payload.trim().is_empty()
                                && batch.qr_payload.trim() == progress.qr_payload.trim())
                    })
                })
        } else {
            false
        };
        let session_uses_opening_wip = active_session
            .as_ref()
            .is_some_and(|session| session_progress_links(session).source_kind == "opening_wip");
        let input_opening_wip_batch = if has_bulk_opening_wip_batch {
            None
        } else if (session_uses_opening_wip || !has_progress_input)
            && (!progress.progress_batch_id.trim().is_empty()
                || !progress.qr_payload.trim().is_empty())
        {
            self
                .store
                .opening_wip_batch(
                    progress.progress_batch_id.trim(),
                    progress.qr_payload.trim(),
                )
                .await?
        } else {
            None
        };
        Ok(ProgressBuildReadSnapshot {
            active_session,
            progress_batches,
            opening_wip_records,
            input_progress_batch,
            input_opening_wip_batch,
        })
    }

    fn completion_input_batch_id_from_snapshot(
        progress: &QueueProgressInput,
        snapshot: &ProgressBuildReadSnapshot,
    ) -> String {
        if !progress.progress_batch_id.trim().is_empty() {
            return progress.progress_batch_id.trim().to_string();
        }
        if !progress.qr_payload.trim().is_empty() {
            if let Some(batch) = snapshot
                .progress_batches
                .iter()
                .find(|batch| {
                    batch
                        .qr_payload
                        .trim()
                        .eq_ignore_ascii_case(progress.qr_payload.trim())
                })
                .or_else(|| {
                    snapshot.input_progress_batch.as_ref().filter(|batch| {
                        batch
                            .qr_payload
                            .trim()
                            .eq_ignore_ascii_case(progress.qr_payload.trim())
                    })
                })
            {
                return batch.batch_id.clone();
            }
            return snapshot
                .opening_wip_batch_id("", progress.qr_payload.trim())
                .unwrap_or_default()
                .to_string();
        }
        snapshot
            .active_session
            .as_ref()
            .map(session_progress_links)
            .map(|links| links.batch_id)
            .unwrap_or_default()
    }

    pub(crate) async fn commit_prepared_queue_action(
        &self,
        prepared: PreparedApparatusQueueAction,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        self.commit_prepared_queue_action_with_raw_material_stock(
            prepared,
            Vec::new(),
            Vec::new(),
            None,
        )
        .await
    }

    pub(crate) async fn commit_prepared_queue_action_with_raw_material_stock(
        &self,
        prepared: PreparedApparatusQueueAction,
        raw_material_stock_transitions: Vec<RawMaterialStockTransition>,
        qolip_checkouts: Vec<crate::core::qolip::QolipCheckout>,
        returned_paint_report: Option<crate::core::returned_paint::ReturnedPaintRequest>,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        let write = QueueActionProgressWrite {
            apparatus: prepared.apparatus,
            map_update: prepared.claimed_alternative_map,
            states: prepared.states,
            sequence_updates: prepared.sequence_updates,
            schedule_reservation_status: schedule_reservation_status_for_action(
                prepared.event.action,
            ),
            event: prepared.event,
            session: prepared.session,
            progress_event: prepared.progress_event,
            progress_batch: prepared.progress_batch,
            progress_batches: prepared.progress_batches,
            progress_batch_updates: prepared.progress_batch_updates,
            opening_wip_batch_updates: prepared.opening_wip_batch_updates,
            raw_material_stock_transitions,
            qolip_checkouts,
            returned_paint_report,
            order_control_update: prepared.order_control_update,
        };
        let write_result = self
            .store
            .put_apparatus_queue_states_with_event_and_progress(&write)
            .await?;
        let order_status = self.order_status_detail(&write.event.order_id).await?;
        self.notify_live();
        let QueueActionProgressWrite {
            states,
            session,
            progress_event,
            progress_batch,
            progress_batches,
            order_control_update,
            ..
        } = write;
        Ok(ApparatusQueueActionResult {
            states,
            order_status,
            order_control: order_control_update,
            session,
            progress_event,
            progress_batch,
            progress_batches,
            raw_material_stock_warehouses: write_result.raw_material_stock_warehouses,
            raw_material_stock_committed: write_result.raw_material_stock_committed,
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn has_unprocessed_previous_wips_from_sources(
    order_id: &str,
    order_map: &ProductionMapDefinition,
    apparatus: &str,
    canonical: &crate::core::apparatus_standard::RuntimeApparatusConfiguration,
    all_states: &ApparatusQueueStateMap,
    progress_batches: &[OrderProgressBatch],
    progress_batch_updates: &[OrderProgressBatch],
    opening_wip_records: &[OpeningWipRecord],
    opening_wip_batch_updates: &[OpeningWipBatch],
    ignored_batch_id: &str,
    stage_node_id: &str,
) -> bool {
    let mut opening_batches = opening_wip_records
        .iter()
        .filter(|record| {
            record.intake.status == OpeningWipIntakeStatus::Confirmed
                && ProductionMapService::opening_wip_target_stage(
                    order_map,
                    &record.intake,
                    apparatus,
                    stage_node_id,
                )
                .is_some()
        })
        .flat_map(|record| record.batches.iter())
        .map(|batch| (batch.batch_id.trim(), batch))
        .collect::<BTreeMap<_, _>>();
    for batch in opening_wip_batch_updates {
        opening_batches.insert(batch.batch_id.trim(), batch);
    }
    if !opening_batches.is_empty() {
        return opening_batches.values().any(|batch| {
            batch.batch_id.trim() != ignored_batch_id.trim()
                && matches!(
                    batch.wip_status,
                    OpeningWipBatchStatus::Waiting | OpeningWipBatchStatus::InUse
                )
        });
    }

    let mut batches = progress_batches
        .iter()
        .map(|batch| (batch.batch_id.trim(), batch))
        .collect::<BTreeMap<_, _>>();
    for batch in progress_batch_updates {
        batches.insert(batch.batch_id.trim(), batch);
    }
    has_unprocessed_previous_wips_from_batches(
        order_id,
        order_map,
        apparatus,
        canonical,
        all_states,
        batches.into_values(),
        ignored_batch_id,
        stage_node_id,
    )
}

#[allow(clippy::too_many_arguments)]
fn has_unprocessed_previous_wips_from_batches<'a>(
    order_id: &str,
    order_map: &ProductionMapDefinition,
    apparatus: &str,
    canonical: &crate::core::apparatus_standard::RuntimeApparatusConfiguration,
    all_states: &ApparatusQueueStateMap,
    batches: impl IntoIterator<Item = &'a OrderProgressBatch>,
    ignored_batch_id: &str,
    stage_node_id: &str,
) -> bool {
    let previous_apparatus = if stage_node_id.trim().is_empty() {
        chain::previous_work_stage_station(order_map, apparatus)
    } else {
        chain::previous_work_stage_for_node(order_map, stage_node_id)
            .and_then(|stage| stage.apparatus_id)
    };
    let Some(previous_apparatus) = previous_apparatus else {
        return false;
    };
    let requires_previous_stage_completion = apparatus::is_laminatsiya_apparatus(canonical)
        || apparatus::is_rezka_apparatus(canonical);
    let previous_stage_completed = all_states.iter().any(|(candidate, states)| {
        super::super::types::apparatus_ids_match(candidate, &previous_apparatus)
            && states
                .get(order_id)
                .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                == Some(queue_state::ApparatusQueueOrderState::Completed)
    });
    if requires_previous_stage_completion && !previous_stage_completed {
        return true;
    }
    batches
        .into_iter()
        .filter(|batch| {
            batch.order_id.trim() == order_id.trim()
                && super::super::types::apparatus_ids_match(
                    &batch.apparatus,
                    &previous_apparatus,
                )
                && chain::stage_ids_match_for_map(order_map, &batch.next_apparatus, apparatus)
                && (stage_node_id.trim().is_empty()
                    || progress_batch_next_stage_node_id(batch).is_empty()
                    || progress_batch_next_stage_node_id(batch) == stage_node_id.trim())
        })
        .any(|batch| {
            if !ignored_batch_id.trim().is_empty()
                && batch.batch_id.trim() == ignored_batch_id.trim()
            {
                return false;
            }
            batch.wip_status == OrderProgressBatchWipStatus::Waiting
                || (batch.wip_status == OrderProgressBatchWipStatus::InUse
                    && super::super::types::apparatus_ids_match(
                        &batch.used_by_apparatus,
                        apparatus,
                    ))
                || wip_batch_was_consumed_by_producer(batch)
        })
}


fn schedule_reservation_status_for_action(
    action: queue_state::ApparatusQueueAction,
) -> Option<ApparatusScheduleStatus> {
    Some(match action {
        queue_state::ApparatusQueueAction::Start | queue_state::ApparatusQueueAction::Resume => {
            ApparatusScheduleStatus::Active
        }
        queue_state::ApparatusQueueAction::Pause
        | queue_state::ApparatusQueueAction::Freeze
        | queue_state::ApparatusQueueAction::DetachRoll => ApparatusScheduleStatus::Paused,
        queue_state::ApparatusQueueAction::Merge => ApparatusScheduleStatus::Active,
        queue_state::ApparatusQueueAction::RollComplete => ApparatusScheduleStatus::Active,
        queue_state::ApparatusQueueAction::Complete => ApparatusScheduleStatus::Completed,
    })
}

fn validate_freeze_request_pause(
    control: &OrderControlRecord,
    apparatus: &str,
    action: queue_state::ApparatusQueueAction,
    actor: &QueueActionActor,
    supplied_request_id: &str,
) -> Result<(), ProductionMapError> {
    let supplied_request_id = supplied_request_id.trim();
    if control.state != OrderControlState::FreezeRequested {
        if supplied_request_id.is_empty() {
            return Ok(());
        }
        return Err(ProductionMapError::OrderFreezeRequestMismatch);
    }
    if !action.creates_resumable_output()
        && action != queue_state::ApparatusQueueAction::Freeze
    {
        return Ok(());
    }
    let request = control
        .freeze_request
        .as_ref()
        .ok_or(ProductionMapError::OrderFreezeRequestMismatch)?;
    let request_id_matches = !supplied_request_id.is_empty()
        && request.request_id.trim() == supplied_request_id
        && request.status == OrderFreezeRequestStatus::Pending;
    let worker_matches = request.target_worker_role.trim() == actor.role.trim()
        && request.target_worker_ref.trim() == actor.ref_.trim();
    let apparatus_matches =
        super::super::types::apparatus_ids_match(&request.target_apparatus, apparatus);
    if !request_id_matches || !worker_matches || !apparatus_matches {
        return Err(ProductionMapError::OrderFreezeRequestMismatch);
    }
    Ok(())
}

fn validate_freeze_request_target_session(
    control: &OrderControlRecord,
    active_session: Option<&OrderRunSession>,
) -> Result<(), ProductionMapError> {
    let request = control
        .freeze_request
        .as_ref()
        .ok_or(ProductionMapError::OrderFreezeRequestMismatch)?;
    let session = active_session.ok_or(ProductionMapError::OrderFreezeTargetNotFound)?;
    if request.target_session_id.trim().is_empty()
        || request.target_session_id.trim() != session.session_id.trim()
    {
        return Err(ProductionMapError::OrderFreezeRequestMismatch);
    }
    Ok(())
}

fn mark_freeze_request_safe_stop_progress(
    progress: &mut QueueProgressRecords,
    request_id: &str,
    with_issue: bool,
) {
    let request_id = request_id.trim();
    if let Some(session) = progress.session.as_mut() {
        session.status = OrderRunStatus::Frozen;
        if !session.payload_json.is_object() {
            session.payload_json = serde_json::json!({});
        }
        session.payload_json["frozen_order"] = serde_json::json!(true);
        session.payload_json["freeze_request_safe_stop"] = serde_json::json!(true);
        session.payload_json["freeze_request_id"] = serde_json::json!(request_id);
        if with_issue {
            session.payload_json["freeze_with_issue"] = serde_json::json!(true);
            let issue_note = session
                .payload_json
                .get("description")
                .cloned()
                .unwrap_or_else(|| serde_json::json!(""));
            session.payload_json["issue_note"] = issue_note;
        }
    }
    if let Some(event) = progress.progress_event.as_mut() {
        event.payload_json["freeze_request_safe_stop"] = serde_json::json!(true);
        event.payload_json["freeze_request_id"] = serde_json::json!(request_id);
        if with_issue {
            event.payload_json["freeze_with_issue"] = serde_json::json!(true);
            event.payload_json["issue_note"] = serde_json::json!(event.description.trim());
        }
    }
    let mark_batch = |batch: &mut OrderProgressBatch| {
        if !batch.payload_json.is_object() {
            batch.payload_json = serde_json::json!({});
        }
        batch.payload_json["freeze_request_safe_stop"] = serde_json::json!(true);
        batch.payload_json["freeze_request_id"] = serde_json::json!(request_id);
    };
    if let Some(batch) = progress.progress_batch.as_mut() {
        mark_batch(batch);
    }
    for batch in &mut progress.progress_batches {
        mark_batch(batch);
    }
}
