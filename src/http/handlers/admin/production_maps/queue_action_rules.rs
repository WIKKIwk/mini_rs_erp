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
