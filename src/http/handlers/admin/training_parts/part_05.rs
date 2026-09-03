
fn training_laminatsiya_partial_metrics_are_complete(input: &TrainingQueuePrintInput) -> bool {
    let is_positive =
        |value: Option<f64>| value.is_some_and(|value| value.is_finite() && value > 0.0);
    is_positive(input.finished_goods_kg.or(input.gross_qty))
        && is_positive(input.finished_goods_meter.or(input.progress_qty))
        && is_positive(input.bobina_kg)
}

fn training_laminatsiya_full_metrics_are_complete(input: &TrainingQueuePrintInput) -> bool {
    training_laminatsiya_partial_metrics_are_complete(input)
        && (input
            .lamination_print_leftover_rolls
            .is_some_and(|value| value.is_finite() && value >= 0.0)
            || input
                .lamination_film_leftover_rolls
                .is_some_and(|value| value.is_finite() && value >= 0.0))
        && input
            .total_waste
            .is_some_and(|value| value.is_finite() && value > 0.0)
}

fn training_apparatus_node<'a>(
    map: &'a ProductionMapDefinition,
    apparatus: &str,
) -> Option<&'a ProductionMapNode> {
    map.nodes.iter().find(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && (canonical_apparatus_matches(&node.apparatus_id, apparatus)
                || canonical_apparatus_matches(&node.alternative_assigned_apparatus_id, apparatus))
    })
}

fn training_rezka_frame_count(
    map: &ProductionMapDefinition,
    apparatus: &str,
) -> Result<usize, TrainingWorkspaceError> {
    training_rezka_output_kadr_counts(map, apparatus).map(|counts| counts.len())
}

