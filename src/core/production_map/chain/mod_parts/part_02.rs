
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
    map.edges
        .iter()
        .filter(|edge| edge.from == node_id && route_edge_allowed(node, edge))
        .map(|edge| edge.to.as_str())
        .collect()
}

fn route_predecessors<'a>(map: &'a ProductionMapDefinition, node_id: &str) -> Vec<&'a str> {
    map.edges
        .iter()
        .filter(|edge| {
            edge.to == node_id
                && map
                    .nodes
                    .iter()
                    .find(|node| node.id == edge.from)
                    .is_some_and(|node| route_edge_allowed(node, edge))
        })
        .map(|edge| edge.from.as_str())
        .collect()
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

fn normalize_branch(branch: &str) -> String {
    match branch.trim().to_ascii_lowercase().as_str() {
        "ha" | "yes" | "true" | "1" => "true".to_string(),
        "yo'q" | "yoq" | "no" | "false" | "0" => "false".to_string(),
        value => value.to_string(),
    }
}

#[cfg(test)]
#[path = "../tests.rs"]
mod tests;
