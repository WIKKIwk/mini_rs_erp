use super::*;
use queue_state::{ApparatusQueueAction as Action, ApparatusQueueOrderState as State};

fn alternative_map(assigned: &str) -> ProductionMapDefinition {
    serde_json::from_value(serde_json::json!({
        "id": "stage-completion", "product_code": "TEST", "title": "Stage completion",
        "nodes": [
            {"id": "start", "kind": "start", "title": "Start"},
            {"id": "rezka_before", "kind": "apparatus", "title": "Rezka", "apparatus_id": "apparatus:test:rezka"},
            {"id": "lam1", "kind": "apparatus", "title": "Laminatsiya 1", "apparatus_id": "apparatus:test:lam1",
             "alternative_group_id": "lamination", "alternative_assigned_apparatus_id": assigned},
            {"id": "lam2", "kind": "apparatus", "title": "Laminatsiya 2", "apparatus_id": "apparatus:test:lam2",
             "alternative_group_id": "lamination", "alternative_assigned_apparatus_id": assigned},
            {"id": "rezka_after", "kind": "apparatus", "title": "Rezka", "apparatus_id": "apparatus:test:rezka"},
            {"id": "end", "kind": "end", "title": "End"}
        ],
        "edges": [
            {"from": "start", "to": "rezka_before"},
            {"from": "rezka_before", "to": "lam1"},
            {"from": "rezka_before", "to": "lam2"},
            {"from": "lam1", "to": "rezka_after"},
            {"from": "lam2", "to": "rezka_after"},
            {"from": "rezka_after", "to": "end"}
        ]
    })).unwrap()
}

fn event(node: &str, action: Action, to_state: State) -> ProductionStageLifecycleEvent {
    ProductionStageLifecycleEvent {
        stage_node_id: node.into(),
        action,
        to_state,
    }
}

fn complete(node: &str) -> ProductionStageLifecycleEvent {
    event(node, Action::Complete, State::Completed)
}

fn status(
    map: &ProductionMapDefinition,
    events: &[ProductionStageLifecycleEvent],
) -> ProductionOrderLifecycleStatus {
    derive_production_order_lifecycle_with_stage_events(map, &BTreeMap::new(), events).unwrap()
}

#[test]
fn alternative_operation_finishes_on_either_candidate_without_waiting_for_unused_one() {
    for assigned in ["", "apparatus:test:lam1", "apparatus:test:lam2"] {
        for completed_candidate in ["lam1", "lam2"] {
            // Assignment can have changed since execution. The actual operation
            // history remains authoritative; it is not filtered to that machine.
            let map = alternative_map(assigned);
            assert_eq!(
                status(
                    &map,
                    &[
                        complete("rezka_before"),
                        complete(completed_candidate),
                        complete("rezka_after"),
                    ]
                ),
                ProductionOrderLifecycleStatus::ProductionCompleted
            );
        }
    }
}

#[test]
fn split_operation_waits_for_stage_end_not_every_machine_or_first_roll() {
    let map = alternative_map("apparatus:test:lam2");
    let mut events = vec![
        complete("rezka_before"),
        event("lam1", Action::Start, State::InProgress),
        // First half was completed on Lam1, but the operation is not over.
        event("lam1", Action::Complete, State::Pending),
        event("lam2", Action::Start, State::InProgress),
        // Even downstream work is not proof that all lamination is finished.
        complete("rezka_after"),
    ];
    assert_eq!(
        status(&map, &events),
        ProductionOrderLifecycleStatus::InProgress
    );
    events.push(event("lam2", Action::Complete, State::Pending));
    assert_eq!(
        status(&map, &events),
        ProductionOrderLifecycleStatus::InProgress
    );
    // Last remaining work was completed on Lam2. Lam1's old pending state must
    // not block the operation, nor must it be rewritten as a fake completion.
    events.push(complete("lam2"));
    assert_eq!(
        status(&map, &events),
        ProductionOrderLifecycleStatus::ProductionCompleted
    );
}

#[test]
fn later_partial_or_active_operation_state_overrides_old_completion() {
    let map = alternative_map("");
    for (action, state) in [
        (Action::Start, State::InProgress),
        (Action::Pause, State::Paused),
        (Action::Complete, State::Pending),
        (Action::Freeze, State::Frozen),
    ] {
        assert_eq!(
            status(
                &map,
                &[
                    complete("rezka_before"),
                    complete("lam1"),
                    event("lam2", action, state),
                    complete("rezka_after"),
                ]
            ),
            ProductionOrderLifecycleStatus::InProgress
        );
    }
}

#[test]
fn alternatives_do_not_merge_repeated_rezka_occurrences() {
    let map = alternative_map("apparatus:test:lam1");
    for missing in ["rezka_before", "rezka_after"] {
        let mut events = vec![
            complete("rezka_before"),
            complete("lam1"),
            complete("rezka_after"),
        ];
        events.retain(|event| event.stage_node_id != missing);
        events.push(event(missing, Action::Complete, State::Pending));
        assert_eq!(
            status(&map, &events),
            ProductionOrderLifecycleStatus::InProgress
        );
    }
}

#[test]
fn genuine_parallel_operations_still_both_required() {
    let mut map = alternative_map("");
    for node in &mut map.nodes {
        node.alternative_group_id.clear();
    }
    let mut events = vec![
        complete("rezka_before"),
        complete("lam1"),
        complete("rezka_after"),
    ];
    assert_eq!(
        status(&map, &events),
        ProductionOrderLifecycleStatus::InProgress
    );
    events.push(complete("lam2"));
    assert_eq!(
        status(&map, &events),
        ProductionOrderLifecycleStatus::ProductionCompleted
    );
}

#[test]
fn alternative_operation_cannot_be_inferred_from_apparatus_queue_states() {
    let map = alternative_map("apparatus:test:lam1");
    let queues = [
        "apparatus:test:rezka",
        "apparatus:test:lam1",
        "apparatus:test:lam2",
    ]
    .into_iter()
    .map(|id| {
        (
            id.into(),
            BTreeMap::from([(map.id.clone(), "completed".into())]),
        )
    })
    .collect();
    assert_eq!(
        derive_production_order_lifecycle_with_stage_events(
            &map,
            &queues,
            &[complete("rezka_before"), complete("rezka_after")]
        ),
        Some(ProductionOrderLifecycleStatus::InProgress)
    );
}

#[test]
fn unrelated_stage_events_do_not_complete_an_operation() {
    let map = alternative_map("");
    assert_eq!(
        status(
            &map,
            &[
                complete("rezka_before"),
                complete("removed_lamination"),
                complete("rezka_after")
            ]
        ),
        ProductionOrderLifecycleStatus::InProgress
    );
}

#[test]
fn closed_history_uses_executed_stage_even_when_assignment_has_changed() {
    let map = alternative_map("apparatus:test:lam2");
    let log: ProductionOrderLogEntry = serde_json::from_value(serde_json::json!({
        "event_id": "finished", "order_id": map.id, "stage_node_id": "lam1",
        "apparatus": "apparatus:test:lam1", "action": "complete",
        "from_state": "in_progress", "to_state": "completed",
        "actor_role": "aparatchi", "actor_ref": "worker", "actor_display_name": "Worker",
        "created_at_unix": 10
    }))
    .unwrap();
    let required = required_apparatus_for_closed_order(&map).unwrap();
    assert!(required.contains(&log.apparatus), "historical assignment no longer hides candidates");
    let logs = vec![log];
    assert_eq!(
        latest_required_complete_event(&map, &logs, &required).map(|entry| entry.event_id.as_str()),
        Some("finished")
    );
}
