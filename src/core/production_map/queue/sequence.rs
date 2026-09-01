use std::collections::{BTreeMap, BTreeSet};

use super::state::ApparatusQueueOrderState;

pub fn effective_apparatus_sequence(
    stored_sequence: &[String],
    visible_order_ids: &[String],
) -> Vec<String> {
    effective_apparatus_sequence_excluding(stored_sequence, visible_order_ids, &BTreeSet::new())
}

pub fn effective_apparatus_sequence_excluding<'a>(
    stored_sequence: &'a [String],
    visible_order_ids: &'a [String],
    excluded_order_ids: &'a BTreeSet<String>,
) -> Vec<String> {
    let excluded = excluded_order_ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .collect::<BTreeSet<_>>();
    let visible = visible_order_ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty() && !excluded.contains(id))
        .collect::<BTreeSet<_>>();
    if visible.is_empty() {
        return Vec::new();
    }
    let mut result = Vec::with_capacity(visible.len());
    let mut seen = BTreeSet::new();
    for id in stored_sequence {
        let id = id.trim();
        if !id.is_empty() && visible.contains(id) && seen.insert(id) {
            result.push(id.to_string());
        }
    }
    // Production maps are loaded newest-first. Orders that are not yet part
    // of a saved sequence must therefore be appended oldest-first so a new
    // order cannot jump ahead of the existing queue.
    for id in visible_order_ids.iter().rev() {
        let id = id.trim();
        if !id.is_empty() && !excluded.contains(id) && seen.insert(id) {
            result.push(id.to_string());
        }
    }
    result
}

pub fn first_actionable_order_id<'a>(
    sequence: &'a [String],
    states: &BTreeMap<String, ApparatusQueueOrderState>,
) -> Option<&'a str> {
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
            return Some(id);
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
            ApparatusQueueOrderState::Pending => return Some(id),
        }
    }
    None
}
