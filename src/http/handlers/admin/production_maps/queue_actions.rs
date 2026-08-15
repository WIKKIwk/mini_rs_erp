use super::*;
use crate::core::production_map::pechat;
use crate::core::returned_paint::{
    ReturnedPaintError, ReturnedPaintItem, ReturnedPaintRequestCreate, ReturnedPaintStatus,
    returned_paint_astatka_total, returned_paint_report_can_close, returned_paint_value_count,
};

#[derive(serde::Deserialize)]
struct ApparatusQueueActionRequest {
    #[serde(default)]
    apparatus: String,
    #[serde(default)]
    order_id: String,
    #[serde(default)]
    material_barcode: String,
    #[serde(default)]
    material_barcodes: Vec<String>,
    #[serde(default)]
    qolip_code: String,
    #[serde(default)]
    qolip_codes: Vec<String>,
    #[serde(default)]
    produced_qty: Option<f64>,
    #[serde(default)]
    qty: Option<f64>,
    #[serde(default)]
    gross_qty: Option<f64>,
    #[serde(default)]
    return_ink_kg: Option<f64>,
    #[serde(default)]
    lamination_print_leftover_rolls: Option<f64>,
    #[serde(default)]
    lamination_film_leftover_rolls: Option<f64>,
    #[serde(default)]
    rezka_bosma_waste: Option<f64>,
    #[serde(default)]
    rezka_lamination_waste: Option<f64>,
    #[serde(default)]
    rezka_edge_waste: Option<f64>,
    #[serde(default)]
    total_waste: Option<f64>,
    #[serde(default)]
    finished_goods_kg: Option<f64>,
    #[serde(default, alias = "babina_kg")]
    bobina_kg: Option<f64>,
    #[serde(default)]
    finished_goods_meter: Option<f64>,
    #[serde(default)]
    diameter: Option<f64>,
    #[serde(default)]
    uom: String,
    #[serde(default)]
    unit: String,
    #[serde(default)]
    progress_batch_id: String,
    #[serde(default)]
    progress_qr: String,
    #[serde(default)]
    qr_payload: String,
    #[serde(default)]
    driver_url: String,
    #[serde(default)]
    printer: String,
    #[serde(default)]
    print_mode: String,
    #[serde(default)]
    customer_name: String,
    #[serde(default)]
    print_count: u32,
    #[serde(default)]
    print_transport: String,
    #[serde(default)]
    completion_request_note: String,
    #[serde(default)]
    full_completion_report_required: bool,
    #[serde(default)]
    worker_handoff: bool,
    #[serde(default)]
    remove_roll_from_apparatus: bool,
    #[serde(default)]
    description: String,
    #[serde(default)]
    returned_paint_items: Vec<ReturnedPaintItem>,
    #[serde(default)]
    returned_paint_image_id: String,
    #[serde(default)]
    freeze_request_id: String,
    #[serde(default)]
    freeze_with_issue: bool,
    #[serde(default)]
    issue_note: String,
    #[serde(default)]
    rezka_frames: Vec<RezkaFrameProgressInput>,
    action: queue_state::ApparatusQueueAction,
}

