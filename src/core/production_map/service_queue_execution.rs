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
        let admin_freeze_finalization = control.state == OrderControlState::FreezeRequested
            && matches!(
                requested_action,
                queue_state::ApparatusQueueAction::Pause
                    | queue_state::ApparatusQueueAction::Freeze
            );
        let freeze_with_issue = progress.freeze_with_issue
            || (requested_action == queue_state::ApparatusQueueAction::Freeze
                && control.state == OrderControlState::Active);
        let action = if freeze_with_issue || admin_freeze_finalization {
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
            action,
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
        let policies = self.store.apparatus_queue_policies().await?;
        if action == queue_state::ApparatusQueueAction::Freeze
            && control.state == OrderControlState::Active
            && order_has_frozen_queue_state(&all_states, order_id)
        {
            return Err(ProductionMapError::OrderFrozen);
        }
        let known_keys = known_apparatus_storage_keys(&sequences, &all_states);
        let storage_key = queue_state::resolve_apparatus_storage_key(apparatus, &known_keys);
        let policy = queue_policy_for_apparatus(apparatus, &storage_key, &policies);
        let stored_sequence = sequences.get(&storage_key).cloned().unwrap_or_default();
        let all_maps = self.store.maps().await?;
        let visible_order_ids = visible_order_ids_for_apparatus(&all_maps, apparatus);
        let sequence =
            queue_state::effective_apparatus_sequence(&stored_sequence, &visible_order_ids);
        if !sequence.iter().any(|id| id.trim() == order_id) {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let stored_states = all_states.get(&storage_key).cloned().unwrap_or_default();
        validate_requested_queue_state(&stored_states, order_id)?;
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
            Some(ClaimedAlternativeMapUpdate {
                previous: order_map.clone(),
                updated: effective_order_map.clone(),
            })
        } else {
            None
        };
        let order_map = &effective_order_map;
        let previous_progress_ready = self
            .previous_progress_ready_for_action(action, order_id, order_map, apparatus, &progress)
            .await?;
        let mut parsed = parsed_queue_states(stored_states);
        let from_state = parsed
            .get(order_id)
            .copied()
            .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
        // Keep frozen orders in the durable queue map as `frozen`. Removing
        // them here would make the serializer silently recreate them as
        // pending on the next write.
        for (frozen_order_id, frozen_control) in self.store.order_control_states().await? {
            if frozen_control.state == OrderControlState::Frozen {
                parsed.insert(
                    frozen_order_id,
                    queue_state::ApparatusQueueOrderState::Frozen,
                );
            }
        }
        let remove_roll_from_apparatus =
            action == queue_state::ApparatusQueueAction::DetachRoll
                && progress.remove_roll_from_apparatus;
        if remove_roll_from_apparatus {
            if !apparatus::is_laminatsiya_title(&storage_key)
                || from_state != queue_state::ApparatusQueueOrderState::Paused
            {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
            if policy == ApparatusQueuePolicy::StrictSequence
                && queue_state::first_actionable_order_id(&sequence, &parsed).as_deref()
                    != Some(order_id)
            {
                return Err(ProductionMapError::QueueActionNotAllowed);
            }
            parsed.insert(
                order_id.to_string(),
                queue_state::ApparatusQueueOrderState::Paused,
            );
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
        if matches!(
            action,
            queue_state::ApparatusQueueAction::Start
                | queue_state::ApparatusQueueAction::Resume
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
            action,
            from_state,
            to_state,
            policy,
            actor: &actor,
            assigned_apparatus,
            sequence: &sequence,
            visible_order_ids: &visible_order_ids,
        });
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
        if admin_freeze_finalization {
            event.payload_json["admin_freeze_finalization"] = serde_json::json!(true);
        }
        if action == queue_state::ApparatusQueueAction::Complete
            && (apparatus::is_laminatsiya_title(&storage_key)
                || apparatus::is_rezka_title(&storage_key))
            && !progress.force_full_completion_metrics
        {
            let input_batch_id = self
                .completion_input_batch_id(apparatus, order_id, &progress)
                .await?;
            progress.allow_partial_station_completion = self
                .has_unprocessed_previous_wips(
                    order_id,
                    order_map,
                    &storage_key,
                    &all_states,
                    &[],
                    &input_batch_id,
                )
                .await?;
        }
        let progress = self
            .build_progress_records(&storage_key, order_id, order_map, action, &actor, progress)
            .await?;
        let has_unprocessed_previous_wips = action == queue_state::ApparatusQueueAction::Complete
            && to_state == queue_state::ApparatusQueueOrderState::Completed
            && self
                .has_unprocessed_previous_wips(
                    order_id,
                    order_map,
                    &storage_key,
                    &all_states,
                    &progress.progress_batch_updates,
                    "",
                )
                .await?;
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
        Ok(PreparedApparatusQueueAction {
            apparatus: storage_key,
            states: saved,
            event,
            session: progress.session,
            progress_event: progress.progress_event,
            progress_batch: progress.progress_batch,
            progress_batches: progress.progress_batches,
            progress_batch_updates: progress.progress_batch_updates,
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
        Ok(self
            .previous_stage_start_progress_batch(order_id, order_map, apparatus, progress)
            .await?
            .is_some())
    }

    async fn has_unprocessed_previous_wips(
        &self,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        all_states: &ApparatusQueueStateMap,
        progress_batch_updates: &[OrderProgressBatch],
        ignored_batch_id: &str,
    ) -> Result<bool, ProductionMapError> {
        let Some(previous_apparatus) = chain::previous_work_stage_station(order_map, apparatus)
        else {
            return Ok(false);
        };
        let requires_previous_stage_completion = apparatus::is_laminatsiya_title(apparatus)
            || apparatus::is_rezka_title(apparatus);
        let previous_stage_completed = all_states.iter().any(|(candidate, states)| {
            queue_state::apparatus_titles_match(candidate, &previous_apparatus)
                && states
                    .get(order_id)
                    .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
                    == Some(queue_state::ApparatusQueueOrderState::Completed)
        });
        if requires_previous_stage_completion && !previous_stage_completed {
            return Ok(true);
        }
        let mut batches = self
            .store
            .progress_batches_for_order(order_id)
            .await?
            .into_iter()
            .map(|batch| (batch.batch_id.trim().to_string(), batch))
            .collect::<BTreeMap<_, _>>();
        for batch in progress_batch_updates {
            batches.insert(batch.batch_id.trim().to_string(), batch.clone());
        }
        Ok(batches
            .values()
            .filter(|batch| {
                batch.order_id.trim() == order_id.trim()
                    && queue_state::apparatus_titles_match(&batch.apparatus, &previous_apparatus)
                    && queue_state::next_stage_title_matches_apparatus(
                        &batch.next_apparatus,
                        apparatus,
                    )
            })
            .any(|batch| {
                if !ignored_batch_id.trim().is_empty()
                    && batch.batch_id.trim() == ignored_batch_id.trim()
                {
                    return false;
                }
                batch.wip_status == OrderProgressBatchWipStatus::Waiting
                    || (batch.wip_status == OrderProgressBatchWipStatus::InUse
                        && queue_state::apparatus_titles_match(&batch.used_by_apparatus, apparatus))
                    || wip_batch_was_consumed_by_producer(batch)
            }))
    }

    async fn completion_input_batch_id(
        &self,
        apparatus: &str,
        order_id: &str,
        progress: &QueueProgressInput,
    ) -> Result<String, ProductionMapError> {
        if !progress.progress_batch_id.trim().is_empty() {
            return Ok(progress.progress_batch_id.trim().to_string());
        }
        if !progress.qr_payload.trim().is_empty() {
            return Ok(self
                .store
                .progress_batch_by_qr(progress.qr_payload.trim())
                .await?
                .map(|batch| batch.batch_id)
                .unwrap_or_default());
        }
        let Some(session) = self
            .store
            .active_order_run_session(apparatus, order_id)
            .await?
        else {
            return Ok(String::new());
        };
        Ok(session_progress_links(&session).batch_id)
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
        let order_id = prepared.event.order_id.clone();
        let claimed_alternative_map = prepared.claimed_alternative_map.clone();
        if let Some(update) = &claimed_alternative_map {
            self.store.put_map(update.updated.clone()).await?;
        }
        let schedule_reservation_status =
            schedule_reservation_status_for_action(prepared.event.action);
        let order_control = prepared.order_control_update.clone();
        let write_result = self
            .store
            .put_apparatus_queue_states_with_event_and_progress(QueueActionProgressWrite {
                apparatus: prepared.apparatus.clone(),
                states: prepared.states.clone(),
                event: prepared.event,
                session: prepared.session.clone(),
                progress_event: prepared.progress_event.clone(),
                progress_batch: prepared.progress_batch.clone(),
                progress_batches: prepared.progress_batches.clone(),
                progress_batch_updates: prepared.progress_batch_updates.clone(),
                raw_material_stock_transitions,
                qolip_checkouts,
                returned_paint_report,
                order_control_update: prepared.order_control_update.clone(),
                schedule_reservation_status,
            })
            .await;
        let write_result = match write_result {
            Ok(result) => result,
            Err(error) => {
                if let Some(update) = claimed_alternative_map {
                    let _ = self.store.put_map(update.previous).await;
                }
                return Err(error);
            }
        };
        let order_status = self.order_status_detail(&order_id).await?;
        self.notify_live();
        Ok(ApparatusQueueActionResult {
            states: prepared.states,
            order_status,
            order_control,
            session: prepared.session,
            progress_event: prepared.progress_event,
            progress_batch: prepared.progress_batch,
            progress_batches: prepared.progress_batches,
            raw_material_stock_warehouses: write_result.raw_material_stock_warehouses,
            qolip_checkout_committed: write_result.qolip_checkout_committed,
        })
    }
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
    if !matches!(
        action,
        queue_state::ApparatusQueueAction::Pause
            | queue_state::ApparatusQueueAction::Freeze
    ) {
        return Ok(());
    }
    let request = control
        .freeze_request
        .as_ref()
        .ok_or(ProductionMapError::OrderFreezeRequestMismatch)?;
    let request_id_matches =
        supplied_request_id.is_empty() || request.request_id.trim() == supplied_request_id;
    let worker_matches = request.target_worker_role.trim() == actor.role.trim()
        && request.target_worker_ref.trim() == actor.ref_.trim();
    let apparatus_matches =
        queue_state::apparatus_titles_match(&request.target_apparatus, apparatus);
    if !request_id_matches || !worker_matches || !apparatus_matches {
        return Err(ProductionMapError::OrderFreezeRequestMismatch);
    }
    Ok(())
}
