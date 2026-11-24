fn order_watermarks(
    snapshot: &ProductionMapLiveSnapshot,
    map: &ProductionMapDefinition,
    state: &str,
) -> Vec<serde_json::Value> {
    use crate::core::production_map::chain::linear_work_stages;
    let id = &map.id;
    let descriptor =
        |apparatus: &str, kind: &str| serde_json::json!({"apparatus_id":apparatus,"kind":kind});
    if matches!(
        state,
        "pending" | "completed" | "print_preflight" | "print_preflight_passed"
    ) {
        return Vec::new();
    }
    let stages = linear_work_stages(map);
    if state == "waiting_next_stage" {
        let mut completed_seen = false;
        for stage in &stages {
            let Some(apparatus) = stage.apparatus_id.as_deref() else {
                continue;
            };
            let Some(node) = map.nodes.iter().find(|node| node.id == stage.node_id) else {
                continue;
            };
            let station = if !node.alternative_group_id.trim().is_empty()
                || node.alternative_assigned_apparatus_id.trim().is_empty()
            {
                node.apparatus_id.trim()
            } else {
                node.alternative_assigned_apparatus_id.trim()
            };
            let current = snapshot
                .stage_states
                .get(id)
                .and_then(|states| states.get(&stage.node_id))
                .filter(|state| !state.trim().is_empty())
                .map(String::as_str)
                .unwrap_or_else(|| queue_state(snapshot, station, id));
            if current == "completed" {
                completed_seen = true;
            } else if completed_seen && current == "pending" {
                return vec![descriptor(apparatus, "waiting")];
            }
        }
        return vec![descriptor("", "waiting")];
    }
    let mut seen = BTreeSet::new();
    let apparatuses = stages
        .iter()
        .filter_map(|stage| stage.apparatus_id.as_deref())
        .filter(|apparatus| seen.insert(*apparatus))
        .collect::<Vec<_>>();
    if state == "in_progress" {
        return apparatuses
            .into_iter()
            .filter(|apparatus| queue_state(snapshot, apparatus, id) == "in_progress")
            .map(|apparatus| descriptor(apparatus, "active"))
            .collect();
    }
    let (target, kind) = match state {
        "paused" => ("paused", "paused"),
        "frozen" => ("frozen", "frozen"),
        "issue" => ("frozen", "issue"),
        _ => return Vec::new(),
    };
    let matching = apparatuses
        .iter()
        .copied()
        .filter(|apparatus| queue_state(snapshot, apparatus, id) == target)
        .collect::<Vec<_>>();
    if !matching.is_empty() {
        return matching
            .into_iter()
            .map(|apparatus| descriptor(apparatus, kind))
            .collect();
    }
    let fallback = apparatuses
        .iter()
        .copied()
        .find(|apparatus| queue_state(snapshot, apparatus, id) != "completed")
        .or_else(|| apparatuses.first().copied())
        .unwrap_or("");
    vec![descriptor(fallback, kind)]
}
