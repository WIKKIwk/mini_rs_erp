use super::*;

fn repeated_rezka_map() -> ProductionMapDefinition {
    serde_json::from_value(serde_json::json!({
        "id": "zakaz-rezka-groups",
        "product_code": "REZKA-GROUPS",
        "title": "Rezka groups",
        "nodes": [
            {"id": "start", "kind": "start", "title": "Start"},
            {"id": "rezka_before_lamination", "kind": "apparatus", "title": "Rezka", "apparatus_id": "apparatus:default:asset-010", "rezka_kadr_count": 3, "rezka_frame_groups": [1, 2]},
            {"id": "lamination", "kind": "apparatus", "title": "Laminatsiya", "apparatus_id": "apparatus:catalog:lam-001"},
            {"id": "rezka_final", "kind": "apparatus", "title": "Rezka", "apparatus_id": "apparatus:default:asset-010", "rezka_kadr_count": 3},
            {"id": "end", "kind": "end", "title": "End"}
        ],
        "edges": [
            {"from": "start", "to": "rezka_before_lamination"},
            {"from": "rezka_before_lamination", "to": "lamination"},
            {"from": "lamination", "to": "rezka_final"},
            {"from": "rezka_final", "to": "end"}
        ]
    }))
    .expect("repeated Rezka map")
}

#[test]
fn intermediate_rezka_uses_configured_groups_and_final_uses_input_metadata() {
    let map = repeated_rezka_map();

    assert_eq!(
        rezka_output_kadr_counts(
            &map,
            "apparatus:default:asset-010",
            "rezka_before_lamination",
            None,
        ),
        Ok(vec![1, 2])
    );
    assert_eq!(
        rezka_output_kadr_counts(
            &map,
            "apparatus:default:asset-010",
            "rezka_final",
            Some(2),
        ),
        Ok(vec![1, 1])
    );
    assert_eq!(
        rezka_output_kadr_counts(
            &map,
            "apparatus:default:asset-010",
            "rezka_final",
            None,
        ),
        Ok(vec![1, 1, 1])
    );
}

#[test]
fn bobina_sale_applies_only_to_the_final_rezka_occurrence() {
    let mut map = repeated_rezka_map();
    assert!(!map.includes_bobina_in_output_net(true, "rezka_final"));
    map.include_bobina_in_net = Some(true);
    assert!(!map.includes_bobina_in_output_net(true, "rezka_before_lamination"));
    assert!(!map.includes_bobina_in_output_net(false, "rezka_final"));
    assert!(!map.includes_bobina_in_output_net(true, ""));
    assert!(!map.includes_bobina_in_output_net(true, "missing"));
    assert!(map.includes_bobina_in_output_net(true, "rezka_final"));
}

#[test]
fn projected_rezka_groups_match_reference_for_repeated_stages_and_invalid_data() {
    for variant in 0..4 {
        let mut map = repeated_rezka_map();
        match variant {
            1 => map.nodes[1].rezka_frame_groups = vec![0],
            2 => map.nodes[1].rezka_kadr_count = None,
            3 => map.nodes[1].rezka_frame_groups.clear(),
            _ => {}
        }
        let projection = chain::ProjectionMap::new(&map);
        for stage in ["rezka_before_lamination", "rezka_final", "", "missing"] {
            for count in [None, Some(0), Some(1), Some(5)] {
                let reference = rezka_output_kadr_counts(
                    &map, "apparatus:default:asset-010", stage, count,
                );
                for _ in 0..2 {
                    assert_eq!(
                        rezka_output_kadr_counts_projected(
                            &projection, "apparatus:default:asset-010", stage, count,
                        ),
                        reference,
                        "variant={variant} stage={stage} count={count:?}",
                    );
                }
            }
        }
    }
}
