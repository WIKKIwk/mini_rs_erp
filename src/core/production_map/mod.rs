use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

pub mod chain;
pub mod materials;
pub mod pechat;
pub mod queue_state;

pub use materials::{
    ApparatusMaterialRule, ApparatusMaterialRuleUpsert, RawMaterialAssignment,
    RawMaterialAssignmentInput,
};

const MAX_LAMINATSIYA_RUBBER_SIZE_MM: i64 = 1050;

#[cfg(test)]
mod memory_store;
mod service;
mod types;

#[cfg(test)]
pub use memory_store::MemoryProductionMapStore;
pub use service::{PreparedApparatusQueueAction, ProductionMapLiveSnapshot, ProductionMapService};
pub use types::*;

fn visible_order_ids_for_apparatus(
    maps: &[ProductionMapDefinition],
    apparatus: &str,
) -> Vec<String> {
    maps.iter()
        .filter(|map| {
            !flexo_order_blocked_for_color_pechat(map, apparatus)
                && chain::map_has_work_stage_for_station(map, apparatus)
        })
        .map(|map| map.id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect()
}

fn move_allowed(map: &ProductionMapDefinition, from: &str, to: &str) -> bool {
    let from_is_laminatsiya = is_laminatsiya_title(from);
    let to_is_laminatsiya = is_laminatsiya_title(to);
    if from_is_laminatsiya || to_is_laminatsiya {
        return from_is_laminatsiya
            && to_is_laminatsiya
            && alternative_assigned_group_contains_target(map, from, to);
    }
    let Some(target_color) = pechat::pechat_color_count(to) else {
        return true;
    };
    if is_flexo_order(map) {
        return false;
    }
    let source_color = pechat::pechat_color_count(from).or_else(|| {
        pechat::order_pechat_color_count(
            map.nodes
                .iter()
                .filter(|node| node.kind == ProductionMapNodeKind::Apparatus)
                .map(|node| node.title.as_str()),
        )
    });
    pechat::pechat_can_move_order(target_color, map.roll_count, map.width_mm, source_color)
}

fn flexo_order_blocked_for_color_pechat(map: &ProductionMapDefinition, apparatus: &str) -> bool {
    is_flexo_order(map) && pechat::pechat_color_count(apparatus).is_some()
}

fn is_flexo_order(map: &ProductionMapDefinition) -> bool {
    let mut haystack = format!("{} {} {}", map.title, map.product_code, map.code).to_lowercase();
    for node in &map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus {
            continue;
        }
        haystack.push(' ');
        haystack.push_str(&node.title.to_lowercase());
        haystack.push(' ');
        haystack.push_str(&node.item_code.to_lowercase());
    }
    ["fleksa", "fleska", "flex", "flexe", "flexo"]
        .iter()
        .any(|keyword| haystack.contains(keyword))
}

fn alternative_assigned_group_contains_target(
    map: &ProductionMapDefinition,
    from: &str,
    to: &str,
) -> bool {
    let candidate_groups: BTreeSet<String> = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && !node.alternative_group_id.trim().is_empty()
                && queue_state::apparatus_titles_match(&node.alternative_assigned_title, from)
        })
        .map(|node| node.alternative_group_id.trim().to_string())
        .collect();
    if candidate_groups.is_empty() {
        return true;
    }
    map.nodes.iter().any(|node| {
        node.kind == ProductionMapNodeKind::Apparatus
            && candidate_groups.contains(node.alternative_group_id.trim())
            && queue_state::apparatus_titles_match(&node.title, to)
    })
}

fn reassign_apparatus_nodes(map: &mut ProductionMapDefinition, from: &str, to: &str) -> bool {
    let to = to.trim();
    let mut changed = false;
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && queue_state::apparatus_titles_match(&node.title, from)
        {
            node.title = to.to_string();
            changed = true;
        }
    }
    changed
}

fn reassign_alternative_apparatus_assignment(
    map: &mut ProductionMapDefinition,
    from: &str,
    to: &str,
) -> bool {
    let to = to.trim();
    if to.is_empty() {
        return false;
    }
    let candidate_groups: BTreeSet<String> = map
        .nodes
        .iter()
        .filter(|node| {
            node.kind == ProductionMapNodeKind::Apparatus
                && !node.alternative_group_id.trim().is_empty()
                && queue_state::apparatus_titles_match(&node.alternative_assigned_title, from)
        })
        .map(|node| node.alternative_group_id.trim().to_string())
        .collect();
    if candidate_groups.is_empty() {
        return false;
    }
    let mut changed = false;
    for node in &mut map.nodes {
        if node.kind == ProductionMapNodeKind::Apparatus
            && candidate_groups.contains(node.alternative_group_id.trim())
        {
            node.alternative_assigned_title = to.to_string();
            changed = true;
        }
    }
    changed
}

