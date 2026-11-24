use super::{
    StoredQueueEventIdentity, merge_session_transition_matches, queue_event_identity_matches,
};
use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{
    ApparatusQueueActionEvent, ApparatusQueuePolicy, OrderRunSession, OrderRunStatus,
    QueueActionActor,
};
use crate::core::production_map::queue_state::{
    ApparatusQueueAction, ApparatusQueueOrderState,
};

#[test]
fn queue_event_replay_requires_original_stage_and_payload() {
    let event = ApparatusQueueActionEvent {
        event_id: "event-1".to_string(),
        apparatus: "apparatus:test:a".to_string(),
        order_id: "zakaz-1".to_string(),
        stage_node_id: "stage-a".to_string(),
        action: ApparatusQueueAction::Start,
        from_state: ApparatusQueueOrderState::Pending,
        to_state: ApparatusQueueOrderState::InProgress,
        policy: ApparatusQueuePolicy::FreePick,
        actor: QueueActionActor {
            role: "operator".to_string(),
            ref_: "worker-1".to_string(),
            display_name: "Worker".to_string(),
        },
        assigned_apparatus: vec!["apparatus:test:a".to_string()],
        payload_json: serde_json::json!({"request": "one"}),
    };
    let apparatus_id = ApparatusId::new("apparatus:test:a".to_string()).unwrap();
    let assigned = vec!["apparatus:test:a".to_string()];
    let existing_assigned = serde_json::json!(["apparatus:test:a"]);
    let existing = StoredQueueEventIdentity {
        apparatus: Some("apparatus:test:a"),
        order_id: "zakaz-1",
        stage_node_id: "stage-a",
        action: "start",
        from_state: "pending",
        to_state: "in_progress",
        policy: "free_pick",
        actor_role: "operator",
        actor_ref: "worker-1",
        actor_display_name: "Worker",
        assigned_apparatus: &existing_assigned,
        payload: &event.payload_json,
    };
    let matching =
        queue_event_identity_matches(&existing, &apparatus_id, &event, &assigned).unwrap();
    assert!(matching);

    let changed_stage = StoredQueueEventIdentity {
        stage_node_id: "stage-b",
        ..existing
    };
    let changed_stage =
        queue_event_identity_matches(&changed_stage, &apparatus_id, &event, &assigned).unwrap();
    assert!(!changed_stage);

    let changed_payload = serde_json::json!({"request": "two"});
    let changed = StoredQueueEventIdentity {
        payload: &changed_payload,
        ..existing
    };
    let changed =
        queue_event_identity_matches(&changed, &apparatus_id, &event, &assigned).unwrap();
    assert!(!changed);
}

#[test]
fn merge_write_rejects_a_stale_current_input() {
    let proposed = OrderRunSession {
        session_id: "session-1".to_string(),
        apparatus: "apparatus:test:rezka".to_string(),
        order_id: "zakaz-1".to_string(),
        stage_node_id: "rezka".to_string(),
        status: OrderRunStatus::Active,
        worker_role: "aparatchi".to_string(),
        worker_ref: "worker-1".to_string(),
        worker_display_name: "Worker".to_string(),
        started_at_unix: 1,
        updated_at_unix: 2,
        payload_json: serde_json::json!({
            "merge_from_input_batch_id": "wip-a",
            "merge_to_input_batch_id": "wip-b",
            "input_progress_batch_id": "wip-b",
            "input_lineage": [
                {
                    "input_batch_id": "wip-a",
                    "input_qr_payload": "qr-a",
                    "source_apparatus": "apparatus:test:lamination",
                    "source_kind": "progress_batch",
                    "stage_node_id": "rezka",
                    "sequence_no": 1,
                    "status": "processed",
                    "linked_at_unix": 1,
                    "processed_at_unix": 2
                },
                {
                    "input_batch_id": "wip-b",
                    "input_qr_payload": "qr-b",
                    "source_apparatus": "apparatus:test:lamination",
                    "source_kind": "progress_batch",
                    "stage_node_id": "rezka",
                    "sequence_no": 2,
                    "status": "in_use",
                    "linked_at_unix": 2
                }
            ]
        }),
    };

    assert!(merge_session_transition_matches(
        &serde_json::json!({"input_progress_batch_id": "wip-a"}),
        &proposed,
    ));
    assert!(!merge_session_transition_matches(
        &serde_json::json!({"input_progress_batch_id": "wip-b"}),
        &proposed,
    ));
}
