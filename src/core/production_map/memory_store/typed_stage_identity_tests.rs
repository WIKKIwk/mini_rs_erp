use super::*;

#[test]
fn queue_log_uses_typed_stage_identity_not_payload_metadata() {
    let event = ApparatusQueueActionEvent {
        event_id: "event-typed-stage".to_string(),
        apparatus: "apparatus:test:stage".to_string(),
        order_id: "order-typed-stage".to_string(),
        stage_node_id: "typed-stage".to_string(),
        action: queue_state::ApparatusQueueAction::Start,
        from_state: queue_state::ApparatusQueueOrderState::Pending,
        to_state: queue_state::ApparatusQueueOrderState::InProgress,
        policy: ApparatusQueuePolicy::FreePick,
        actor: QueueActionActor::default(),
        assigned_apparatus: Vec::new(),
        payload_json: serde_json::json!({"stage_node_id": "legacy-stage"}),
    };

    let log = production_order_log_entry(&event, 0, String::new());

    assert_eq!(log.stage_node_id, "typed-stage");
}

#[tokio::test]
async fn repeated_apparatus_lifecycle_uses_typed_stage_occurrences() {
    let store = MemoryProductionMapStore::new();
    let order_id = "order-typed-repeated-stage";
    let apparatus = "apparatus:test:typed-rezka";
    let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
        "id": order_id,
        "product_code": "TYPED-STAGE",
        "title": "Typed repeated stage",
        "nodes": [
            {"id": "start", "kind": "start", "title": "Start"},
            {"id": "rezka_first", "kind": "apparatus", "title": "Rezka first", "apparatus_id": apparatus},
            {"id": "rezka_final", "kind": "apparatus", "title": "Rezka final", "apparatus_id": apparatus},
            {"id": "end", "kind": "end", "title": "End"}
        ],
        "edges": [
            {"from": "start", "to": "rezka_first"},
            {"from": "rezka_first", "to": "rezka_final"},
            {"from": "rezka_final", "to": "end"}
        ]
    }))
    .expect("repeated-stage map");
    store.put_map(map).await.expect("store repeated-stage map");

    let event = |event_id: &str, stage_node_id: &str| ApparatusQueueActionEvent {
        event_id: event_id.to_string(),
        apparatus: apparatus.to_string(),
        order_id: order_id.to_string(),
        stage_node_id: stage_node_id.to_string(),
        action: queue_state::ApparatusQueueAction::Complete,
        from_state: queue_state::ApparatusQueueOrderState::InProgress,
        to_state: queue_state::ApparatusQueueOrderState::Completed,
        policy: ApparatusQueuePolicy::FreePick,
        actor: QueueActionActor::default(),
        assigned_apparatus: vec![apparatus.to_string()],
        payload_json: serde_json::json!({}),
    };

    store
        .append_apparatus_queue_action_event(event("event-first", "rezka_first"))
        .await
        .expect("first occurrence completion");
    let after_first = store
        .production_order_lifecycles(&[order_id.to_string()])
        .await
        .expect("lifecycle after first occurrence");
    assert_ne!(
        after_first.get(order_id).expect("first lifecycle").status,
        ProductionOrderLifecycleStatus::ProductionCompleted
    );

    store
        .append_apparatus_queue_action_event(event("event-final", "rezka_final"))
        .await
        .expect("final occurrence completion");
    let after_final = store
        .production_order_lifecycles(&[order_id.to_string()])
        .await
        .expect("lifecycle after final occurrence");
    assert_eq!(
        after_final.get(order_id).expect("final lifecycle").status,
        ProductionOrderLifecycleStatus::ProductionCompleted
    );
}
