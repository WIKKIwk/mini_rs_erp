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
    let mut seen_stage_titles = BTreeSet::<String>::new();
    let mut seen_apparatus = false;
    while visited.insert(current_id.to_string()) {
        let Some(node) = node_by_id.get(current_id) else {
            break;
        };
        if node.kind == ProductionMapNodeKind::End {
            break;
        }
        if is_work_stage(node, seen_apparatus) {
            for (node_id, title) in stage_titles_for_node(map, node) {
                if node.kind == ProductionMapNodeKind::Apparatus {
                    seen_apparatus = true;
                }
                if !title.is_empty() && seen_stage_titles.insert(title.to_ascii_lowercase()) {
                    stages.push(ChainStage {
                        node_id,
                        station_title: title,
                    });
                }
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
    let work_stage_node_ids = linear_work_stages(map)
        .into_iter()
        .map(|stage| stage.node_id)
        .collect::<BTreeSet<_>>();
    let node_by_id: BTreeMap<&str, &ProductionMapNode> = map
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut incoming = BTreeMap::<&str, Vec<&ProductionMapEdge>>::new();
    for edge in &map.edges {
        incoming.entry(edge.to.as_str()).or_default().push(edge);
    }
    let mut found = Vec::<String>::new();
    let mut seen_titles = BTreeSet::<String>::new();
    for node in &map.nodes {
        if !is_station_node(node) || !station_matches(node, station) {
            continue;
        }
        collect_previous_station_titles(
            node.id.as_str(),
            &node_by_id,
            &incoming,
            &work_stage_node_ids,
            &mut found,
            &mut seen_titles,
        );
    }
    found.into_iter().next()
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
    let assigned = node.alternative_assigned_title.trim();
    if !assigned.is_empty() {
        return queue_state::apparatus_titles_match(assigned, station);
    }
    if node.kind == ProductionMapNodeKind::Apparatus && !node.alternative_group_id.trim().is_empty()
    {
        return queue_state::apparatus_titles_match(node.title.trim(), station);
    }
    queue_state::apparatus_titles_match(node.title.trim(), station)
}

fn is_unassigned_alternative_apparatus(node: &ProductionMapNode) -> bool {
    node.kind == ProductionMapNodeKind::Apparatus
        && !node.alternative_group_id.trim().is_empty()
        && node.alternative_assigned_title.trim().is_empty()
}

fn stage_titles_for_node(
    map: &ProductionMapDefinition,
    node: &ProductionMapNode,
) -> Vec<(String, String)> {
    if !is_unassigned_alternative_apparatus(node) {
        let title = station_title(node).trim();
        return if title.is_empty() {
            Vec::new()
        } else {
            vec![(node.id.clone(), title.to_string())]
        };
    }
    let group_id = node.alternative_group_id.trim();
    map.nodes
        .iter()
        .filter(|candidate| {
            candidate.kind == ProductionMapNodeKind::Apparatus
                && candidate.alternative_group_id.trim() == group_id
                && candidate.alternative_assigned_title.trim().is_empty()
        })
        .filter_map(|candidate| {
            let title = candidate.title.trim();
            (!title.is_empty()).then(|| (candidate.id.clone(), title.to_string()))
        })
        .collect()
}

fn unassigned_alternative_stage_title(node: &ProductionMapNode) -> &str {
    let label = node.alternative_group_label.trim();
    if label.is_empty() {
        node.title.trim()
    } else {
        label
    }
}

fn collect_previous_station_titles(
    start_id: &str,
    node_by_id: &BTreeMap<&str, &ProductionMapNode>,
    incoming: &BTreeMap<&str, Vec<&ProductionMapEdge>>,
    work_stage_node_ids: &BTreeSet<String>,
    found: &mut Vec<String>,
    seen_titles: &mut BTreeSet<String>,
) {
    let mut queue = VecDeque::<&str>::new();
    let mut visited = BTreeSet::<String>::new();
    if let Some(edges) = incoming.get(start_id) {
        queue.extend(edges.iter().map(|edge| edge.from.as_str()));
    }
    while let Some(node_id) = queue.pop_front() {
        if !visited.insert(node_id.to_string()) {
            continue;
        }
        let Some(node) = node_by_id.get(node_id) else {
            continue;
        };
        if node.kind == ProductionMapNodeKind::Start {
            continue;
        }
        if is_station_node(node) && work_stage_node_ids.contains(node.id.as_str()) {
            let title = if is_unassigned_alternative_apparatus(node) {
                unassigned_alternative_stage_title(node)
            } else {
                station_title(node)
            };
            if !title.is_empty() && seen_titles.insert(title.to_ascii_lowercase()) {
                found.push(title.to_string());
            }
            continue;
        }
        if let Some(edges) = incoming.get(node_id) {
            queue.extend(edges.iter().map(|edge| edge.from.as_str()));
        }
    }
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
        if is_unassigned_alternative_apparatus(node) {
            let title = unassigned_alternative_stage_title(node);
            if !title.is_empty() && seen_titles.insert(title.to_ascii_lowercase()) {
                found.push(title.to_string());
            }
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
mod tests;
