use std::collections::BTreeMap;

use super::*;
use crate::core::production_map::{ProductionMapEdge, ProductionMapNode, ProductionMapNodeKind};

fn node(id: &str, kind: ProductionMapNodeKind, title: &str) -> ProductionMapNode {
    ProductionMapNode {
        id: id.to_string(),
        kind,
        title: title.to_string(),
        formula: None,
        role_code: String::new(),
        item_code: String::new(),
        qty_formula: String::new(),
        from_location: String::new(),
        to_location: String::new(),
        alternative_group_id: String::new(),
        alternative_group_label: String::new(),
        alternative_assigned_title: String::new(),
        rezka_kadr_count: None,
        rezka_label_length: None,
        x: 0.0,
        y: 0.0,
    }
}

fn assigned_node(
    id: &str,
    kind: ProductionMapNodeKind,
    title: &str,
    assigned_title: &str,
) -> ProductionMapNode {
    ProductionMapNode {
        alternative_assigned_title: assigned_title.to_string(),
        ..node(id, kind, title)
    }
}

fn alternative_node(id: &str, title: &str, group_id: &str) -> ProductionMapNode {
    alternative_node_with_label(id, title, group_id, group_id)
}

fn alternative_node_with_label(
    id: &str,
    title: &str,
    group_id: &str,
    group_label: &str,
) -> ProductionMapNode {
    ProductionMapNode {
        alternative_group_id: group_id.to_string(),
        alternative_group_label: group_label.to_string(),
        ..node(id, ProductionMapNodeKind::Apparatus, title)
    }
}

fn hotlunch_map() -> ProductionMapDefinition {
    ProductionMapDefinition {
        id: "zakaz-hot".to_string(),
        product_code: "HOT".to_string(),
        title: "Hotlunch".to_string(),
        code: String::new(),
        order_number: "100".to_string(),
        roll_count: None,
        width_mm: None,
        order_kg: None,
        base_length: None,
        nodes: vec![
            node("start", ProductionMapNodeKind::Start, "Start"),
            node("order", ProductionMapNodeKind::Task, "Hotlunch mahsulot"),
            node(
                "pechat",
                ProductionMapNodeKind::Apparatus,
                "9 ta rangli pechat - A",
            ),
            node("lamin", ProductionMapNodeKind::Task, "Laminatsiya"),
            node(
                "rezka",
                ProductionMapNodeKind::Apparatus,
                "Rezka aparat - A",
            ),
            node("end", ProductionMapNodeKind::End, "End"),
        ],
        edges: vec![
            ProductionMapEdge {
                from: "start".to_string(),
                to: "order".to_string(),
                branch: String::new(),
            },
            ProductionMapEdge {
                from: "order".to_string(),
                to: "pechat".to_string(),
                branch: String::new(),
            },
            ProductionMapEdge {
                from: "pechat".to_string(),
                to: "lamin".to_string(),
                branch: String::new(),
            },
            ProductionMapEdge {
                from: "lamin".to_string(),
                to: "rezka".to_string(),
                branch: String::new(),
            },
            ProductionMapEdge {
                from: "rezka".to_string(),
                to: "end".to_string(),
                branch: String::new(),
            },
        ],
    }
}

#[test]
fn map_has_work_stage_matches_warehouse_suffixes() {
    let map = hotlunch_map();
    assert!(map_has_work_stage_for_station(&map, "Laminatsiya - A"));
    assert!(map_has_work_stage_for_station(&map, "9 ta rangli pechat"));
    assert!(!map_has_work_stage_for_station(&map, "Hotlunch mahsulot"));
}

#[test]
fn linear_work_stages_follows_production_chain() {
    let stages = linear_work_stages(&hotlunch_map());
    assert_eq!(
        stages
            .iter()
            .map(|stage| stage.station_title.as_str())
            .collect::<Vec<_>>(),
        vec!["9 ta rangli pechat - A", "Laminatsiya", "Rezka aparat - A"]
    );
}

