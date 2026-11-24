fn transfer_move_allowed_by_id(map: &ProductionMapDefinition, from_id: &str, to_id: &str) -> bool {
    let (Some(from_id), Some(to_id)) =
        (canonical_transfer_id(from_id), canonical_transfer_id(to_id))
    else {
        return false;
    };
    if from_id == to_id {
        return false;
    }
    let source_nodes = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && effective_transfer_apparatus_id(node) == from_id
        })
        .collect::<Vec<_>>();
    if source_nodes.is_empty() {
        return false;
    }
    let source_groups = source_nodes
        .iter()
        .map(|node| node.alternative_group_id.trim())
        .filter(|group_id| !group_id.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    if source_groups.is_empty() {
        return true;
    }
    map.nodes.iter().any(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && source_groups.contains(node.alternative_group_id.trim())
            && effective_transfer_apparatus_id(node) == to_id
    })
}

fn reassign_alternative_apparatus_assignment_by_id(
    map: &mut ProductionMapDefinition,
    from_id: &str,
    to_id: &str,
    target_display: &str,
) -> bool {
    let groups = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && !node.alternative_group_id.trim().is_empty()
                && effective_transfer_apparatus_id(node) == from_id
        })
        .map(|node| node.alternative_group_id.trim().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    if groups.is_empty()
        || !map.nodes.iter().any(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && groups.contains(node.alternative_group_id.trim())
                && node.apparatus_id.trim() == to_id
        })
    {
        return false;
    }
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && groups.contains(node.alternative_group_id.trim())
        {
            node.alternative_assigned_apparatus_id = to_id.to_string();
            node.alternative_assigned_title = target_display.to_string();
        }
    }
    true
}


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
