use super::*;

pub(super) fn adjacent_physical_stages_for_node_with_stages(
    map: &ProductionMapDefinition,
    stage_node_id: &str,
    previous: bool,
    stages: Vec<ChainStage>,
) -> Vec<ChainStage> {
    if !stages
        .iter()
        .any(|stage| stage.node_id.trim() == stage_node_id.trim())
    {
        return Vec::new();
    }
    let physical_by_node = stages
        .iter()
        .filter(|stage| stage.apparatus_id.is_some())
        .map(|stage| (stage.node_id.as_str(), stage))
        .collect::<BTreeMap<_, _>>();
    let mut queue = VecDeque::<&str>::new();
    let mut visited = BTreeSet::<String>::new();
    let mut found = Vec::<ChainStage>::new();
    let mut found_nodes = BTreeSet::<String>::new();
    if previous {
        queue.extend(route_predecessors(map, stage_node_id.trim()));
    } else {
        queue.extend(route_successors(map, stage_node_id.trim()));
    }
    while let Some(node_id) = queue.pop_front() {
        if !visited.insert(node_id.to_string()) {
            continue;
        }
        let Some(node) = map.nodes.iter().find(|node| node.id == node_id) else {
            continue;
        };
        if matches!(
            node.kind,
            ProductionMapNodeKind::Start | ProductionMapNodeKind::End
        ) {
            continue;
        }
        if let Some(stage) = physical_by_node.get(node_id) {
            if found_nodes.insert(node_id.to_string()) {
                found.push((*stage).clone());
            }
            continue;
        }
        if previous {
            queue.extend(route_predecessors(map, node_id));
        } else {
            queue.extend(route_successors(map, node_id));
        }
    }
    found
}
