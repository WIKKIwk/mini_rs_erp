fn stages_for_node(map: &ProductionMapDefinition, node: &ProductionMapNode) -> Vec<ChainStage> {
    let identity = if node.kind == ProductionMapNodeKind::Apparatus {
        let Some(identity) = canonical_apparatus_identity(node) else {
            return Vec::new();
        };
        identity
    } else {
        station_identity(node)
    };
    if !is_unassigned_alternative_apparatus(node) {
        return vec![ChainStage {
            node_id: node.id.clone(),
            apparatus_id: (node.kind == ProductionMapNodeKind::Apparatus).then_some(identity),
            station_title: display_title(node),
        }];
    }
    let group_id = node.alternative_group_id.trim();
    map.nodes
        .iter()
        .filter(|candidate| {
            candidate.kind == ProductionMapNodeKind::Apparatus
                && candidate.alternative_group_id.trim() == group_id
                && ApparatusId::new(candidate.apparatus_id.trim().to_string()).is_ok()
        })
        .map(|candidate| ChainStage {
            node_id: candidate.id.clone(),
            apparatus_id: Some(candidate.apparatus_id.trim().to_string()),
            station_title: display_title(candidate),
        })
        .collect()
}

fn display_title(node: &ProductionMapNode) -> String {
    let assigned = node.alternative_assigned_title.trim();
    if node.kind == ProductionMapNodeKind::Apparatus
        && node.alternative_group_id.trim().is_empty()
        && !assigned.is_empty()
    {
        assigned.to_string()
    } else {
        node.title.trim().to_string()
    }
}

impl ChainStage {
    fn identity(&self) -> String {
        self.apparatus_id
            .as_deref()
            .map(str::to_string)
            .unwrap_or_else(|| task_stage_identity(&self.node_id))
    }

    fn occurrence_identity(&self) -> String {
        self.node_id.trim().to_string()
    }
}

fn chain_stage_matches_station(stage: &ChainStage, station_id: &str) -> bool {
    if let Some(apparatus_id) = stage.apparatus_id.as_deref() {
        return super::types::apparatus_ids_match(apparatus_id, station_id);
    }
    stage.identity() == station_id.trim()
}

fn adjacent_physical_stages_for_node(
    map: &ProductionMapDefinition,
    stage_node_id: &str,
    previous: bool,
) -> Vec<ChainStage> {
    adjacent_physical_stages_for_node_with_stages(
        map,
        stage_node_id,
        previous,
        linear_work_stages(map),
    )
}

fn collect_previous_stage_ids(
    start_id: &str,
    map: &ProductionMapDefinition,
    physical_stage_node_ids: &BTreeSet<String>,
    found: &mut Vec<String>,
    seen_ids: &mut BTreeSet<String>,
) {
    let mut queue = VecDeque::<&str>::new();
    let mut visited = BTreeSet::<String>::new();
    queue.extend(route_predecessors(map, start_id));
    while let Some(node_id) = queue.pop_front() {
        if !visited.insert(node_id.to_string()) {
            continue;
        }
        let Some(node) = map.nodes.iter().find(|node| node.id == node_id) else {
            continue;
        };
        if node.kind == ProductionMapNodeKind::Start {
            continue;
        }
        if node.kind == ProductionMapNodeKind::Apparatus
            && physical_stage_node_ids.contains(node.id.as_str())
        {
            let Some(identity) = canonical_apparatus_identity(node) else {
                continue;
            };
            if !identity.is_empty() && seen_ids.insert(identity.to_ascii_lowercase()) {
                found.push(identity);
            }
            continue;
        }
        queue.extend(route_predecessors(map, node_id));
    }
}