pub async fn production_map_queue_action(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AdminError> {
    let principal = authorize_any_capability(
        &state,
        &headers,
        &[
            Capability::AdminAccess,
            Capability::ProductionMapManage,
            Capability::ApparatusQueueManage,
        ],
    )
    .await?;
    if method != Method::POST {
        return Err(method_not_allowed());
    }
    let mut input: ApparatusQueueActionRequest = parse_json(&body)?;
    if input.apparatus.trim().is_empty() || input.order_id.trim().is_empty() {
        return Err(bad_request("apparatus and order_id are required"));
    }
    input.action = canonical_queue_action(
        input.action,
        input.worker_handoff,
        input.remove_roll_from_apparatus,
        &input.freeze_request_id,
        input.freeze_with_issue,
        &principal,
    );
    let explicit_worker_freeze = input.action == queue_state::ApparatusQueueAction::Freeze
        && input.freeze_request_id.trim().is_empty();
    if input.freeze_with_issue || explicit_worker_freeze {
        if principal.role != PrincipalRole::Aparatchi {
            return Err(forbidden());
        }
        if input.action != queue_state::ApparatusQueueAction::Freeze {
            return Err(bad_request("freeze_with_issue_only_on_freeze"));
        }
        if input.issue_note.trim().is_empty() && !input.description.trim().is_empty() {
            input.issue_note = input.description.clone();
        }
        if input.issue_note.trim().is_empty() {
            return Err(bad_request("issue_note_required"));
        }
        if !input.freeze_request_id.trim().is_empty() {
            return Err(bad_request("freeze_with_issue_cannot_use_freeze_request_id"));
        }
        if input.worker_handoff || input.remove_roll_from_apparatus {
            return Err(bad_request("freeze_with_issue_actions_conflict"));
        }
        if input.order_id.trim().starts_with("training-") {
            return Err(bad_request("freeze_with_issue_not_supported_for_training"));
        }
        // `freeze_with_issue` remains accepted for old clients, but the
        // persisted intent is always the explicit frozen transition.
        input.freeze_with_issue = true;
    }
    if input.worker_handoff && !matches!(input.action, queue_state::ApparatusQueueAction::Pause) {
        return Err(bad_request("worker_handoff_only_on_pause"));
    }
    if input.remove_roll_from_apparatus
        && !matches!(input.action, queue_state::ApparatusQueueAction::DetachRoll)
    {
        return Err(bad_request("roll_removal_only_on_detach_roll"));
    }
    if input.worker_handoff && input.remove_roll_from_apparatus {
        return Err(bad_request("worker_handoff_actions_conflict"));
    }
    if !input.rezka_frames.is_empty()
        && (!input.apparatus.trim().to_ascii_lowercase().contains("rezka")
            || !matches!(
                input.action,
                queue_state::ApparatusQueueAction::Pause
                    | queue_state::ApparatusQueueAction::DetachRoll
                    | queue_state::ApparatusQueueAction::RollComplete
                    | queue_state::ApparatusQueueAction::Complete
            ))
    {
        return Err(bad_request("rezka_frames_only_on_rezka_progress"));
    }
    if let Some(training_result) = super::super::training::training_queue_action(
        &state,
        &principal,
        &input.apparatus,
        &input.order_id,
        input.action,
        &input.material_barcode,
        &input.material_barcodes,
        &input.progress_batch_id,
        if input.qr_payload.trim().is_empty() {
            &input.progress_qr
        } else {
            &input.qr_payload
        },
        super::super::training::TrainingQueuePrintInput {
            driver_url: input.driver_url.clone(),
            printer: input.printer.clone(),
            print_mode: input.print_mode.clone(),
            print_transport: input.print_transport.clone(),
            progress_qty: input.produced_qty.or(input.qty),
            gross_qty: input.gross_qty,
            finished_goods_kg: input.finished_goods_kg,
            bobina_kg: input.bobina_kg,
            return_ink_kg: input.return_ink_kg,
            lamination_print_leftover_rolls: input.lamination_print_leftover_rolls,
            lamination_film_leftover_rolls: input.lamination_film_leftover_rolls,
            rezka_bosma_waste: input.rezka_bosma_waste,
            rezka_lamination_waste: input.rezka_lamination_waste,
            rezka_edge_waste: input.rezka_edge_waste,
            total_waste: input.total_waste,
            finished_goods_meter: input.finished_goods_meter,
            diameter: input.diameter,
            returned_paint_items: input.returned_paint_items.clone(),
            returned_paint_image_id: input.returned_paint_image_id.clone(),
            description: if input.completion_request_note.trim().is_empty() {
                input.description.clone()
            } else {
                input.completion_request_note.clone()
            },
            uom: if input.uom.trim().is_empty() {
                input.unit.clone()
            } else {
                input.uom.clone()
            },
            customer_name: input.customer_name.clone(),
            print_count: input.print_count,
        },
    )
    .await
    .map_err(super::super::training::training_workspace_error)?
    {
        return Ok(json_response(training_result));
    }
    let assigned_apparatus = state.admin.principal_assigned_apparatus(&principal).await;
    let material_barcodes = input.material_barcodes.clone();
    let material_barcode = if material_barcodes.is_empty() {
        input.material_barcode.clone()
    } else {
        material_barcodes.join(",")
    };
    let state_material_barcodes =
        if matches!(input.action, queue_state::ApparatusQueueAction::Start) {
            super::raw_materials::raw_material_state_barcodes_for_order_apparatus(
                &state,
                &input.order_id,
                &input.apparatus,
            )
            .await?
        } else {
            Vec::new()
        };
    let produced_qty = input.produced_qty.or(input.qty);
    let completion_request_note = if input.freeze_with_issue {
        input.issue_note.clone()
    } else if input.completion_request_note.trim().is_empty() {
        input.description.clone()
    } else {
        input.completion_request_note.clone()
    };
    if !matches!(input.action, queue_state::ApparatusQueueAction::Complete)
        && (!input.returned_paint_items.is_empty()
            || !input.returned_paint_image_id.trim().is_empty())
    {
        return Err(bad_request("returned_paint_only_on_complete"));
    }
    let returned_paint_field_count = returned_paint_value_count(&input.returned_paint_items);
    let has_returned_paint_image = !input.returned_paint_image_id.trim().is_empty();
    let is_bosma_complete = matches!(input.action, queue_state::ApparatusQueueAction::Complete)
        && pechat::is_pechat_apparatus(&input.apparatus);
    if is_bosma_complete
        && !returned_paint_report_can_close(&input.returned_paint_items, has_returned_paint_image)
    {
        return Err(bad_request(
            "returned_paint_minimum_three_fields_or_image_only",
        ));
    }
    let _queue_action_guard = state.production_maps.queue_action_guard().await;
    let returned_paint_report =
        if matches!(input.action, queue_state::ApparatusQueueAction::Complete)
            && (returned_paint_field_count > 0 || has_returned_paint_image)
        {
            let map = state
                .production_maps
                .raw_map(&input.order_id)
                .await
                .map_err(production_map_error)?
                .ok_or_else(|| production_map_error(ProductionMapError::MapNotFound))?;
            let order_code = if map.code.trim().is_empty() {
                map.order_number.clone()
            } else {
                map.code.clone()
            };
            Some(
                state
                    .returned_paint
                    .prepare_request(
                        ReturnedPaintRequestCreate {
                            order_id: map.id,
                            order_code,
                            order_name: map.title,
                            apparatus: input.apparatus.clone(),
                            image_id: input.returned_paint_image_id.clone(),
                            items: input.returned_paint_items.clone(),
                        },
                        &principal,
                        format!(
                            "returned_paint_complete:{}:{}",
                            input.order_id.trim(),
                            input.apparatus.trim()
                        ),
                    )
                    .await
                    .map_err(returned_paint_queue_error)?,
            )
        } else {
            None
        };
    let returned_paint_report_attached = returned_paint_report.is_some();
    let return_ink_kg = match &returned_paint_report {
        Some(report) if report.status == ReturnedPaintStatus::Completed => {
            let total =
                returned_paint_astatka_total(&report.items).map_err(returned_paint_queue_error)?;
            (total > 0.0).then_some(total)
        }
        Some(_) => None,
        None => input.return_ink_kg,
    };
    let progress = QueueProgressInput {
        freeze_request_id: input.freeze_request_id.clone(),
        freeze_with_issue: input.freeze_with_issue,
        rezka_frames: input.rezka_frames.clone(),
        produced_qty,
        gross_qty: input.gross_qty,
        uom: if input.uom.trim().is_empty() {
            input.unit.clone()
        } else {
            input.uom.clone()
        },
        progress_batch_id: input.progress_batch_id.clone(),
        qr_payload: if input.qr_payload.trim().is_empty() {
            input.progress_qr.clone()
        } else {
            input.qr_payload.clone()
        },
        return_ink_kg,
        lamination_print_leftover_rolls: input.lamination_print_leftover_rolls,
        lamination_film_leftover_rolls: input.lamination_film_leftover_rolls,
        rezka_bosma_waste: input.rezka_bosma_waste,
        rezka_lamination_waste: input.rezka_lamination_waste,
        rezka_edge_waste: input.rezka_edge_waste,
        total_waste: input.total_waste,
        finished_goods_kg: input.finished_goods_kg,
        bobina_kg: input.bobina_kg,
        finished_goods_meter: input.finished_goods_meter,
        diameter: input.diameter,
        description: completion_request_note.clone(),
        returned_paint_report_attached,
        force_full_completion_metrics: input.full_completion_report_required,
        allow_partial_station_completion: false,
        worker_handoff: input.worker_handoff,
        remove_roll_from_apparatus: input.remove_roll_from_apparatus,
    };
    let has_complete_bosma_metrics = (return_ink_kg.is_some() || returned_paint_report_attached)
        && input.total_waste.is_some()
        && input.finished_goods_kg.is_some()
        && input.finished_goods_meter.is_some();
    let has_complete_laminatsiya_metrics = (input.lamination_print_leftover_rolls.is_some()
        || input.lamination_film_leftover_rolls.is_some())
        && input.total_waste.is_some()
        && input.finished_goods_kg.is_some()
        && input.finished_goods_meter.is_some();
    let is_rezka = input
        .apparatus
        .trim()
        .to_ascii_lowercase()
        .contains("rezka");
    let has_rezka_progress_metrics =
        is_rezka && rezka_queue_quantity_metrics_are_complete(&input, produced_qty);
    let has_rezka_frame_metrics = is_rezka && !input.rezka_frames.is_empty();
    if matches!(
        input.action,
        queue_state::ApparatusQueueAction::Pause
            | queue_state::ApparatusQueueAction::DetachRoll
            | queue_state::ApparatusQueueAction::RollComplete
            | queue_state::ApparatusQueueAction::Complete
    ) && is_rezka
        && !input.freeze_with_issue
        && !has_rezka_frame_metrics
        && !has_rezka_progress_metrics
    {
        return Err(bad_request("rezka_progress_metrics_required"));
    }
    let zero_metric_codes = if has_rezka_frame_metrics {
        Vec::new()
    } else {
        zero_completion_metric_codes(&input, return_ink_kg)
    };
    if matches!(input.action, queue_state::ApparatusQueueAction::Complete)
        && !zero_metric_codes.is_empty()
        && completion_request_note.trim().is_empty()
    {
        return Err(bad_request("zero_metric_explanation_required"));
    }
    let missing_output_with_explanation = !has_complete_bosma_metrics
        && !has_complete_laminatsiya_metrics
        && !has_rezka_frame_metrics
        && !has_rezka_progress_metrics
        && input.gross_qty.is_none()
        && !completion_request_note.trim().is_empty();
    if matches!(input.action, queue_state::ApparatusQueueAction::Complete)
        && (!zero_metric_codes.is_empty() || missing_output_with_explanation)
    {
        let result = state
            .production_maps
            .request_completion_with_issue(
                &input.apparatus,
                &input.order_id,
                &assigned_apparatus,
                queue_action_actor(&principal),
                &completion_request_note,
                zero_metric_codes,
                returned_paint_report,
            )
            .await
            .map_err(production_map_error)?;
        return Ok(json_response(serde_json::json!({
            "ok": true,
            "states": result.states,
            "session": null,
            "progress_event": null,
            "progress_batch": null,
            "print": null,
            "completion_request": result.completion_request,
        })));
    }
    let mut prepared = state
        .production_maps
        .prepare_apparatus_queue_action_with_material_scan_and_progress(
            MaterialScanProgressAction {
                apparatus: &input.apparatus,
                order_id: &input.order_id,
                action: input.action,
                assigned_apparatus: &assigned_apparatus,
                actor: queue_action_actor(&principal),
                material_barcode: &material_barcode,
                state_material_barcodes: &state_material_barcodes,
                progress,
            },
        )
        .await
        .map_err(production_map_error)?;
    let qolip_preparations = if matches!(input.action, queue_state::ApparatusQueueAction::Start) {
        prepare_qolips_for_bosma_start(&state, &principal, &input).await?
    } else {
        Vec::new()
    };
    if !qolip_preparations.is_empty() {
        prepared.attach_qolip_codes(
            &qolip_preparations
                .iter()
                .map(|preparation| preparation.spec.qolip_code.clone())
                .collect::<Vec<_>>(),
        );
    }
    let qolip_checkouts = qolip_preparations
        .into_iter()
        .filter_map(|preparation| preparation.checkout)
        .collect::<Vec<_>>();
    let mut raw_material_stock_transitions = Vec::new();
    if matches!(input.action, queue_state::ApparatusQueueAction::Start) {
        let material_stock_barcodes = material_barcode
            .split(',')
            .map(|barcode| barcode.trim().to_string())
            .filter(|barcode| !barcode.is_empty())
            .collect::<Vec<_>>();
        if !prepared.material_scan_skipped() && !material_stock_barcodes.is_empty() {
            raw_material_stock_transitions.push(RawMaterialStockTransition::new(
                RawMaterialStockTransitionKind::InUse,
                material_stock_barcodes,
                &input.order_id,
            ));
        }
    }
    let completed_material_barcodes =
        if matches!(input.action, queue_state::ApparatusQueueAction::Complete) {
            raw_material_barcodes_for_order_apparatus(&state, &input.order_id, &input.apparatus)
                .await?
        } else {
            Vec::new()
        };
    if !completed_material_barcodes.is_empty() {
        raw_material_stock_transitions.push(RawMaterialStockTransition::new(
            RawMaterialStockTransitionKind::Consumed,
            completed_material_barcodes,
            &input.order_id,
        ));
    }
    let print_batches = if prepared.progress_batches().is_empty() {
        prepared
            .progress_batch()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
    } else {
        prepared.progress_batches().to_vec()
    };
    let print_requests = if matches!(
        input.action,
        queue_state::ApparatusQueueAction::Pause
            | queue_state::ApparatusQueueAction::DetachRoll
            | queue_state::ApparatusQueueAction::RollComplete
            | queue_state::ApparatusQueueAction::Complete
    ) {
        let frame_specific_metrics = !input.rezka_frames.is_empty();
        print_batches
            .iter()
            .map(|batch| ProgressLabelPrintRequest {
                driver_url: input.driver_url.clone(),
                qr_payload: batch.qr_payload.clone(),
                item_code: batch.label_item_code.clone(),
                item_name: batch.label_item_name.clone(),
                apparatus: batch.apparatus.clone(),
                customer_name: input.customer_name.trim().to_string(),
                executor_name: batch.executor_name.clone(),
                printer: input.printer.clone(),
                print_mode: input.print_mode.clone(),
                gross_qty: if frame_specific_metrics {
                    batch
                        .payload_json
                        .get("gross_qty")
                        .and_then(serde_json::Value::as_f64)
                        .or(batch.finished_goods_kg)
                        .unwrap_or(batch.produced_qty)
                } else {
                    input
                        .gross_qty
                        .or(input.finished_goods_kg)
                        .unwrap_or(batch.produced_qty)
                },
                tare_enabled: if frame_specific_metrics {
                    batch.bobina_kg.is_some_and(|value| value > 0.0)
                } else {
                    input.bobina_kg.is_some_and(|value| value > 0.0)
                },
                tare_kg: if frame_specific_metrics {
                    batch.bobina_kg.unwrap_or(0.0)
                } else {
                    input.bobina_kg.unwrap_or(0.0)
                },
                progress_qty: if frame_specific_metrics {
                    batch.finished_goods_meter.unwrap_or(batch.produced_qty)
                } else {
                    batch.produced_qty
                },
                unit: "kg".to_string(),
                progress_unit: if batch.uom.trim().is_empty() {
                    "m".to_string()
                } else {
                    batch.uom.clone()
                },
                label_kind: "progress".to_string(),
                print_count: if frame_specific_metrics {
                    1
                } else {
                    input.print_count
                },
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let fallback_qolip_checkouts = qolip_checkouts.clone();
    let result = state
        .production_maps
        .commit_prepared_queue_action_with_raw_material_stock(
            prepared,
            raw_material_stock_transitions.clone(),
            qolip_checkouts,
            returned_paint_report,
        )
        .await
        .map_err(production_map_error)?;
    if !result.qolip_checkout_committed && !fallback_qolip_checkouts.is_empty() {
        for checkout in fallback_qolip_checkouts {
            state
                .qolip
                .issue_prepared_checkout(checkout)
                .await
                .map_err(qolip_queue_error)?;
        }
    }
    let mut warehouse_stock_update_warehouses = result.raw_material_stock_warehouses.clone();
    if !raw_material_stock_transitions.is_empty() && warehouse_stock_update_warehouses.is_empty() {
        for transition in &raw_material_stock_transitions {
            let updates = match transition.kind {
                RawMaterialStockTransitionKind::InUse => {
                    state
                        .gscale
                        .mark_raw_material_stock_in_use(&transition.barcodes, &transition.order_id)
                        .await
                }
                RawMaterialStockTransitionKind::Consumed => {
                    state
                        .gscale
                        .mark_raw_material_stock_consumed(
                            &transition.barcodes,
                            &transition.order_id,
                        )
                        .await
                }
            }
            .map_err(raw_material_stock_status_error)?;
            warehouse_stock_update_warehouses.extend(
                updates
                    .into_iter()
                    .map(|stock| stock.warehouse)
                    .filter(|warehouse| !warehouse.trim().is_empty()),
            );
        }
    }
    for warehouse in warehouse_stock_update_warehouses {
        state
            .warehouse_events
            .notify_updated(&warehouse, "raw_material_stock");
    }
    let prints = dispatch_progress_label_prints(
        state.gscale.clone(),
        print_requests,
        &input.print_transport,
        &input.apparatus,
        &input.order_id,
        input.action,
    );
    let print = prints.first().cloned().unwrap_or(serde_json::Value::Null);
    let order_control = result.order_control;
    let mut response = serde_json::json!({
        "ok": true,
        "states": result.states,
        "order_status": result.order_status,
        "session": result.session,
        "progress_event": result.progress_event,
        "progress_batch": result.progress_batch,
        "progress_batches": result.progress_batches,
        "print": print,
        "prints": prints,
    });
    if let Some(order_control) = order_control {
        response["order_control"] = serde_json::json!(order_control);
    }
    Ok(json_response(response))
}

fn canonical_queue_action(
    action: queue_state::ApparatusQueueAction,
    worker_handoff: bool,
    remove_roll_from_apparatus: bool,
    freeze_request_id: &str,
    freeze_with_issue: bool,
    principal: &Principal,
) -> queue_state::ApparatusQueueAction {
    if action != queue_state::ApparatusQueueAction::Pause {
        return if freeze_with_issue {
            queue_state::ApparatusQueueAction::Freeze
        } else {
            action
        };
    }
    if freeze_with_issue {
        return queue_state::ApparatusQueueAction::Freeze;
    }
    let legacy_roll_removal = remove_roll_from_apparatus;
    let legacy_worker_detach = principal.role == PrincipalRole::Aparatchi
        && freeze_request_id.trim().is_empty()
        && !freeze_with_issue
        && !worker_handoff;
    if legacy_roll_removal || legacy_worker_detach {
        queue_state::ApparatusQueueAction::DetachRoll
    } else {
        queue_state::ApparatusQueueAction::Pause
    }
}

include!("queue_action_completion_support.rs");

#[cfg(test)]
mod tests {
    use super::{
        apparatus_requires_qolip_scan, canonical_queue_action, returned_paint_queue_error,
    };
    use crate::core::auth::models::{Principal, PrincipalRole};
    use crate::core::production_map::queue_state::ApparatusQueueAction;
    use crate::core::returned_paint::ReturnedPaintError;

    fn principal(role: PrincipalRole) -> Principal {
        Principal {
            role,
            display_name: "Test".to_string(),
            legal_name: String::new(),
            ref_: "test-ref".to_string(),
            phone: String::new(),
            avatar_url: String::new(),
        }
    }

    #[test]
    fn legacy_worker_pause_maps_to_detach_roll_but_admin_and_freeze_pause_do_not() {
        let worker = principal(PrincipalRole::Aparatchi);
        let admin = principal(PrincipalRole::Admin);

        assert_eq!(
            canonical_queue_action(ApparatusQueueAction::Pause, false, false, "", false, &worker),
            ApparatusQueueAction::DetachRoll
        );
        assert_eq!(
            canonical_queue_action(
                ApparatusQueueAction::Pause,
                false,
                false,
                "freeze-request",
                false,
                &worker,
            ),
            ApparatusQueueAction::Pause
        );
        assert_eq!(
            canonical_queue_action(ApparatusQueueAction::Pause, true, false, "", false, &worker),
            ApparatusQueueAction::Pause
        );
        assert_eq!(
            canonical_queue_action(ApparatusQueueAction::Pause, false, false, "", false, &admin),
            ApparatusQueueAction::Pause
        );
        assert_eq!(
            canonical_queue_action(ApparatusQueueAction::Pause, false, false, "", true, &worker),
            ApparatusQueueAction::Freeze
        );
    }

    #[test]
    fn qolip_scan_is_required_only_for_seven_eight_and_nine_color_bosma_family() {
        assert!(apparatus_requires_qolip_scan("7 ta rangli pechat - A"));
        assert!(apparatus_requires_qolip_scan("8 ta rangli bosma aparat"));
        assert!(apparatus_requires_qolip_scan("9 rangli val"));
        assert!(!apparatus_requires_qolip_scan("Laminatsiya"));
        assert!(!apparatus_requires_qolip_scan("Rezka aparat"));
        assert!(!apparatus_requires_qolip_scan("Pechat"));
    }

    #[test]
    fn astatka_exceeding_rasxot_returns_stable_queue_error_code() {
        let (status, axum::Json(body)) =
            returned_paint_queue_error(ReturnedPaintError::NegativeFinalValue);

        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(body.error, "returned_paint_astatka_exceeds_rasxot");
    }
}
