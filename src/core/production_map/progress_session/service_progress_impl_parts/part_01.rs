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

    pub(in crate::core::production_map) async fn previous_stage_start_progress_batch(
        &self,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        progress: &QueueProgressInput,
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        let default_stage = chain::work_stage_for_station(order_map, apparatus, "")
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let Some(default_previous) =
            chain::previous_work_stage_for_node(order_map, &default_stage.node_id)
        else {
            return Ok(None);
        };
        if progress.qr_payload.trim().is_empty() {
            return Err(ProductionMapError::ProgressQrRequired);
        }
        let batch = self
            .progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)
            .await?;
        let preferred_stage_node_id = json_string_field(&batch.payload_json, "next_stage_node_id");
        let stage = if preferred_stage_node_id.is_empty() {
            default_stage
        } else {
            chain::work_stage_for_station(order_map, apparatus, &preferred_stage_node_id)
                .ok_or(ProductionMapError::ProgressBatchNotAccepted)?
        };
        let previous = chain::previous_work_stage_for_node(order_map, &stage.node_id)
            .unwrap_or(default_previous);
        let previous_apparatus = previous
            .apparatus_id
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        if batch.order_id.trim() != order_id
            || !super::types::apparatus_ids_match(&batch.apparatus, &previous_apparatus)
            || !batch.action.records_progress_output()
            || (!batch.next_apparatus.trim().is_empty()
                && !chain::stage_ids_match_for_map(order_map, &batch.next_apparatus, apparatus))
            || (!preferred_stage_node_id.is_empty()
                && preferred_stage_node_id.trim() != stage.node_id.trim())
            || batch.wip_status != OrderProgressBatchWipStatus::Waiting
        {
            return Err(ProductionMapError::ProgressBatchNotAccepted);
        }
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
    ) -> Result<Option<OrderProgressBatch>, ProductionMapError> {
        let stage = chain::work_stage_for_station(order_map, apparatus, stage_node_id)
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let Some(previous) = chain::previous_work_stage_for_node(order_map, &stage.node_id) else {
            return Ok(None);
        };
        let previous_apparatus = previous
            .apparatus_id
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        if progress.qr_payload.trim().is_empty() {
            return Err(ProductionMapError::ProgressQrRequired);
        }
        let batch = self
            .progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)
            .await?;
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
        if batch.order_id.trim() != order_id
            || !super::types::apparatus_ids_match(&batch.apparatus, &previous_apparatus)
            || !batch.action.records_progress_output()
            || !source_wip_is_usable
            || (!batch.next_apparatus.trim().is_empty()
                && !chain::stage_ids_match_for_map(order_map, &batch.next_apparatus, apparatus))
            || (!json_string_field(&batch.payload_json, "next_stage_node_id").is_empty()
                && json_string_field(&batch.payload_json, "next_stage_node_id")
                    != stage.node_id.trim())
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
    ) -> Result<Option<RecoveredSessionInputBatch>, ProductionMapError> {
        let session_links = session_progress_links(session);
        let stage = chain::work_stage_for_station(
            order_map,
            apparatus,
            &session_links.stage_node_id,
        )
        .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let Some(previous) = chain::previous_work_stage_for_node(order_map, &stage.node_id) else {
            return Ok(None);
        };
        let previous_apparatus = previous
            .apparatus_id
            .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let batches = self.store.progress_batches_for_order(order_id).await?;
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
                && super::types::apparatus_ids_match(&batch.apparatus, &previous_apparatus)
                && (batch.next_apparatus.trim().is_empty()
                    || chain::stage_ids_match_for_map(order_map, &batch.next_apparatus, apparatus))
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
        let mut input_batch = wip_batch_in_use(parent_batch, apparatus, &session.session_id, now);
        input_batch.payload_json["recovered_original_input_link"] = serde_json::json!(true);
        input_batch.payload_json["recovered_at_unix"] = serde_json::json!(now);
        sync_wip_payload_fields(&mut input_batch);
        Ok(Some(RecoveredSessionInputBatch {
            input_batch,
            output_update: restore_misbound_output_wip(output_batch, now),
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn build_progress_records(
        &self,
        apparatus: &str,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        action: queue_state::ApparatusQueueAction,
        actor: &QueueActionActor,
        progress: QueueProgressInput,
        canonical: &RuntimeApparatusConfiguration,
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
                self.build_output_progress(context, progress).await
            }
            queue_state::ApparatusQueueAction::Freeze => {
                unreachable!("freeze is handled before progress action dispatch")
            }
            queue_state::ApparatusQueueAction::Resume => {
                self.build_resumed_progress(context, progress).await
            }
            queue_state::ApparatusQueueAction::Merge => {
                self.build_merged_progress(context, progress).await
            }
        }
    }

    async fn build_started_progress(
        &self,
        context: ProgressBuildContext<'_>,
        progress: QueueProgressInput,
    ) -> Result<QueueProgressRecords, ProductionMapError> {
        let ProgressBuildContext {
            apparatus,
            order_id,
            order_map,
            action,
            actor,
            canonical,
            now,
        } = context;
        let opening_wip_batch = self
            .opening_wip_start_batch(order_id, order_map, apparatus, &progress)
            .await?;
        let input_progress_batch = if opening_wip_batch.is_none() {
            self.previous_stage_start_progress_batch(order_id, order_map, apparatus, &progress)
                .await?
        } else {
            None
        };
        let opening_wip_target_stage_node_id = opening_wip_batch
            .as_ref()
            .and_then(|record| {
                Self::opening_wip_target_stage(order_map, &record.intake, apparatus, "")
            })
            .map(|stage| stage.node_id)
            .unwrap_or_default();
        let input_progress = input_progress_batch
            .as_ref()
            .map(progress_links_from_batch)
            .or_else(|| {
                opening_wip_batch
                    .as_ref()
                    .map(|record| {
                        progress_links_from_opening_wip(
                            record,
                            &opening_wip_target_stage_node_id,
                        )
                    })
            })
            .unwrap_or_default();
        let stage = chain::work_stage_for_station(
            order_map,
            apparatus,
            &input_progress.stage_node_id,
        )
        .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let mut session_payload = start_session_payload(
            actor,
            &input_progress,
            input_progress_batch.as_ref(),
            &stage.node_id,
            now,
        );
        if apparatus::is_rezka_apparatus(canonical) {
            let output_kadr_counts = rezka_output_kadr_counts(
                order_map,
                apparatus,
                &stage.node_id,
                input_progress.contained_kadr_count,
            )?;
            initialize_rezka_active_partial_rolls(
                &mut session_payload,
                &output_kadr_counts,
                now,
            )?;
        }
        let session = OrderRunSession {
            session_id: progress_session_id(apparatus, order_id, actor),
            apparatus: apparatus.to_string(),
            order_id: order_id.to_string(),
            status: OrderRunStatus::Active,
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            started_at_unix: now,
            updated_at_unix: now,
            payload_json: session_payload,
        };
        let context = ProgressRecordContext {
            session: &session,
            apparatus,
            order_id,
            action,
            actor,
        };
        let event = zero_quantity_event(
            context,
            String::new(),
            String::new(),
            start_event_payload(
                &input_progress,
                input_progress_batch.as_ref(),
                &stage.node_id,
            ),
        );
        let mut progress_batch_updates = Vec::new();
        if let Some(input_batch) = input_progress_batch {
            let recovered_self_consumed = input_batch
                .payload_json
                .get("recovered_self_consumed_wip")
                .and_then(serde_json::Value::as_bool)
                == Some(true);
            if recovered_self_consumed {
                for mut sibling in self.store.progress_batches_for_order(order_id).await? {
                    if repair_self_consumed_sibling_lineage(&mut sibling, &input_batch) {
                        progress_batch_updates.push(sibling);
                    }
                }
            }
            progress_batch_updates.push(wip_batch_in_use(
                input_batch,
                apparatus,
                &session.session_id,
                now,
            ));
        }
        let opening_wip_batch_updates = opening_wip_batch
            .map(|record| {
                vec![opening_wip_batch_in_use(
                    record.batch,
                    apparatus,
                    &session.session_id,
                    now,
                )]
            })
            .unwrap_or_default();
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(event),
            progress_batch: None,
            progress_batches: Vec::new(),
            progress_batch_updates,
            opening_wip_batch_updates,
        })
    }
}
