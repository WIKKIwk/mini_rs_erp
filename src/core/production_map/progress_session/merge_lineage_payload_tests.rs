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

#[test]
fn lineage_round_trip_preserves_splice_order_and_active_roll_sources() {
    let links = vec![
        input_link("wip-a", 1, OrderRunInputStatus::Processed),
        input_link("wip-b", 2, OrderRunInputStatus::InUse),
    ];
    let rolls = vec![RezkaActivePartialRoll {
        slot_index: 1,
        generation: 1,
        contained_kadr_count: 2,
        status: RezkaPartialRollStatus::Active,
        source_input_batch_ids: vec!["wip-a".to_string(), "wip-b".to_string()],
        started_at_unix: 10,
        updated_at_unix: 20,
    }];
    let mut payload = serde_json::json!({});
    write_order_run_input_links(&mut payload, &links);
    write_rezka_active_partial_rolls(&mut payload, &rolls);

    assert_eq!(order_run_input_links_from_payload(&payload), Ok(links));
    assert_eq!(rezka_active_partial_rolls_from_payload(&payload), Ok(rolls));
}

#[test]
fn lineage_rejects_two_active_inputs_and_duplicate_roll_sources() {
    let mut payload = serde_json::json!({});
    write_order_run_input_links(
        &mut payload,
        &[
            input_link("wip-a", 1, OrderRunInputStatus::InUse),
            input_link("wip-b", 2, OrderRunInputStatus::InUse),
        ],
    );
    assert!(order_run_input_links_from_payload(&payload).is_err());

    payload = serde_json::json!({});
    payload[REZKA_ACTIVE_PARTIAL_ROLLS_PAYLOAD_FIELD] = serde_json::json!([{
            "slot_index": 1,
            "generation": 1,
            "contained_kadr_count": 1,
            "status": "active",
            "source_input_batch_ids": ["wip-a", "wip-a"],
            "started_at_unix": 10,
            "updated_at_unix": 20,
        }]);
    assert!(rezka_active_partial_rolls_from_payload(&payload).is_err());
}

#[test]
fn output_lineage_rejects_duplicate_sequences() {
    let mut payload = serde_json::json!({});
    write_progress_batch_input_links(
        &mut payload,
        &[
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
                sequence_no: 1,
            },
        ],
    );

    assert!(progress_batch_input_links_from_payload(&payload).is_err());
}

#[test]
fn active_roll_sources_must_exist_in_lineage_and_include_current_input() {
    let links = vec![
        input_link("wip-a", 1, OrderRunInputStatus::Processed),
        input_link("wip-b", 2, OrderRunInputStatus::InUse),
    ];
    let valid_roll = RezkaActivePartialRoll {
        slot_index: 1,
        generation: 1,
        contained_kadr_count: 1,
        status: RezkaPartialRollStatus::Active,
        source_input_batch_ids: vec!["wip-a".to_string(), "wip-b".to_string()],
        started_at_unix: 10,
        updated_at_unix: 20,
    };
    assert!(rezka_merge_state_is_consistent(
        &links,
        std::slice::from_ref(&valid_roll)
    ));

    let mut missing_current = valid_roll.clone();
    missing_current.source_input_batch_ids = vec!["wip-a".to_string()];
    assert!(!rezka_merge_state_is_consistent(
        &links,
        &[missing_current]
    ));

    let mut unknown_source = valid_roll;
    unknown_source.source_input_batch_ids.push("wip-c".to_string());
    assert!(!rezka_merge_state_is_consistent(
        &links,
        &[unknown_source]
    ));
}