pub fn compile_map(
    map: &ProductionMapDefinition,
) -> Result<ProductionMapProgram, ProductionMapError> {
    validate_map(map)?;
    let order = topological_order(map)?;
    let node_by_id: BTreeMap<&str, &ProductionMapNode> = map
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut operations = Vec::with_capacity(order.len());
    for (index, node_id) in order.into_iter().enumerate() {
        let node = node_by_id
            .get(node_id.as_str())
            .expect("topological order only contains known node ids");
        operations.push(compile_node(index + 1, node)?);
    }
    Ok(ProductionMapProgram {
        map_id: map.id.clone(),
        product_code: map.product_code.clone(),
        operations,
    })
}

#[cfg(test)]
fn reject_order_number_immutable(
    maps: &BTreeMap<String, ProductionMapDefinition>,
    next: &ProductionMapDefinition,
) -> Result<(), ProductionMapError> {
    let id = next.id.trim();
    if !id.starts_with("zakaz-") {
        return Ok(());
    }
    let order_number = next.order_number.trim();
    if order_number.is_empty() {
        return Ok(());
    }
    let Some(existing) = maps.get(id) else {
        return Ok(());
    };
    let existing_number = existing.order_number.trim();
    if !existing_number.is_empty() && existing_number != order_number {
        return Err(ProductionMapError::OrderNumberImmutable);
    }
    Ok(())
}

fn normalize_map(map: &mut ProductionMapDefinition) {
    map.id = map.id.trim().to_ascii_lowercase();
    map.product_code = map.product_code.trim().to_string();
    map.title = map.title.trim().to_string();
    map.code = map.code.trim().to_string();
    map.order_number = map.order_number.trim().to_string();
    if map
        .roll_count
        .is_some_and(|value| !value.is_finite() || value <= 0.0)
    {
        map.roll_count = None;
    }
    if map
        .width_mm
        .is_some_and(|value| !value.is_finite() || value <= 0.0)
    {
        map.width_mm = None;
    }
    for node in &mut map.nodes {
        node.id = node.id.trim().to_ascii_lowercase();
        node.title = node.title.trim().to_string();
        node.role_code = node.role_code.trim().to_string();
        node.item_code = node.item_code.trim().to_string();
        node.qty_formula = node.qty_formula.trim().to_string();
        node.from_location = node.from_location.trim().to_string();
        node.to_location = node.to_location.trim().to_string();
        node.alternative_group_id = node.alternative_group_id.trim().to_string();
        node.alternative_group_label = node.alternative_group_label.trim().to_string();
        node.alternative_assigned_title = node.alternative_assigned_title.trim().to_string();
        if !node.x.is_finite() {
            node.x = 0.0;
        }
        if !node.y.is_finite() {
            node.y = 0.0;
        }
        if let Some(formula) = &mut node.formula {
            formula.target = formula.target.trim().to_string();
            formula.expression = formula.expression.trim().to_string();
        }
    }
    for edge in &mut map.edges {
        edge.from = edge.from.trim().to_ascii_lowercase();
        edge.to = edge.to.trim().to_ascii_lowercase();
        edge.branch = normalize_branch(&edge.branch);
    }
}

fn validate_map(map: &ProductionMapDefinition) -> Result<(), ProductionMapError> {
    if map.id.trim().is_empty() {
        return Err(ProductionMapError::MissingId);
    }
    if map.product_code.trim().is_empty() {
        return Err(ProductionMapError::MissingProductCode);
    }
    if map.title.trim().is_empty() {
        return Err(ProductionMapError::MissingTitle);
    }
    if laminatsiya_rubber_too_large(map) {
        return Err(ProductionMapError::LaminatsiyaRubberTooLarge);
    }

    let mut ids = BTreeSet::new();
    let mut start_count = 0;
    let mut end_count = 0;
    for node in &map.nodes {
        if !ids.insert(node.id.as_str()) {
            return Err(ProductionMapError::DuplicateNode(node.id.clone()));
        }
        match node.kind {
            ProductionMapNodeKind::Start => start_count += 1,
            ProductionMapNodeKind::End => end_count += 1,
            ProductionMapNodeKind::Formula => {
                let Some(formula) = &node.formula else {
                    return Err(ProductionMapError::MissingFormulaExpression);
                };
                if formula.target.trim().is_empty() {
                    return Err(ProductionMapError::MissingFormulaTarget);
                }
                if formula.expression.trim().is_empty() {
                    return Err(ProductionMapError::MissingFormulaExpression);
                }
                validate_formula_target(&formula.target)?;
                validate_formula_expression(&formula.expression)?;
            }
            ProductionMapNodeKind::Condition => {
                let Some(formula) = &node.formula else {
                    return Err(ProductionMapError::MissingFormulaExpression);
                };
                if formula.expression.trim().is_empty() {
                    return Err(ProductionMapError::MissingFormulaExpression);
                }
                validate_condition_expression(&formula.expression)?;
            }
            ProductionMapNodeKind::Location => {}
            ProductionMapNodeKind::Material
            | ProductionMapNodeKind::Apparatus
            | ProductionMapNodeKind::KkProduct
            | ProductionMapNodeKind::Task
            | ProductionMapNodeKind::Wait
            | ProductionMapNodeKind::Output => {
                if !node.qty_formula.trim().is_empty() {
                    validate_formula_expression(&node.qty_formula)?;
                }
            }
        }
        validate_location_ref(&node.from_location)?;
        validate_location_ref(&node.to_location)?;
    }
    if start_count != 1 {
        return Err(ProductionMapError::MissingStart);
    }
    if end_count != 1 {
        return Err(ProductionMapError::MissingEnd);
    }
    for edge in &map.edges {
        if !ids.contains(edge.from.as_str()) {
            return Err(ProductionMapError::MissingEdgeNode(edge.from.clone()));
        }
        if !ids.contains(edge.to.as_str()) {
            return Err(ProductionMapError::MissingEdgeNode(edge.to.clone()));
        }
    }
    for node in &map.nodes {
        if node.kind != ProductionMapNodeKind::Condition {
            continue;
        }
        let mut has_true = false;
        let mut has_false = false;
        for edge in map.edges.iter().filter(|edge| edge.from == node.id) {
            match normalize_branch(&edge.branch).as_str() {
                "true" => has_true = true,
                "false" => has_false = true,
                _ => {}
            }
        }
        if !has_true || !has_false {
            return Err(ProductionMapError::MissingConditionBranch);
        }
    }
    Ok(())
}

