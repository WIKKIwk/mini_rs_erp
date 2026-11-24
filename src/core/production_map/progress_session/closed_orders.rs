use std::collections::BTreeMap;

use super::super::chain;
use super::super::queue_state;
use super::super::types::{
    ProductionMapDefinition, ProductionOrderLifecycleStatus, ProductionOrderLogEntry,
    ProductionOrderOperationalStatus,
};

#[cfg(test)]
#[path = "stage_lifecycle_tests.rs"]
mod stage_lifecycle_tests;

pub(in crate::core::production_map) fn required_apparatus_for_closed_order(
    map: &ProductionMapDefinition,
) -> Option<Vec<String>> {
    chain::physical_work_stage_ids(map)
}

pub(in crate::core::production_map) fn order_completed_on_apparatus(
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    order_id: &str,
    apparatus: &str,
) -> bool {
    queue_states.iter().any(|(state_apparatus, states)| {
        super::super::types::apparatus_ids_match(state_apparatus, apparatus)
            && matches!(
                states
                    .get(order_id.trim())
                    .map(|value| value.trim().to_ascii_lowercase()),
                Some(state) if state == "completed"
            )
    })
}

pub(crate) fn derive_production_order_lifecycle(
    map: &ProductionMapDefinition,
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
) -> Option<ProductionOrderLifecycleStatus> {
    derive_production_order_lifecycle_with_stage_events(map, queue_states, &[])
}

/// Queue transitions in persistence order, not just individual roll completions.
/// `complete -> pending` means that the operation still has work remaining.
#[derive(Debug, Clone)]
pub(crate) struct ProductionStageLifecycleEvent {
    pub stage_node_id: String,
    pub action: queue_state::ApparatusQueueAction,
    pub to_state: queue_state::ApparatusQueueOrderState,
}

pub(crate) fn derive_production_order_lifecycle_with_stage_events(
    map: &ProductionMapDefinition,
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    stage_events: &[ProductionStageLifecycleEvent],
) -> Option<ProductionOrderLifecycleStatus> {
    // Do not silently omit an invalid physical stage from the completion check.
    required_apparatus_for_closed_order(map)?;
    let physical_stages = chain::linear_work_stages(map)
        .into_iter()
        .filter(|stage| stage.apparatus_id.is_some())
        .collect::<Vec<_>>();
    // A group identifies one operation, irrespective of which candidates did
    // its work. Never collapse repeated uses of an apparatus in other stages.
    let mut stages_by_operation = BTreeMap::new();
    let mut operation_by_node = BTreeMap::new();
    let mut occurrence_counts = BTreeMap::<&str, usize>::new();
    for stage in &physical_stages {
        let node = map
            .nodes
            .iter()
            .find(|node| node.id.trim() == stage.node_id.trim())?;
        let group = node.alternative_group_id.trim();
        let operation = if group.is_empty() {
            (false, node.id.trim())
        } else {
            (true, group)
        };
        operation_by_node.insert(node.id.trim(), operation);
        stages_by_operation
            .entry(operation)
            .or_insert_with(Vec::new)
            .push(stage);
        *occurrence_counts
            .entry(stage.apparatus_id.as_deref().unwrap_or_default())
            .or_default() += 1;
    }
    let mut latest_operation_events = BTreeMap::new();
    for event in stage_events {
        if let Some(operation) = operation_by_node.get(event.stage_node_id.trim()) {
            latest_operation_events.insert(*operation, event);
        }
    }
    let all_occurrences_completed = !stages_by_operation.is_empty()
        && stages_by_operation.iter().all(|(operation, stages)| {
            let apparatus = stages[0].apparatus_id.as_deref().unwrap_or_default();
            // Preserve the authoritative queue projection for a single plain
            // occurrence, including legacy state-only writes. It is ambiguous
            // for alternatives and repeated apparatus, which require history.
            if !operation.0
                && occurrence_counts.get(apparatus) == Some(&1)
                && queue_states
                    .get(apparatus)
                    .is_some_and(|orders| orders.contains_key(map.id.trim()))
            {
                return order_completed_on_apparatus(queue_states, &map.id, apparatus);
            }
            if let Some(event) = latest_operation_events.get(operation) {
                return event.action == queue_state::ApparatusQueueAction::Complete
                    && event.to_state == queue_state::ApparatusQueueOrderState::Completed;
            }
            // Never choose a candidate or use OR over apparatus states to
            // infer an alternative operation's end.
            false
        });
    if all_occurrences_completed {
        return Some(ProductionOrderLifecycleStatus::ProductionCompleted);
    }

    let has_started_operation = !latest_operation_events.is_empty()
        || queue_states.values().any(|states| {
            states.get(map.id.trim()).is_some_and(|state| {
                !state.trim().is_empty()
                    && !state.trim().eq_ignore_ascii_case("pending")
                    && !state.trim().eq_ignore_ascii_case("print_preflight")
            })
        });
    Some(if has_started_operation {
        ProductionOrderLifecycleStatus::InProgress
    } else {
        ProductionOrderLifecycleStatus::Released
    })
}

