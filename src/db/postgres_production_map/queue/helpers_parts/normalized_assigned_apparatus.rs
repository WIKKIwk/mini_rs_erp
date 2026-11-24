fn normalized_assigned_apparatus(
    event: &ApparatusQueueActionEvent,
) -> Result<Vec<String>, ProductionMapError> {
    event
        .assigned_apparatus
        .iter()
        .map(|value| {
            ApparatusId::new(value.trim().to_string())
                .map(|id| id.to_string())
                .map_err(|_| ProductionMapError::ScheduleInputInvalid)
        })
        .collect::<Result<Vec<_>, _>>()
}

#[derive(Clone, Copy)]
struct StoredQueueEventIdentity<'a> {
    apparatus: Option<&'a str>,
    order_id: &'a str,
    stage_node_id: &'a str,
    action: &'a str,
    from_state: &'a str,
    to_state: &'a str,
    policy: &'a str,
    actor_role: &'a str,
    actor_ref: &'a str,
    actor_display_name: &'a str,
    assigned_apparatus: &'a serde_json::Value,
    payload: &'a serde_json::Value,
}

fn queue_event_identity_matches(
    existing: &StoredQueueEventIdentity<'_>,
    apparatus_id: &ApparatusId,
    event: &ApparatusQueueActionEvent,
    assigned_apparatus: &[String],
) -> Result<bool, ProductionMapError> {
    let assigned_apparatus =
        serde_json::to_value(assigned_apparatus).map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(existing.apparatus == Some(apparatus_id.as_str())
        && existing.order_id.trim() == event.order_id.trim()
        && existing.stage_node_id.trim() == event.stage_node_id.trim()
        && existing.action == event.action.as_str()
        && existing.from_state == event.from_state.as_str()
        && existing.to_state == event.to_state.as_str()
        && existing.policy == event.policy.as_str()
        && existing.actor_role.trim() == event.actor.role.trim()
        && existing.actor_ref.trim() == event.actor.ref_.trim()
        && existing.actor_display_name.trim() == event.actor.display_name.trim()
        && existing.assigned_apparatus == &assigned_apparatus
        && existing.payload == &event.payload_json)
}