fn laminatsiya_rubber_too_large(map: &ProductionMapDefinition) -> bool {
    let Some(width_mm) = map.width_mm.filter(|value| *value > 0.0) else {
        return false;
    };
    if pechat::rubber_size_from_width(width_mm) <= MAX_LAMINATSIYA_RUBBER_SIZE_MM {
        return false;
    }
    map.nodes.iter().any(|node| {
        matches!(
            node.kind,
            ProductionMapNodeKind::Apparatus | ProductionMapNodeKind::Task
        ) && is_laminatsiya_title(&node.title)
    })
}

fn is_laminatsiya_title(title: &str) -> bool {
    title.trim().to_lowercase().contains("laminatsiya")
}

fn required_apparatus_for_closed_order(map: &ProductionMapDefinition) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut apparatus = Vec::new();
    for node in &map.nodes {
        if node.kind != ProductionMapNodeKind::Apparatus {
            continue;
        }
        let title = if node.alternative_assigned_title.trim().is_empty() {
            node.title.trim()
        } else {
            node.alternative_assigned_title.trim()
        };
        if title.is_empty() || !seen.insert(title.to_ascii_lowercase()) {
            continue;
        }
        apparatus.push(title.to_string());
    }
    apparatus
}

fn order_completed_on_apparatus(
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    order_id: &str,
    apparatus: &str,
) -> bool {
    queue_states.iter().any(|(state_apparatus, states)| {
        queue_state::apparatus_titles_match(state_apparatus, apparatus)
            && matches!(
                states
                    .get(order_id.trim())
                    .map(|value| value.trim().to_ascii_lowercase()),
                Some(state) if state == "completed"
            )
    })
}

fn latest_required_complete_event<'a>(
    logs: &'a [ProductionOrderLogEntry],
    required_apparatus: &[String],
) -> Option<&'a ProductionOrderLogEntry> {
    logs.iter()
        .filter(|entry| {
            entry.action == queue_state::ApparatusQueueAction::Complete
                && entry.to_state == queue_state::ApparatusQueueOrderState::Completed
                && required_apparatus.iter().any(|apparatus| {
                    queue_state::apparatus_titles_match(&entry.apparatus, apparatus)
                })
        })
        .max_by_key(|entry| entry.created_at_unix)
}

#[cfg(test)]
fn completion_request_notification_from_event(
    event: &ApparatusQueueActionEvent,
    created_at_unix: i64,
) -> Option<CompletionRequestNotification> {
    if event.action != queue_state::ApparatusQueueAction::Complete
        || event.payload_json.get("completion_request")?.as_bool() != Some(true)
    {
        return None;
    }
    let description = event.payload_json.get("description")?.as_str()?.trim();
    if description.is_empty() {
        return None;
    }
    let status = json_string_field(&event.payload_json, "completion_request_status");
    if !status.is_empty() && status != "pending" {
        return None;
    }
    Some(CompletionRequestNotification {
        event_id: event.event_id.trim().to_string(),
        apparatus: event.apparatus.trim().to_string(),
        order_id: event.order_id.trim().to_string(),
        order_number: json_string_field(&event.payload_json, "order_number"),
        order_title: json_string_field(&event.payload_json, "order_title"),
        product_code: json_string_field(&event.payload_json, "product_code"),
        worker_role: event.actor.role.trim().to_string(),
        worker_ref: event.actor.ref_.trim().to_string(),
        worker_display_name: actor_display_name(&event.actor),
        description: description.to_string(),
        created_at_unix,
    })
}

