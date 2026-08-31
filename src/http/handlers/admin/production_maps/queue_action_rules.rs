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

fn queue_action_has_any_output(input: &QueueActionCommand) -> bool {
    !input.progress.rezka_frames.is_empty()
        || input.progress.produced_qty.is_some()
        || input.progress.gross_qty.is_some()
        || input.progress.return_ink_kg.is_some()
        || input.progress.lamination_print_leftover_rolls.is_some()
        || input.progress.lamination_film_leftover_rolls.is_some()
        || input.progress.rezka_bosma_waste.is_some()
        || input.progress.rezka_lamination_waste.is_some()
        || input.progress.rezka_edge_waste.is_some()
        || input.progress.total_waste.is_some()
        || input.progress.finished_goods_kg.is_some()
        || input.progress.bobina_kg.is_some()
        || input.progress.finished_goods_meter.is_some()
        || input.progress.diameter.is_some()
}

fn freeze_safe_stop_output_is_complete(
    input: &QueueActionCommand,
    apparatus: &QueueApparatusMetadata,
) -> bool {
    if apparatus.is_rezka() {
        return !input.progress.rezka_frames.is_empty()
            || rezka_queue_quantity_metrics_are_complete(input);
    }
    input
        .progress
        .produced_qty
        .or(input.progress.finished_goods_meter)
        .is_some()
        && input
            .progress
            .gross_qty
            .or(input.progress.finished_goods_kg)
            .is_some()
        && input.progress.bobina_kg.is_some()
}