pub(crate) fn derive_production_order_operational_status(
    lifecycle_status: ProductionOrderLifecycleStatus,
    queue_states: &BTreeMap<String, BTreeMap<String, String>>,
    order_id: &str,
    completed_with_issue_count: usize,
    has_roll_detached: bool,
    waiting_next_stage_count: usize,
) -> ProductionOrderOperationalStatus {
    if matches!(
        lifecycle_status,
        ProductionOrderLifecycleStatus::ProductionCompleted
            | ProductionOrderLifecycleStatus::Closed
    ) {
        return if completed_with_issue_count > 0 {
            ProductionOrderOperationalStatus::CompletedWithIssue
        } else {
            ProductionOrderOperationalStatus::Completed
        };
    }

    let states = queue_states
        .values()
        .filter_map(|states| states.get(order_id.trim()))
        .map(|state| state.trim().to_ascii_lowercase())
        .collect::<Vec<_>>();
    if states.iter().any(|state| state == "frozen") {
        ProductionOrderOperationalStatus::Frozen
    } else if states.iter().any(|state| state == "print_preflight") {
        ProductionOrderOperationalStatus::PrintPreflight
    } else if has_roll_detached || states.iter().any(|state| state == "in_progress") {
        ProductionOrderOperationalStatus::InProgress
    } else if states.iter().any(|state| state == "paused") {
        ProductionOrderOperationalStatus::Paused
    } else if states.iter().any(|state| state == "completed") {
        ProductionOrderOperationalStatus::PartiallyCompleted
    } else if waiting_next_stage_count > 0 {
        ProductionOrderOperationalStatus::WaitingNextStage
    } else if states.iter().any(|state| state == "pending") {
        ProductionOrderOperationalStatus::Ready
    } else {
        ProductionOrderOperationalStatus::NotStarted
    }
}

pub(in crate::core::production_map) fn latest_required_complete_event<'a>(
    map: &ProductionMapDefinition,
    logs: &'a [ProductionOrderLogEntry],
    required_apparatus: &[String],
) -> Option<&'a ProductionOrderLogEntry> {
    let stage_nodes = chain::linear_work_stages(map)
        .into_iter()
        .filter(|stage| stage.apparatus_id.is_some())
        .map(|stage| stage.node_id)
        .collect::<std::collections::BTreeSet<_>>();
    logs.iter()
        .filter(|entry| {
            entry.action == queue_state::ApparatusQueueAction::Complete
                && entry.to_state == queue_state::ApparatusQueueOrderState::Completed
                && if entry.stage_node_id.trim().is_empty() {
                    required_apparatus.iter().any(|apparatus| {
                        super::super::types::apparatus_ids_match(&entry.apparatus, apparatus)
                    })
                } else {
                    stage_nodes.contains(entry.stage_node_id.trim())
                }
        })
        .max_by_key(|entry| entry.created_at_unix)
}

#[cfg(test)]
#[path = "closed_orders_tests.rs"]
mod tests;
