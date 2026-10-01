use super::*;
use serde_json::json;

const LAM: &str = "apparatus:default:asset-007";
const CUT: &str = "apparatus:default:asset-010";
const CUT2: &str = "apparatus:default:asset-011";

fn map() -> ProductionMapDefinition {
    serde_json::from_value(json!({"id":"order-0004","product_code":"lazer","title":"Lazer guruch uzun don 2 kg",
        "nodes":[
            {"id":"start","kind":"start","title":"Start"},
            {"id":"lam1","kind":"apparatus","title":"Laminatsiya1","apparatus_id":LAM},
            {"id":"apparatus_6","kind":"apparatus","title":"Rezka","apparatus_id":CUT,"alternative_group_id":"alt_cut_6"},
            {"id":"apparatus_7","kind":"apparatus","title":"Rezka2","apparatus_id":CUT2,"alternative_group_id":"alt_cut_6"},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"lam1"},{"from":"lam1","to":"apparatus_6"},
            {"from":"lam1","to":"apparatus_7"},{"from":"apparatus_6","to":"end"},{"from":"apparatus_7","to":"end"}]
    })).unwrap()
}

fn batch() -> OrderProgressBatch {
    serde_json::from_value(json!({"batch_id":"old-roll","session_id":"lam-session","started_at_unix":1,"completed_at_unix":2,
        "apparatus":LAM,"order_id":"order-0004","action":"detach_roll","status":"completed",
        "produced_qty":6170.0,"uom":"m","qr_payload":"400118DA2F17C3617F59DDC6",
        "label_item_code":"","label_item_name":"","executor_name":"","worker_role":"aparatchi","worker_ref":"worker","worker_display_name":"Worker",
        "wip_status":"waiting","current_apparatus":LAM,"next_apparatus":CUT,
        "payload_json":{"stage_node_id":"lam1","next_stage_node_id":"rezka_5"}
    })).unwrap()
}

#[test]
fn stale_destination_resolves_exact_reported_group_without_changing_printed_identity() {
    let original = batch();
    let route = resolve_wip_input_route(&map(), &original).unwrap();
    assert_eq!(route.source_stage_node_id, "lam1");
    assert_eq!(route.stage_node_id, "apparatus_6");
    assert_eq!(route.consumer_apparatus_ids, vec![CUT, CUT2]);
    assert!(route.remapped);
    for _ in 0..3 {
        assert_eq!(resolve_wip_input_route(&map(), &original).unwrap(), route);
    }
    assert_eq!(original, batch(), "scan/preview are pure");
}

#[test]
fn explicit_valid_target_is_preserved_even_with_other_future_branch() {
    let mut map = map();
    let mut b = batch();
    b.payload_json["next_stage_node_id"] = json!("apparatus_6");
    let mut other = map.nodes[2].clone();
    other.id = "other-stage".into();
    other.alternative_group_id.clear();
    map.nodes.push(other);
    map.edges.push(super::super::ProductionMapEdge {
        from: "lam1".into(),
        to: "other-stage".into(),
        branch: String::new(),
    });
    let route = resolve_wip_input_route(&map, &b).unwrap();
    assert_eq!(route.stage_node_id, "apparatus_6");
    assert!(!route.remapped);
}

#[test]
fn missing_or_replaced_producer_identity_is_never_guessed() {
    let mut b = batch();
    b.payload_json["stage_node_id"] = json!("deleted-lam");
    assert_eq!(
        resolve_wip_input_route(&map(), &b),
        Err(ProductionMapError::WipRouteSourceUnresolved)
    );
    b.payload_json["stage_node_id"] = json!("");
    assert_eq!(
        resolve_wip_input_route(&map(), &b),
        Err(ProductionMapError::WipRouteSourceUnresolved)
    );
    b.payload_json["stage_node_id"] = json!("apparatus_6");
    assert_eq!(
        resolve_wip_input_route(&map(), &b),
        Err(ProductionMapError::WipRouteSourceUnresolved)
    );
}

#[test]
fn multiple_immediate_logical_stages_are_ambiguous() {
    let mut map = map();
    map.nodes[3].alternative_group_id.clear();
    assert_eq!(
        resolve_wip_input_route(&map, &batch()),
        Err(ProductionMapError::WipRouteAmbiguous)
    );
}

