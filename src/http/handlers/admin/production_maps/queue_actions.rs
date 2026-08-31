use super::*;
use crate::core::apparatus_standard::{ApparatusId, ExecutionOperation};
use crate::core::production_map::pechat;
use crate::core::returned_paint::{
    ReturnedPaintError, ReturnedPaintItem, ReturnedPaintRequestCreate, ReturnedPaintStatus,
    returned_paint_astatka_total, returned_paint_report_can_close, returned_paint_value_count,
};

include!("queue_action_request.rs");

#[derive(Debug, Clone)]
pub(super) struct QueueApparatusMetadata {
    pub(super) id: ApparatusId,
    pub(super) display_name: String,
    operation: ExecutionOperation,
    qolip_scan_required: bool,
}

impl QueueApparatusMetadata {
    fn is_pechat(&self) -> bool {
        self.operation == ExecutionOperation::Print
    }

    fn is_rezka(&self) -> bool {
        self.operation == ExecutionOperation::Cut
    }

    pub(super) fn requires_qolip_scan(&self) -> bool {
        self.qolip_scan_required
    }
}

include!("queue_action_command.rs");

pub(super) async fn resolve_queue_apparatus(
    state: &AppState,
    requested: &str,
) -> Result<QueueApparatusMetadata, AdminError> {
    let requested_id = parse_canonical_queue_apparatus_id(requested)?;
    let canonical = state
        .production_maps
        .resolve_canonical_apparatus(&requested_id)
        .await
        .map_err(production_map_error)?;
    Ok(QueueApparatusMetadata {
        id: canonical.runtime.apparatus_id.clone(),
        display_name: canonical.runtime.display.display_name.clone(),
        operation: canonical.runtime.execution_profile.operation,
        qolip_scan_required: pechat::requires_qolip_scan(canonical.as_ref()),
    })
}

