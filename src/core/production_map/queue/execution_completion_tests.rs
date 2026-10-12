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
fn another_existing_concrete_target_is_not_counted_as_this_stages_input() {
    let mut map = map();
    map.nodes.push(serde_json::from_value(serde_json::json!({
        "id": "later-lamination", "kind": "apparatus", "title": "Later", "apparatus_id": LAM2
    })).unwrap());
    map.edges.retain(|edge| edge.from != "rezka");
    map.edges.extend([
        ProductionMapEdge { from: "rezka".into(), to: "later-lamination".into(), branch: String::new() },
        ProductionMapEdge { from: "later-lamination".into(), to: "end".into(), branch: String::new() },
    ]);
    let other_stage = batch(PRESS, "press", LAM2, "later-lamination");
    assert!(!unfinished(&map, LAM2, "lam2", &[other_stage]));
}

#[test]
fn removed_target_with_one_canonical_successor_is_still_outstanding_input() {
    let map = map();
    let remapped = batch(PRESS, "press", LAM2, "removed-lamination");
    assert!(unfinished(&map, LAM2, "lam2", &[remapped]));
}

fn local_report_has_input(
    map: &ProductionMapDefinition,
    batches: &[OrderProgressBatch],
    updates: &[OrderProgressBatch],
    ignored_batch_id: &str,
) -> bool {
    let canonical =
        crate::core::apparatus_standard::test_support::standard_runtime_configurations()
            .into_iter()
            .find(|config| config.runtime.apparatus_id.as_str() == LAM2)
            .unwrap();
    let states = BTreeMap::from([(
        PRESS.into(),
        BTreeMap::from([(map.id.clone(), "completed".into())]),
    )]);
    has_available_previous_wips_for_local_report(
        &map.id,
        map,
        LAM2,
        &canonical,
        &states,
        batches,
        updates,
        &[],
        &[],
        ignored_batch_id,
        "lam2",
        Some(true),
    )
}

#[test]
fn waiting_layer_remainder_defers_local_final_report_without_reopening_production() {
    let map = map();
    let mut remainder = batch(PRESS, "press", LAM2, "lam2");
    remainder.payload_json["origin"] = serde_json::json!("layer_remainder");
    assert!(local_report_has_input(&map, &[remainder.clone()], &[], ""));
    assert!(!unfinished(&map, LAM2, "lam2", &[remainder.clone()]));
    assert!(!local_report_has_input(
        &map,
        &[remainder.clone()],
        &[],
        &remainder.batch_id,
    ));
}

#[test]
fn local_final_report_only_counts_actionable_layer_remainders_for_this_stage() {
    let map = map();
    let mut remainder = batch(PRESS, "press", LAM2, "lam2");
    remainder.payload_json["origin"] = serde_json::json!("layer_remainder");
    for status in [
        OrderProgressBatchWipStatus::InUse,
        OrderProgressBatchWipStatus::Processed,
    ] {
        let mut unavailable = remainder.clone();
        unavailable.wip_status = status;
        assert!(!local_report_has_input(&map, &[unavailable], &[], ""));
    }
    let mut other_order = remainder.clone();
    other_order.order_id = "other-order".into();
    assert!(!local_report_has_input(&map, &[other_order], &[], ""));
    let mut other_stage = remainder.clone();
    other_stage.payload_json["stage_node_id"] = serde_json::json!("lam1");
    other_stage.payload_json["next_stage_node_id"] = serde_json::json!("rezka");
    other_stage.apparatus = LAM1.into();
    other_stage.next_apparatus = REZKA.into();
    assert!(!local_report_has_input(&map, &[other_stage], &[], ""));
    let mut claimed = remainder;
    claimed.used_by_session_id = "peer-session".into();
    claimed.used_by_apparatus = LAM1.into();
    assert!(!local_report_has_input(&map, &[claimed], &[], ""));
}

#[test]
fn local_final_report_uses_latest_layer_remainder_status() {
    let map = map();
    let mut remainder = batch(PRESS, "press", LAM2, "lam2");
    remainder.payload_json["origin"] = serde_json::json!("layer_remainder");
    let mut processed = remainder.clone();
    processed.wip_status = OrderProgressBatchWipStatus::Processed;
    assert!(!local_report_has_input(
        &map,
        &[remainder.clone()],
        &[processed.clone()],
        "",
    ));
    assert!(local_report_has_input(&map, &[processed], &[remainder], ""));
}