#[test]
fn destination_requires_original_canonical_membership_not_a_title_or_operation() {
    let mut b = batch();
    b.next_apparatus = "apparatus:default:missing".into();
    assert_eq!(
        resolve_wip_input_route(&map(), &b),
        Err(ProductionMapError::WipRouteDestinationUnresolved)
    );
    b.next_apparatus.clear();
    assert_eq!(
        resolve_wip_input_route(&map(), &b),
        Err(ProductionMapError::WipRouteDestinationUnresolved)
    );
}

#[test]
fn repeated_canonical_destination_cannot_choose_an_occurrence() {
    let mut map = map();
    map.nodes[3].apparatus_id = CUT.into();
    assert_eq!(
        resolve_wip_input_route(&map, &batch()),
        Err(ProductionMapError::WipRouteAmbiguous)
    );
}

#[test]
fn claimed_processed_or_dirty_waiting_roll_is_not_rerouted() {
    for status in [
        OrderProgressBatchWipStatus::InUse,
        OrderProgressBatchWipStatus::Processed,
    ] {
        let mut b = batch();
        b.wip_status = status;
        assert_eq!(
            resolve_wip_input_route(&map(), &b),
            Err(ProductionMapError::WipRouteDestinationUnresolved)
        );
    }
    let mut b = batch();
    b.used_by_apparatus = CUT2.into();
    assert_eq!(
        resolve_wip_input_route(&map(), &b),
        Err(ProductionMapError::WipRouteDestinationUnresolved)
    );
}

#[test]
fn missing_destination_from_map_extension_remains_supported_with_exact_source() {
    let mut b = batch();
    b.next_apparatus.clear();
    b.payload_json["next_stage_node_id"] = json!("");
    assert_eq!(
        resolve_wip_input_route(&map(), &b)
            .unwrap()
            .consumer_apparatus_ids,
        vec![CUT, CUT2]
    );
}

#[test]
fn route_save_guard_allows_safe_conversion_and_rejects_orphaning() {
    let next = map();
    let mut previous = next.clone();
    previous.nodes[2].id = "rezka_5".into();
    previous.nodes[2].alternative_group_id.clear();
    previous.nodes.remove(3);
    previous.edges = vec![
        super::super::ProductionMapEdge {
            from: "start".into(),
            to: "lam1".into(),
            branch: String::new(),
        },
        super::super::ProductionMapEdge {
            from: "lam1".into(),
            to: "rezka_5".into(),
            branch: String::new(),
        },
        super::super::ProductionMapEdge {
            from: "rezka_5".into(),
            to: "end".into(),
            branch: String::new(),
        },
    ];
    assert!(validate_map_wip_routes(&previous, &next, &[batch()]).is_ok());
    let mut invalid = next.clone();
    invalid.nodes[3].alternative_group_id.clear();
    assert_eq!(
        validate_map_wip_routes(&previous, &invalid, &[batch()]),
        Err(ProductionMapError::WipRouteAmbiguous)
    );
}

#[test]
fn claimed_route_pin_is_not_replaced_by_a_later_equivalent_machine() {
    let mut b = batch();
    let mut route = resolve_wip_input_route(&map(), &b).unwrap();
    route.stage_node_id = "apparatus_7".into();
    b.payload_json["wip_route_binding"] = json!(route);
    b.wip_status = OrderProgressBatchWipStatus::InUse;
    b.used_by_apparatus = CUT2.into();
    assert_eq!(
        resolve_wip_input_route(&map(), &b).unwrap().stage_node_id,
        "apparatus_7"
    );
    let mut changed = map();
    changed.nodes[3].id = "replacement".into();
    for edge in &mut changed.edges {
        if edge.to == "apparatus_7" {
            edge.to = "replacement".into();
        }
        if edge.from == "apparatus_7" {
            edge.from = "replacement".into();
        }
    }
    assert_eq!(
        resolve_wip_input_route(&changed, &b),
        Err(ProductionMapError::WipRouteDestinationUnresolved)
    );
}

#[test]
fn representative_edge_exposes_all_canonical_peers_in_the_successor_group() {
    let mut current = map();
    current
        .edges
        .retain(|edge| edge.from != "lam1" || edge.to != "apparatus_7");
    let route = resolve_wip_input_route(&current, &batch()).unwrap();
    assert_eq!(route.consumer_apparatus_ids, vec![CUT, CUT2]);
}