fn parse_canonical_queue_apparatus_id(requested: &str) -> Result<ApparatusId, AdminError> {
    ApparatusId::new(requested.trim().to_string())
        .map_err(|_| bad_request("canonical_apparatus_id_required"))
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
    let request: ApparatusQueueActionRequest = parse_json(&body)?;
    if request.apparatus.trim().is_empty() || request.order_id.trim().is_empty() {
        return Err(bad_request("apparatus and order_id are required"));
    }
    let apparatus = resolve_queue_apparatus(&state, &request.apparatus).await?;
    let input = QueueActionCommand::from_request(request, &apparatus, &principal)?;
    if let Some(training_result) = super::super::training::training_queue_action(
        &state,
        &principal,
        &input.apparatus,
        &input.order_id,
        input.action,
        &input.materials.legacy_barcode,
        &input.materials.barcodes,
        &input.progress.progress_batch_id,
        &input.progress.qr_payload,
        super::super::training::TrainingQueuePrintInput {
            driver_url: input.print.driver_url.clone(),
            printer: input.print.printer.clone(),
            print_mode: input.print.print_mode.clone(),
            print_transport: input.print.print_transport.clone(),
            progress_qty: input.progress.produced_qty,
            gross_qty: input.progress.gross_qty,
            finished_goods_kg: input.progress.finished_goods_kg,
            bobina_kg: input.progress.bobina_kg,
            return_ink_kg: input.progress.return_ink_kg,
            lamination_print_leftover_rolls: input.progress.lamination_print_leftover_rolls,
            lamination_film_leftover_rolls: input.progress.lamination_film_leftover_rolls,
            rezka_bosma_waste: input.progress.rezka_bosma_waste,
            rezka_lamination_waste: input.progress.rezka_lamination_waste,
            rezka_edge_waste: input.progress.rezka_edge_waste,
            total_waste: input.progress.total_waste,
            finished_goods_meter: input.progress.finished_goods_meter,
            diameter: input.progress.diameter,
            returned_paint_items: input.completion.returned_paint_items.clone(),
            returned_paint_image_id: input.completion.returned_paint_image_id.clone(),
            description: input.progress.description.clone(),
            uom: input.print.submitted_uom.clone(),
            customer_name: input.print.customer_name.clone(),
            print_count: input.print.print_count,
        },
    )
    .await
    .map_err(super::super::training::training_workspace_error)?
    {
        return Ok(json_response(training_result));
    }
    let assigned_apparatus = state.admin.principal_assigned_apparatus(&principal).await;
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
    let freeze_request_safe_stop = !input.progress.freeze_request_id.trim().is_empty()
        && matches!(
            input.action,
            queue_state::ApparatusQueueAction::Pause
                | queue_state::ApparatusQueueAction::DetachRoll
        );
    let freeze_request_safe_stop_has_output = queue_action_has_any_output(&input);
    let freeze_request_safe_stop_with_issue = freeze_request_safe_stop
        && !freeze_request_safe_stop_has_output
        && !input.progress.description.trim().is_empty();
    if freeze_request_safe_stop {
        if !freeze_request_safe_stop_has_output {
            if input.progress.description.trim().is_empty() {
                return Err(bad_request(
                    "freeze_safe_stop_output_or_issue_note_required",
                ));
            }
        } else if !freeze_safe_stop_output_is_complete(&input, &apparatus) {
            return Err(bad_request("freeze_safe_stop_output_incomplete"));
        }
    }
    if !matches!(input.action, queue_state::ApparatusQueueAction::Complete)
        && (!input.completion.returned_paint_items.is_empty()
            || !input.completion.returned_paint_image_id.trim().is_empty())
    {
        return Err(bad_request("returned_paint_only_on_complete"));
    }
    let returned_paint_field_count =
        returned_paint_value_count(&input.completion.returned_paint_items);
    let has_returned_paint_image = !input.completion.returned_paint_image_id.trim().is_empty();
    let is_bosma_complete = matches!(input.action, queue_state::ApparatusQueueAction::Complete)
        && apparatus.is_pechat();
    if is_bosma_complete
        && !returned_paint_report_can_close(
            &input.completion.returned_paint_items,
            has_returned_paint_image,
        )
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
                            image_id: input.completion.returned_paint_image_id.clone(),
                            items: input.completion.returned_paint_items.clone(),
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
        None => input.progress.return_ink_kg,
    };
    let mut progress = input.progress.clone();
    progress.return_ink_kg = return_ink_kg;
    progress.returned_paint_report_attached = returned_paint_report_attached;
    let has_complete_bosma_metrics = (return_ink_kg.is_some() || returned_paint_report_attached)
        && input.progress.total_waste.is_some()
        && input.progress.finished_goods_kg.is_some()
        && input.progress.finished_goods_meter.is_some();
    let has_complete_laminatsiya_metrics =
        (input.progress.lamination_print_leftover_rolls.is_some()
            || input.progress.lamination_film_leftover_rolls.is_some())
            && input.progress.total_waste.is_some()
            && input.progress.finished_goods_kg.is_some()
            && input.progress.finished_goods_meter.is_some();
    let is_rezka = apparatus.is_rezka();
    let has_rezka_progress_metrics = is_rezka && rezka_queue_quantity_metrics_are_complete(&input);
    let has_rezka_frame_metrics = is_rezka && !input.progress.rezka_frames.is_empty();
    if matches!(
        input.action,
        queue_state::ApparatusQueueAction::Pause
            | queue_state::ApparatusQueueAction::DetachRoll
            | queue_state::ApparatusQueueAction::RollComplete
            | queue_state::ApparatusQueueAction::Complete
    ) && is_rezka
        && !input.progress.freeze_with_issue
        && !freeze_request_safe_stop_with_issue
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
        && input.progress.description.trim().is_empty()
    {
        return Err(bad_request("zero_metric_explanation_required"));
    }
    let missing_output_with_explanation = !has_complete_bosma_metrics
        && !has_complete_laminatsiya_metrics
        && !has_rezka_frame_metrics
        && !has_rezka_progress_metrics
        && input.progress.gross_qty.is_none()
        && !input.progress.description.trim().is_empty();
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
                &input.progress.description,
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
    execute_queue_action(QueueActionExecution {
        state: &state,
        principal: &principal,
        input: &input,
        apparatus: &apparatus,
        assigned_apparatus,
        material_barcode: input.materials.combined_barcode.clone(),
        state_material_barcodes,
        progress,
        returned_paint_report,
    })
    .await
}

include!("queue_action_execution.rs");
include!("queue_action_rules.rs");
include!("queue_action_completion_support.rs");
include!("queue_action_tests.rs");