#[cfg(test)]
fn completion_request_decision_notification_from_event(
    event: &ApparatusQueueActionEvent,
    created_at_unix: i64,
) -> Option<CompletionRequestDecisionNotification> {
    if event.action != queue_state::ApparatusQueueAction::Complete
        || event.payload_json.get("completion_request")?.as_bool() != Some(true)
    {
        return None;
    }
    let decision = json_string_field(&event.payload_json, "completion_request_status");
    if decision != "approved" && decision != "rejected" {
        return None;
    }
    let decision_at_unix = event
        .payload_json
        .get("decision_at_unix")
        .and_then(|value| value.as_i64())
        .unwrap_or(created_at_unix);
    Some(CompletionRequestDecisionNotification {
        event_id: json_string_field(&event.payload_json, "decision_event_id"),
        request_event_id: event.event_id.trim().to_string(),
        decision,
        apparatus: event.apparatus.trim().to_string(),
        order_id: event.order_id.trim().to_string(),
        order_number: json_string_field(&event.payload_json, "order_number"),
        order_title: json_string_field(&event.payload_json, "order_title"),
        product_code: json_string_field(&event.payload_json, "product_code"),
        worker_role: event.actor.role.trim().to_string(),
        worker_ref: event.actor.ref_.trim().to_string(),
        worker_display_name: actor_display_name(&event.actor),
        decided_by_role: json_string_field(&event.payload_json, "decided_by_role"),
        decided_by_ref: json_string_field(&event.payload_json, "decided_by_ref"),
        decided_by_display_name: json_string_field(&event.payload_json, "decided_by_display_name"),
        description: json_string_field(&event.payload_json, "description"),
        message: json_string_field(&event.payload_json, "decision_message"),
        created_at_unix: decision_at_unix,
    })
}

#[cfg(test)]
fn json_string_field(payload: &serde_json::Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn effective_apparatus_queue_policy(
    apparatus: &str,
    stored: Option<ApparatusQueuePolicy>,
) -> ApparatusQueuePolicy {
    if pechat::pechat_color_count(apparatus).is_some() {
        ApparatusQueuePolicy::StrictSequence
    } else {
        stored.unwrap_or(ApparatusQueuePolicy::StrictSequence)
    }
}

fn effective_apparatus_queue_policy_record(
    apparatus: &str,
    stored: ApparatusQueuePolicy,
) -> ApparatusQueuePolicyRecord {
    let locked = pechat::pechat_color_count(apparatus).is_some();
    ApparatusQueuePolicyRecord {
        apparatus: apparatus.trim().to_string(),
        policy: if locked {
            ApparatusQueuePolicy::StrictSequence
        } else {
            stored
        },
        locked,
        reason: if locked {
            "pechat_always_strict".to_string()
        } else {
            String::new()
        },
    }
}

fn queue_action_event_id(
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    format!(
        "production-map-queue:{nanos}:{}:{}:{}",
        apparatus.trim(),
        order_id.trim(),
        queue_action_str(action)
    )
}

fn completion_request_decision_event_id(
    request_event_id: &str,
    decision: CompletionRequestDecision,
) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    format!(
        "production-map-completion-request:{nanos}:{}:{}",
        request_event_id.trim(),
        decision.as_str()
    )
}

fn queue_action_str(action: queue_state::ApparatusQueueAction) -> &'static str {
    match action {
        queue_state::ApparatusQueueAction::Start => "start",
        queue_state::ApparatusQueueAction::Pause => "pause",
        queue_state::ApparatusQueueAction::Resume => "resume",
        queue_state::ApparatusQueueAction::Complete => "complete",
    }
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs() as i64)
        .unwrap_or_default()
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default()
}

fn progress_session_id(
    apparatus: &str,
    order_id: &str,
    actor: &QueueActionActor,
    _now: i64,
) -> String {
    let stamp = unix_nanos();
    format!(
        "order-run:{stamp}:{}:{}:{}",
        sanitize_id(apparatus),
        sanitize_id(order_id),
        sanitize_id(&actor.ref_)
    )
}

fn progress_event_id(
    session_id: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    _now: i64,
) -> String {
    let stamp = unix_nanos();
    format!(
        "order-progress:{stamp}:{}:{}:{}",
        sanitize_id(session_id),
        sanitize_id(order_id),
        queue_action_str(action)
    )
}

fn progress_batch_id(
    apparatus: &str,
    order_id: &str,
    action: queue_state::ApparatusQueueAction,
    _now: i64,
) -> String {
    let stamp = unix_nanos();
    format!(
        "progress-batch:{stamp}:{}:{}:{}",
        sanitize_id(apparatus),
        sanitize_id(order_id),
        queue_action_str(action)
    )
}

