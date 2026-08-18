use std::collections::{BTreeMap, BTreeSet};

use super::state::ApparatusQueueOrderState;

pub fn effective_apparatus_sequence(
    stored_sequence: &[String],
    visible_order_ids: &[String],
) -> Vec<String> {
    effective_apparatus_sequence_excluding(stored_sequence, visible_order_ids, &BTreeSet::new())
}

pub fn effective_apparatus_sequence_excluding(
    stored_sequence: &[String],
    visible_order_ids: &[String],
    excluded_order_ids: &BTreeSet<String>,
) -> Vec<String> {
    let excluded: BTreeSet<String> = excluded_order_ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect();
    let visible: BTreeSet<String> = visible_order_ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .filter(|id| !excluded.contains(*id))
        .map(|id| id.to_string())
        .collect();
    if visible.is_empty() {
        return Vec::new();
    }
    let mut result = Vec::new();
    for id in stored_sequence {
        let id = id.trim();
        if id.is_empty() || !visible.contains(id) {
            continue;
        }
        if !result.iter().any(|existing| existing == id) {
            result.push(id.to_string());
        }
    }
    // Production maps are loaded newest-first. Orders that are not yet part
    // of a saved sequence must therefore be appended oldest-first so a new
    // order cannot jump ahead of the existing queue.
    let visible_order_ids = visible_order_ids.iter().rev();
    for id in visible_order_ids {
        let id = id.trim();
        if id.is_empty() || excluded.contains(id) {
            continue;
        }
        if !result.iter().any(|existing| existing == id) {
            result.push(id.to_string());
        }
    }
    result
}

pub fn first_actionable_order_id(
    sequence: &[String],
    states: &BTreeMap<String, ApparatusQueueOrderState>,
) -> Option<String> {
    for id in sequence {
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        if states
            .get(id)
            .copied()
            .unwrap_or(ApparatusQueueOrderState::Pending)
            .is_active()
        {
            return Some(id.to_string());
        }
    }
    for id in sequence {
        let id = id.trim();
        if id.is_empty() {
            continue;
        }
        match states
            .get(id)
            .copied()
            .unwrap_or(ApparatusQueueOrderState::Pending)
        {
            ApparatusQueueOrderState::Completed => continue,
            ApparatusQueueOrderState::InProgress => continue,
            ApparatusQueueOrderState::Paused => continue,
            ApparatusQueueOrderState::Frozen => continue,
            ApparatusQueueOrderState::Pending => return Some(id.to_string()),
        }
    }
    None
}
