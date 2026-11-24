pub(super) async fn queue_action_logs_for_orders(
    store: &MemoryProductionMapStore,
    order_ids: &[String],
) -> Result<BTreeMap<String, Vec<ProductionOrderLogEntry>>, ProductionMapError> {
    let order_ids = order_ids
        .iter()
        .map(|order_id| order_id.trim().to_string())
        .filter(|order_id| !order_id.is_empty())
        .collect::<BTreeSet<_>>();
    if order_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let events = store.queue_events.read().await;
    let mut by_order: BTreeMap<String, Vec<ProductionOrderLogEntry>> = BTreeMap::new();
    for (index, event) in events.iter().enumerate() {
        if !order_ids.contains(event.order_id.trim()) {
            continue;
        }
        by_order
            .entry(event.order_id.trim().to_string())
            .or_default()
            .push(production_order_log_entry(
                event,
                index,
                event.actor.display_name.trim().to_string(),
            ));
    }
    Ok(by_order)
}

pub(super) async fn queue_action_logs_for_worker(
    store: &MemoryProductionMapStore,
    worker_refs: &[String],
    _worker_display_name: &str,
    limit: usize,
) -> Result<Vec<ProductionOrderLogEntry>, ProductionMapError> {
    let worker_refs = worker_refs
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<BTreeSet<_>>();
    if worker_refs.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let events = store.queue_events.read().await;
    let mut logs = Vec::new();
    for (index, event) in events.iter().enumerate().rev() {
        let matches_ref = worker_refs.contains(event.actor.ref_.trim());
        if !matches_ref {
            continue;
        }
        logs.push(production_order_log_entry(
            event,
            index,
            actor_display_name(&event.actor),
        ));
        if logs.len() >= limit.min(500) {
            break;
        }
    }
    Ok(logs)
}

fn production_order_log_entry(
    event: &ApparatusQueueActionEvent,
    index: usize,
    actor_display_name: String,
) -> ProductionOrderLogEntry {
    ProductionOrderLogEntry {
        event_id: event.event_id.trim().to_string(),
        apparatus: event.apparatus.trim().to_string(),
        order_id: event.order_id.trim().to_string(),
        stage_node_id: event.stage_node_id.trim().to_string(),
        action: event.action,
        from_state: event.from_state,
        to_state: event.to_state,
        actor_role: event.actor.role.trim().to_string(),
        actor_ref: event.actor.ref_.trim().to_string(),
        actor_display_name,
        created_at_unix: index as i64 + 1,
        completed_with_issue: event
            .payload_json
            .get("completed_with_issue")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        issue_note: json_string_field(&event.payload_json, "issue_note"),
        transfer: None,
        freeze: None,
        zero_output_waste: event.payload_json.get("zero_output_waste").cloned(),
    }
}