fn collect_next_stage_ids(
    start_id: &str,
    map: &ProductionMapDefinition,
    physical_stage_node_ids: &BTreeSet<String>,
    found: &mut Vec<String>,
    seen_ids: &mut BTreeSet<String>,
) {
    let mut queue = VecDeque::<&str>::new();
    let mut visited = BTreeSet::<String>::new();
    queue.extend(route_successors(map, start_id));
    while let Some(node_id) = queue.pop_front() {
        if !visited.insert(node_id.to_string()) {
            continue;
        }
        let Some(node) = map.nodes.iter().find(|node| node.id == node_id) else {
            continue;
        };
        if node.kind == ProductionMapNodeKind::End {
            continue;
        }
        if node.kind == ProductionMapNodeKind::Apparatus
            && physical_stage_node_ids.contains(node.id.as_str())
        {
            let Some(identity) = canonical_apparatus_identity(node) else {
                continue;
            };
            if !identity.is_empty() && seen_ids.insert(identity.to_ascii_lowercase()) {
                found.push(identity);
            }
            continue;
        }
        queue.extend(route_successors(map, node_id));
    }
}

fn reachable_node_ids(map: &ProductionMapDefinition) -> Vec<String> {
    let Some(start_id) = map
        .nodes
        .iter()
        .find(|node| node.kind == ProductionMapNodeKind::Start)
        .map(|node| node.id.as_str())
    else {
        return Vec::new();
    };
    let mut queue = VecDeque::from([start_id]);
    let mut visited = BTreeSet::new();
    let mut result = Vec::new();
    while let Some(node_id) = queue.pop_front() {
        if !visited.insert(node_id.to_string()) {
            continue;
        }
        if map.nodes.iter().any(|node| node.id == node_id) {
            result.push(node_id.to_string());
            queue.extend(route_successors(map, node_id));
        }
    }
    result
}

fn route_successors<'a>(map: &'a ProductionMapDefinition, node_id: &str) -> Vec<&'a str> {
    let Some(node) = map.nodes.iter().find(|node| node.id == node_id) else {
        return Vec::new();
    };
    let normalized_id = node_id.trim();
    let aliases = route_node_has_aliases(map, normalized_id);
    let matches = |candidate: &str| {
        let candidate = candidate.trim();
        !normalized_id.is_empty()
            && !candidate.is_empty()
            && (candidate == normalized_id
                || (aliases && stage_node_ids_match_for_map(map, candidate, normalized_id)))
    };
    map.edges
        .iter()
        .filter(|edge| {
            (edge.from == node_id || matches(&edge.from))
                && !matches(&edge.to)
                && route_edge_allowed(node, edge)
        })
        .map(|edge| edge.to.as_str())
        .collect()
}

fn route_predecessors<'a>(map: &'a ProductionMapDefinition, node_id: &str) -> Vec<&'a str> {
    let normalized_id = node_id.trim();
    let aliases = route_node_has_aliases(map, normalized_id);
    let matches = |candidate: &str| {
        let candidate = candidate.trim();
        !normalized_id.is_empty()
            && !candidate.is_empty()
            && (candidate == normalized_id
                || (aliases && stage_node_ids_match_for_map(map, candidate, normalized_id)))
    };
    map.edges
        .iter()
        .filter(|edge| {
            (edge.to == node_id || matches(&edge.to))
                && !matches(&edge.from)
                && map
                    .nodes
                    .iter()
                    .find(|node| node.id == edge.from)
                    .is_some_and(|node| route_edge_allowed(node, edge))
        })
        .map(|edge| edge.from.as_str())
        .collect()
}

/// Ordinary occurrences can match only their own normalized node ID. Determine
/// once whether this occurrence can alias, instead of looking up each unrelated
/// edge's node throughout every graph traversal.
fn route_node_has_aliases(map: &ProductionMapDefinition, normalized_id: &str) -> bool {
    !normalized_id.is_empty()
        && map.nodes.iter().find(|node| node.id.trim() == normalized_id).is_some_and(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && !node.alternative_group_id.trim().is_empty()
                && canonical_apparatus_identity(node).is_some()
        })
}

fn route_edge_allowed(node: &ProductionMapNode, edge: &ProductionMapEdge) -> bool {
    node.kind != ProductionMapNodeKind::Condition
        || matches!(normalize_branch(&edge.branch).as_str(), "true" | "false")
}

fn node_by_id(map: &ProductionMapDefinition) -> BTreeMap<&str, &ProductionMapNode> {
    map.nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect()
}
