use super::*;

fn request(status: OrderFreezeRequestStatus) -> OrderFreezeRequest {
    OrderFreezeRequest {
        scope: "order".into(),
        stage_node_id: String::new(),
        request_id: "freeze-1".to_string(),
        status,
        target_session_id: "session-1".to_string(),
        target_apparatus: "apparatus:test:one".to_string(),
        target_worker_role: "worker".to_string(),
        target_worker_ref: "worker-1".to_string(),
        target_worker_display_name: "Worker".to_string(),
        requested_at_unix: 1,
        transitioned_at_unix: 2,
    }
}

#[test]
fn repeated_freeze_control_writes_are_idempotent() {
    let pending_current = ("freeze_requested".to_string(), Some("freeze-1".to_string()));
    assert!(validate_freeze_transition(
        Some(&pending_current),
        OrderControlState::FreezeRequested,
        &request(OrderFreezeRequestStatus::Pending),
    )
    .is_ok());

    let unfrozen_current = ("active".to_string(), Some("freeze-1".to_string()));
    assert!(validate_freeze_transition(
        Some(&unfrozen_current),
        OrderControlState::Active,
        &request(OrderFreezeRequestStatus::Unfrozen),
    )
    .is_ok());
}
