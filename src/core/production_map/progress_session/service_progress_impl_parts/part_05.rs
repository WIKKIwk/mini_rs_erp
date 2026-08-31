enum MergeInputRecord {
    Progress(OrderProgressBatch),
    Opening(OpeningWipBatchRecord),
}

impl MergeInputRecord {
    fn batch_id(&self) -> &str {
        match self {
            Self::Progress(batch) => &batch.batch_id,
            Self::Opening(record) => &record.batch.batch_id,
        }
    }

    fn links(&self, stage_node_id: &str) -> SessionProgressLinks {
        match self {
            Self::Progress(batch) => progress_links_from_batch(batch),
            Self::Opening(record) => progress_links_from_opening_wip(record, stage_node_id),
        }
    }

    fn material_balance_payload(&self, splice_waste_kg: Option<f64>) -> serde_json::Value {
        let (meter, net_kg) = match self {
            Self::Progress(batch) => (
                batch.finished_goods_meter.or_else(|| {
                    batch
                        .uom
                        .trim()
                        .eq_ignore_ascii_case("m")
                        .then_some(batch.produced_qty)
                }),
                batch.finished_goods_kg,
            ),
            Self::Opening(record) => (
                record.batch.finished_goods_meter.or_else(|| {
                    record
                        .batch
                        .uom
                        .trim()
                        .eq_ignore_ascii_case("m")
                        .then_some(record.batch.quantity)
                        .flatten()
                }),
                record.batch.finished_goods_kg,
            ),
        };
        serde_json::json!({
            "processed_input_batch_id": self.batch_id(),
            "processed_input_meter": meter,
            "processed_input_net_kg": net_kg,
            "splice_waste_kg": splice_waste_kg,
            "output_measurement_deferred": true,
            "diameter_combined": false,
        })
    }
}

impl ProductionMapService {
    async fn merge_input_record(
        &self,
        order_id: &str,
        order_map: &ProductionMapDefinition,
        apparatus: &str,
        stage_node_id: &str,
        progress: &QueueProgressInput,
    ) -> Result<MergeInputRecord, ProductionMapError> {
        if progress.qr_payload.trim().is_empty() {
            return Err(ProductionMapError::MergeInputRequired);
        }
        if let Some(record) = self
            .store
            .opening_wip_batch(
                progress.progress_batch_id.trim(),
                progress.qr_payload.trim(),
            )
            .await?
        {
            if record.intake.status != OpeningWipIntakeStatus::Confirmed
                || record.intake.order_id.trim() != order_id.trim()
                || record.batch.order_id.trim() != order_id.trim()
                || Self::opening_wip_target_stage(
                    order_map,
                    &record.intake,
                    apparatus,
                    stage_node_id,
                )
                .is_none()
                || (!progress.progress_batch_id.trim().is_empty()
                    && record.batch.batch_id.trim() != progress.progress_batch_id.trim())
            {
                return Err(ProductionMapError::MergeInputNotAccepted);
            }
            if record.batch.wip_status != OpeningWipBatchStatus::Waiting {
                return Err(ProductionMapError::MergeInputAlreadyUsed);
            }
            return Ok(MergeInputRecord::Opening(record));
        }

        let batch = self
            .progress_batch_for_qr(&progress.progress_batch_id, &progress.qr_payload)
            .await
            .map_err(|error| match error {
                ProductionMapError::ProgressBatchNotFound => {
                    ProductionMapError::MergeInputNotAccepted
                }
                other => other,
            })?;
        let stage = chain::work_stage_for_station(order_map, apparatus, stage_node_id)
            .ok_or(ProductionMapError::MergeInputNotAccepted)?;
        let previous_apparatus = chain::previous_work_stage_for_node(order_map, &stage.node_id)
            .and_then(|stage| stage.apparatus_id)
            .ok_or(ProductionMapError::MergeInputNotAccepted)?;
        if batch.order_id.trim() != order_id.trim()
            || !super::types::apparatus_ids_match(&batch.apparatus, &previous_apparatus)
            || !batch.action.records_progress_output()
            || !matches!(
                batch.status,
                OrderProgressBatchStatus::Paused
                    | OrderProgressBatchStatus::RollDetached
                    | OrderProgressBatchStatus::Completed
                    | OrderProgressBatchStatus::Resumed
            )
            || (!batch.next_apparatus.trim().is_empty()
                && !chain::stage_ids_match_for_map(order_map, &batch.next_apparatus, apparatus))
            || (!json_string_field(&batch.payload_json, "next_stage_node_id").is_empty()
                && json_string_field(&batch.payload_json, "next_stage_node_id")
                    != stage.node_id.trim())
        {
            return Err(ProductionMapError::MergeInputNotAccepted);
        }
        if batch.wip_status != OrderProgressBatchWipStatus::Waiting {
            return Err(ProductionMapError::MergeInputAlreadyUsed);
        }
        Ok(MergeInputRecord::Progress(batch))
    }

