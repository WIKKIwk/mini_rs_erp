
fn reassign_apparatus_nodes_by_id(
    map: &mut ProductionMapDefinition,
    from_id: &str,
    to_id: &str,
    target_display: &str,
) -> bool {
    let mut changed = false;
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && node.alternative_group_id.trim().is_empty()
            && effective_transfer_apparatus_id(node) == from_id
        {
            node.apparatus_id = to_id.to_string();
            node.title = target_display.to_string();
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    fn transfer_map(source_title: &str, target_title: &str) -> ProductionMapDefinition {
        serde_json::from_value(serde_json::json!({
            "id": "order-transfer-test",
            "product_code": "TRANSFER-TEST",
            "title": "Transfer test",
            "nodes": [
                {"id": "start", "kind": "start", "title": "Start"},
                {
                    "id": "source", "kind": "apparatus", "title": source_title,
                    "apparatus_id": "apparatus:test:source"
                },
                {
                    "id": "target", "kind": "apparatus", "title": target_title,
                    "apparatus_id": "apparatus:test:target"
                },
                {"id": "end", "kind": "end", "title": "End"}
            ],
            "edges": [
                {"from": "start", "to": "source"},
                {"from": "source", "to": "end"}
            ]
        }))
        .expect("transfer map")
    }

    #[test]
    fn exact_id_transfer_reassigns_identity_and_keeps_target_display_snapshot() {
        let mut map = transfer_map("Old title", "New title");
        assert!(transfer_move_allowed_by_id(
            &map,
            "apparatus:test:source",
            "apparatus:test:target"
        ));
        assert!(reassign_apparatus_nodes_by_id(
            &mut map,
            "apparatus:test:source",
            "apparatus:test:target",
            "Renamed target"
        ));
        let node = &map.nodes[1];
        assert_eq!(node.apparatus_id, "apparatus:test:target");
        assert_eq!(node.title, "Renamed target");
    }

    #[test]
    fn transfer_identity_is_independent_of_display_rename() {
        let map = transfer_map("Renamed source", "Renamed target");
        assert!(transfer_move_allowed_by_id(
            &map,
            "apparatus:test:source",
            "apparatus:test:target"
        ));
        assert!(!transfer_move_allowed_by_id(
            &map,
            "apparatus:legacy:renamed-source",
            "apparatus:test:target"
        ));
    }

    #[test]
    fn transfer_rejects_title_only_and_mismatched_identity() {
        assert!(canonical_transfer_id("apparatus:test:source").is_some());
        assert!(canonical_transfer_id("Renamed source").is_none());
        assert!(!apparatus_ids_match(
            "apparatus:test:source",
            "Renamed source"
        ));
        let map = transfer_map("Renamed source", "Renamed target");
        assert!(!transfer_move_allowed_by_id(
            &map,
            "apparatus:test:other",
            "apparatus:test:target"
        ));
    }
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
