use super::*;
use crate::core::apparatus_standard::{ApparatusId, ExecutionOperation};
use crate::core::production_map::pechat;
use crate::core::returned_paint::{
    ReturnedPaintError, ReturnedPaintItem, ReturnedPaintRequestCreate, ReturnedPaintStatus,
    returned_paint_astatka_total, returned_paint_report_can_close,
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
include!("queue_action_plan.rs");

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
    let mut input = QueueActionCommand::from_request(request, &apparatus, &principal)?;
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
    let preflight = validate_queue_action_preflight(&input, &apparatus)?;
    let _queue_action_guard = state.production_maps.queue_action_guard().await;
    let returned_paint_report = prepare_returned_paint_report(
        &state,
        &principal,
        &input,
        preflight.returned_paint_requested,
    )
    .await?;
    let returned_paint_report_attached = returned_paint_report.is_some();
    let return_ink_kg = effective_return_ink_kg(&input, returned_paint_report.as_ref())?;
    match plan_queue_action(
        &mut input,
        &apparatus,
        return_ink_kg,
        returned_paint_report_attached,
        preflight.freeze_safe_stop_with_issue,
    )? {
        QueueActionDecision::RequestCompletion {
            note,
            zero_metric_codes,
        } => {
            let result = state
                .production_maps
                .request_completion_with_issue(
                    &input.apparatus,
                    &input.order_id,
                    &assigned_apparatus,
                    queue_action_actor(&principal),
                    &note,
                    zero_metric_codes,
                    returned_paint_report,
                )
                .await
                .map_err(production_map_error)?;
            Ok(json_response(serde_json::json!({
                "ok": true,
                "states": result.states,
                "session": null,
                "progress_event": null,
                "progress_batch": null,
                "print": null,
                "completion_request": result.completion_request,
            })))
        }
        QueueActionDecision::Execute => {
            execute_queue_action(
                &state,
                &principal,
                input,
                &apparatus,
                assigned_apparatus,
                state_material_barcodes,
                returned_paint_report,
            )
            .await
        }
    }
}

include!("queue_action_execution.rs");
include!("queue_action_rules.rs");
include!("queue_action_completion_support.rs");
include!("queue_action_tests.rs");
