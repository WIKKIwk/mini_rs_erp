impl ProductionMapService {
    async fn build_output_progress(
        &self,
        context: ProgressBuildContext<'_>,
        mut progress: QueueProgressInput,
        read_snapshot: Option<&ProgressBuildReadSnapshot>,
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
        if action == queue_state::ApparatusQueueAction::RollComplete
            && !apparatus::is_rezka_apparatus(canonical)
        {
            return Err(ProductionMapError::ProgressInputInvalid);
        }
        let description = progress.description.trim().to_string();
        let session = if let Some(read_snapshot) = read_snapshot {
            read_snapshot.active_session.clone()
        } else {
            self.store
                .active_order_run_session(apparatus, order_id)
                .await?
        }
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
        let session_input_progress = session_progress_links(&session);
        let stage = chain::work_stage_for_station(
            order_map,
            apparatus,
            &session_input_progress.stage_node_id,
        )
        .ok_or(ProductionMapError::ProgressBatchNotAccepted)?;
        let session_uses_opening_wip = session_input_progress.source_kind == "opening_wip";
        if !session_uses_opening_wip
            && action != queue_state::ApparatusQueueAction::Freeze
            && apparatus::requires_previous_stage(canonical)
            && chain::previous_stage_resolution_is_unavailable(order_map, apparatus)
        {
            return Err(ProductionMapError::ProgressQrRequired);
        }
        let opening_input_batch = if session_uses_opening_wip {
            let record = if let Some(read_snapshot) = read_snapshot {
                read_snapshot.opening_wip_batch(
                    &session_input_progress.batch_id,
                    &session_input_progress.qr_payload,
                )
            } else {
                self.store
                    .opening_wip_batch(
                        &session_input_progress.batch_id,
                        &session_input_progress.qr_payload,
                    )
                    .await?
            }
                .ok_or(ProductionMapError::ProgressBatchNotFound)?;
            let explicit_batch_matches = progress.progress_batch_id.trim().is_empty()
                || record.batch.batch_id.trim() == progress.progress_batch_id.trim();
            let explicit_qr_matches = progress.qr_payload.trim().is_empty()
                || record
                    .batch
                    .qr_payload
                    .trim()
                    .eq_ignore_ascii_case(progress.qr_payload.trim());
            if record.intake.status != OpeningWipIntakeStatus::Confirmed
                || record.intake.order_id.trim() != order_id.trim()
                || record.batch.order_id.trim() != order_id.trim()
                || Self::opening_wip_target_stage(
                    order_map,
                    &record.intake,
                    apparatus,
                    &stage.node_id,
                )
                .is_none()
                || record.batch.wip_status != OpeningWipBatchStatus::InUse
                || !super::types::apparatus_ids_match(
                    &record.batch.used_by_apparatus,
                    apparatus,
                )
                || record.batch.used_by_session_id.trim() != session.session_id.trim()
                || !explicit_batch_matches
                || !explicit_qr_matches
            {
                return Err(ProductionMapError::ProgressBatchNotAccepted);
            }
            Some(record.batch)
        } else {
            None
        };
        let session_input_batch = if session_uses_opening_wip
            || session_input_progress.batch_id.trim().is_empty()
        {
            None
        } else if let Some(read_snapshot) = read_snapshot {
            read_snapshot.progress_batch(&session_input_progress.batch_id)
        } else {
            self.store
                .progress_batch(&session_input_progress.batch_id)
                .await?
        };
        let explicit_input_batch = if !session_uses_opening_wip
            && (!progress.progress_batch_id.trim().is_empty()
            || !progress.qr_payload.trim().is_empty()
            ) {
            self.previous_stage_active_progress_batch(
                order_id,
                order_map,
                apparatus,
                &progress,
                &session.session_id,
                &stage.node_id,
                read_snapshot,
            )
            .await?
        } else {
            None
        };
        let previous_apparatus = chain::previous_work_stage_for_node(order_map, &stage.node_id)
            .and_then(|stage| stage.apparatus_id);
        let linked_input_batch = session_input_batch
            .as_ref()
            .filter(|batch| {
                let used_by_apparatus = if batch.used_by_apparatus.trim().is_empty() {
                    batch.current_apparatus.as_str()
                } else {
                    batch.used_by_apparatus.as_str()
                };
                previous_apparatus.as_ref().is_some_and(|previous| {
                    batch.order_id.trim() == order_id.trim()
                        && super::types::apparatus_ids_match(&batch.apparatus, previous)
                        && (batch.next_apparatus.trim().is_empty()
                            || chain::stage_ids_match_for_map(
                                order_map,
                                &batch.next_apparatus,
                                apparatus,
                            ))
                        && (json_string_field(&batch.payload_json, "next_stage_node_id").is_empty()
                            || json_string_field(&batch.payload_json, "next_stage_node_id")
                                == stage.node_id.trim())
                        && batch.wip_status == OrderProgressBatchWipStatus::InUse
                        && super::types::apparatus_ids_match(used_by_apparatus, apparatus)
                        && (batch.used_by_session_id.trim().is_empty()
                            || batch.used_by_session_id.trim() == session.session_id.trim())
                })
            })
            .cloned();
        let recovered_input =
            if explicit_input_batch.is_none() && linked_input_batch.is_none() {
                self.recoverable_session_input_batch(
                    apparatus,
                    order_id,
                    order_map,
                    &session,
                    now,
                    read_snapshot,
                )
                .await?
            } else {
                None
            };
        let input_batch = if let Some(batch) = explicit_input_batch {
            if linked_input_batch.as_ref().is_some_and(|session_batch| {
                session_batch.batch_id.trim() != batch.batch_id.trim()
            }) {
                return Err(ProductionMapError::ProgressBatchNotAccepted);
            }
            Some(batch)
        } else if let Some(batch) = linked_input_batch {
            Some(batch)
        } else if let Some(recovered) = recovered_input.as_ref() {
            Some(recovered.input_batch.clone())
        } else if previous_apparatus.is_some() && !session_uses_opening_wip {
            return Err(ProductionMapError::ProgressQrRequired);
        } else {
            None
        };
        let mut input_progress = input_batch
            .as_ref()
            .map(progress_links_from_batch)
            .unwrap_or_else(|| session_input_progress.clone());
        if input_progress.stage_node_id.trim().is_empty() {
            input_progress.stage_node_id = stage.node_id.clone();
        }
        if input_progress.contained_kadr_count.is_none() {
            input_progress.contained_kadr_count = session_input_progress.contained_kadr_count;
        }
        let mut output_identities = if apparatus::is_rezka_apparatus(canonical) {
            let input_lineage = order_run_input_links_from_payload(&session.payload_json)
                .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
            let active_rolls = rezka_active_partial_rolls_from_payload(&session.payload_json)
                .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
            if !rezka_merge_state_is_consistent(&input_lineage, &active_rolls) {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
            if active_rolls.is_empty() {
                rezka_output_identities(
                    apparatus,
                    order_id,
                    action,
                    order_map,
                    &stage.node_id,
                    input_progress.contained_kadr_count,
                )?
            } else {
                let active_output_kadr_counts = active_rolls
                    .iter()
                    .map(|roll| {
                        usize::try_from(roll.contained_kadr_count)
                            .map_err(|_| ProductionMapError::InvalidRezkaFrameGroups)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                rezka_output_identities_from_kadr_counts(
                    apparatus,
                    order_id,
                    action,
                    order_map,
                    &stage.node_id,
                    &active_output_kadr_counts,
                )?
            }
        } else {
            vec![progress_output_identity(
                apparatus,
                order_id,
                action,
                &progress,
                &input_progress,
            )]
        };
        let report = if apparatus::is_rezka_apparatus(canonical) {
            Some(RezkaOutputReport::prepare(
                &session, &mut progress, &mut output_identities,
            )?)
        } else {
            None
        };
        let recording = progress.rezka_record_frame_index.is_some();
        let frame_values = if let Some(index) = progress.rezka_record_frame_index {
            let value = progress_values_for_outputs(
                canonical, action, &progress, &output_identities[index - 1..index], false,
            )?.remove(0);
            let mut values: Vec<_> = (0..output_identities.len()).map(|_| ProgressOutputValue {
                quantity: None, metrics: ProgressMetrics::default(), issue_note: String::new(),
            }).collect();
            if !report.as_ref().is_some_and(|report| report.is_saved(index - 1)) {
                values[index - 1] = value;
            }
            values
        } else {
            progress_values_for_outputs(
                canonical,
                action,
                &progress,
                &output_identities,
                apparatus::is_rezka_apparatus(canonical)
                    && action == queue_state::ApparatusQueueAction::Complete
                    && !chain::is_final_work_stage_node(order_map, &stage.node_id),
            )?
        };
        let frame_issues = if apparatus::is_rezka_apparatus(canonical) {
            rezka_frame_issues_json(&frame_values, output_identities.len(), &input_progress)
        } else {
            serde_json::Value::Array(Vec::new())
        };
        let first_healthy_index = frame_values
            .iter()
            .position(|value| value.quantity.is_some());
        let (session_qty, session_uom, session_metrics) =
            if let Some(index) = first_healthy_index {
                let value = &frame_values[index];
                let quantity = value.quantity.as_ref().expect("healthy output");
                (quantity.produced_qty, quantity.uom.as_str(), value.metrics)
            } else {
                // An issue has no output quantity, but closing the cycle must
                // retain any waste entered in the ordinary completion report.
                let mut metrics = ProgressMetrics::default();
                if !recording && matches!(action,
                    queue_state::ApparatusQueueAction::Complete | queue_state::ApparatusQueueAction::RollComplete)
                {
                    let valid = |value: Option<f64>| -> Result<Option<f64>, ProductionMapError> {
                        value.map(|value| crate::core::quantity::positive_erp_quantity(value)
                            .ok_or(ProductionMapError::ProgressInputInvalid)).transpose()
                    };
                    metrics.total_waste = valid(progress.total_waste)?;
                    if chain::is_final_work_stage_node(order_map, &stage.node_id)
                        || action != queue_state::ApparatusQueueAction::Complete {
                        metrics.rezka_bosma_waste = valid(progress.rezka_bosma_waste)?;
                        metrics.rezka_lamination_waste = valid(progress.rezka_lamination_waste)?;
                        metrics.rezka_edge_waste = valid(progress.rezka_edge_waste)?;
                    }
                }
                (0.0, "m", metrics)
            };
        let mut payload_json = preserve_qolip_lineage(
            &session,
            progress_session_payload(
                action,
                session_qty,
                session_uom,
                session_metrics,
                &description,
                &input_progress,
            ),
        );
        if frame_issues
            .as_array()
            .is_some_and(|items| !items.is_empty())
        {
            payload_json["rezka_frame_issues"] = frame_issues.clone();
        }
        if recording {
            payload_json = session.payload_json.clone();
        }
        let mut session = OrderRunSession {
            status: run_status_for_progress_action(action),
            worker_role: actor.role.trim().to_string(),
            worker_ref: actor.ref_.trim().to_string(),
            worker_display_name: actor.display_name.trim().to_string(),
            updated_at_unix: now,
            payload_json,
            ..session
        };
        let context = ProgressRecordContext {
            session: &session,
            apparatus,
            order_id,
            action,
            actor,
        };
        let mut batches = Vec::with_capacity(output_identities.len());
        for (index, identity) in output_identities.iter().enumerate() {
            if report.as_ref().is_some_and(|report| report.is_saved(index)) {
                continue;
            }
            let frame_value = if progress.rezka_frames.is_empty() {
                frame_values.first()
            } else {
                frame_values.get(index)
            };
            let Some(frame_value) = frame_value else {
                continue;
            };
            let Some(frame_quantity) = frame_value.quantity.as_ref() else {
                continue;
            };
            let mut batch = progress_batch_record(ProgressBatchRecordInput {
                order_map,
                context,
                quantity: frame_quantity,
                output_identity: identity,
                input_progress: &input_progress,
                metrics: frame_value.metrics,
                frame_gross_qty: progress
                    .rezka_frames
                    .get(if recording { 0 } else { index })
                    .and_then(|frame| frame.gross_qty),
                description: &description,
            })?;
            if apparatus::is_rezka_apparatus(canonical) {
                apply_rezka_frame_metadata(&mut batch, identity, order_map, apparatus);
                if frame_issues
                    .as_array()
                    .is_some_and(|items| !items.is_empty())
                {
                    batch.payload_json["rezka_frame_issues"] = frame_issues.clone();
                }
                if index > 0 && progress.rezka_frames.is_empty() {
                    clear_rezka_duplicate_metrics(&mut batch);
                }
            }
            batches.push(batch);
        }
        let input_was_recovered = recovered_input.is_some();
        let mut progress_batch_updates = recovered_input
            .into_iter()
            .map(|recovered| recovered.output_update)
            .collect::<Vec<_>>();
        // Printed rolls keep their identity, measurements and WIP location.
        // Completion only attaches the accounting metrics that were not known
        // at print time; optimistic batch revisions protect concurrent intake.
        if !recording && let Some(report) = &report {
            for slot in &report.saved {
                let metrics = frame_values[slot.frame_index - 1].metrics;
                if metrics.total_waste.is_some() || metrics.rezka_bosma_waste.is_some()
                    || metrics.rezka_lamination_waste.is_some() || metrics.rezka_edge_waste.is_some()
                {
                    let mut batch = self.store.progress_batch(&slot.batch_id).await?
                        .ok_or(ProductionMapError::ProgressBatchNotFound)?;
                    batch.total_waste = metrics.total_waste;
                    batch.rezka_bosma_waste = metrics.rezka_bosma_waste;
                    batch.rezka_lamination_waste = metrics.rezka_lamination_waste;
                    batch.rezka_edge_waste = metrics.rezka_edge_waste;
                    for (key, value) in [
                        ("total_waste", metrics.total_waste),
                        ("rezka_bosma_waste", metrics.rezka_bosma_waste),
                        ("rezka_lamination_waste", metrics.rezka_lamination_waste),
                        ("rezka_edge_waste", metrics.rezka_edge_waste),
                    ] { batch.payload_json[key] = serde_json::json!(value); }
                    progress_batch_updates.push(batch);
                }
            }
        }
        if let Some(input_batch) = input_batch {
            if matches!(
                action,
                queue_state::ApparatusQueueAction::Pause
                    | queue_state::ApparatusQueueAction::DetachRoll
                    | queue_state::ApparatusQueueAction::RollComplete
            ) {
                if input_was_recovered {
                    progress_batch_updates.push(input_batch);
                }
            } else {
                let mut processed_input =
                    wip_batch_processed(input_batch, apparatus, &session.session_id, now);
                if frame_issues
                    .as_array()
                    .is_some_and(|items| !items.is_empty())
                {
                    processed_input.payload_json["rezka_frame_issues"] =
                        frame_issues.clone();
                    processed_input.payload_json["rezka_issue"] = serde_json::json!(true);
                }
                progress_batch_updates.push(processed_input);
            }
        }
        let opening_wip_batch_updates = opening_input_batch
            .and_then(|batch| {
                (!matches!(
                    action,
                    queue_state::ApparatusQueueAction::Pause
                        | queue_state::ApparatusQueueAction::DetachRoll
                        | queue_state::ApparatusQueueAction::RollComplete
                ))
                .then(|| {
                    opening_wip_batch_processed(batch, apparatus, &session.session_id, now)
                })
            })
            .into_iter()
            .collect();
        let event_output_index = frame_values.iter().enumerate().position(|(index, value)|
            value.quantity.is_some() && !report.as_ref().is_some_and(|report| report.is_saved(index)));
        let mut event = if let Some(index) = event_output_index {
            let output_identity = output_identities
                .get(index)
                .ok_or(ProductionMapError::ProgressInputInvalid)?;
            let frame_value = frame_values
                .get(index)
                .ok_or(ProductionMapError::ProgressInputInvalid)?;
            let quantity = frame_value
                .quantity
                .as_ref()
                .ok_or(ProductionMapError::ProgressInputInvalid)?;
            progress_event_record(ProgressEventRecordInput {
                context,
                quantity: quantity.clone(),
                output_identity: ProgressOutputIdentity {
                    batch_id: output_identity.batch_id.clone(),
                    qr_payload: output_identity.qr_payload.clone(),
                    frame_index: output_identity.frame_index,
                    frame_count: output_identity.frame_count,
                    contained_kadr_count: output_identity.contained_kadr_count,
                    rezka_output_kind: output_identity.rezka_output_kind,
                },
                metrics: frame_value.metrics,
                description: &description,
            })
        } else {
            let mut event = zero_quantity_event(
                context,
                String::new(),
                String::new(),
                progress_event_payload(action, ProgressMetrics::default(), &description),
            );
            event.description = description.clone();
            event
        };
        if !recording && let Some(report) = &report && !report.saved.is_empty() {
            event.payload_json["rezka_previously_recorded_batches"] = serde_json::json!(
                report.saved.iter().filter(|slot| !slot.batch_id.is_empty()).map(|slot| &slot.batch_id).collect::<Vec<_>>()
            );
            // Waste belongs to the closing report even when its first healthy
            // roll was already printed or every card was resolved as an issue.
            // Keep any newly produced roll's quantity metrics on this event.
            if first_healthy_index.is_none_or(|index| report.is_saved(index)) {
                event.total_waste = session_metrics.total_waste;
                event.rezka_bosma_waste = session_metrics.rezka_bosma_waste;
                event.rezka_lamination_waste = session_metrics.rezka_lamination_waste;
                event.rezka_edge_waste = session_metrics.rezka_edge_waste;
                for (key, value) in [
                    ("total_waste", session_metrics.total_waste),
                    ("rezka_bosma_waste", session_metrics.rezka_bosma_waste),
                    ("rezka_lamination_waste", session_metrics.rezka_lamination_waste),
                    ("rezka_edge_waste", session_metrics.rezka_edge_waste),
                ] { event.payload_json[key] = serde_json::json!(value); }
            }
        }
        if frame_issues
            .as_array()
            .is_some_and(|items| !items.is_empty())
        {
            event.payload_json["rezka_frame_issues"] = frame_issues;
        }
        if batches.len() > 1 {
            event.payload_json["rezka_output_batches"] = serde_json::Value::Array(
                batches
                    .iter()
                    .map(|batch| {
                        serde_json::json!({
                            "batch_id": batch.batch_id,
                            "qr_payload": batch.qr_payload,
                            "frame_index": batch
                                .payload_json
                                .get("rezka_frame_index")
                                .and_then(serde_json::Value::as_u64),
                            "frame_count": batch
                                .payload_json
                                .get("rezka_frame_count")
                                .and_then(serde_json::Value::as_u64),
                        })
                    })
                    .collect(),
            );
        }
        if recording {
            report.as_ref().expect("validated Rezka recording")
                .finish_record(&mut session, &progress, &batches)?;
            event.payload_json["rezka_record_frame_index"] = serde_json::json!(progress.rezka_record_frame_index);
        } else {
            apply_output_boundary_to_session_payload(
                &mut session.payload_json,
                action,
                &input_progress.batch_id,
                now,
            )?;
            if report.is_some() {
                session.payload_json["rezka_output_report"] = serde_json::json!([]);
                session.payload_json["rezka_output_cycle"] = serde_json::json!(event.event_id);
                if action.creates_resumable_output()
                    && report.as_ref().is_some_and(|report| !report.saved.is_empty())
                {
                    session.payload_json["rezka_recorded_output_closed"] = serde_json::json!(true);
                }
            }
        }
        let progress_batch = batches.first().cloned();
        Ok(QueueProgressRecords {
            session: Some(session),
            progress_event: Some(event),
            progress_batch,
            progress_batches: batches,
            progress_batch_updates,
            opening_wip_batch_updates,
        })
    }
}