#[test]
fn unassigned_bosma_alternative_group_exposes_each_candidate_as_work_stage() {
    let mut map = hotlunch_map();
    map.nodes = vec![
        node("start", ProductionMapNodeKind::Start, "Start"),
        node("order", ProductionMapNodeKind::Task, "Zakaz"),
        alternative_node("pechat_7", "7 ta rangli bosma aparat", "alt_bosma"),
        alternative_node("pechat_8", "8 ta rangli bosma aparat", "alt_bosma"),
        node("end", ProductionMapNodeKind::End, "End"),
    ];
    map.edges = vec![
        ProductionMapEdge {
            from: "start".to_string(),
            to: "order".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "order".to_string(),
            to: "pechat_7".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "order".to_string(),
            to: "pechat_8".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "pechat_7".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "pechat_8".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
    ];

    assert_eq!(
        linear_work_stages(&map)
            .iter()
            .map(|stage| stage.station_title.as_str())
            .collect::<Vec<_>>(),
        vec!["7 ta rangli bosma aparat", "8 ta rangli bosma aparat"]
    );
    assert!(map_has_work_stage_for_station(
        &map,
        "7 ta rangli bosma aparat"
    ));
    assert!(map_has_work_stage_for_station(
        &map,
        "8 ta rangli bosma aparat"
    ));
    assert_eq!(
        next_work_stage_station(&map, "7 ta rangli bosma aparat"),
        None
    );
}

#[test]
fn unassigned_laminatsiya_alternative_group_exposes_candidates_after_previous_stage() {
    let mut map = hotlunch_map();
    map.nodes = vec![
        node("start", ProductionMapNodeKind::Start, "Start"),
        node(
            "pechat",
            ProductionMapNodeKind::Apparatus,
            "7 ta rangli bosma aparat",
        ),
        alternative_node_with_label("lamin_1", "Laminatsiya 1", "alt_laminatsiya", "Laminatsiya"),
        alternative_node_with_label("lamin_2", "Laminatsiya 2", "alt_laminatsiya", "Laminatsiya"),
        node("end", ProductionMapNodeKind::End, "End"),
    ];
    map.edges = vec![
        ProductionMapEdge {
            from: "start".to_string(),
            to: "pechat".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "pechat".to_string(),
            to: "lamin_1".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "pechat".to_string(),
            to: "lamin_2".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "lamin_1".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "lamin_2".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
    ];

    assert_eq!(
        linear_work_stages(&map)
            .iter()
            .map(|stage| stage.station_title.as_str())
            .collect::<Vec<_>>(),
        vec!["7 ta rangli bosma aparat", "Laminatsiya 1", "Laminatsiya 2"]
    );
    assert!(map_has_work_stage_for_station(&map, "Laminatsiya 1"));
    assert!(map_has_work_stage_for_station(&map, "Laminatsiya 2"));
    assert_eq!(
        previous_work_stage_station(&map, "Laminatsiya 1"),
        Some("7 ta rangli bosma aparat".to_string())
    );
    assert_eq!(
        previous_work_stage_station(&map, "Laminatsiya 2"),
        Some("7 ta rangli bosma aparat".to_string())
    );
    assert_eq!(
        next_work_stage_station(&map, "7 ta rangli bosma aparat"),
        Some("Laminatsiya".to_string())
    );
}

#[test]
fn next_work_stage_uses_assigned_titles_across_branch_alternatives() {
    let mut map = hotlunch_map();
    map.nodes = vec![
        node("start", ProductionMapNodeKind::Start, "Start"),
        node("order", ProductionMapNodeKind::Task, "Paynet"),
        assigned_node(
            "pechat_7",
            ProductionMapNodeKind::Apparatus,
            "7 ta rangli pechat",
            "8 ta rangli pechat",
        ),
        assigned_node(
            "pechat_8",
            ProductionMapNodeKind::Apparatus,
            "8 ta rangli pechat",
            "8 ta rangli pechat",
        ),
        assigned_node(
            "lamin_1",
            ProductionMapNodeKind::Apparatus,
            "Laminatsiya 1",
            "Laminatsiya 1",
        ),
        assigned_node(
            "lamin_2",
            ProductionMapNodeKind::Apparatus,
            "Laminatsiya 2",
            "Laminatsiya 1",
        ),
        node("end", ProductionMapNodeKind::End, "End"),
    ];
    map.edges = vec![
        ProductionMapEdge {
            from: "start".to_string(),
            to: "order".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "order".to_string(),
            to: "pechat_7".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "order".to_string(),
            to: "pechat_8".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "pechat_7".to_string(),
            to: "lamin_1".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "pechat_7".to_string(),
            to: "lamin_2".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "lamin_1".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "lamin_2".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
    ];

    assert_eq!(next_work_stage_station(&map, "7 ta rangli pechat"), None);
    assert_eq!(
        next_work_stage_station(&map, "8 ta rangli pechat"),
        Some("Laminatsiya 1".to_string())
    );
}

#[test]
fn later_stage_waits_for_previous_completion() {
    let map = hotlunch_map();
    let mut states = BTreeMap::new();
    assert!(order_ready_for_station(
        &map,
        "zakaz-hot",
        "9 ta rangli pechat",
        &states,
        &[],
    ));
    assert!(!order_ready_for_station(
        &map,
        "zakaz-hot",
        "Laminatsiya",
        &states,
        &[],
    ));
    states.insert(
        "9 ta rangli pechat".to_string(),
        BTreeMap::from([("zakaz-hot".to_string(), "completed".to_string())]),
    );
    assert!(order_ready_for_station(
        &map,
        "zakaz-hot",
        "Laminatsiya",
        &states,
        &[],
    ));
    assert!(!order_ready_for_station(
        &map,
        "zakaz-hot",
        "Rezka aparat",
        &states,
        &[],
    ));
}
