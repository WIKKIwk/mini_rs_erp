
#[allow(clippy::too_many_arguments)]
pub(super) async fn training_queue_action(
    state: &AppState,
    principal: &Principal,
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    material_barcode: &str,
    material_barcodes: &[String],
    progress_batch_id: &str,
    progress_qr: &str,
    print_input: TrainingQueuePrintInput,
) -> Result<Option<serde_json::Value>, TrainingWorkspaceError> {
    let order_id = order_id.trim();
    if !order_id.starts_with("training-") {
        return Ok(None);
    }
    let requested_apparatus = canonical_training_apparatus(apparatus)?;
    let overlay = worker_training_overlay(state, principal).await?;
    let Some(apparatus) = overlay
        .active_apparatuses
        .iter()
        .find(|candidate| canonical_apparatus_matches(candidate, &requested_apparatus))
        .cloned()
    else {
        return Err(TrainingWorkspaceError::MapNotFound);
    };
    let canonical = state
        .production_maps
        .resolve_canonical_apparatus_text(&apparatus)
        .await
        .map_err(|_| TrainingWorkspaceError::StoreFailed)?;
    let mut training_map = overlay
        .maps
        .iter()
        .find(|saved| {
            saved.map.id.trim() == order_id && training_map_has_apparatus(saved, &apparatus)
        })
        .map(|saved| saved.map.clone())
        .ok_or(TrainingWorkspaceError::MapNotFound)?;
    let controls = overlay
        .queue_action_controls
        .get(&apparatus)
        .and_then(|controls| controls.get(order_id))
        .ok_or(TrainingWorkspaceError::MapNotFound)?;
    if !controls.allowed_actions.contains(&action) {
        return Err(TrainingWorkspaceError::InvalidInput(
            "queue_action_not_allowed".to_string(),
        ));
    }
    let store = state
        .training_workspace
        .as_ref()
        .ok_or(TrainingWorkspaceError::StoreFailed)?;
    if canonical.runtime.execution_profile.operation == ExecutionOperation::Cut
        && training_print_action(action)
        && training_rezka_frame_count(&training_map, &apparatus).is_err()
        && let Some(template) = store
            .template_for_order(order_id, &training_map.order_number)
            .await?
    {
        let cut_apparatus_ids =
            std::collections::BTreeSet::from([canonical.runtime.apparatus_id.clone()]);
        super::production_maps::apply_order_rezka_kadr_count(
            &mut training_map,
            &template,
            &cut_apparatus_ids,
        );
        if training_rezka_frame_count(&training_map, &apparatus).is_ok() {
            store.save_map(training_map.clone()).await?;
        }
    }
    let current = overlay
        .queue_states
        .get(&apparatus)
        .and_then(|states| states.get(order_id))
        .and_then(|state| queue_state::ApparatusQueueOrderState::parse(state))
        .unwrap_or(queue_state::ApparatusQueueOrderState::Pending);
    let next = queue_state::next_queue_state(current, action).map_err(|_| {
        TrainingWorkspaceError::InvalidInput("queue_action_not_allowed".to_string())
    })?;

    let previous_stage = training_input_stage_for_map(&training_map, &apparatus);
    let input_batches = overlay
        .input_progress_batches
        .get(order_id)
        .cloned()
        .unwrap_or_default();
    let active_input_batch = previous_stage.as_deref().and_then(|stage| {
        input_batches
            .iter()
            .find(|batch| {
                batch.wip_status == OrderProgressBatchWipStatus::InUse
                    && training_input_batch_matches(batch, order_id, stage, &apparatus)
                    && canonical_apparatus_matches(&batch.used_by_apparatus, &apparatus)
            })
            .cloned()
    });
    let current_input_batch_id = active_input_batch
        .as_ref()
        .map(|batch| batch.batch_id.as_str())
        .unwrap_or_default();
    let has_unprocessed_previous_wips = previous_stage.as_deref().is_some_and(|stage| {
        training_has_unprocessed_previous_wips(
            &input_batches,
            order_id,
            stage,
            &apparatus,
            current_input_batch_id,
        )
    });
    let full_completion_report_required = training_complete_requires_full_report(
        &training_map,
        &apparatus,
        has_unprocessed_previous_wips,
    );

    let is_complete = matches!(action, queue_state::ApparatusQueueAction::Complete);
    let has_returned_paint_items = !print_input.returned_paint_items.is_empty();
    let has_returned_paint_image = !print_input.returned_paint_image_id.trim().is_empty();
    if !is_complete && (has_returned_paint_items || has_returned_paint_image) {
        return Err(TrainingWorkspaceError::InvalidInput(
            "returned_paint_only_on_complete".to_string(),
        ));
    }
    if is_complete
        && pechat::is_pechat_apparatus(canonical.as_ref())
        && !returned_paint_report_can_close(
            &print_input.returned_paint_items,
            has_returned_paint_image,
        )
    {
        return Err(TrainingWorkspaceError::InvalidInput(
            "returned_paint_minimum_three_fields_or_image_only".to_string(),
        ));
    }
    let returned_paint_calculation = if has_returned_paint_items {
        Some(
            calculate_returned_paint(&print_input.returned_paint_items).map_err(|error| {
                TrainingWorkspaceError::InvalidInput(format!(
                    "training_returned_paint_invalid: {error}"
                ))
            })?,
        )
    } else {
        None
    };
    let returned_paint_astatka_kg = if has_returned_paint_items {
        let total =
            returned_paint_astatka_total(&print_input.returned_paint_items).map_err(|error| {
                TrainingWorkspaceError::InvalidInput(format!(
                    "training_returned_paint_invalid: {error}"
                ))
            })?;
        (total > 0.0).then_some(total)
    } else {
        None
    };
    let return_ink_kg = returned_paint_astatka_kg.or_else(|| {
        print_input
            .return_ink_kg
            .filter(|value| value.is_finite() && *value >= 0.0)
    });
    if is_rezka_apparatus(&training_map, &apparatus) && training_print_action(action) {
        if !training_rezka_progress_metrics_are_complete(&print_input) {
            return Err(TrainingWorkspaceError::InvalidInput(
                "rezka_progress_metrics_required".to_string(),
            ));
        }
        training_rezka_frame_count(&training_map, &apparatus)?;
    }
    if is_complete && is_laminatsiya_apparatus(&training_map, &apparatus) {
        let metrics_ready = if full_completion_report_required {
            training_laminatsiya_full_metrics_are_complete(&print_input)
        } else {
            training_laminatsiya_partial_metrics_are_complete(&print_input)
        };
        if !metrics_ready {
            return Err(TrainingWorkspaceError::InvalidInput(
                "laminatsiya_completion_metrics_required".to_string(),
            ));
        }
    }
    if is_complete
        && is_rezka_apparatus(&training_map, &apparatus)
        && full_completion_report_required
        && !training_rezka_waste_metrics_are_complete(&print_input)
    {
        return Err(TrainingWorkspaceError::InvalidInput(
            "rezka_progress_metrics_required".to_string(),
        ));
    }
    let returned_paint_report =
        if is_complete && (has_returned_paint_items || has_returned_paint_image) {
            Some(
                store
                    .save_returned_paint_report(
                        order_id,
                        &apparatus,
                        training_action_value(action),
                        &print_input.returned_paint_items,
                        &print_input.returned_paint_image_id,
                        return_ink_kg,
                        returned_paint_calculation.as_ref(),
                    )
                    .await?,
            )
        } else {
            None
        };

    let mut input_batch_update = None;
    let mut action_input_batch = active_input_batch.clone();
    if matches!(action, queue_state::ApparatusQueueAction::Start) {
        if let Some(previous_stage) = previous_stage.as_deref() {
            let scanned_qr = if progress_qr.trim().is_empty() {
                progress_batch_id.trim()
            } else {
                progress_qr.trim()
            };
            if scanned_qr.is_empty() {
                return Err(TrainingWorkspaceError::InvalidInput(
                    "progress_qr_required".to_string(),
                ));
            }
            let selected = input_batches.iter().find(|batch| {
                training_input_batch_matches(batch, order_id, previous_stage, &apparatus)
                    && (batch.qr_payload.eq_ignore_ascii_case(scanned_qr)
                        || batch.batch_id.eq_ignore_ascii_case(scanned_qr))
            });
            let Some(selected) = selected else {
                return Err(TrainingWorkspaceError::InvalidInput(
                    "progress_batch_not_accepted".to_string(),
                ));
            };
            if selected.wip_status != OrderProgressBatchWipStatus::Waiting {
                return Err(TrainingWorkspaceError::InvalidInput(
                    "progress_batch_not_accepted".to_string(),
                ));
            }
            let claimed = training_claim_input_batch(selected, &apparatus, order_id);
            action_input_batch = Some(claimed.clone());
            input_batch_update = Some(claimed);
        }
        let assigned = store
            .raw_material_barcodes_for_order_apparatus(order_id, &apparatus)
            .await?;
        let mut scanned = material_barcodes
            .iter()
            .map(|barcode| barcode.trim().to_string())
            .filter(|barcode| !barcode.is_empty())
            .collect::<Vec<_>>();
        if scanned.is_empty() && !material_barcode.trim().is_empty() {
            scanned.push(material_barcode.trim().to_string());
        }
        if !assigned.is_empty() && scanned.is_empty() {
            return Err(TrainingWorkspaceError::InvalidInput(
                "training_material_scan_required".to_string(),
            ));
        }
        if !assigned.is_empty()
            && scanned.iter().any(|barcode| {
                !assigned
                    .iter()
                    .any(|expected| expected.eq_ignore_ascii_case(barcode))
            })
        {
            return Err(TrainingWorkspaceError::InvalidInput(
                "training_material_not_assigned".to_string(),
            ));
        }
    }

    if training_print_action(action) && previous_stage.is_some() && action_input_batch.is_none() {
        return Err(TrainingWorkspaceError::InvalidInput(
            "progress_qr_required".to_string(),
        ));
    }

    let parent_batch_id = if training_print_action(action) {
        action_input_batch
            .as_ref()
            .map(|batch| batch.batch_id.clone())
            .unwrap_or_default()
    } else {
        String::new()
    };
    let progress_batches = if training_print_action(action) {
        training_progress_batches(
            &training_map,
            &apparatus,
            order_id,
            action,
            principal,
            &print_input,
            returned_paint_report.as_ref(),
            return_ink_kg,
            &parent_batch_id,
        )?
    } else {
        Vec::new()
    };

    if matches!(
        action,
        queue_state::ApparatusQueueAction::Complete
            | queue_state::ApparatusQueueAction::RollComplete
    ) {
        input_batch_update = action_input_batch
            .as_ref()
            .map(|batch| training_process_input_batch(batch, &apparatus, order_id));
    }
    let mut persisted_progress_batches = progress_batches.clone();
    if let Some(input_batch_update) = input_batch_update {
        persisted_progress_batches.push(input_batch_update);
    }

    let mut states = overlay
        .queue_states
        .get(&apparatus)
        .cloned()
        .unwrap_or_default();
    let persisted_state = if is_complete && has_unprocessed_previous_wips {
        queue_state::ApparatusQueueOrderState::Pending
    } else {
        next
    };
    states.insert(order_id.to_string(), persisted_state.as_str().to_string());
    let event_id = format!("training-queue-event-{}-{}", unix_micros(), order_id);
    store
        .put_queue_state_with_event(
            &apparatus,
            order_id,
            persisted_state.as_str(),
            &event_id,
            training_action_value(action),
            current.as_str(),
            &principal.ref_,
            &principal.display_name,
            &persisted_progress_batches,
        )
        .await?;
    state.production_maps.notify_live();
    let order_status = training_order_status(persisted_state);
    let print_requests = progress_batches
        .iter()
        .map(|batch| {
            training_progress_print_request(
                batch,
                &print_input,
                &canonical.runtime.display.display_name,
            )
        })
        .collect::<Vec<_>>();
    let prints = training_progress_prints(
        state,
        print_requests,
        &print_input.print_transport,
        &apparatus,
        order_id,
        action,
    );
    let progress_batch = progress_batches.first().cloned();
    let print = prints.first().cloned();
    Ok(Some(serde_json::json!({
        "ok": true,
        "states": states,
        "order_status": order_status,
        "session": null,
        "progress_event": null,
        "progress_batch": progress_batch,
        "progress_batches": progress_batches,
        "print": print,
        "prints": prints,
    })))
}

