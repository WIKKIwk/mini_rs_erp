
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainStage {
    /// Stable graph identity for this stage. Alternative candidates retain
    /// their own node IDs even when they share a display label.
    pub node_id: String,
    /// Canonical apparatus identity for apparatus stages. Task stages have no
    /// apparatus ID; their virtual identity is the `task:<node-id>` form.
    pub apparatus_id: Option<String>,
    /// Display/history snapshot only; never used for stage matching.
    pub station_title: String,
}

pub fn linear_work_stages(map: &ProductionMapDefinition) -> Vec<ChainStage> {
    let mut stages = Vec::new();
    let mut seen_stage_ids = BTreeSet::<String>::new();
    let mut seen_apparatus = false;
    let node_by_id = node_by_id(map);
    for current_id in reachable_node_ids(map) {
        let Some(node) = node_by_id.get(current_id.as_str()) else {
            continue;
        };
        if is_work_stage(node, seen_apparatus) {
            for stage in stages_for_node(map, node) {
                if node.kind == ProductionMapNodeKind::Apparatus {
                    seen_apparatus = true;
                }
                if seen_stage_ids.insert(stage.occurrence_identity()) {
                    stages.push(stage);
                }
            }
        } else if node.kind == ProductionMapNodeKind::Apparatus {
            seen_apparatus = true;
        }
    }
    stages
}

/// Returns the previous physical stage identity for the supplied canonical
/// apparatus ID (or virtual task ID). Virtual task nodes are traversed but do
/// not become canonical production apparatus stages. Display titles are not
/// accepted as identity.
pub fn previous_work_stage_station(
    map: &ProductionMapDefinition,
    station_id: &str,
) -> Option<String> {
    previous_work_stage_stations(map, station_id)
        .into_iter()
        .next()
}

/// Return all physical predecessors reached through the same branch-aware
/// topology used by [`linear_work_stages`].
pub fn previous_work_stage_stations(
    map: &ProductionMapDefinition,
    station_id: &str,
) -> Vec<String> {
    let physical_stage_node_ids = linear_work_stages(map)
        .into_iter()
        .filter_map(|stage| stage.apparatus_id.map(|_| stage.node_id))
        .collect::<BTreeSet<_>>();
    let mut found = Vec::<String>::new();
    let mut seen_ids = BTreeSet::<String>::new();
    for node in &map.nodes {
        if !is_station_node(node) || !station_matches(node, station_id) {
            continue;
        }
        collect_previous_stage_ids(
            node.id.as_str(),
            map,
            &physical_stage_node_ids,
            &mut found,
            &mut seen_ids,
        );
    }
    found
}

/// Returns true when the requested downstream station cannot be resolved
/// against the physical stages present in the order map.
///
/// A standalone first-stage apparatus remains valid for legacy/direct flows;
/// an unresolvable station in a map that has physical stages is not treated as
/// proof that no predecessor exists.
pub fn previous_stage_resolution_is_unavailable(
    map: &ProductionMapDefinition,
    station_id: &str,
) -> bool {
    let physical_stages = linear_work_stages(map)
        .into_iter()
        .filter(|stage| stage.apparatus_id.is_some())
        .collect::<Vec<_>>();
    let physical_stage_node_ids = physical_stages
        .iter()
        .map(|stage| stage.node_id.as_str())
        .collect::<BTreeSet<_>>();
    if map.nodes.iter().any(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && node.alternative_group_id.trim().is_empty()
            && canonical_apparatus_identity(node).is_some()
            && !physical_stage_node_ids.contains(node.id.as_str())
    }) {
        return true;
    }
    let Some(index) = physical_stages
        .iter()
        .position(|stage| stage.identity() == station_id.trim())
    else {
        return !physical_stages.is_empty();
    };
    index > 0 && previous_work_stage_station(map, station_id).is_none()
}