    async fn active_merge_input_record(
        &self,
        session: &OrderRunSession,
        input: &SessionProgressLinks,
    ) -> Result<MergeInputRecord, ProductionMapError> {
        if input.source_kind == OrderRunInputSourceKind::OpeningWip.as_str() {
            let record = self
                .store
                .opening_wip_batch(&input.batch_id, &input.qr_payload)
                .await?
                .ok_or(ProductionMapError::MergeInputNotAccepted)?;
            if record.batch.wip_status != OpeningWipBatchStatus::InUse
                || record.batch.used_by_session_id.trim() != session.session_id.trim()
                || !super::types::apparatus_ids_match(
                    &record.batch.used_by_apparatus,
                    &session.apparatus,
                )
            {
                return Err(ProductionMapError::MergeInputNotAccepted);
            }
            return Ok(MergeInputRecord::Opening(record));
        }

        let batch = self
            .store
            .progress_batch(&input.batch_id)
            .await?
            .ok_or(ProductionMapError::MergeInputNotAccepted)?;
        if batch.wip_status != OrderProgressBatchWipStatus::InUse
            || (!batch.used_by_session_id.trim().is_empty()
                && batch.used_by_session_id.trim() != session.session_id.trim())
            || !super::types::apparatus_ids_match(
                if batch.used_by_apparatus.trim().is_empty() {
                    &batch.current_apparatus
                } else {
                    &batch.used_by_apparatus
                },
                &session.apparatus,
            )
        {
            return Err(ProductionMapError::MergeInputNotAccepted);
        }
        Ok(MergeInputRecord::Progress(batch))
    }

