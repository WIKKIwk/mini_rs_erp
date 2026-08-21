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

fn queue_action_has_any_output(input: &ApparatusQueueActionRequest) -> bool {
    !input.rezka_frames.is_empty()
        || input.produced_qty.is_some()
        || input.qty.is_some()
        || input.gross_qty.is_some()
        || input.return_ink_kg.is_some()
        || input.lamination_print_leftover_rolls.is_some()
        || input.lamination_film_leftover_rolls.is_some()
        || input.rezka_bosma_waste.is_some()
        || input.rezka_lamination_waste.is_some()
        || input.rezka_edge_waste.is_some()
        || input.total_waste.is_some()
        || input.finished_goods_kg.is_some()
        || input.bobina_kg.is_some()
        || input.finished_goods_meter.is_some()
        || input.diameter.is_some()
}

fn freeze_safe_stop_output_is_complete(
    input: &ApparatusQueueActionRequest,
    produced_qty: Option<f64>,
    apparatus: &QueueApparatusMetadata,
) -> bool {
    if apparatus.is_rezka() {
        return !input.rezka_frames.is_empty()
            || rezka_queue_quantity_metrics_are_complete(input, produced_qty);
    }
    produced_qty.or(input.finished_goods_meter).is_some()
        && input.gross_qty.or(input.finished_goods_kg).is_some()
        && input.bobina_kg.is_some()
}