/// Returns the next physical stage identity for the supplied canonical
/// apparatus ID (or virtual task ID). Virtual task nodes are traversed but do
/// not become canonical production apparatus stages. Display titles are not
/// accepted as identity.
pub fn next_work_stage_station(map: &ProductionMapDefinition, station_id: &str) -> Option<String> {
    next_work_stage_stations(map, station_id).into_iter().next()
}

/// Return all physical successors reached through the same branch-aware
/// topology used by [`linear_work_stages`].
pub fn next_work_stage_stations(map: &ProductionMapDefinition, station_id: &str) -> Vec<String> {
    let physical_stage_node_ids = linear_work_stages(map)
        .into_iter()
        .filter_map(|stage| stage.apparatus_id.map(|_| stage.node_id))
        .collect::<BTreeSet<_>>();
    let mut found = Vec::<String>::new();
    let mut seen_ids = BTreeSet::<String>::new();
    for node in &map.nodes {
        if !is_station_node(node) || !station_matches(node, station_id) {
            continue;
        }
        collect_next_stage_ids(
            node.id.as_str(),
            map,
            &physical_stage_node_ids,
            &mut found,
            &mut seen_ids,
        );
    }
    found
}

/// Resolve one physical stage occurrence. `preferred_node_id` is authoritative
/// when supplied by WIP/session metadata; the canonical station fallback keeps
/// legacy maps working when an apparatus occurs only once.
pub fn work_stage_for_station(
    map: &ProductionMapDefinition,
    station_id: &str,
    preferred_node_id: &str,
) -> Option<ChainStage> {
    let stages = linear_work_stages(map);
    let preferred_node_id = preferred_node_id.trim();
    if !preferred_node_id.is_empty() {
        return stages.into_iter().find(|stage| {
            stage.node_id.trim() == preferred_node_id
                && chain_stage_matches_station(stage, station_id)
        });
    }
    stages
        .into_iter()
        .find(|stage| chain_stage_matches_station(stage, station_id))
}

/// Previous physical stage for one concrete graph occurrence.
pub fn previous_work_stage_for_node(
    map: &ProductionMapDefinition,
    stage_node_id: &str,
) -> Option<ChainStage> {
    adjacent_physical_stages_for_node(map, stage_node_id, true)
        .into_iter()
        .next()
}

/// Next physical stage for one concrete graph occurrence.
pub fn next_work_stage_for_node(
    map: &ProductionMapDefinition,
    stage_node_id: &str,
) -> Option<ChainStage> {
    next_work_stages_for_node(map, stage_node_id)
        .into_iter()
        .next()
}

pub fn next_work_stages_for_node(
    map: &ProductionMapDefinition,
    stage_node_id: &str,
) -> Vec<ChainStage> {
    adjacent_physical_stages_for_node(map, stage_node_id, false)
}

pub fn is_final_work_stage_node(map: &ProductionMapDefinition, stage_node_id: &str) -> bool {
    linear_work_stages(map).iter().any(|stage| {
        stage.node_id.trim() == stage_node_id.trim() && stage.apparatus_id.is_some()
    }) && next_work_stage_for_node(map, stage_node_id).is_none()
}

/// Physical stages reachable from Start. Virtual Task nodes are intentionally
/// omitted so closure/status checks never create an apparatus queue identity
/// for a transparent stage.
pub fn physical_work_stage_ids(map: &ProductionMapDefinition) -> Option<Vec<String>> {
    let mut seen = BTreeSet::new();
    let node_by_id = node_by_id(map);
    let reachable_ids = reachable_node_ids(map);
    if reachable_ids.is_empty() {
        let mut result = Vec::new();
        for node in &map.nodes {
            if node.kind != ProductionMapNodeKind::Apparatus {
                continue;
            }
            let identity = canonical_apparatus_identity(node)?;
            if seen.insert(identity.clone()) {
                result.push(identity);
            }
        }
        return Some(result);
    }
    for node_id in reachable_ids {
        let Some(node) = node_by_id.get(node_id.as_str()) else {
            continue;
        };
        if node.kind == ProductionMapNodeKind::Apparatus
            && canonical_apparatus_identity(node).is_none()
        {
            return None;
        }
    }
    Some(
        linear_work_stages(map)
            .into_iter()
            .filter_map(|stage| stage.apparatus_id)
            .filter(|identity| seen.insert(identity.clone()))
            .collect(),
    )
}

