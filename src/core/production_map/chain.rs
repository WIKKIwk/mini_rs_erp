use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::queue_state::{self, ApparatusQueueOrderState};
use super::{ProductionMapDefinition, ProductionMapEdge, ProductionMapNode, ProductionMapNodeKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainStage {
    pub node_id: String,
    pub station_title: String,
}

pub fn linear_work_stages(map: &ProductionMapDefinition) -> Vec<ChainStage> {
    let node_by_id: BTreeMap<&str, &ProductionMapNode> = map
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut outgoing = BTreeMap::<&str, Vec<&ProductionMapEdge>>::new();
    for edge in &map.edges {
        outgoing.entry(edge.from.as_str()).or_default().push(edge);
    }
    let Some(mut current_id) = map
        .nodes
        .iter()
        .find(|node| node.kind == ProductionMapNodeKind::Start)
        .map(|node| node.id.as_str())
    else {
        return Vec::new();
    };
    let mut stages = Vec::new();
    let mut visited = BTreeSet::new();
    let mut seen_apparatus = false;
    while visited.insert(current_id.to_string()) {
        let Some(node) = node_by_id.get(current_id) else {
            break;
        };
        if node.kind == ProductionMapNodeKind::End {
            break;
        }
        if is_work_stage(node, seen_apparatus) {
            let title = station_title(node);
            if !title.is_empty() {
                if node.kind == ProductionMapNodeKind::Apparatus {
                    seen_apparatus = true;
                }
                stages.push(ChainStage {
                    node_id: node.id.clone(),
                    station_title: title.to_string(),
                });
            }
        } else if node.kind == ProductionMapNodeKind::Apparatus {
            seen_apparatus = true;
        }
        let edges = outgoing.get(current_id).cloned().unwrap_or_default();
        if node.kind == ProductionMapNodeKind::Condition {
            let branch = "true";
            let Some(next) = edges
                .into_iter()
                .find(|edge| normalize_branch(&edge.branch) == branch)
            else {
                break;
            };
            current_id = next.to.as_str();
        } else {
            let Some(next) = edges.first() else {
                break;
            };
            current_id = next.to.as_str();
        }
    }
    stages
}

pub fn previous_work_stage_station(map: &ProductionMapDefinition, station: &str) -> Option<String> {
    let stages = linear_work_stages(map);
    let index = stages
        .iter()
        .position(|stage| queue_state::apparatus_titles_match(&stage.station_title, station))?;
    if index == 0 {
        None
    } else {
        Some(stages[index - 1].station_title.clone())
    }
}

pub fn next_work_stage_station(map: &ProductionMapDefinition, station: &str) -> Option<String> {
    let node_by_id: BTreeMap<&str, &ProductionMapNode> = map
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut outgoing = BTreeMap::<&str, Vec<&ProductionMapEdge>>::new();
    for edge in &map.edges {
        outgoing.entry(edge.from.as_str()).or_default().push(edge);
    }
    let mut found = Vec::<String>::new();
    let mut seen_titles = BTreeSet::<String>::new();
    for node in &map.nodes {
        if !is_station_node(node) || !station_matches(node, station) {
            continue;
        }
        collect_next_station_titles(
            node.id.as_str(),
            &node_by_id,
            &outgoing,
            &mut found,
            &mut seen_titles,
        );
    }
    found.into_iter().next()
}

pub fn order_ready_for_station(
    map: &ProductionMapDefinition,
    order_id: &str,
    station: &str,
    all_states: &BTreeMap<String, BTreeMap<String, String>>,
    known_keys: &[String],
) -> bool {
    let Some(previous) = previous_work_stage_station(map, station) else {
        return true;
    };
    queue_state_for_station(&previous, order_id, all_states, known_keys)
        == ApparatusQueueOrderState::Completed
}

pub fn map_has_work_stage_for_station(map: &ProductionMapDefinition, station: &str) -> bool {
    linear_work_stages(map)
        .iter()
        .any(|stage| queue_state::apparatus_titles_match(&stage.station_title, station))
}

fn queue_state_for_station(
    station: &str,
    order_id: &str,
    all_states: &BTreeMap<String, BTreeMap<String, String>>,
    known_keys: &[String],
) -> ApparatusQueueOrderState {
    let storage_key = queue_state::resolve_apparatus_storage_key(station, known_keys);
    let states = all_states
        .get(&storage_key)
        .or_else(|| {
            all_states
                .iter()
                .find(|(key, _)| queue_state::apparatus_titles_match(key, station))
                .map(|(_, value)| value)
        })
        .cloned()
        .unwrap_or_default();
    states
        .get(order_id.trim())
        .and_then(|value| ApparatusQueueOrderState::parse(value))
        .unwrap_or(ApparatusQueueOrderState::Pending)
}

fn is_work_stage(node: &ProductionMapNode, seen_apparatus: bool) -> bool {
    match node.kind {
        ProductionMapNodeKind::Apparatus => true,
        // Product/order task nodes come before the first apparatus and are not
        // operator stations. Later task nodes (e.g. laminatsiya) are stations.
        ProductionMapNodeKind::Task => seen_apparatus,
        _ => false,
    }
}

fn is_station_node(node: &ProductionMapNode) -> bool {
    matches!(
        node.kind,
        ProductionMapNodeKind::Apparatus | ProductionMapNodeKind::Task
    )
}

fn station_title(node: &ProductionMapNode) -> &str {
    if node.kind == ProductionMapNodeKind::Apparatus
        && !node.alternative_assigned_title.trim().is_empty()
    {
        node.alternative_assigned_title.trim()
    } else {
        node.title.trim()
    }
}

fn station_matches(node: &ProductionMapNode, station: &str) -> bool {
    queue_state::apparatus_titles_match(node.title.trim(), station)
        || (!node.alternative_assigned_title.trim().is_empty()
            && queue_state::apparatus_titles_match(node.alternative_assigned_title.trim(), station))
}

fn collect_next_station_titles(
    start_id: &str,
    node_by_id: &BTreeMap<&str, &ProductionMapNode>,
    outgoing: &BTreeMap<&str, Vec<&ProductionMapEdge>>,
    found: &mut Vec<String>,
    seen_titles: &mut BTreeSet<String>,
) {
    let mut queue = VecDeque::<&str>::new();
    let mut visited = BTreeSet::<String>::new();
    if let Some(edges) = outgoing.get(start_id) {
        queue.extend(edges.iter().map(|edge| edge.to.as_str()));
    }
    while let Some(node_id) = queue.pop_front() {
        if !visited.insert(node_id.to_string()) {
            continue;
        }
        let Some(node) = node_by_id.get(node_id) else {
            continue;
        };
        if node.kind == ProductionMapNodeKind::End {
            continue;
        }
        if is_station_node(node) {
            let title = station_title(node);
            if !title.is_empty() && seen_titles.insert(title.to_ascii_lowercase()) {
                found.push(title.to_string());
            }
            continue;
        }
        if let Some(edges) = outgoing.get(node_id) {
            queue.extend(edges.iter().map(|edge| edge.to.as_str()));
        }
    }
}

fn normalize_branch(branch: &str) -> String {
    match branch.trim().to_ascii_lowercase().as_str() {
        "ha" | "yes" | "true" | "1" => "true".to_string(),
        "yo'q" | "yoq" | "no" | "false" | "0" => "false".to_string(),
        value => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::production_map::{
        ProductionMapEdge, ProductionMapNode, ProductionMapNodeKind,
    };

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

        assert_eq!(
            next_work_stage_station(&map, "7 ta rangli pechat"),
            Some("Laminatsiya 1".to_string())
        );
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
}
