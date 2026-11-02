use super::*;

/// Keep every producer occurrence addressable by historical QR records. The
/// target is another candidate of the same logical stage, selected exclusively
/// for the remaining printing work.
pub(super) fn select_target(
    map: &mut ProductionMapDefinition,
    session: &OrderRunSession,
    from: &str,
    to: &str,
    display: &str,
) -> Result<String, ProductionMapError> {
    let candidates: Vec<_> = super::super::chain::linear_work_stages(map)
        .into_iter()
        .filter(|stage| stage.apparatus_id.as_deref() == Some(from))
        .filter(|stage| {
            session.stage_node_id.trim().is_empty()
                || super::super::chain::stage_node_ids_match_for_map(
                    map,
                    &stage.node_id,
                    &session.stage_node_id,
                )
        })
        .collect();
    if candidates.len() != 1 {
        return Err(ProductionMapError::MoveNotAllowed);
    }
    let source_index = map
        .nodes
        .iter()
        .position(|node| node.id == candidates[0].node_id)
        .ok_or(ProductionMapError::MoveNotAllowed)?;
    let source = map.nodes[source_index].clone();
    let existing_group = source.alternative_group_id.trim();
    let group = if existing_group.is_empty() {
        let base = format!("print_transfer_{}", source.id);
        let mut group = base.clone();
        let mut suffix = 1;
        while map
            .nodes
            .iter()
            .any(|node| node.alternative_group_id == group)
        {
            group = format!("{base}_{suffix}");
            suffix += 1;
        }
        group
    } else {
        if map
            .nodes
            .iter()
            .filter(|node| node.alternative_group_id == existing_group)
            .any(|node| {
                !node.alternative_assigned_apparatus_id.trim().is_empty()
                    && node.alternative_assigned_apparatus_id.trim() != from
            })
        {
            return Err(ProductionMapError::MoveNotAllowed);
        }
        existing_group.to_string()
    };
    let target_id = map
        .nodes
        .iter()
        .find(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && node.alternative_group_id == group
                && node.apparatus_id.trim() == to
        })
        .map(|node| node.id.clone());
    let target_id = match target_id {
        Some(id) => id,
        None => {
            // User-defined alternatives restrict the eligible machines. Groups
            // created by a failover may grow to support another later failure.
            if !existing_group.is_empty() && !existing_group.starts_with("print_transfer_") {
                return Err(ProductionMapError::MoveNotAllowed);
            }
            let base = format!("{}_transfer", source.id);
            let mut id = base.clone();
            let mut suffix = 1;
            while map.nodes.iter().any(|node| node.id == id) {
                id = format!("{base}_{suffix}");
                suffix += 1;
            }
            let mut target = source.clone();
            target.id = id.clone();
            target.apparatus_id = to.to_string();
            target.title = display.to_string();
            target.alternative_group_id = group.clone();
            target.x += 180.0;
            let edges: Vec<_> = map
                .edges
                .iter()
                .filter_map(|edge| {
                    let mut edge = edge.clone();
                    if edge.from == source.id {
                        edge.from = id.clone();
                    } else if edge.to == source.id {
                        edge.to = id.clone();
                    } else {
                        return None;
                    }
                    Some(edge)
                })
                .collect();
            map.nodes.push(target);
            map.edges.extend(edges);
            id
        }
    };
    map.nodes[source_index].alternative_group_id = group.clone();
    for node in map
        .nodes
        .iter_mut()
        .filter(|node| node.alternative_group_id == group)
    {
        node.alternative_assigned_apparatus_id = to.to_string();
        node.alternative_assigned_title = display.to_string();
    }
    super::super::compiler::compile_map(map)?;
    Ok(target_id)
}