fn progress_qr_payload(batch_id: &str) -> String {
    let stamp = batch_id
        .split(':')
        .nth(1)
        .and_then(|value| value.parse::<u128>().ok())
        .unwrap_or_else(unix_nanos);
    let stamp = (stamp & u128::from(u64::MAX)) as u64;
    let hash = progress_qr_hash(batch_id);
    format!("4001{stamp:016X}{hash:04X}")
}

fn progress_qr_hash(value: &str) -> u16 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.trim().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash & 0xffff) as u16
}

fn valid_progress_qty(value: Option<f64>) -> Result<f64, ProductionMapError> {
    let value = value.ok_or(ProductionMapError::ProgressInputInvalid)?;
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(ProductionMapError::ProgressInputInvalid)
    }
}

fn non_empty_or(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.trim().to_string()
    } else {
        value.to_string()
    }
}

fn progress_label_item_name(
    order_map: &ProductionMapDefinition,
    apparatus: &str,
    action: queue_state::ApparatusQueueAction,
) -> String {
    let order_title = non_empty_or(&order_map.title, &order_map.product_code);
    let state_label = match action {
        queue_state::ApparatusQueueAction::Pause => "pauza",
        queue_state::ApparatusQueueAction::Complete => "tugatildi",
        _ => queue_action_str(action),
    };
    format!(
        "{order_title} yarim tayyor, {} holatda, {state_label}",
        apparatus.trim()
    )
}

fn actor_display_name(actor: &QueueActionActor) -> String {
    non_empty_or(&actor.display_name, &actor.ref_)
}

fn legacy_order_run_session(
    apparatus: &str,
    order_id: &str,
    actor: &QueueActionActor,
    now: i64,
) -> OrderRunSession {
    OrderRunSession {
        session_id: progress_session_id(apparatus, order_id, actor, now),
        apparatus: apparatus.trim().to_string(),
        order_id: order_id.trim().to_string(),
        status: OrderRunStatus::Active,
        worker_role: actor.role.trim().to_string(),
        worker_ref: actor.ref_.trim().to_string(),
        worker_display_name: actor.display_name.trim().to_string(),
        started_at_unix: now,
        updated_at_unix: now,
        payload_json: serde_json::json!({"legacy_session": true}),
    }
}

fn sanitize_id(value: &str) -> String {
    let sanitized = value
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "blank".to_string()
    } else {
        sanitized
    }
}

fn validate_formula_target(target: &str) -> Result<(), ProductionMapError> {
    if is_identifier(target.trim()) {
        Ok(())
    } else {
        Err(ProductionMapError::InvalidFormulaTarget(target.to_string()))
    }
}

fn validate_location_ref(location: &str) -> Result<(), ProductionMapError> {
    let location = location.trim();
    if location.is_empty() {
        return Ok(());
    }
    let valid = location.len() <= 120
        && location.chars().any(char::is_alphanumeric)
        && location.chars().all(|ch| {
            ch.is_alphanumeric()
                || ch.is_whitespace()
                || matches!(ch, '-' | '_' | '.' | '/' | '(' | ')')
        });
    if valid {
        Ok(())
    } else {
        Err(ProductionMapError::InvalidLocation(location.to_string()))
    }
}

fn validate_formula_expression(expression: &str) -> Result<(), ProductionMapError> {
    let mut parser = FormulaParser::new(expression);
    parser.parse_expression()?;
    parser.skip_whitespace();
    if parser.is_eof() {
        Ok(())
    } else {
        Err(ProductionMapError::InvalidFormulaExpression(
            expression.to_string(),
        ))
    }
}

fn validate_condition_expression(expression: &str) -> Result<(), ProductionMapError> {
    evaluate_condition(expression, &BTreeMap::new())
        .map(|_| ())
        .or_else(|error| {
            if matches!(error, ProductionMapError::UnknownFormulaVariable(_)) {
                Ok(())
            } else {
                Err(error)
            }
        })
}

fn evaluate_formula(
    expression: &str,
    variables: &BTreeMap<String, f64>,
) -> Result<f64, ProductionMapError> {
    let mut parser = FormulaParser::new(expression);
    let value = parser.evaluate_expression(variables)?;
    parser.skip_whitespace();
    if parser.is_eof() {
        Ok(value)
    } else {
        Err(ProductionMapError::InvalidFormulaExpression(
            expression.to_string(),
        ))
    }
}

fn evaluate_condition(
    expression: &str,
    variables: &BTreeMap<String, f64>,
) -> Result<bool, ProductionMapError> {
    if let Some((left, operator, right)) = split_condition(expression) {
        let left = evaluate_formula(left, variables)?;
        let right = evaluate_formula(right, variables)?;
        return match operator {
            ">" => Ok(left > right),
            ">=" => Ok(left >= right),
            "<" => Ok(left < right),
            "<=" => Ok(left <= right),
            "==" => Ok((left - right).abs() < f64::EPSILON),
            "!=" => Ok((left - right).abs() >= f64::EPSILON),
            _ => Err(ProductionMapError::InvalidFormulaExpression(
                expression.to_string(),
            )),
        };
    }
    Ok(evaluate_formula(expression, variables)? != 0.0)
}