fn training_rezka_output_kadr_counts(
    map: &ProductionMapDefinition,
    apparatus: &str,
) -> Result<Vec<usize>, TrainingWorkspaceError> {
    let stage = chain::work_stage_for_station(map, apparatus, "").ok_or_else(|| {
        TrainingWorkspaceError::InvalidInput("rezka_kadr_count_required".to_string())
    })?;
    let node = map
        .nodes
        .iter()
        .find(|node| node.id.trim() == stage.node_id.trim())
        .ok_or_else(|| {
            TrainingWorkspaceError::InvalidInput("rezka_kadr_count_required".to_string())
        })?;
    let total = node
        .rezka_kadr_count
        .filter(|value| *value > 0)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| {
            TrainingWorkspaceError::InvalidInput("rezka_kadr_count_required".to_string())
        })?;
    if chain::is_final_work_stage_node(map, &stage.node_id) || node.rezka_frame_groups.is_empty() {
        return Ok(vec![1; total]);
    }
    node.rezka_frame_groups
        .iter()
        .map(|value| {
            usize::try_from(*value)
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| {
                    TrainingWorkspaceError::InvalidInput(
                        "rezka_frame_groups_invalid".to_string(),
                    )
                })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn training_progress_batches(
    map: &ProductionMapDefinition,
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    principal: &Principal,
    input: &TrainingQueuePrintInput,
    returned_paint_report: Option<&serde_json::Value>,
    return_ink_kg: Option<f64>,
    parent_batch_id: &str,
) -> Result<Vec<OrderProgressBatch>, TrainingWorkspaceError> {
    let stamp = unix_micros();
    let base_batch_id = progress_batch_id(apparatus, order_id, action);
    let rezka_node = training_apparatus_node(map, apparatus);
    let rezka_output_is_grouped = rezka_node.is_some_and(|node| {
        !node.rezka_frame_groups.is_empty()
            && !chain::is_final_work_stage_node(map, &node.id)
    });
    let is_rezka = is_rezka_apparatus(map, apparatus);
    let output_kadr_counts = if is_rezka {
        training_rezka_output_kadr_counts(map, apparatus)?
    } else {
        vec![1]
    };
    let frame_count = output_kadr_counts.len();
    let item_code = if map.product_code.trim().is_empty() {
        if map.order_number.trim().is_empty() {
            order_id.trim().to_string()
        } else {
            map.order_number.trim().to_string()
        }
    } else {
        map.product_code.trim().to_string()
    };
    let title = if map.title.trim().is_empty() {
        item_code.clone()
    } else {
        map.title.trim().to_string()
    };
    let executor_name = if principal.display_name.trim().is_empty() {
        principal.ref_.trim().to_string()
    } else {
        principal.display_name.trim().to_string()
    };
    let uom = if input.uom.trim().is_empty() {
        "m"
    } else {
        input.uom.trim()
    };
    let produced_qty = training_output_quantity(input);
    let finished_goods_kg =
        training_positive_quantity(input.finished_goods_kg.or(input.gross_qty), produced_qty);
    let timestamp = (stamp / 1_000_000) as i64;
    let status = match action {
        queue_state::ApparatusQueueAction::Pause => OrderProgressBatchStatus::Paused,
        queue_state::ApparatusQueueAction::Freeze => OrderProgressBatchStatus::Completed,
        queue_state::ApparatusQueueAction::DetachRoll => OrderProgressBatchStatus::RollDetached,
        _ => OrderProgressBatchStatus::Completed,
    };
    let description = if input.description.trim().is_empty() {
        "Training progress"
    } else {
        input.description.trim()
    };
    let next_apparatus = chain::next_work_stage_station(map, apparatus).unwrap_or_default();
    let label_name = format!(
        "{title}, apparat: {}, {}",
        apparatus.trim(),
        training_action_label(action),
    );
    let session_id = format!("training-session:{base_batch_id}");
    let mut batches = Vec::with_capacity(frame_count);
    for (index, contained_kadr_count) in output_kadr_counts.iter().copied().enumerate() {
        let owns_metrics = !is_rezka || index == 0;
        let batch_id = if is_rezka {
            format!("{base_batch_id}:frame:{}", index + 1)
        } else {
            base_batch_id.clone()
        };
        let qr_payload = progress_qr_payload(&batch_id);
        let rezka_bosma_waste = owns_metrics.then_some(input.rezka_bosma_waste).flatten();
        let rezka_lamination_waste = owns_metrics
            .then_some(input.rezka_lamination_waste)
            .flatten();
        let rezka_edge_waste = owns_metrics.then_some(input.rezka_edge_waste).flatten();
        let total_waste = owns_metrics.then_some(input.total_waste).flatten();
        let bobina_kg = owns_metrics.then_some(input.bobina_kg).flatten();
        let diameter = owns_metrics.then_some(input.diameter).flatten();
        let mut payload_json = serde_json::json!({
            "training": true,
            "order_title": map.title.trim(),
            "customer_name": map.customer_name.trim(),
            "apparatus": apparatus.trim(),
            "action": training_action_value(action),
            "action_label": training_action_label(action),
            "astatka_kg": return_ink_kg,
            "return_ink_kg": return_ink_kg,
            "lamination_print_leftover_rolls": input.lamination_print_leftover_rolls,
            "lamination_film_leftover_rolls": input.lamination_film_leftover_rolls,
            "rezka_bosma_waste": rezka_bosma_waste,
            "rezka_lamination_waste": rezka_lamination_waste,
            "rezka_edge_waste": rezka_edge_waste,
            "total_waste": total_waste,
            "total_waste_uom": "kg",
            "finished_goods_kg": finished_goods_kg,
            "bobina_kg": bobina_kg,
            "finished_goods_meter": input.finished_goods_meter,
            "diameter": diameter,
            "description": description,
            "returned_paint_report": returned_paint_report
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        });
        if is_rezka {
            payload_json["rezka_frame_index"] = serde_json::json!(index + 1);
            payload_json["rezka_frame_count"] = serde_json::json!(frame_count);
            payload_json["rezka_kadr_count"] = serde_json::json!(
                rezka_node.and_then(|node| node.rezka_kadr_count).unwrap_or_default()
            );
            payload_json["contained_kadr_count"] = serde_json::json!(contained_kadr_count);
            payload_json["rezka_output_kind"] = serde_json::json!(
                if rezka_output_is_grouped {
                    "grouped_roll"
                } else {
                    "frame"
                }
            );
            payload_json["rezka_metrics_owner"] = serde_json::json!(owns_metrics);
            if let Some(label_length) = rezka_node.and_then(|node| node.rezka_label_length) {
                payload_json["rezka_label_length"] = serde_json::json!(label_length);
            }
        }
        let mut batch = OrderProgressBatch {
            batch_id,
            revision: 1,
            session_id: session_id.clone(),
            started_at_unix: timestamp,
            completed_at_unix: timestamp,
            apparatus: apparatus.trim().to_string(),
            order_id: order_id.trim().to_string(),
            action,
            status,
            produced_qty,
            uom: uom.to_string(),
            qr_payload,
            label_item_code: item_code.clone(),
            label_item_name: label_name.clone(),
            executor_name: executor_name.clone(),
            worker_role: "training".to_string(),
            worker_ref: principal.ref_.trim().to_string(),
            worker_display_name: principal.display_name.trim().to_string(),
            wip_status: OrderProgressBatchWipStatus::Waiting,
            status_detail: OrderProgressBatchStatusDetail::default(),
            current_apparatus: apparatus.trim().to_string(),
            current_location: apparatus.trim().to_string(),
            next_apparatus: next_apparatus.clone(),
            parent_batch_id: parent_batch_id.trim().to_string(),
            used_by_session_id: String::new(),
            used_by_apparatus: String::new(),
            processed_by_session_id: String::new(),
            processed_by_apparatus: String::new(),
            return_ink_kg,
            lamination_print_leftover_rolls: input.lamination_print_leftover_rolls,
            lamination_film_leftover_rolls: input.lamination_film_leftover_rolls,
            rezka_bosma_waste,
            rezka_lamination_waste,
            rezka_edge_waste,
            total_waste,
            finished_goods_kg: Some(finished_goods_kg),
            bobina_kg,
            finished_goods_meter: input.finished_goods_meter,
            diameter,
            description: description.to_string(),
            payload_json,
        };
        batch.refresh_status_detail();
        batches.push(batch);
    }
    Ok(batches)
}

fn training_progress_print_request(
    batch: &OrderProgressBatch,
    input: &TrainingQueuePrintInput,
    apparatus_display_name: &str,
) -> crate::core::gscale::models::ProgressLabelPrintRequest {
    crate::core::gscale::models::ProgressLabelPrintRequest {
        driver_url: input.driver_url.clone(),
        qr_payload: batch.qr_payload.clone(),
        item_code: batch.label_item_code.clone(),
        item_name: batch.label_item_name.clone(),
        apparatus: batch.apparatus.clone(),
        apparatus_display_name: apparatus_display_name.trim().to_string(),
        customer_name: input.customer_name.trim().to_string(),
        executor_name: batch.executor_name.clone(),
        printer: input.printer.clone(),
        print_mode: input.print_mode.clone(),
        gross_qty: input
            .gross_qty
            .or(input.finished_goods_kg)
            .unwrap_or(batch.produced_qty),
        tare_enabled: input.bobina_kg.is_some_and(|value| value > 0.0),
        tare_kg: input.bobina_kg.unwrap_or(0.0),
        progress_qty: batch.produced_qty,
        unit: "kg".to_string(),
        progress_unit: if batch.uom.trim().is_empty() {
            "m".to_string()
        } else {
            batch.uom.clone()
        },
        label_kind: "progress".to_string(),
        print_count: input.print_count,
    }
}

fn training_progress_prints(
    state: &AppState,
    requests: Vec<crate::core::gscale::models::ProgressLabelPrintRequest>,
    print_transport: &str,
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
) -> Vec<serde_json::Value> {
    if print_transport.trim().eq_ignore_ascii_case("offline") {
        return requests
            .into_iter()
            .map(
                |request| match state.gscale.prepare_progress_label(request) {
                    Ok(response) => {
                        serde_json::to_value(response).unwrap_or(serde_json::Value::Null)
                    }
                    Err(_) => training_progress_print_failure(),
                },
            )
            .collect();
    }

    let mut prints = Vec::with_capacity(requests.len());
    let mut queued_requests = Vec::with_capacity(requests.len());
    for request in requests {
        match state.gscale.prepare_progress_label(request.clone()) {
            Ok(mut response) => {
                response.status = "queued".to_string();
                response.printer_status = "server_print_queued".to_string();
                prints.push(serde_json::to_value(response).unwrap_or(serde_json::Value::Null));
                queued_requests.push(request);
            }
            Err(_) => prints.push(training_progress_print_failure()),
        }
    }

    if !queued_requests.is_empty() {
        let gscale = state.gscale.clone();
        let apparatus = apparatus.trim().to_string();
        let order_id = order_id.trim().to_string();
        tokio::spawn(async move {
            for request in queued_requests {
                if let Err(error) = gscale.print_progress_label(request).await {
                    tracing::warn!(
                        error = %error,
                        apparatus = %apparatus,
                        order_id = %order_id,
                        action = ?action,
                        "training progress label print failed"
                    );
                }
            }
        });
    }
    prints
}

fn training_progress_print_failure() -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "status": "failed",
        "code": "training_print_failed",
        "error": "training_progress_label_prepare_failed",
    })
}

fn is_training_apparatus(apparatus: &str, active_apparatuses: &[String]) -> bool {
    active_apparatuses
        .iter()
        .any(|active| canonical_apparatus_matches(active, apparatus))
}