/// Match a persisted progress stage against a requested topology station.
///
/// An unassigned alternative group is the one topology case where the first
/// producer batch may carry one candidate while another candidate is still a
/// valid consumer. The match remains canonical-only: both values must resolve
/// to apparatus IDs in the same still-unassigned alternative group.
pub fn stage_ids_match_for_map(map: &ProductionMapDefinition, left: &str, right: &str) -> bool {
    if super::types::stage_ids_match(left, right) {
        return true;
    }
    let Some(left_id) = ApparatusId::new(left.trim().to_string()).ok() else {
        return false;
    };
    let Some(right_id) = ApparatusId::new(right.trim().to_string()).ok() else {
        return false;
    };
    let Some(left_node) = map.nodes.iter().find(|node| {
        node.apparatus_id.trim() == left_id.as_str()
            || canonical_apparatus_identity(node).as_deref() == Some(left_id.as_str())
    }) else {
        return false;
    };
    let Some(right_node) = map.nodes.iter().find(|node| {
        node.apparatus_id.trim() == right_id.as_str()
            || canonical_apparatus_identity(node).as_deref() == Some(right_id.as_str())
    }) else {
        return false;
    };
    let group_id = left_node.alternative_group_id.trim();
    if group_id.is_empty() || group_id != right_node.alternative_group_id.trim() {
        return false;
    }
    let assigned_ids = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && node.alternative_group_id.trim() == group_id
                && !node.alternative_assigned_apparatus_id.trim().is_empty()
        })
        .map(|node| node.alternative_assigned_apparatus_id.trim().to_string())
        .collect::<BTreeSet<_>>();
    if assigned_ids.is_empty() {
        return left_node
            .alternative_assigned_apparatus_id
            .trim()
            .is_empty()
            && right_node
                .alternative_assigned_apparatus_id
                .trim()
                .is_empty();
    }
    assigned_ids.len() == 1
        && assigned_ids.contains(right_id.as_str())
        && left_node.alternative_assigned_apparatus_id.trim() == right_id.as_str()
        && right_node.alternative_assigned_apparatus_id.trim() == right_id.as_str()
}

/// Match two concrete topology occurrences without collapsing repeated uses of
/// the same apparatus. Different node IDs are interchangeable only when they
/// belong to the same valid alternative group.
pub fn stage_node_ids_match_for_map(
    map: &ProductionMapDefinition,
    left: &str,
    right: &str,
) -> bool {
    let left = left.trim();
    let right = right.trim();
    if left.is_empty() || right.is_empty() {
        return false;
    }
    if left == right {
        return true;
    }
    let Some(left_node) = map.nodes.iter().find(|node| node.id.trim() == left) else {
        return false;
    };
    let Some(right_node) = map.nodes.iter().find(|node| node.id.trim() == right) else {
        return false;
    };
    if left_node.kind != ProductionMapNodeKind::Apparatus
        || right_node.kind != ProductionMapNodeKind::Apparatus
    {
        return false;
    }
    let group_id = left_node.alternative_group_id.trim();
    if group_id.is_empty() || group_id != right_node.alternative_group_id.trim() {
        return false;
    }
    let Some(left_apparatus) = canonical_apparatus_identity(left_node) else {
        return false;
    };
    let Some(right_apparatus) = canonical_apparatus_identity(right_node) else {
        return false;
    };
    stage_ids_match_for_map(map, &left_apparatus, &right_apparatus)
}

pub fn order_ready_for_station(
    map: &ProductionMapDefinition,
    order_id: &str,
    station_id: &str,
    all_states: &BTreeMap<String, BTreeMap<String, String>>,
    _known_keys: &[String],
) -> bool {
    let Some(previous_id) = previous_work_stage_station(map, station_id) else {
        return true;
    };
    queue_state_for_station(&previous_id, order_id, all_states)
        == ApparatusQueueOrderState::Completed
}

