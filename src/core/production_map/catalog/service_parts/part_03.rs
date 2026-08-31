
fn closed_order_log_rank(log: &ProductionOrderLogEntry) -> u8 {
    if let Some(freeze) = &log.freeze {
        return match freeze.status.trim() {
            "pending" => 1,
            "unfrozen" => 4,
            _ => 2,
        };
    }
    if log.transfer.is_some() {
        return 3;
    }
    match log.action {
        queue_state::ApparatusQueueAction::Start => 0,
        queue_state::ApparatusQueueAction::Pause => 1,
        queue_state::ApparatusQueueAction::Freeze => 2,
        queue_state::ApparatusQueueAction::DetachRoll => 1,
        queue_state::ApparatusQueueAction::Resume => 4,
        queue_state::ApparatusQueueAction::Merge => 3,
        queue_state::ApparatusQueueAction::RollComplete => 3,
        queue_state::ApparatusQueueAction::Complete => 5,
    }
}

fn closed_order_freeze_log_entry(freeze: &OrderFreezeAuditRecord) -> ProductionOrderLogEntry {
    let request = &freeze.request;
    let status = request.status.as_str();
    ProductionOrderLogEntry {
        event_id: format!("order-freeze:{}:{}", request.request_id, status),
        apparatus: request.target_apparatus.clone(),
        order_id: freeze.order_id.clone(),
        stage_node_id: String::new(),
        action: queue_state::ApparatusQueueAction::Freeze,
        from_state: queue_state::ApparatusQueueOrderState::InProgress,
        to_state: if status == "frozen" {
            queue_state::ApparatusQueueOrderState::Frozen
        } else {
            queue_state::ApparatusQueueOrderState::InProgress
        },
        actor_role: freeze.actor.role.clone(),
        actor_ref: freeze.actor.ref_.clone(),
        actor_display_name: freeze.actor.display_name.clone(),
        created_at_unix: freeze.occurred_at_unix,
        completed_with_issue: false,
        issue_note: String::new(),
        transfer: None,
        freeze: Some(ProductionOrderFreezeDetails {
            request_id: request.request_id.clone(),
            status: status.to_string(),
            target_session_id: request.target_session_id.clone(),
            target_apparatus: request.target_apparatus.clone(),
            target_worker_role: request.target_worker_role.clone(),
            target_worker_ref: request.target_worker_ref.clone(),
            target_worker_display_name: request.target_worker_display_name.clone(),
            requested_at_unix: request.requested_at_unix,
            transitioned_at_unix: request.transitioned_at_unix,
        }),
    }
}

fn insert_apparatus_id(target: &mut BTreeSet<ApparatusId>, value: &str) {
    if let Ok(apparatus_id) = ApparatusId::new(value.trim().to_string()) {
        target.insert(apparatus_id);
    }
}

fn effective_apparatus_id(node: &ProductionMapNode) -> Option<ApparatusId> {
    node.canonical_apparatus_id()
}

fn move_allowed_by_id(
    map: &ProductionMapDefinition,
    from_id: &ApparatusId,
    to_id: &ApparatusId,
) -> bool {
    if from_id == to_id {
        return false;
    }
    let source_nodes = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && effective_apparatus_id(node).is_some_and(|id| id == *from_id)
        })
        .collect::<Vec<_>>();
    if source_nodes.is_empty() {
        return false;
    }
    let source_groups = source_nodes
        .iter()
        .map(|node| node.alternative_group_id.trim())
        .filter(|group_id| !group_id.is_empty())
        .collect::<BTreeSet<_>>();
    if source_groups.is_empty() {
        return true;
    }
    map.nodes.iter().any(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && source_groups.contains(node.alternative_group_id.trim())
            && node.base_apparatus_id().is_some_and(|id| id == *to_id)
    })
}

fn reassign_alternative_apparatus_assignment_by_id(
    map: &mut ProductionMapDefinition,
    from: &ApparatusId,
    to: &ApparatusId,
) -> bool {
    let groups = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && !node.alternative_group_id.trim().is_empty()
                && effective_apparatus_id(node).is_some_and(|id| id == *from)
        })
        .map(|node| node.alternative_group_id.trim().to_string())
        .collect::<BTreeSet<_>>();
    if groups.is_empty() {
        return false;
    }
    let target_title = map
        .nodes
        .iter()
        .find(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && groups.contains(node.alternative_group_id.trim())
                && node.base_apparatus_id().is_some_and(|id| id == *to)
        })
        .map(|node| node.title.trim().to_string());
    let mut changed = false;
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && groups.contains(node.alternative_group_id.trim())
        {
            node.set_alternative_assigned_apparatus_id(to);
            if let Some(title) = &target_title {
                node.alternative_assigned_title = title.clone();
            }
            changed = true;
        }
    }
    changed
}

fn reassign_apparatus_nodes_by_id(
    map: &mut ProductionMapDefinition,
    from: &ApparatusId,
    to: &ApparatusId,
) -> bool {
    let mut changed = false;
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && node.alternative_group_id.trim().is_empty()
            && effective_apparatus_id(node).is_some_and(|id| id == *from)
        {
            node.set_apparatus_id(to);
            changed = true;
        }
    }
    changed
}
