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
                if action == queue_state::ApparatusQueueAction::Pause => {}
            OrderControlState::FreezeRequested => {
                return Err(ProductionMapError::OrderFreezeRequested);
            }
            OrderControlState::Frozen => return Err(ProductionMapError::OrderFrozen),
        }
        let sequences = self.store.apparatus_sequences().await?;
        let all_states = self.store.apparatus_queue_states().await?;
        let policies = self.store.apparatus_queue_policies().await?;
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
        if action == queue_state::ApparatusQueueAction::Resume
            && sequence
                .first()
                .is_none_or(|first| first.trim() != order_id)
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
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
        let states = all_states.get(&storage_key).cloned().unwrap_or_default();
        let mut parsed = parsed_queue_states(states);
        let from_state = parsed
            .get(order_id)
            .copied()
            .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
        let frozen_queue_states = self
            .store
            .order_control_states()
            .await?
            .into_iter()
            .filter_map(|(frozen_order_id, control)| {
                if control.state != OrderControlState::Frozen {
                    return None;
                }
                parsed
                    .remove(&frozen_order_id)
                    .map(|state| (frozen_order_id, state))
            })
            .collect::<Vec<_>>();
        apply_queue_policy(
            policy,
            previous_progress_ready,
            &sequence,
            &mut parsed,
            order_id,
            action,
        )?;
        parsed.extend(frozen_queue_states);
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
        let progress = self
            .build_progress_records(&storage_key, order_id, order_map, action, &actor, progress)
            .await?;
        if action == queue_state::ApparatusQueueAction::Complete
            && to_state == queue_state::ApparatusQueueOrderState::Completed
            && self
                .has_unprocessed_previous_wips(
                    order_id,
                    order_map,
                    &storage_key,
                    &progress.progress_batch_updates,
                )
                .await?
        {
            downgrade_completed_state_to_pending(order_id, &mut saved, &mut event);
        }
        append_laminatsiya_double_leftover_notice(
            action,
            progress.progress_batch.as_ref(),
            order_map,
            &mut event,
        );
        let order_control_update = if control.state == OrderControlState::FreezeRequested
            && action == queue_state::ApparatusQueueAction::Pause
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
            progress_batch_updates: progress.progress_batch_updates,
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
        progress_batch_updates: &[OrderProgressBatch],
    ) -> Result<bool, ProductionMapError> {
        let Some(previous_apparatus) = chain::previous_work_stage_station(order_map, apparatus)
        else {
            return Ok(false);
        };
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
                batch.wip_status == OrderProgressBatchWipStatus::Waiting
                    || (batch.wip_status == OrderProgressBatchWipStatus::InUse
                        && queue_state::apparatus_titles_match(&batch.used_by_apparatus, apparatus))
            }))
    }

    pub(crate) async fn commit_prepared_queue_action(
        &self,
        prepared: PreparedApparatusQueueAction,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        self.commit_prepared_queue_action_with_raw_material_stock(prepared, Vec::new(), None, None)
            .await
    }

    pub(crate) async fn commit_prepared_queue_action_with_raw_material_stock(
        &self,
        prepared: PreparedApparatusQueueAction,
        raw_material_stock_transitions: Vec<RawMaterialStockTransition>,
        qolip_checkout: Option<crate::core::qolip::QolipCheckout>,
        returned_paint_report: Option<crate::core::returned_paint::ReturnedPaintRequest>,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        let order_id = prepared.event.order_id.clone();
        let claimed_alternative_map = prepared.claimed_alternative_map.clone();
        if let Some(update) = &claimed_alternative_map {
            self.store.put_map(update.updated.clone()).await?;
        }
        let write_result = self
            .store
            .put_apparatus_queue_states_with_event_and_progress(QueueActionProgressWrite {
                apparatus: prepared.apparatus.clone(),
                states: prepared.states.clone(),
                event: prepared.event,
                session: prepared.session.clone(),
                progress_event: prepared.progress_event.clone(),
                progress_batch: prepared.progress_batch.clone(),
                progress_batch_updates: prepared.progress_batch_updates.clone(),
                raw_material_stock_transitions,
                qolip_checkout,
                returned_paint_report,
                order_control_update: prepared.order_control_update.clone(),
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
            session: prepared.session,
            progress_event: prepared.progress_event,
            progress_batch: prepared.progress_batch,
            raw_material_stock_warehouses: write_result.raw_material_stock_warehouses,
            qolip_checkout_committed: write_result.qolip_checkout_committed,
        })
    }
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
    if action != queue_state::ApparatusQueueAction::Pause {
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
