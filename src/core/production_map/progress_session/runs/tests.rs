use super::*;

fn input_link(batch_id: &str, sequence_no: u32, status: OrderRunInputStatus) -> OrderRunInputLink {
    OrderRunInputLink {
        input_batch_id: batch_id.to_string(),
        input_qr_payload: format!("qr:{batch_id}"),
        source_apparatus: "apparatus:catalog:print-001".to_string(),
        source_kind: OrderRunInputSourceKind::ProgressBatch,
        stage_node_id: "rezka".to_string(),
        sequence_no,
        status,
        linked_at_unix: 10,
        processed_at_unix: (status == OrderRunInputStatus::Processed).then_some(20),
    }
}

#[tokio::test]
async fn memory_store_persists_session_partial_roll_and_output_source_lineage() {
    let store = MemoryProductionMapStore::new();
    let input_links = vec![
        input_link("wip-a", 1, OrderRunInputStatus::Processed),
        input_link("wip-b", 2, OrderRunInputStatus::InUse),
    ];
    let active_rolls = vec![RezkaActivePartialRoll {
        slot_index: 1,
        generation: 1,
        contained_kadr_count: 1,
        status: RezkaPartialRollStatus::Active,
        source_input_batch_ids: vec!["wip-a".to_string(), "wip-b".to_string()],
        started_at_unix: 10,
        updated_at_unix: 20,
    }];
    let mut session_payload = serde_json::json!({});
    write_order_run_input_links(&mut session_payload, &input_links);
    write_rezka_active_partial_rolls(&mut session_payload, &active_rolls);
    let session = OrderRunSession {
        session_id: "run-rezka-1".to_string(),
        apparatus: "apparatus:default:asset-010".to_string(),
        order_id: "order-1".to_string(),
        stage_node_id: "rezka".to_string(),
        status: OrderRunStatus::Active,
        worker_role: "aparatchi".to_string(),
        worker_ref: "worker-1".to_string(),
        worker_display_name: "Worker".to_string(),
        started_at_unix: 10,
        updated_at_unix: 20,
        payload_json: session_payload,
    };
    put_order_run_session(&store, session)
        .await
        .expect("session");

    let stored_sessions = ProductionMapStorePort::order_run_sessions_for_order(&store, "order-1")
        .await
        .expect("sessions");
    let stored_session = stored_sessions
        .iter()
        .find(|session| session.session_id.trim() == "run-rezka-1")
        .expect("stored session");
    assert_eq!(
        order_run_input_links_from_payload(&stored_session.payload_json).expect("lineage"),
        input_links
    );
    assert_eq!(
        rezka_active_partial_rolls_from_payload(&stored_session.payload_json).expect("rolls"),
        active_rolls
    );

    let output_links = vec![
        ProgressBatchInputLink {
            input_batch_id: "wip-a".to_string(),
            input_qr_payload: "qr:wip-a".to_string(),
            source_apparatus: "apparatus:catalog:print-001".to_string(),
            source_kind: OrderRunInputSourceKind::ProgressBatch,
            sequence_no: 1,
        },
        ProgressBatchInputLink {
            input_batch_id: "wip-b".to_string(),
            input_qr_payload: "qr:wip-b".to_string(),
            source_apparatus: "apparatus:catalog:print-001".to_string(),
            source_kind: OrderRunInputSourceKind::ProgressBatch,
            sequence_no: 2,
        },
    ];
    let mut batch_payload = serde_json::json!({});
    write_progress_batch_input_links(&mut batch_payload, &output_links);
    let batch: OrderProgressBatch = serde_json::from_value(serde_json::json!({
        "batch_id": "rezka-output-1",
        "session_id": "run-rezka-1",
        "started_at_unix": 10,
        "completed_at_unix": 20,
        "apparatus": "apparatus:default:asset-010",
        "order_id": "order-1",
        "action": "roll_complete",
        "status": "completed",
        "produced_qty": 100.0,
        "uom": "m",
        "qr_payload": "qr:rezka-output-1",
        "label_item_code": "order-1",
        "label_item_name": "Rezka output",
        "executor_name": "Worker",
        "worker_role": "aparatchi",
        "worker_ref": "worker-1",
        "worker_display_name": "Worker",
        "wip_status": "waiting",
        "parent_batch_id": "wip-b",
        "payload_json": batch_payload,
    }))
    .expect("output batch");
    put_order_progress_batch(&store, batch)
        .await
        .expect("output batch persistence");

    let stored_batch = ProductionMapStorePort::progress_batch(&store, "rezka-output-1")
        .await
        .expect("batch")
        .expect("stored batch");
    assert_eq!(
        progress_batch_input_links_from_payload(&stored_batch.payload_json).expect("links"),
        output_links
    );
}