#[test]
fn claimed_consumer_survives_removal_of_an_unused_original_alternative() {
    let mut b = batch();
    let mut route = resolve_wip_input_route(&map(), &b).unwrap();
    route.stage_node_id = "apparatus_7".into();
    b.payload_json["wip_route_binding"] = json!(route);
    b.wip_status = OrderProgressBatchWipStatus::InUse;
    b.used_by_apparatus = CUT2.into();
    let mut changed = map();
    changed.nodes.retain(|node| node.id != "apparatus_6");
    changed
        .edges
        .retain(|edge| edge.from != "apparatus_6" && edge.to != "apparatus_6");
    assert_eq!(
        resolve_wip_input_route(&changed, &b)
            .unwrap()
            .consumer_apparatus_ids,
        vec![CUT2]
    );
    assert!(validate_map_wip_routes(&map(), &changed, &[b.clone()]).is_ok());
    changed
        .nodes
        .iter_mut()
        .find(|node| node.id == "apparatus_7")
        .unwrap()
        .apparatus_id = CUT.into();
    assert_eq!(
        resolve_wip_input_route(&changed, &b),
        Err(ProductionMapError::WipRouteDestinationUnresolved)
    );
}

#[test]
fn consumed_legacy_canonical_pair_identifies_an_existing_route_without_remapping() {
    let mut b = batch();
    b.payload_json = json!({});
    b.wip_status = OrderProgressBatchWipStatus::Processed;
    b.used_by_apparatus = CUT2.into();
    b.processed_by_apparatus = CUT2.into();
    let route = resolve_wip_input_route(&map(), &b).unwrap();
    assert!(!route.remapped);
    assert_eq!(route.source_stage_node_id, "lam1");
    assert_eq!(route.stage_node_id, "apparatus_6");
    b.payload_json["next_stage_node_id"] = json!("rezka_5");
    assert_eq!(
        resolve_wip_input_route(&map(), &b),
        Err(ProductionMapError::WipRouteSourceUnresolved)
    );
    b.payload_json = json!({});
    let mut repeated = map();
    let mut later = repeated.nodes[2].clone();
    later.id = "later_cut".into();
    later.alternative_group_id.clear();
    repeated.nodes.push(later);
    repeated.edges.push(super::super::ProductionMapEdge {
        from: "apparatus_7".into(),
        to: "later_cut".into(),
        branch: String::new(),
    });
    assert_eq!(
        resolve_wip_input_route(&repeated, &b),
        Err(ProductionMapError::WipRouteAmbiguous)
    );
}

#[test]
fn unpinned_active_input_protects_its_actual_consumer_in_the_exact_group() {
    let mut b = batch();
    b.payload_json["next_stage_node_id"] = json!("apparatus_6");
    b.wip_status = OrderProgressBatchWipStatus::InUse;
    b.used_by_apparatus = CUT2.into();
    b.used_by_session_id = "existing-cut2-session".into();
    let original = b.clone();
    assert_eq!(
        resolve_wip_input_route(&map(), &b).unwrap().stage_node_id,
        "apparatus_7"
    );
    assert_eq!(b, original, "legacy ownership projection is read-only");
    let mut missing_consumer = map();
    missing_consumer
        .nodes
        .retain(|node| node.id != "apparatus_7");
    missing_consumer
        .edges
        .retain(|edge| edge.from != "apparatus_7" && edge.to != "apparatus_7");
    assert_eq!(
        validate_map_wip_routes(&map(), &missing_consumer, &[b.clone()]),
        Err(ProductionMapError::WipRouteDestinationUnresolved)
    );
    let mut replaced_consumer = map();
    replaced_consumer
        .nodes
        .iter_mut()
        .find(|node| node.id == "apparatus_7")
        .unwrap()
        .id = "replacement_cut2".into();
    for edge in &mut replaced_consumer.edges {
        if edge.from == "apparatus_7" {
            edge.from = "replacement_cut2".into();
        }
        if edge.to == "apparatus_7" {
            edge.to = "replacement_cut2".into();
        }
    }
    assert_eq!(
        validate_map_wip_routes(&map(), &replaced_consumer, &[b.clone()]),
        Err(ProductionMapError::WipRouteDestinationUnresolved),
        "a canonical peer replacement cannot move an already active occurrence"
    );
    b.used_by_apparatus = CUT.into();
    assert!(
        validate_map_wip_routes(&map(), &missing_consumer, &[b]).is_ok(),
        "the unused peer may be removed when producer and destination remain proven"
    );
}