fn training_map_has_apparatus(saved: &ProductionMapSaved, apparatus: &str) -> bool {
    saved.map.nodes.iter().any(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && !is_training_input_node(node)
            && canonical_apparatus_matches(&node.apparatus_id, apparatus)
    })
}

fn training_print_action(action: queue_state::ApparatusQueueAction) -> bool {
    matches!(
        action,
        queue_state::ApparatusQueueAction::Pause
            | queue_state::ApparatusQueueAction::DetachRoll
            | queue_state::ApparatusQueueAction::RollComplete
            | queue_state::ApparatusQueueAction::Complete
    )
}

fn training_action_label(action: queue_state::ApparatusQueueAction) -> &'static str {
    match action {
        queue_state::ApparatusQueueAction::Pause => "pauza",
        queue_state::ApparatusQueueAction::Freeze => "muzlatildi",
        queue_state::ApparatusQueueAction::DetachRoll => "rulon yechildi",
        queue_state::ApparatusQueueAction::RollComplete => "rulon tugatildi",
        queue_state::ApparatusQueueAction::Complete => "ish tugatildi",
        queue_state::ApparatusQueueAction::Start => "ish boshlandi",
        queue_state::ApparatusQueueAction::Resume => "ish davom etdi",
    }
}

fn training_action_value(action: queue_state::ApparatusQueueAction) -> &'static str {
    match action {
        queue_state::ApparatusQueueAction::Pause => "pause",
        queue_state::ApparatusQueueAction::Freeze => "freeze",
        queue_state::ApparatusQueueAction::DetachRoll => "detach_roll",
        queue_state::ApparatusQueueAction::RollComplete => "roll_complete",
        queue_state::ApparatusQueueAction::Complete => "complete",
        queue_state::ApparatusQueueAction::Start => "start",
        queue_state::ApparatusQueueAction::Resume => "resume",
    }
}

fn training_positive_quantity(value: Option<f64>, fallback: f64) -> f64 {
    value
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or(fallback)
}

fn training_output_quantity(input: &TrainingQueuePrintInput) -> f64 {
    training_positive_quantity(
        input.progress_qty.or(input.finished_goods_meter),
        training_positive_quantity(input.finished_goods_kg, 1.0),
    )
}

fn training_rezka_progress_metrics_are_complete(input: &TrainingQueuePrintInput) -> bool {
    let is_positive =
        |value: Option<f64>| value.is_some_and(|value| value.is_finite() && value > 0.0);
    is_positive(input.progress_qty.or(input.finished_goods_meter))
        && is_positive(input.gross_qty.or(input.finished_goods_kg))
        && is_positive(input.diameter)
}

fn training_rezka_waste_metrics_are_complete(input: &TrainingQueuePrintInput) -> bool {
    [
        input.total_waste,
        input.rezka_bosma_waste,
        input.rezka_lamination_waste,
        input.rezka_edge_waste,
    ]
    .into_iter()
    .any(|value| value.is_some_and(|value| value.is_finite() && value > 0.0))
}
