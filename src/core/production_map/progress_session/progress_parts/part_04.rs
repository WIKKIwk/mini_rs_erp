pub(crate) const INPUT_LINEAGE_PAYLOAD_FIELD: &str = "input_lineage";
pub(crate) const REZKA_ACTIVE_PARTIAL_ROLLS_PAYLOAD_FIELD: &str =
    "rezka_active_partial_rolls";
pub(crate) const SOURCE_INPUT_LINKS_PAYLOAD_FIELD: &str = "source_input_links";

pub(crate) fn order_run_input_links_from_payload(
    payload: &serde_json::Value,
) -> Result<Vec<OrderRunInputLink>, ()> {
    let Some(value) = payload.get(INPUT_LINEAGE_PAYLOAD_FIELD) else {
        return Ok(Vec::new());
    };
    let links: Vec<OrderRunInputLink> = serde_json::from_value(value.clone()).map_err(|_| ())?;
    let mut batch_ids = std::collections::BTreeSet::new();
    let mut sequence_numbers = std::collections::BTreeSet::new();
    let mut active_count = 0usize;
    for link in &links {
        if !link.is_valid()
            || !batch_ids.insert(link.input_batch_id.trim())
            || !sequence_numbers.insert(link.sequence_no)
        {
            return Err(());
        }
        if link.status == OrderRunInputStatus::InUse {
            active_count += 1;
        }
    }
    if active_count > 1 {
        return Err(());
    }
    Ok(links)
}

pub(crate) fn write_order_run_input_links(
    payload: &mut serde_json::Value,
    links: &[OrderRunInputLink],
) {
    ensure_payload_object(payload);
    payload[INPUT_LINEAGE_PAYLOAD_FIELD] = serde_json::json!(links);
}

pub(crate) fn rezka_active_partial_rolls_from_payload(
    payload: &serde_json::Value,
) -> Result<Vec<RezkaActivePartialRoll>, ()> {
    let Some(value) = payload.get(REZKA_ACTIVE_PARTIAL_ROLLS_PAYLOAD_FIELD) else {
        return Ok(Vec::new());
    };
    let rolls: Vec<RezkaActivePartialRoll> =
        serde_json::from_value(value.clone()).map_err(|_| ())?;
    let mut slots = std::collections::BTreeSet::new();
    for roll in &rolls {
        if !roll.is_valid() || !slots.insert(roll.slot_index) {
            return Err(());
        }
    }
    Ok(rolls)
}

pub(crate) fn write_rezka_active_partial_rolls(
    payload: &mut serde_json::Value,
    rolls: &[RezkaActivePartialRoll],
) {
    ensure_payload_object(payload);
    payload[REZKA_ACTIVE_PARTIAL_ROLLS_PAYLOAD_FIELD] = serde_json::json!(rolls);
}

pub(crate) fn progress_batch_input_links_from_payload(
    payload: &serde_json::Value,
) -> Result<Vec<ProgressBatchInputLink>, ()> {
    let Some(value) = payload.get(SOURCE_INPUT_LINKS_PAYLOAD_FIELD) else {
        return Ok(Vec::new());
    };
    let links: Vec<ProgressBatchInputLink> =
        serde_json::from_value(value.clone()).map_err(|_| ())?;
    let mut batch_ids = std::collections::BTreeSet::new();
    let mut sequence_numbers = std::collections::BTreeSet::new();
    for link in &links {
        if !link.is_valid()
            || !batch_ids.insert(link.input_batch_id.trim())
            || !sequence_numbers.insert(link.sequence_no)
        {
            return Err(());
        }
    }
    Ok(links)
}

pub(crate) fn rezka_merge_state_is_consistent(
    input_links: &[OrderRunInputLink],
    active_rolls: &[RezkaActivePartialRoll],
) -> bool {
    let lineage_batch_ids = input_links
        .iter()
        .map(|link| link.input_batch_id.trim())
        .collect::<std::collections::BTreeSet<_>>();
    let active_input_batch_id = input_links
        .iter()
        .find(|link| link.status == OrderRunInputStatus::InUse)
        .map(|link| link.input_batch_id.trim());

    active_rolls.iter().all(|roll| {
        let sources_exist_in_lineage = roll
            .source_input_batch_ids
            .iter()
            .all(|batch_id| lineage_batch_ids.contains(batch_id.trim()));
        let active_source_is_present = match active_input_batch_id {
            Some(active) => roll
                .source_input_batch_ids
                .iter()
                .any(|source| source.trim() == active),
            None => roll.source_input_batch_ids.is_empty(),
        };
        sources_exist_in_lineage && active_source_is_present
    })
}

pub(crate) fn write_progress_batch_input_links(
    payload: &mut serde_json::Value,
    links: &[ProgressBatchInputLink],
) {
    ensure_payload_object(payload);
    payload[SOURCE_INPUT_LINKS_PAYLOAD_FIELD] = serde_json::json!(links);
}

fn ensure_payload_object(payload: &mut serde_json::Value) {
    if !payload.is_object() {
        *payload = serde_json::json!({});
    }
}

#[cfg(test)]
mod merge_lineage_payload_tests {
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
}
