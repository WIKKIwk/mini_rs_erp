use std::collections::BTreeMap;

use super::*;
use crate::core::production_map::{
    OrderRunSession, OrderRunStatus, ProductionMapEdge, ProductionMapNode, ProductionMapNodeKind,
};

fn node(id: &str, kind: ProductionMapNodeKind, title: &str) -> ProductionMapNode {
    let is_apparatus = matches!(kind, ProductionMapNodeKind::Apparatus);
    ProductionMapNode {
        id: id.to_string(),
        kind,
        title: title.to_string(),
        apparatus_id: if is_apparatus {
            format!("apparatus:test:{id}")
        } else {
            String::new()
        },
        formula: None,
        role_code: String::new(),
        item_code: String::new(),
        qty_formula: String::new(),
        from_location: String::new(),
        to_location: String::new(),
        alternative_group_id: String::new(),
        alternative_group_label: String::new(),
        alternative_assigned_title: String::new(),
        alternative_assigned_apparatus_id: String::new(),
        rezka_kadr_count: None,
        rezka_frame_groups: Vec::new(),
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
    assigned_id: &str,
) -> ProductionMapNode {
    ProductionMapNode {
        alternative_assigned_title: assigned_title.to_string(),
        alternative_assigned_apparatus_id: assigned_id.to_string(),
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
        customer_name: String::new(),
        image_id: String::new(),
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
fn map_has_work_stage_matches_canonical_and_virtual_ids() {
    let map = hotlunch_map();
    assert!(map_has_work_stage_for_station(&map, "task:lamin"));
    assert!(map_has_work_stage_for_station(
        &map,
        "apparatus:test:pechat"
    ));
    assert!(!map_has_work_stage_for_station(&map, "Laminatsiya"));
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
    assert_eq!(stages[1].node_id, "lamin");
    assert_eq!(stages[1].apparatus_id, None);
    assert_eq!(
        next_work_stage_station(&hotlunch_map(), "apparatus:test:pechat"),
        Some("apparatus:test:rezka".to_string())
    );
    assert_eq!(
        previous_work_stage_station(&hotlunch_map(), "apparatus:test:rezka"),
        Some("apparatus:test:pechat".to_string())
    );
    assert_eq!(
        next_work_stage_station(&hotlunch_map(), "task:lamin"),
        Some("apparatus:test:rezka".to_string())
    );
    assert_eq!(
        previous_work_stage_station(&hotlunch_map(), "task:lamin"),
        Some("apparatus:test:pechat".to_string())
    );
    assert!(!is_final_work_stage_station(
        &hotlunch_map(),
        "apparatus:test:pechat"
    ));
    assert!(is_final_work_stage_station(
        &hotlunch_map(),
        "apparatus:test:rezka"
    ));
    assert!(!is_final_work_stage_station(
        &hotlunch_map(),
        "Noma'lum aparat"
    ));
}

#[test]
fn repeated_apparatus_occurrences_remain_distinct_stages() {
    const REZKA_ID: &str = "apparatus:default:asset-010";
    let mut map = hotlunch_map();
    map.nodes = vec![
        node("start", ProductionMapNodeKind::Start, "Start"),
        node("bosma", ProductionMapNodeKind::Apparatus, "Bosma"),
        node(
            "rezka_before_lamination",
            ProductionMapNodeKind::Apparatus,
            "Rezka",
        ),
        node(
            "lamination",
            ProductionMapNodeKind::Apparatus,
            "Laminatsiya",
        ),
        node(
            "rezka_final",
            ProductionMapNodeKind::Apparatus,
            "Rezka",
        ),
        node("end", ProductionMapNodeKind::End, "End"),
    ];
    for node in map
        .nodes
        .iter_mut()
        .filter(|node| node.id.starts_with("rezka_"))
    {
        node.apparatus_id = REZKA_ID.to_string();
    }
    map.edges = vec![
        ProductionMapEdge {
            from: "start".to_string(),
            to: "bosma".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "bosma".to_string(),
            to: "rezka_before_lamination".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "rezka_before_lamination".to_string(),
            to: "lamination".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "lamination".to_string(),
            to: "rezka_final".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "rezka_final".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
    ];

    let stages = linear_work_stages(&map);
    assert_eq!(
        stages
            .iter()
            .map(|stage| stage.node_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "bosma",
            "rezka_before_lamination",
            "lamination",
            "rezka_final",
        ]
    );
    assert_eq!(
        stages
            .iter()
            .filter_map(|stage| stage.apparatus_id.as_deref())
            .filter(|apparatus| *apparatus == REZKA_ID)
            .count(),
        2
    );
    assert_eq!(
        next_work_stage_for_node(&map, "rezka_before_lamination")
            .map(|stage| stage.node_id),
        Some("lamination".to_string())
    );
    assert_eq!(
        previous_work_stage_for_node(&map, "rezka_final").map(|stage| stage.node_id),
        Some("lamination".to_string())
    );
    assert!(!is_final_work_stage_node(
        &map,
        "rezka_before_lamination"
    ));
    assert!(is_final_work_stage_node(&map, "rezka_final"));
}

#[test]
fn qolip_bearing_canonical_chain_preserves_identity_through_laminatsiya_task() {
    const PECHAT_ID: &str = "apparatus:default:bosma_7";
    const LAMINATSIYA_TASK_ID: &str = "task:lamin";
    const REZKA_ID: &str = "apparatus:default:asset-010";

    let mut map = hotlunch_map();
    map.id = "zakaz-qolip-chain".to_string();
    map.nodes
        .iter_mut()
        .find(|node| node.id == "pechat")
        .expect("pechat stage")
        .apparatus_id = PECHAT_ID.to_string();
    map.nodes
        .iter_mut()
        .find(|node| node.id == "rezka")
        .expect("rezka stage")
        .apparatus_id = REZKA_ID.to_string();

    let stages = linear_work_stages(&map);
    assert_eq!(
        stages
            .iter()
            .map(|stage| stage.apparatus_id.as_deref())
            .collect::<Vec<_>>(),
        vec![Some(PECHAT_ID), None, Some(REZKA_ID)]
    );
    assert_eq!(stages[1].node_id, "lamin");
    assert_eq!(stages[0].apparatus_id.as_deref(), Some(PECHAT_ID));
    assert!(map_has_work_stage_for_station(&map, LAMINATSIYA_TASK_ID));

    assert_eq!(
        next_work_stage_station(&map, PECHAT_ID),
        Some(REZKA_ID.to_string())
    );
    assert_eq!(
        previous_work_stage_station(&map, REZKA_ID),
        Some(PECHAT_ID.to_string())
    );
    assert_eq!(
        next_work_stage_station(&map, LAMINATSIYA_TASK_ID),
        Some(REZKA_ID.to_string())
    );
    assert_eq!(
        previous_work_stage_station(&map, LAMINATSIYA_TASK_ID),
        Some(PECHAT_ID.to_string())
    );

    let session = OrderRunSession {
        session_id: "session-qolip-chain".to_string(),
        apparatus: PECHAT_ID.to_string(),
        order_id: map.id.clone(),
        stage_node_id: String::new(),
        status: OrderRunStatus::Active,
        worker_role: "operator".to_string(),
        worker_ref: "worker-qolip-chain".to_string(),
        worker_display_name: "Qolip operator".to_string(),
        started_at_unix: 1,
        updated_at_unix: 1,
        payload_json: serde_json::json!({
            "qolip_code": "QOLIP-CHAIN-001",
            "qolip_codes": ["QOLIP-CHAIN-001"],
        }),
    };
    let downstream_payload =
        crate::core::production_map::service_progress_support::preserve_qolip_lineage(
            &session,
            serde_json::json!({
                "next_apparatus": REZKA_ID,
            }),
        );
    assert_eq!(
        downstream_payload["next_apparatus"],
        serde_json::json!(REZKA_ID)
    );
    assert_eq!(
        downstream_payload["qolip_code"],
        serde_json::json!("QOLIP-CHAIN-001")
    );
    assert_eq!(
        downstream_payload["qolip_codes"],
        serde_json::json!(["QOLIP-CHAIN-001"])
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
        "apparatus:test:pechat_7"
    ));
    assert!(map_has_work_stage_for_station(
        &map,
        "apparatus:test:pechat_8"
    ));
    assert_eq!(
        next_work_stage_station(&map, "apparatus:test:pechat_7"),
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
    assert!(map_has_work_stage_for_station(
        &map,
        "apparatus:test:lamin_1"
    ));
    assert!(map_has_work_stage_for_station(
        &map,
        "apparatus:test:lamin_2"
    ));
    assert_eq!(
        previous_work_stage_station(&map, "apparatus:test:lamin_1"),
        Some("apparatus:test:pechat".to_string())
    );
    assert_eq!(
        previous_work_stage_station(&map, "apparatus:test:lamin_2"),
        Some("apparatus:test:pechat".to_string())
    );
    assert_eq!(
        next_work_stage_station(&map, "apparatus:test:pechat"),
        Some("apparatus:test:lamin_1".to_string())
    );
}

#[test]
fn next_work_stage_uses_assigned_apparatus_ids_across_branch_alternatives() {
    let mut map = hotlunch_map();
    map.nodes = vec![
        node("start", ProductionMapNodeKind::Start, "Start"),
        node("order", ProductionMapNodeKind::Task, "Paynet"),
        assigned_node(
            "pechat_7",
            ProductionMapNodeKind::Apparatus,
            "7 ta rangli pechat",
            "8 ta rangli pechat",
            "apparatus:test:bosma_8",
        ),
        assigned_node(
            "pechat_8",
            ProductionMapNodeKind::Apparatus,
            "8 ta rangli pechat",
            "8 ta rangli pechat",
            "apparatus:test:bosma_8",
        ),
        assigned_node(
            "lamin_1",
            ProductionMapNodeKind::Apparatus,
            "Laminatsiya 1",
            "Laminatsiya 1",
            "apparatus:test:lamin_1",
        ),
        assigned_node(
            "lamin_2",
            ProductionMapNodeKind::Apparatus,
            "Laminatsiya 2",
            "Laminatsiya 1",
            "apparatus:test:lamin_1",
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

    assert_eq!(
        next_work_stage_station(&map, "apparatus:test:bosma_8"),
        Some("apparatus:test:lamin_1".to_string())
    );
}

#[test]
fn later_stage_waits_for_previous_completion() {
    let map = hotlunch_map();
    let mut states = BTreeMap::new();
    assert!(order_ready_for_station(
        &map,
        "zakaz-hot",
        "apparatus:test:pechat",
        &states,
        &[],
    ));
    assert!(!order_ready_for_station(
        &map,
        "zakaz-hot",
        "task:lamin",
        &states,
        &[],
    ));
    states.insert(
        "apparatus:test:pechat".to_string(),
        BTreeMap::from([("zakaz-hot".to_string(), "completed".to_string())]),
    );
    assert!(order_ready_for_station(
        &map,
        "zakaz-hot",
        "task:lamin",
        &states,
        &[],
    ));
    assert!(order_ready_for_station(
        &map,
        "zakaz-hot",
        "apparatus:test:rezka",
        &states,
        &[],
    ));
}

#[test]
fn branch_true_false_and_join_share_one_stage_traversal() {
    let mut map = hotlunch_map();
    map.nodes = vec![
        node("start", ProductionMapNodeKind::Start, "Start"),
        node("pechat", ProductionMapNodeKind::Apparatus, "Pechat"),
        node("condition", ProductionMapNodeKind::Condition, "Condition"),
        node("true_task", ProductionMapNodeKind::Task, "True task"),
        node("false_task", ProductionMapNodeKind::Task, "False task"),
        node("true_stage", ProductionMapNodeKind::Apparatus, "True stage"),
        node(
            "false_stage",
            ProductionMapNodeKind::Apparatus,
            "False stage",
        ),
        node("join", ProductionMapNodeKind::Apparatus, "Join"),
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
            to: "condition".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "condition".to_string(),
            to: "true_task".to_string(),
            branch: "ha".to_string(),
        },
        ProductionMapEdge {
            from: "condition".to_string(),
            to: "false_task".to_string(),
            branch: "no".to_string(),
        },
        ProductionMapEdge {
            from: "true_task".to_string(),
            to: "true_stage".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "false_task".to_string(),
            to: "false_stage".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "true_stage".to_string(),
            to: "join".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "false_stage".to_string(),
            to: "join".to_string(),
            branch: String::new(),
        },
        ProductionMapEdge {
            from: "join".to_string(),
            to: "end".to_string(),
            branch: String::new(),
        },
    ];

    let pechat_id = "apparatus:test:pechat".to_string();
    let true_id = "apparatus:test:true_stage".to_string();
    let false_id = "apparatus:test:false_stage".to_string();
    let join_id = "apparatus:test:join".to_string();
    assert_eq!(
        next_work_stage_stations(&map, &pechat_id),
        vec![true_id.clone(), false_id.clone()]
    );
    assert_eq!(
        previous_work_stage_stations(&map, &join_id),
        vec![true_id.clone(), false_id.clone()]
    );
    assert_eq!(
        next_work_stage_stations(&map, &true_id),
        vec![join_id.clone()]
    );
    assert_eq!(
        next_work_stage_stations(&map, &false_id),
        vec![join_id.clone()]
    );
    assert_eq!(
        previous_work_stage_stations(&map, &join_id),
        next_work_stage_stations(&map, &pechat_id)
    );
    assert_eq!(
        physical_work_stage_ids(&map),
        Some(vec![pechat_id, true_id, false_id, join_id.clone()])
    );
    assert!(!is_final_work_stage_station(&map, "task:true_task"));
    assert!(is_final_work_stage_station(&map, &join_id));
}