fn split_condition(expression: &str) -> Option<(&str, &str, &str)> {
    let mut depth = 0usize;
    let bytes = expression.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => {
                for operator in [">=", "<=", "==", "!=", ">", "<"] {
                    if expression[index..].starts_with(operator) {
                        let left = expression[..index].trim();
                        let right = expression[index + operator.len()..].trim();
                        if !left.is_empty() && !right.is_empty() {
                            return Some((left, operator, right));
                        }
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn normalize_branch(branch: &str) -> String {
    match branch.trim().to_ascii_lowercase().as_str() {
        "ha" | "yes" | "true" | "1" => "true".to_string(),
        "yo'q" | "yoq" | "no" | "false" | "0" => "false".to_string(),
        value => value.to_string(),
    }
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

struct FormulaParser<'a> {
    input: &'a str,
    position: usize,
}

impl<'a> FormulaParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, position: 0 }
    }

    fn parse_expression(&mut self) -> Result<(), ProductionMapError> {
        self.parse_term()?;
        loop {
            self.skip_whitespace();
            if self.consume('+') || self.consume('-') {
                self.parse_term()?;
            } else {
                return Ok(());
            }
        }
    }

    fn evaluate_expression(
        &mut self,
        variables: &BTreeMap<String, f64>,
    ) -> Result<f64, ProductionMapError> {
        let mut value = self.evaluate_term(variables)?;
        loop {
            self.skip_whitespace();
            if self.consume('+') {
                value += self.evaluate_term(variables)?;
            } else if self.consume('-') {
                value -= self.evaluate_term(variables)?;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_term(&mut self) -> Result<(), ProductionMapError> {
        self.parse_factor()?;
        loop {
            self.skip_whitespace();
            if self.consume('*') || self.consume('/') {
                self.parse_factor()?;
            } else {
                return Ok(());
            }
        }
    }

    fn evaluate_term(
        &mut self,
        variables: &BTreeMap<String, f64>,
    ) -> Result<f64, ProductionMapError> {
        let mut value = self.evaluate_factor(variables)?;
        loop {
            self.skip_whitespace();
            if self.consume('*') {
                value *= self.evaluate_factor(variables)?;
            } else if self.consume('/') {
                let divisor = self.evaluate_factor(variables)?;
                if divisor == 0.0 {
                    return Err(ProductionMapError::FormulaDivisionByZero);
                }
                value /= divisor;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_factor(&mut self) -> Result<(), ProductionMapError> {
        self.skip_whitespace();
        if self.consume('-') {
            return self.parse_factor();
        }
        if self.consume('(') {
            self.parse_expression()?;
            self.skip_whitespace();
            return if self.consume(')') {
                Ok(())
            } else {
                self.invalid()
            };
        }
        if self.parse_identifier() || self.parse_number() {
            Ok(())
        } else {
            self.invalid()
        }
    }

    fn evaluate_factor(
        &mut self,
        variables: &BTreeMap<String, f64>,
    ) -> Result<f64, ProductionMapError> {
        self.skip_whitespace();
        if self.consume('-') {
            return Ok(-self.evaluate_factor(variables)?);
        }
        if self.consume('(') {
            let value = self.evaluate_expression(variables)?;
            self.skip_whitespace();
            return if self.consume(')') {
                Ok(value)
            } else {
                self.invalid()
            };
        }
        if let Some(identifier) = self.read_identifier() {
            return variables
                .get(&identifier)
                .copied()
                .ok_or(ProductionMapError::UnknownFormulaVariable(identifier));
        }
        if let Some(number) = self.read_number() {
            return Ok(number);
        }
        self.invalid()
    }

    fn parse_identifier(&mut self) -> bool {
        self.read_identifier().is_some()
    }

    fn read_identifier(&mut self) -> Option<String> {
        let start = self.position;
        while let Some(ch) = self.peek() {
            if self.position == start {
                if ch.is_ascii_alphabetic() || ch == '_' {
                    self.position += ch.len_utf8();
                } else {
                    break;
                }
            } else if ch.is_ascii_alphanumeric() || ch == '_' {
                self.position += ch.len_utf8();
            } else {
                break;
            }
        }
        (self.position > start).then(|| self.input[start..self.position].to_string())
    }

    fn parse_number(&mut self) -> bool {
        self.read_number().is_some()
    }

    fn read_number(&mut self) -> Option<f64> {
        let start = self.position;
        while matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
            self.position += 1;
        }
        if self.consume('.') {
            while matches!(self.peek(), Some(ch) if ch.is_ascii_digit()) {
                self.position += 1;
            }
        }
        (self.position > start)
            .then(|| self.input[start..self.position].parse::<f64>().ok())
            .flatten()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(ch) if ch.is_ascii_whitespace()) {
            self.position += 1;
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.position += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }

    fn is_eof(&self) -> bool {
        self.position >= self.input.len()
    }

    fn invalid<T>(&self) -> Result<T, ProductionMapError> {
        Err(ProductionMapError::InvalidFormulaExpression(
            self.input.to_string(),
        ))
    }
}

#[cfg(test)]
pub fn run_map(
    map: &ProductionMapDefinition,
    order_qty: f64,
) -> Result<ProductionMapRunResult, ProductionMapError> {
    run_map_with_variables(map, order_qty, BTreeMap::new())
}

pub fn run_map_with_variables(
    map: &ProductionMapDefinition,
    order_qty: f64,
    run_variables: BTreeMap<String, f64>,
) -> Result<ProductionMapRunResult, ProductionMapError> {
    if order_qty <= 0.0 {
        return Err(ProductionMapError::InvalidOrderQty);
    }
    compile_map(map)?;
    let node_by_id: BTreeMap<&str, &ProductionMapNode> = map
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut outgoing = BTreeMap::<&str, Vec<&ProductionMapEdge>>::new();
    for edge in &map.edges {
        outgoing.entry(edge.from.as_str()).or_default().push(edge);
    }
    let mut variables = input_variables(order_qty, run_variables);
    let mut tasks = Vec::new();
    let Some(mut current_id) = map
        .nodes
        .iter()
        .find(|node| node.kind == ProductionMapNodeKind::Start)
        .map(|node| node.id.as_str())
    else {
        return Err(ProductionMapError::MissingStart);
    };
    let mut visited = BTreeSet::new();
    let mut visited_node_ids = Vec::new();
    while visited.insert(current_id.to_string()) {
        let node = node_by_id
            .get(current_id)
            .expect("compiled map only contains known node ids");
        visited_node_ids.push(node.id.clone());
        if node.kind == ProductionMapNodeKind::End {
            break;
        }
        match node.kind {
            ProductionMapNodeKind::Formula => {
                let Some(formula) = &node.formula else {
                    return Err(ProductionMapError::MissingFormulaExpression);
                };
                let value = evaluate_formula(&formula.expression, &variables)?;
                variables.insert(formula.target.clone(), value);
            }
            ProductionMapNodeKind::Condition => {
                let Some(formula) = &node.formula else {
                    return Err(ProductionMapError::MissingFormulaExpression);
                };
                let result = match evaluate_condition(&formula.expression, &variables) {
                    Ok(result) => result,
                    Err(ProductionMapError::UnknownFormulaVariable(variable)) => {
                        return Ok(ProductionMapRunResult {
                            map_id: map.id.clone(),
                            product_code: map.product_code.clone(),
                            order_qty,
                            variables,
                            tasks,
                            visited_node_ids,
                            awaiting_node_id: node.id.clone(),
                            awaiting_variable: variable,
                            awaiting_expression: formula.expression.clone(),
                        });
                    }
                    Err(error) => return Err(error),
                };
                variables.insert(node.id.clone(), if result { 1.0 } else { 0.0 });
            }
            ProductionMapNodeKind::Location => {}
            ProductionMapNodeKind::Material
            | ProductionMapNodeKind::Apparatus
            | ProductionMapNodeKind::KkProduct
            | ProductionMapNodeKind::Task
            | ProductionMapNodeKind::Wait
            | ProductionMapNodeKind::Output => {
                let qty = node_qty(node, order_qty, &variables)?;
                tasks.push(ProductionTaskDraft {
                    order: tasks.len() + 1,
                    node_id: node.id.clone(),
                    task_kind: compile_node(tasks.len() + 1, node)?.op_code,
                    title: node.title.clone(),
                    role_code: node.role_code.clone(),
                    item_code: node.item_code.clone(),
                    from_location: node.from_location.clone(),
                    to_location: node.to_location.clone(),
                    qty,
                })
            }
            ProductionMapNodeKind::Start | ProductionMapNodeKind::End => {}
        }
        let edges = outgoing.get(current_id).cloned().unwrap_or_default();
        if node.kind == ProductionMapNodeKind::Condition {
            let branch = if variables.get(&node.id).copied().unwrap_or(0.0) != 0.0 {
                "true"
            } else {
                "false"
            };
            let Some(next) = edges
                .into_iter()
                .find(|edge| normalize_branch(&edge.branch) == branch)
            else {
                return Err(ProductionMapError::MissingConditionBranch);
            };
            current_id = next.to.as_str();
        } else {
            let Some(next) = edges.first() else {
                break;
            };
            current_id = next.to.as_str();
        }
    }
    Ok(ProductionMapRunResult {
        map_id: map.id.clone(),
        product_code: map.product_code.clone(),
        order_qty,
        variables,
        tasks,
        visited_node_ids,
        awaiting_node_id: String::new(),
        awaiting_variable: String::new(),
        awaiting_expression: String::new(),
    })
}

fn input_variables(order_qty: f64, mut variables: BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    variables.insert("order_qty".to_string(), order_qty);
    variables
}

fn node_qty(
    node: &ProductionMapNode,
    order_qty: f64,
    variables: &BTreeMap<String, f64>,
) -> Result<f64, ProductionMapError> {
    let qty = if node.qty_formula.trim().is_empty() {
        order_qty
    } else {
        evaluate_formula(&node.qty_formula, variables)?
    };
    if qty.is_finite() && qty > 0.0 {
        Ok(qty)
    } else {
        Err(ProductionMapError::InvalidNodeQty(node.id.clone()))
    }
}

fn topological_order(map: &ProductionMapDefinition) -> Result<Vec<String>, ProductionMapError> {
    let mut indegree = BTreeMap::<String, usize>::new();
    let mut outgoing = BTreeMap::<String, Vec<String>>::new();
    for node in &map.nodes {
        indegree.insert(node.id.clone(), 0);
        outgoing.insert(node.id.clone(), Vec::new());
    }
    for edge in &map.edges {
        *indegree
            .get_mut(&edge.to)
            .expect("validated edge target exists") += 1;
        outgoing
            .get_mut(&edge.from)
            .expect("validated edge source exists")
            .push(edge.to.clone());
    }

    let mut queue = indegree
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(id.clone()))
        .collect::<VecDeque<_>>();
    let mut order = Vec::new();
    while let Some(id) = queue.pop_front() {
        order.push(id.clone());
        for child in outgoing.get(&id).into_iter().flatten() {
            let count = indegree
                .get_mut(child)
                .expect("validated child exists in indegree map");
            *count = count.saturating_sub(1);
            if *count == 0 {
                queue.push_back(child.clone());
            }
        }
    }
    if order.len() != map.nodes.len() {
        return Err(ProductionMapError::Cycle);
    }
    Ok(order)
}

fn compile_node(
    order: usize,
    node: &ProductionMapNode,
) -> Result<ProductionMapOperation, ProductionMapError> {
    let mut args = BTreeMap::new();
    args.insert("title".to_string(), node.title.clone());
    if !node.role_code.is_empty() {
        args.insert("role_code".to_string(), node.role_code.clone());
    }
    if !node.item_code.is_empty() {
        args.insert("item_code".to_string(), node.item_code.clone());
    }
    if !node.qty_formula.is_empty() {
        args.insert("qty_formula".to_string(), node.qty_formula.clone());
    }
    if !node.from_location.is_empty() {
        args.insert("from_location".to_string(), node.from_location.clone());
    }
    if !node.to_location.is_empty() {
        args.insert("to_location".to_string(), node.to_location.clone());
    }
    if !node.alternative_group_id.is_empty() {
        args.insert(
            "alternative_group_id".to_string(),
            node.alternative_group_id.clone(),
        );
    }
    if !node.alternative_group_label.is_empty() {
        args.insert(
            "alternative_group_label".to_string(),
            node.alternative_group_label.clone(),
        );
    }
    if !node.alternative_assigned_title.is_empty() {
        args.insert(
            "alternative_assigned_title".to_string(),
            node.alternative_assigned_title.clone(),
        );
    }
    if let Some(value) = node.rezka_kadr_count {
        args.insert("rezka_kadr_count".to_string(), value.to_string());
    }
    if let Some(value) = node.rezka_label_length {
        args.insert("rezka_label_length".to_string(), value.to_string());
    }
    let op_code = match node.kind {
        ProductionMapNodeKind::Start => "start",
        ProductionMapNodeKind::Location => "warehouse_location",
        ProductionMapNodeKind::Material => "require_material",
        ProductionMapNodeKind::Apparatus => "apparatus",
        ProductionMapNodeKind::KkProduct => "kk_product",
        ProductionMapNodeKind::Formula => {
            let Some(formula) = &node.formula else {
                return Err(ProductionMapError::MissingFormulaExpression);
            };
            args.insert("target".to_string(), formula.target.clone());
            args.insert("expression".to_string(), formula.expression.clone());
            "calculate"
        }
        ProductionMapNodeKind::Condition => {
            let Some(formula) = &node.formula else {
                return Err(ProductionMapError::MissingFormulaExpression);
            };
            args.insert("expression".to_string(), formula.expression.clone());
            "condition"
        }
        ProductionMapNodeKind::Task => "create_task",
        ProductionMapNodeKind::Wait => "wait_dependency",
        ProductionMapNodeKind::Output => "produce_output",
        ProductionMapNodeKind::End => "end",
    };
    Ok(ProductionMapOperation {
        order,
        node_id: node.id.clone(),
        op_code: op_code.to_string(),
        args,
    })
}

#[cfg(test)]
mod tests;
