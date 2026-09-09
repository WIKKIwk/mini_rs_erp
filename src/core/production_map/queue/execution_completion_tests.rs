use super::*;

const PRESS: &str = "apparatus:default:bosma_9";
const LAM1: &str = "apparatus:default:asset-007";
const LAM2: &str = "apparatus:default:asset-008";
const REZKA: &str = "apparatus:default:asset-010";

fn map() -> ProductionMapDefinition {
    serde_json::from_value(serde_json::json!({
        "id": "split-work", "product_code": "SPLIT", "title": "Split work",
        "nodes": [
            {"id": "start", "kind": "start", "title": "Start"},
            {"id": "press", "kind": "apparatus", "title": "Press", "apparatus_id": PRESS},
            {"id": "lam1", "kind": "apparatus", "title": "Lam1", "apparatus_id": LAM1,
             "alternative_group_id": "lamination", "alternative_assigned_apparatus_id": LAM2},
            {"id": "lam2", "kind": "apparatus", "title": "Lam2", "apparatus_id": LAM2,
             "alternative_group_id": "lamination", "alternative_assigned_apparatus_id": LAM2},
            {"id": "rezka", "kind": "apparatus", "title": "Rezka", "apparatus_id": REZKA},
            {"id": "end", "kind": "end", "title": "End"}
        ],
        "edges": [
            {"from": "start", "to": "press"}, {"from": "press", "to": "lam1"},
            {"from": "press", "to": "lam2"}, {"from": "lam1", "to": "rezka"},
            {"from": "lam2", "to": "rezka"}, {"from": "rezka", "to": "end"}
        ]
    }))
    .unwrap()
}

fn batch(source: &str, source_node: &str, target: &str, target_node: &str) -> OrderProgressBatch {
    serde_json::from_value(serde_json::json!({
        "batch_id": "half-work", "session_id": "session", "started_at_unix": 1,
        "completed_at_unix": 2, "apparatus": source, "order_id": "split-work",
        "action": "complete", "status": "completed", "produced_qty": 50.0,
        "uom": "m", "qr_payload": "test", "label_item_code": "TEST",
        "label_item_name": "Test", "executor_name": "Test", "worker_role": "aparatchi",
        "worker_ref": "test", "worker_display_name": "Test", "wip_status": "waiting",
        "next_apparatus": target,
        "payload_json": {"stage_node_id": source_node, "next_stage_node_id": target_node}
    }))
    .unwrap()
}

fn unfinished(
    map: &ProductionMapDefinition,
    apparatus: &str,
    stage: &str,
    batches: &[OrderProgressBatch],
) -> bool {
    let canonical =
        crate::core::apparatus_standard::test_support::standard_runtime_configurations()
            .into_iter()
            .find(|config| config.runtime.apparatus_id.as_str() == apparatus)
            .unwrap();
    let states = [PRESS, LAM1, LAM2]
        .into_iter()
        .map(|id| {
            (
                id.into(),
                BTreeMap::from([(map.id.clone(), "completed".into())]),
            )
        })
        .collect();
    has_unprocessed_previous_wips_from_batches(
        &map.id, map, apparatus, &canonical, &states, batches, "", stage,
    )
}

#[test]
fn stage_end_waits_for_input_still_being_worked_on_at_peer_alternative() {
    let map = map();
    let mut half = batch(PRESS, "press", LAM1, "lam1");
    assert!(unfinished(&map, LAM2, "lam2", &[half.clone()]));
    half.wip_status = OrderProgressBatchWipStatus::InUse;
    half.used_by_apparatus = LAM1.into();
    assert!(unfinished(&map, LAM2, "lam2", &[half.clone()]));
    half.wip_status = OrderProgressBatchWipStatus::Processed;
    half.processed_by_apparatus = LAM1.into();
    assert!(!unfinished(&map, LAM2, "lam2", &[half]));
}

#[test]
fn downstream_stage_end_includes_output_from_both_alternative_producers() {
    let map = map();
    let mut first = batch(LAM1, "lam1", REZKA, "rezka");
    let mut second = batch(LAM2, "lam2", REZKA, "rezka");
    second.batch_id = "other-half".into();
    second.wip_status = OrderProgressBatchWipStatus::Processed;
    second.processed_by_apparatus = REZKA.into();
    assert!(unfinished(
        &map,
        REZKA,
        "rezka",
        &[first.clone(), second.clone()]
    ));
    first.wip_status = OrderProgressBatchWipStatus::Processed;
    first.processed_by_apparatus = REZKA.into();
    assert!(!unfinished(&map, REZKA, "rezka", &[first, second]));
}

#[test]
fn another_concrete_target_occurrence_is_not_counted_as_this_stages_input() {
    let map = map();
    let other_stage = batch(PRESS, "press", LAM2, "removed-or-later-lamination");
    assert!(!unfinished(&map, LAM2, "lam2", &[other_stage]));
}
