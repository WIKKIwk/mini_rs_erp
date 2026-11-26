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
        let OutputProgressSources {
            mut session, stage, opening_input_batch, input_batch, recovered_input, input_progress,
        } = self.output_progress_sources(context, &progress, read_snapshot).await?;
        let mut output_identities = if progress.complete_without_output {
            Vec::new()
        } else if apparatus::is_rezka_apparatus(canonical) {
            let input_lineage = order_run_input_links_from_payload(&session.payload_json)
                .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
            let active_rolls = rezka_active_partial_rolls_from_payload(&session.payload_json)
                .map_err(|_| ProductionMapError::ProgressInputInvalid)?;
            if !rezka_merge_state_is_consistent(&input_lineage, &active_rolls) {
                return Err(ProductionMapError::ProgressInputInvalid);
            }
            if active_rolls.is_empty() && !order_map.products.is_empty() {
                let counts = super::layout::cut_counts(order_map, &stage.node_id, &input_progress.layout_lanes)?;
                rezka_output_identities_from_kadr_counts(apparatus, order_id, action, order_map, &stage.node_id, &counts)?
            } else if active_rolls.is_empty() {
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
        if !order_map.products.is_empty() && apparatus::is_rezka_apparatus(canonical) {
            let counts = output_identities.iter().map(|output| output.contained_kadr_count.unwrap_or(0) as i64).collect::<Vec<_>>();
            let layout = super::layout::output_layout(order_map, &session, &counts);
            session.payload_json["rezka_layout_outputs"] = layout;
        }
        let report = if apparatus::is_rezka_apparatus(canonical) {
            Some(RezkaOutputReport::prepare(
                &session, &mut progress, &mut output_identities,
            )?)
        } else {
            None
        };
        let recording = progress.rezka_record_frame_index.is_some();
        if !order_map.products.is_empty()
            && apparatus::is_rezka_apparatus(canonical)
            && !progress.complete_without_output
            && !recording
            && output_identities.len() > 1
            && progress.rezka_frames.is_empty()
        {
            return Err(ProductionMapError::RezkaFrameCountMismatch);
        }
        let frame_values = if progress.complete_without_output {
            Vec::new()
        } else if let Some(index) = progress.rezka_record_frame_index {
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
            if progress.complete_without_output {
                (0.0, "m", ProgressMetrics {
                    total_waste: progress.total_waste,
                    return_ink_kg: progress.return_ink_kg,
                    ..ProgressMetrics::default()
                })
            } else if let Some(index) = first_healthy_index {
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
                    let allow_zero_waste = action == queue_state::ApparatusQueueAction::Complete
                        && !chain::is_final_work_stage_node(order_map, &stage.node_id);
                    let valid = |value: Option<f64>| -> Result<Option<f64>, ProductionMapError> {
                        value.map(|value| (if allow_zero_waste && value == 0.0 {
                            Some(0.0)
                        } else {
                            crate::core::quantity::positive_erp_quantity(value)
                        })
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
        let is_rezka = apparatus::is_rezka_apparatus(canonical);
        let final_rezka = is_rezka
            && chain::is_final_work_stage_node(order_map, &stage.node_id);
        let includes_bobina = final_rezka && order_map.include_bobina_in_net == Some(true);
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
            if final_rezka
                && !progress.rezka_frames.is_empty()
                && !includes_bobina
                && (order_map.include_bobina_in_net.is_some()
                    || !progress.rezka_expected_kadr_counts.is_empty())
                && frame_value.metrics.bobina_kg.is_none()
            {
                return Err(ProductionMapError::RezkaBobinaModeChanged(false));
            }
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
            if is_rezka {
                apply_rezka_frame_metadata(&mut batch, identity, order_map, apparatus);
                if let Some(frame) = progress.rezka_frames.get(if recording { 0 } else { index })
                    && !frame.peremotka_reason.is_empty()
                {
                    batch.wip_status = OrderProgressBatchWipStatus::Peremotka;
                    batch.payload_json["peremotka_reason"] = serde_json::json!(frame.peremotka_reason);
                    batch.payload_json["order_number"] = serde_json::json!(order_map.order_number);
                    let product = if order_map.products.is_empty() {
                        non_empty_or(&order_map.title, &order_map.product_code)
                    } else {
                        batch.label_item_name.clone()
                    };
                    batch.label_item_name = format!("PEREMOTKA - {product}");
                    batch.refresh_status_detail();
                }
                if includes_bobina {
                    batch.payload_json["include_bobina_in_net"] = serde_json::json!(true);
                }
                if frame_issues
                    .as_array()
                    .is_some_and(|items| !items.is_empty())
                {
                    batch.payload_json["rezka_frame_issues"] = frame_issues.clone();
                }
                if index > 0 && progress.rezka_frames.is_empty() {
                    clear_rezka_duplicate_metrics(&mut batch);
                }
                crate::core::production_map::paddon_weights::validate_batch_bobina(&batch)?;
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
                    lane_offset: output_identity.lane_offset,
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
        if progress.complete_without_output {
            event.total_waste = session_metrics.total_waste;
            event.return_ink_kg = session_metrics.return_ink_kg;
            session_metrics.write_payload_fields(&mut event.payload_json, &description);
        }
        if !recording && let Some(report) = &report && !report.saved.is_empty() {
            event.payload_json["rezka_previously_recorded_batches"] = serde_json::json!(
                report.saved.iter().filter(|slot| !slot.batch_id.is_empty()).map(|slot| &slot.batch_id).collect::<Vec<_>>()
            );
        }
        // Closing waste belongs to the durable event even if every output is
        // an unsaved issue. A later roll replaces the mutable session payload.
        if !recording && apparatus::is_rezka_apparatus(canonical)
            && matches!(action, queue_state::ApparatusQueueAction::Complete | queue_state::ApparatusQueueAction::RollComplete)
            && first_healthy_index.is_none_or(|index| report.as_ref().is_some_and(|report| report.is_saved(index)))
        {
            event.total_waste = session_metrics.total_waste;
            event.rezka_bosma_waste = session_metrics.rezka_bosma_waste;
            event.rezka_lamination_waste = session_metrics.rezka_lamination_waste;
            event.rezka_edge_waste = session_metrics.rezka_edge_waste;
            let waste = serde_json::json!({
                "total_waste": session_metrics.total_waste,
                "rezka_bosma_waste": session_metrics.rezka_bosma_waste,
                "rezka_lamination_waste": session_metrics.rezka_lamination_waste,
                "rezka_edge_waste": session_metrics.rezka_edge_waste,
            });
            for (key, value) in waste.as_object().expect("waste object") {
                event.payload_json[key] = value.clone();
            }
            if first_healthy_index.is_none() && waste.as_object().unwrap().values().any(|value| !value.is_null()) {
                event.payload_json["zero_output_waste"] = waste;
            }
        }
        if is_rezka && !recording && matches!(action,
            queue_state::ApparatusQueueAction::Complete | queue_state::ApparatusQueueAction::RollComplete)
        {
            // A closing report can cover several frames, including ones printed
            // earlier. Each frame's accounting belongs to this closing event.
            let kg = if first_healthy_index.is_none() {
                session_metrics.reported_waste_kg()
            } else {
                frame_values.iter().map(|frame| frame.metrics.reported_waste_kg()).sum()
            };
            event.payload_json["waste_bin_report_kg"] = serde_json::json!(kg);
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
                session.payload_json.as_object_mut().expect("session payload")
                    .remove("rezka_reference_frame_index");
                session.payload_json["rezka_output_cycle"] = serde_json::json!(event.event_id);
                if action.creates_resumable_output()
                    && report.as_ref().is_some_and(|report| !report.saved.is_empty())
                {
                    session.payload_json["rezka_recorded_output_closed"] = serde_json::json!(true);
                }
            }
        }
        if action == queue_state::ApparatusQueueAction::DetachRoll
            && pechat::is_pechat_apparatus(canonical)
            && let Some(batch) = batches.first()
        {
            session.payload_json["last_detached_output_batch_id"] = serde_json::json!(batch.batch_id);
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
