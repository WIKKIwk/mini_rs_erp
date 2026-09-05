#[derive(Debug, Clone)]
struct QueueActionCommand {
    apparatus: String,
    order_id: String,
    action: queue_state::ApparatusQueueAction,
    materials: QueueActionMaterialInput,
    progress: QueueProgressInput,
    completion: QueueActionCompletionInput,
    print: QueueActionPrintInput,
}

#[derive(Debug, Clone)]
struct QueueActionMaterialInput {
    legacy_barcode: String,
    barcodes: Vec<String>,
    combined_barcode: String,
    qolip_codes: Vec<String>,
}

#[derive(Debug, Clone)]
struct QueueActionCompletionInput {
    returned_paint_items: Vec<ReturnedPaintItem>,
    returned_paint_image_id: String,
}

#[derive(Debug, Clone)]
struct QueueActionPrintInput {
    driver_url: String,
    printer: String,
    print_mode: String,
    print_transport: String,
    customer_name: String,
    print_count: u32,
    submitted_uom: String,
}

impl QueueActionCommand {
    fn from_request(
        mut request: ApparatusQueueActionRequest,
        apparatus: &QueueApparatusMetadata,
        principal: &Principal,
    ) -> Result<Self, AdminError> {
        let action = canonical_queue_action(
            request.action,
            request.worker_handoff,
            request.remove_roll_from_apparatus,
            &request.freeze_request_id,
            request.freeze_with_issue,
            principal,
        );
        let explicit_worker_freeze = action == queue_state::ApparatusQueueAction::Freeze
            && request.freeze_request_id.trim().is_empty();
        if request.freeze_with_issue || explicit_worker_freeze {
            if principal.role != PrincipalRole::Aparatchi {
                return Err(forbidden());
            }
            if action != queue_state::ApparatusQueueAction::Freeze {
                return Err(bad_request("freeze_with_issue_only_on_freeze"));
            }
            if request.issue_note.trim().is_empty() && !request.description.trim().is_empty() {
                request.issue_note = request.description.clone();
            }
            if request.issue_note.trim().is_empty() {
                return Err(bad_request("issue_note_required"));
            }
            if !request.freeze_request_id.trim().is_empty() {
                return Err(bad_request(
                    "freeze_with_issue_cannot_use_freeze_request_id",
                ));
            }
            if request.worker_handoff || request.remove_roll_from_apparatus {
                return Err(bad_request("freeze_with_issue_actions_conflict"));
            }
            if request.order_id.trim().starts_with("training-") {
                return Err(bad_request("freeze_with_issue_not_supported_for_training"));
            }
            request.freeze_with_issue = true;
        }
        if request.worker_handoff && action != queue_state::ApparatusQueueAction::Pause {
            return Err(bad_request("worker_handoff_only_on_pause"));
        }
        if request.remove_roll_from_apparatus
            && action != queue_state::ApparatusQueueAction::DetachRoll
        {
            return Err(bad_request("roll_removal_only_on_detach_roll"));
        }
        if request.worker_handoff && request.remove_roll_from_apparatus {
            return Err(bad_request("worker_handoff_actions_conflict"));
        }
        if !request.rezka_frames.is_empty()
            && (!apparatus.is_rezka() || !action.records_progress_output())
        {
            return Err(bad_request("rezka_frames_only_on_rezka_progress"));
        }
        if request.rezka_record_frame_index.is_some()
            && request.order_id.trim().starts_with("training-")
        {
            return Err(bad_request("rezka_individual_print_requires_backend"));
        }
        if request
            .rezka_frames
            .iter()
            .any(|frame| !frame.issue_note.trim().is_empty())
            && !matches!(
                action,
                queue_state::ApparatusQueueAction::RollComplete
                    | queue_state::ApparatusQueueAction::Complete
                    | queue_state::ApparatusQueueAction::Pause
                    | queue_state::ApparatusQueueAction::DetachRoll
            )
        {
            return Err(bad_request("rezka_frame_issue_only_on_roll_progress"));
        }

        let produced_qty = request.produced_qty.or(request.qty);
        let submitted_uom = if request.uom.trim().is_empty() {
            request.unit.clone()
        } else {
            request.uom.clone()
        };
        let uom = if !submitted_uom.trim().is_empty() {
            submitted_uom.clone()
        } else if apparatus.is_pechat()
            && (produced_qty.is_some() || request.finished_goods_meter.is_some())
        {
            "m".to_string()
        } else {
            String::new()
        };
        let description = if request.freeze_with_issue {
            request.issue_note.clone()
        } else if request.completion_request_note.trim().is_empty() {
            request.description.clone()
        } else {
            request.completion_request_note.clone()
        };
        let qr_payload = effective_progress_qr_payload(&request.qr_payload, &request.progress_qr)
            .to_string();
        let combined_barcode = if request.material_barcodes.is_empty() {
            request.material_barcode.clone()
        } else {
            request.material_barcodes.join(",")
        };
        let qolip_codes = normalized_qolip_codes(&request.qolip_codes, &request.qolip_code);

        Ok(Self {
            apparatus: apparatus.id.to_string(),
            order_id: request.order_id,
            action,
            materials: QueueActionMaterialInput {
                legacy_barcode: request.material_barcode,
                barcodes: request.material_barcodes,
                combined_barcode,
                qolip_codes,
            },
            progress: QueueProgressInput {
                freeze_request_id: request.freeze_request_id,
                freeze_with_issue: request.freeze_with_issue,
                rezka_frames: request.rezka_frames,
                rezka_record_frame_index: request.rezka_record_frame_index,
                rezka_output_cycle: request.rezka_output_cycle,
                produced_qty,
                gross_qty: request.gross_qty,
                uom,
                progress_batch_id: request.progress_batch_id,
                qr_payload,
                return_ink_kg: request.return_ink_kg,
                lamination_print_leftover_rolls: request.lamination_print_leftover_rolls,
                lamination_film_leftover_rolls: request.lamination_film_leftover_rolls,
                rezka_bosma_waste: request.rezka_bosma_waste,
                rezka_lamination_waste: request.rezka_lamination_waste,
                rezka_edge_waste: request.rezka_edge_waste,
                total_waste: request.total_waste,
                finished_goods_kg: request.finished_goods_kg,
                bobina_kg: request.bobina_kg,
                finished_goods_meter: request.finished_goods_meter,
                diameter: request.diameter,
                description,
                returned_paint_report_attached: false,
                force_full_completion_metrics: request.full_completion_report_required,
                allow_partial_station_completion: false,
                worker_handoff: request.worker_handoff,
                remove_roll_from_apparatus: request.remove_roll_from_apparatus,
            },
            completion: QueueActionCompletionInput {
                returned_paint_items: request.returned_paint_items,
                returned_paint_image_id: request.returned_paint_image_id,
            },
            print: QueueActionPrintInput {
                driver_url: request.driver_url,
                printer: request.printer,
                print_mode: request.print_mode,
                print_transport: request.print_transport,
                customer_name: request.customer_name,
                print_count: request.print_count,
                submitted_uom,
            },
        })
    }
}

fn normalized_qolip_codes(codes: &[String], legacy_code: &str) -> Vec<String> {
    let mut result = Vec::new();
    for code in codes
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(legacy_code))
    {
        let code = code.trim();
        if code.is_empty()
            || result
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(code))
        {
            continue;
        }
        result.push(code.to_string());
    }
    result
}
