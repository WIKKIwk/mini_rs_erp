impl ProductionMapService {
    pub async fn progress_batch_for_qr(
        &self,
        progress_batch_id: &str,
        qr_payload: &str,
    ) -> Result<OrderProgressBatch, ProductionMapError> {
        let progress_batch_id = progress_batch_id.trim();
        let qr_payload = qr_payload.trim();
        let batch = if !progress_batch_id.is_empty() {
            let batch = self.store.progress_batch(progress_batch_id).await?;
            if let Some(batch) = batch {
                if !qr_payload.is_empty()
                    && !batch.qr_payload.trim().eq_ignore_ascii_case(qr_payload)
                {
                    return Err(ProductionMapError::ProgressBatchNotFound);
                }
                Some(batch)
            } else {
                None
            }
        } else if !qr_payload.is_empty() {
            self.store.progress_batch_by_qr(qr_payload).await?
        } else {
            return Err(ProductionMapError::ProgressInputInvalid);
        };
        let mut batch = batch.ok_or(ProductionMapError::ProgressBatchNotFound)?;
        restore_self_consumed_wip(&mut batch);
        batch.refresh_status_detail();
        Ok(batch)
    }

    /// Read-only scan validation uses the same stage and ownership rules as
    /// Start. A limited/stale WIP list is not an authority for rejecting a QR.
    pub async fn start_input_for_qr(
        &self,
        apparatus: &str,
        order_id: &str,
        batch_id: &str,
        qr_payload: &str,
    ) -> Result<OrderProgressBatch, ProductionMapError> {
        let map = self.raw_map(order_id).await?.ok_or(ProductionMapError::MapNotFound)?;
        let batch = self.previous_stage_start_progress_batch(
            order_id.trim(), &map, apparatus.trim(), &QueueProgressInput {
                progress_batch_id: batch_id.trim().to_string(),
                qr_payload: qr_payload.trim().to_string(),
                ..Default::default()
            },
        ).await?.ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let route = super::wip_route::resolve_wip_input_route(&map, &batch)?;
        match self.effective_order_control_for_stage(order_id, apparatus, &route.stage_node_id).await?.state {
            OrderControlState::Active => Ok(batch),
            OrderControlState::FreezeRequested => Err(ProductionMapError::OrderFreezeRequested),
            OrderControlState::Frozen => Err(ProductionMapError::OrderFrozen),
        }
    }

    /// Route metadata is a projection only; scans never repair stored batches.
    pub async fn progress_batch_input_route(
        &self,
        batch: &OrderProgressBatch,
    ) -> Result<WipInputRoute, ProductionMapError> {
        let map = self.raw_map(&batch.order_id).await?.ok_or(ProductionMapError::MapNotFound)?;
        super::wip_route::resolve_wip_input_route(&map, batch)
    }

    pub(in crate::core::production_map) async fn previous_stage_start_progress_batch(
        &self,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        progress: &QueueProgressInput,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        let default_stage = chain::work_stage_for_station(order_map, apparatus, "")
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        if progress.qr_payload.trim().is_empty() {
            if chain::previous_work_stage_for_node(order_map, &default_stage.node_id).is_none() {
                return Ok(None);
            }
            return Err(ProductionMapError::ProgressQrRequired);
        }
        let batch = self
            .progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)
            .await?;
        if batch.order_id.trim() != order_id
            || !batch.action.records_progress_output()
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let route = super::wip_route::resolve_wip_input_route(order_map, &batch);
        if route.as_ref().is_ok_and(|route|
            !route.consumer_apparatus_ids.iter().any(|id| id == apparatus))
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        if batch.wip_status != OrderProgressBatchWipStatus::Waiting {
            return Err(progress_batch_usage_error(&batch, order_map));
        }
        if !super::wip_route::actionable_wip(&batch) {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        route?;
        Ok(Some(batch))
    }

    pub(in crate::core::production_map) async fn opening_wip_start_batch(
        &self,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        progress: &QueueProgressInput,
    ) -> Result<Option<OpeningWipBatchRecord>, ProductionMapError> {
        let records = self
            .store
            .opening_wip_records(OpeningWipQuery {
                order_id: order_id.trim().to_string(),
                wip_status: None,
                limit: 10_000,
            })
            .await?;
        let opening_wip_exists = records.iter().any(|record| {
            record.intake.status == OpeningWipIntakeStatus::Confirmed
                && Self::opening_wip_target_stage(order_map, &record.intake, apparatus, "").is_some()
                && record
                    .batches
                    .iter()
                    .any(|batch| batch.wip_status == OpeningWipBatchStatus::Waiting)
        });
        if !opening_wip_exists {
            return Ok(None);
        }
        if progress.qr_payload.trim().is_empty() {
            return Err(ProductionMapError::ProgressQrRequired);
        }
        let Some(record) = self
            .store
            .opening_wip_batch(
                progress.progress_batch_id.trim(),
                progress.qr_payload.trim(),
            )
            .await?
        else {
            return Ok(None);
        };
        if record.intake.status != OpeningWipIntakeStatus::Confirmed
            || record.intake.order_id.trim() != order_id.trim()
            || record.batch.order_id.trim() != order_id.trim()
            || Self::opening_wip_target_stage(order_map, &record.intake, apparatus, "").is_none()
            || record.batch.wip_status != OpeningWipBatchStatus::Waiting
            || (!progress.progress_batch_id.trim().is_empty()
                && record.batch.batch_id.trim() != progress.progress_batch_id.trim())
            || !record
                .batch
                .qr_payload
                .trim()
                .eq_ignore_ascii_case(progress.qr_payload.trim())
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        Ok(Some(record))
    }

    pub(in crate::core::production_map) async fn previous_stage_active_progress_batch(
        &self,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        progress: &QueueProgressInput,
        session_id: &str,
        stage_node_id: &str,
        read_snapshot: Option<&ProgressBuildReadSnapshot>,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        let stage = chain::work_stage_for_station(order_map, apparatus, stage_node_id)
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        if chain::previous_work_stage_for_node(order_map, &stage.node_id).is_none() {
            return Ok(None);
        }
        if progress.qr_payload.trim().is_empty() {
            return Err(ProductionMapError::ProgressQrRequired);
        }
        let mut batch = if let Some(read_snapshot) = read_snapshot {
            read_snapshot
                .progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)?
                .ok_or(ProductionMapError::ProgressBatchNotFound)?
        } else {
            self.progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)
                .await?
        };
        if read_snapshot.is_some() {
            restore_self_consumed_wip(&mut batch);
            batch.refresh_status_detail();
        }
        let used_by_apparatus = if batch.used_by_apparatus.trim().is_empty() {
            batch.current_apparatus.as_str()
        } else {
            batch.used_by_apparatus.as_str()
        };
        let source_wip_is_usable = batch.wip_status == OrderProgressBatchWipStatus::Waiting
            || (batch.wip_status == OrderProgressBatchWipStatus::InUse
                && super::types::apparatus_ids_match(used_by_apparatus, apparatus)
                && (batch.used_by_session_id.trim().is_empty()
                    || batch.used_by_session_id.trim() == session_id.trim()));
        if !source_wip_is_usable {
            return Err(progress_batch_usage_error(&batch, order_map));
        }
        let route = super::wip_route::resolve_wip_input_route(order_map, &batch)?;
        if batch.order_id.trim() != order_id
            || !route.consumer_apparatus_ids.iter().any(|id| id == apparatus)
            || !chain::stage_node_ids_match_for_map(order_map, &route.stage_node_id, &stage.node_id)
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        Ok(Some(batch))
    }

    async fn recoverable_session_input_batch(
        &self,
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        session: &OrderRunSession,
        now: i64,
        read_snapshot: Option<&ProgressBuildReadSnapshot>,
    ) -> Result<Option<RecoveredSessionInputBatch>, ProductionMapError> {
        if let Some(read_snapshot) = read_snapshot {
            return Self::recoverable_session_input_batch_from_batches(
                apparatus,
                order_id,
                order_map,
                session,
                now,
                &read_snapshot.progress_batches,
            );
        }
        let batches = self.store.progress_batches_for_order(order_id).await?;
        Self::recoverable_session_input_batch_from_batches(
            apparatus,
            order_id,
            order_map,
            session,
            now,
            &batches,
        )
    }

    fn recoverable_session_input_batch_from_batches(
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        session: &OrderRunSession,
        now: i64,
        batches: &[OrderProgressBatch],
    ) -> Result<Option<RecoveredSessionInputBatch>, ProductionMapError> {
        let session_links = session_progress_links(session);
        let stage = chain::work_stage_for_station(
            order_map,
            apparatus,
            &session_links.stage_node_id,
        )
        .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        if chain::previous_work_stage_for_node(order_map, &stage.node_id).is_none() {
            return Ok(None);
        }
        let linked_batch_id = session_links.batch_id;
        let mut output_candidates = batches
            .iter()
            .filter(|batch| {
                let linked_candidate = !linked_batch_id.trim().is_empty()
                    && batch.batch_id.trim() == linked_batch_id.trim();
                let unlinked_candidate = linked_batch_id.trim().is_empty()
                    && (batch.status.is_resumable()
                        || batch.wip_status == OrderProgressBatchWipStatus::InUse);
                batch.order_id.trim() == order_id.trim()
                    && batch.session_id.trim() == session.session_id.trim()
                    && batch.action.creates_resumable_output()
                    && super::types::apparatus_ids_match(&batch.apparatus, apparatus)
                    && !batch.parent_batch_id.trim().is_empty()
                    && (linked_candidate || unlinked_candidate)
            })
            .cloned()
            .collect::<Vec<_>>();
        if output_candidates.len() > 1 {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
        let Some(output_batch) = output_candidates.pop() else {
            return Ok(None);
        };
        let Some(parent_batch) = batches.into_iter().find(|batch| {
            batch.batch_id.trim() == output_batch.parent_batch_id.trim()
                && batch.order_id.trim() == order_id.trim()
                && batch.payload_json.get("layer_remainder_operation_id").is_none()
                && super::wip_route::resolve_wip_input_route(order_map, batch).is_ok_and(|route|
                    route.consumer_apparatus_ids.iter().any(|id| id == apparatus)
                        && chain::stage_node_ids_match_for_map(order_map, &route.stage_node_id, &stage.node_id))
        }) else {
            return Ok(None);
        };
        let used_by_apparatus = if parent_batch.used_by_apparatus.trim().is_empty() {
            parent_batch.current_apparatus.as_str()
        } else {
            parent_batch.used_by_apparatus.as_str()
        };
        let owned_in_use = parent_batch.wip_status == OrderProgressBatchWipStatus::InUse
            && super::types::apparatus_ids_match(used_by_apparatus, apparatus)
            && (parent_batch.used_by_session_id.trim().is_empty()
                || parent_batch.used_by_session_id.trim() == session.session_id.trim());
        let prematurely_processed = parent_batch.wip_status
            == OrderProgressBatchWipStatus::Processed
            && super::types::apparatus_ids_match(&parent_batch.processed_by_apparatus, apparatus)
            && (parent_batch.processed_by_session_id.trim().is_empty()
                || parent_batch.processed_by_session_id.trim() == session.session_id.trim());
        if parent_batch.wip_status != OrderProgressBatchWipStatus::Waiting
            && !owned_in_use
            && !prematurely_processed
        {
            return Ok(None);
        }
        let mut input_batch =
            wip_batch_in_use(parent_batch.clone(), apparatus, &session.session_id, now);
        input_batch.payload_json["recovered_original_input_link"] = serde_json::json!(true);
        input_batch.payload_json["recovered_at_unix"] = serde_json::json!(now);
        Ok(Some(RecoveredSessionInputBatch {
            input_batch,
            output_update: restore_misbound_output_wip(output_batch, now),
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn build_progress_records_with_snapshot(
        &self,
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        action: queue_state::ApparatusQueueAction,
        actor: &QueueActionActor,
        progress: QueueProgressInput,
        canonical: &RuntimeApparatusConfiguration,
        read_snapshot: Option<&ProgressBuildReadSnapshot>,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let now = unix_seconds();
        if action == queue_state::ApparatusQueueAction::Freeze {
            return self
                .build_frozen_progress(apparatus, order_id, actor, progress, now)
                .await;
        }
        if (action == queue_state::ApparatusQueueAction::Pause && progress.worker_handoff)
            || (action == queue_state::ApparatusQueueAction::DetachRoll
                && progress.remove_roll_from_apparatus)
        {
            return self
                .build_laminatsiya_worker_transition(
                    apparatus, order_id, order_map, action, actor, progress, now, canonical,
                )
                .await;
        }
        let context = ProgressBuildContext {
            apparatus,
            order_id,
            order_map,
            action,
            actor,
            canonical,
            now,
        };
        match action {
            queue_state::ApparatusQueueAction::Start => {
                self.build_started_progress(context, progress).await
            }
            queue_state::ApparatusQueueAction::Pause
            | queue_state::ApparatusQueueAction::DetachRoll
            | queue_state::ApparatusQueueAction::RollComplete
            | queue_state::ApparatusQueueAction::Complete => {
                self.build_output_progress(context, progress, read_snapshot)
                    .await
            }
            queue_state::ApparatusQueueAction::Freeze => {
                unreachable!("freeze is handled before progress action dispatch")
            }
            queue_state::ApparatusQueueAction::Resume => {
                let mut records = self.build_resumed_progress(context, progress).await?;
                if let Some(session) = &mut records.session {
                    resume_wip_clock(session, now);
                }
                Ok(records)
            }
            queue_state::ApparatusQueueAction::Merge => {
                self.build_merged_progress(context, progress).await
            }
        }
    }
}