    async fn build_merged_progress(
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
        if action != queue_state::ApparatusQueueAction::Merge
            || !apparatus::is_rezka_apparatus(canonical)
        {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        let splice_waste_kg = match progress.total_waste {
            Some(value) if value.is_finite() && value >= 0.0 => Some(value),
            Some(_) => return Err(ProductionMapError::ProgressInputInvalid),
            None => None,
        };
        if !progress.rezka_frames.is_empty()
            || progress.produced_qty.is_some()
            || progress.gross_qty.is_some()
            || progress.return_ink_kg.is_some()
            || progress.lamination_print_leftover_rolls.is_some()
            || progress.lamination_film_leftover_rolls.is_some()
            || progress.rezka_bosma_waste.is_some()
            || progress.rezka_lamination_waste.is_some()
            || progress.rezka_edge_waste.is_some()
            || progress.finished_goods_kg.is_some()
            || progress.bobina_kg.is_some()
            || progress.finished_goods_meter.is_some()
            || progress.diameter.is_some()
            || (!progress.uom.trim().is_empty() && !progress.uom.trim().eq_ignore_ascii_case("kg"))
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let current_session = self
            .store
            .active_order_run_session(apparatus, order_id)
            .await?
            .filter(|session| session.status == OrderRunStatus::Active)
            .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        let current_links = session_progress_links(&current_session);
        if current_links.batch_id.trim().is_empty() {
            return Err(ProductionMapError::MergeInputNotAccepted);
        }
        if (!progress.progress_batch_id.trim().is_empty()
            && progress.progress_batch_id.trim() == current_links.batch_id.trim())
            || (!progress.qr_payload.trim().is_empty()
                && progress
                    .qr_payload
                    .trim()
                    .eq_ignore_ascii_case(current_links.qr_payload.trim()))
        {
            return Err(ProductionMapError::MergeInputSame);
        }
        let current_input = self
            .active_merge_input_record(&current_session, &current_links)
            .await?;
        let next_input = self
            .merge_input_record(
                order_id,
                order_map,
                apparatus,
                &current_links.stage_node_id,
                &progress,
            )
            .await?;
        if current_input.batch_id().trim() == next_input.batch_id().trim() {
            return Err(ProductionMapError::MergeInputSame);
        }

        let next_links = next_input.links(&current_links.stage_node_id);
        let material_balance = current_input.material_balance_payload(splice_waste_kg);
        let mut payload = current_session.payload_json.clone();
        let mut input_lineage = order_run_input_links_from_payload(&payload)
            .map_err(|_| ProductionMapError::MergeInputNotAccepted)?;
        if input_lineage.is_empty() {
            let source_kind = OrderRunInputSourceKind::parse(&current_links.source_kind)
                .ok_or(ProductionMapError::MergeInputNotAccepted)?;
            input_lineage.push(OrderRunInputLink {
                input_batch_id: current_links.batch_id.clone(),
                input_qr_payload: current_links.qr_payload.clone(),
                source_apparatus: current_links.apparatus.clone(),
                source_kind,
                stage_node_id: current_links.stage_node_id.clone(),
                sequence_no: 1,
                status: OrderRunInputStatus::InUse,
                linked_at_unix: current_session.started_at_unix,
                processed_at_unix: None,
            });
            write_order_run_input_links(&mut payload, &input_lineage);
        }
        if input_lineage
            .iter()
            .any(|link| link.input_batch_id.trim() == next_links.batch_id.trim())
        {
            return Err(ProductionMapError::MergeInputAlreadyUsed);
        }
        let current_link = input_lineage
            .iter_mut()
            .find(|link| {
                link.input_batch_id.trim() == current_links.batch_id.trim()
                    && link.status == OrderRunInputStatus::InUse
            })
            .ok_or(ProductionMapError::MergeInputNotAccepted)?;
        current_link.status = OrderRunInputStatus::Processed;
        current_link.processed_at_unix = Some(now);

        let mut active_rolls = rezka_active_partial_rolls_from_payload(&payload)
            .map_err(|_| ProductionMapError::MergeInputNotAccepted)?;
        if active_rolls.is_empty() {
            let output_kadr_counts = rezka_output_kadr_counts(
                order_map,
                apparatus,
                &current_links.stage_node_id,
                current_links.contained_kadr_count,
            )?;
            initialize_rezka_active_partial_rolls(&mut payload, &output_kadr_counts, now)?;
            active_rolls = rezka_active_partial_rolls_from_payload(&payload)
                .map_err(|_| ProductionMapError::MergeInputNotAccepted)?;
        }

        let next_source_kind = OrderRunInputSourceKind::parse(&next_links.source_kind)
            .ok_or(ProductionMapError::MergeInputNotAccepted)?;
        let next_sequence = input_lineage
            .iter()
            .map(|link| link.sequence_no)
            .max()
            .unwrap_or_default()
            .checked_add(1)
            .ok_or(ProductionMapError::MergeInputNotAccepted)?;
        input_lineage.push(OrderRunInputLink {
            input_batch_id: next_links.batch_id.clone(),
            input_qr_payload: next_links.qr_payload.clone(),
            source_apparatus: next_links.apparatus.clone(),
            source_kind: next_source_kind,
            stage_node_id: current_links.stage_node_id.clone(),
            sequence_no: next_sequence,
            status: OrderRunInputStatus::InUse,
            linked_at_unix: now,
            processed_at_unix: None,
        });
        for roll in &mut active_rolls {
            if !roll
                .source_input_batch_ids
                .iter()
                .any(|batch_id| batch_id.trim() == next_links.batch_id.trim())
            {
                roll.source_input_batch_ids
                    .push(next_links.batch_id.clone());
            }
            roll.updated_at_unix = now;
        }
        if !rezka_merge_state_is_consistent(&input_lineage, &active_rolls) {
            return Err(ProductionMapError::MergeInputNotAccepted);
        }
        write_order_run_input_links(&mut payload, &input_lineage);
        write_rezka_active_partial_rolls(&mut payload, &active_rolls);
        payload["last_action"] = serde_json::json!("merge");
        payload["input_progress_batch_id"] = serde_json::json!(next_links.batch_id);
        payload["input_progress_qr_payload"] = serde_json::json!(next_links.qr_payload);
        payload["input_progress_apparatus"] = serde_json::json!(next_links.apparatus);
        payload["input_wip_source_kind"] = serde_json::json!(next_links.source_kind);
        payload["merge_from_input_batch_id"] = serde_json::json!(current_links.batch_id);
        payload["merge_to_input_batch_id"] = serde_json::json!(next_links.batch_id);
        payload["merge_count"] = serde_json::json!(next_sequence - 1);

        let session = OrderRunSession {
            status: OrderRunStatus::Active,
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            updated_at_unix: now,
            payload_json: payload,
            ..current_session
        };
        let mut event = zero_quantity_event(
            ProgressRecordContext {
                session: &session,
                apparatus,
                order_id,
                action,
                actor,
                now,
            },
            next_links.batch_id.clone(),
            next_links.qr_payload.clone(),
            serde_json::json!({
                "event": "merge",
                "from_input_batch_id": current_links.batch_id,
                "to_input_batch_id": next_links.batch_id,
                "input_sequence_no": next_sequence,
                "source_input_batch_ids": active_rolls
                    .first()
                    .map(|roll| roll.source_input_batch_ids.clone())
                    .unwrap_or_default(),
                "material_balance_basis": "measured_at_output",
                "material_balance": material_balance,
                "splice_waste_kg": splice_waste_kg,
                "diameter_combined": false,
            }),
        );
        event.description = progress.description.trim().to_string();
        event.total_waste = splice_waste_kg;

        let mut progress_batch_updates = Vec::new();
        let mut opening_wip_batch_updates = Vec::new();
        match current_input {
            MergeInputRecord::Progress(batch) => progress_batch_updates.push(wip_batch_processed(
                batch,
                apparatus,
                &session.session_id,
                now,
            )),
            MergeInputRecord::Opening(record) => opening_wip_batch_updates.push(
                opening_wip_batch_processed(record.batch, apparatus, &session.session_id, now),
            ),
        }
        match next_input {
            MergeInputRecord::Progress(batch) => progress_batch_updates.push(wip_batch_in_use(
                batch,
                apparatus,
                &session.session_id,
                now,
            )),
            MergeInputRecord::Opening(record) => opening_wip_batch_updates.push(
                opening_wip_batch_in_use(record.batch, apparatus, &session.session_id, now),
            ),
        }

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