pub fn order_ready_for_stage_node(
    map: &ProductionMapDefinition,
    order_id: &str,
    stage_node_id: &str,
    all_states: &BTreeMap<String, BTreeMap<String, String>>,
) -> bool {
    let Some(previous) = previous_work_stage_for_node(map, stage_node_id) else {
        return true;
    };
    let Some(previous_apparatus) = previous.apparatus_id else {
        return true;
    };
    queue_state_for_station(&previous_apparatus, order_id, all_states)
        == ApparatusQueueOrderState::Completed
}

pub fn map_has_work_stage_for_station(map: &ProductionMapDefinition, station_id: &str) -> bool {
    linear_work_stages(map)
        .iter()
        .any(|stage| stage.identity() == station_id.trim())
}

pub fn is_final_work_stage_station(map: &ProductionMapDefinition, station_id: &str) -> bool {
    map_has_work_stage_for_station(map, station_id)
        && linear_work_stages(map)
            .iter()
            .any(|stage| stage.apparatus_id.is_some() && stage.identity() == station_id.trim())
        && next_work_stage_station(map, station_id).is_none()
}

fn queue_state_for_station(
    station_id: &str,
    order_id: &str,
    all_states: &BTreeMap<String, BTreeMap<String, String>>,
) -> ApparatusQueueOrderState {
    all_states
        .get(station_id.trim())
        .and_then(|states| states.get(order_id.trim()))
        .and_then(|value| ApparatusQueueOrderState::parse(value))
        .unwrap_or(ApparatusQueueOrderState::Pending)
}

fn is_work_stage(node: &ProductionMapNode, seen_apparatus: bool) -> bool {
    match node.kind {
        ProductionMapNodeKind::Apparatus => true,
        // Product/order task nodes come before the first apparatus and are not
        // operator stations. Later task nodes (e.g. legacy laminatsiya) are
        // retained as explicit virtual stages in the topology.
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

fn station_identity(node: &ProductionMapNode) -> String {
    if node.kind == ProductionMapNodeKind::Apparatus {
        let assigned = node.alternative_assigned_apparatus_id.trim();
        if !assigned.is_empty() {
            return assigned.to_string();
        }
        return node.apparatus_id.trim().to_string();
    }
    task_stage_identity(node.id.as_str())
}

fn task_stage_identity(node_id: &str) -> String {
    format!("task:{}", node_id.trim())
}

fn station_matches(node: &ProductionMapNode, station_id: &str) -> bool {
    let station_id = station_id.trim();
    if station_id.is_empty() {
        return false;
    }
    if node.kind == ProductionMapNodeKind::Apparatus {
        let Some(node_id) = canonical_apparatus_identity(node) else {
            return false;
        };
        let Some(requested_id) = ApparatusId::new(station_id.to_string()).ok() else {
            return false;
        };
        return node_id == requested_id.as_str();
    }
    station_identity(node) == station_id
}

fn canonical_apparatus_identity(node: &ProductionMapNode) -> Option<String> {
    (node.kind == ProductionMapNodeKind::Apparatus)
        .then(|| ApparatusId::new(station_identity(node)).ok())
        .flatten()
        .map(|id| id.to_string())
}

fn is_unassigned_alternative_apparatus(node: &ProductionMapNode) -> bool {
    node.kind == ProductionMapNodeKind::Apparatus
        && !node.alternative_group_id.trim().is_empty()
        && node.alternative_assigned_apparatus_id.trim().is_empty()
}

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
                && candidate
                    .alternative_assigned_apparatus_id
                    .trim()
                    .is_empty()
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
    if node.kind == ProductionMapNodeKind::Apparatus && !assigned.is_empty() {
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
    let stages = linear_work_stages(map);
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
        if matches!(node.kind, ProductionMapNodeKind::Start | ProductionMapNodeKind::End) {
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
