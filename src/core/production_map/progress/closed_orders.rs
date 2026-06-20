use std::collections::{BTreeMap, BTreeSet};

use super::super::queue_state;
use super::super::types::{
    ProductionMapDefinition, ProductionMapNodeKind, ProductionOrderLogEntry,
};

pub(in crate::core::production_map) fn required_apparatus_for_closed_order(
    map: &ProductionMapDefinition,
) -> Vec<String> {
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

pub(in crate::core::production_map) fn order_completed_on_apparatus(
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

pub(in crate::core::production_map) fn latest_required_complete_event<'a>(
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
