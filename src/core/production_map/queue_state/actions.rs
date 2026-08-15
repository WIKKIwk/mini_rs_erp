use std::collections::BTreeMap;

use super::super::ProductionMapError;
use super::sequence::first_actionable_order_id;
use super::state::{next_queue_state, ApparatusQueueAction, ApparatusQueueOrderState};

pub fn apply_queue_action(
    sequence: &[String],
    states: &mut BTreeMap<String, ApparatusQueueOrderState>,
    order_id: &str,
    action: ApparatusQueueAction,
) -> Result<(), ProductionMapError> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Err(ProductionMapError::MissingId);
    }
    let current = states
        .get(order_id)
        .copied()
        .unwrap_or(ApparatusQueueOrderState::Pending);
    // A frozen order is intentionally absent from the normal actionable
    // queue. Once an admin unfreezes it, the worker must still be able to
    // resume that exact order without waiting for it to become the next
    // pending item in the sequence.
    if action == ApparatusQueueAction::Resume
        && current == ApparatusQueueOrderState::Frozen
    {
        states.insert(order_id.to_string(), next_queue_state(current, action)?);
        return Ok(());
    }
    let actionable = first_actionable_order_id(sequence, states)
        .ok_or(ProductionMapError::QueueActionNotAllowed)?;
    if actionable != order_id {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    states.insert(order_id.to_string(), next_queue_state(current, action)?);
    Ok(())
}

pub fn apply_unordered_queue_action(
    states: &mut BTreeMap<String, ApparatusQueueOrderState>,
    order_id: &str,
    action: ApparatusQueueAction,
) -> Result<(), ProductionMapError> {
    let order_id = order_id.trim();
    if order_id.is_empty() {
        return Err(ProductionMapError::MissingId);
    }
    if matches!(action, ApparatusQueueAction::Start)
        && states.iter().any(|(id, state)| {
            id.trim() != order_id && *state == ApparatusQueueOrderState::InProgress
        })
    {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    let current = states
        .get(order_id)
        .copied()
        .unwrap_or(ApparatusQueueOrderState::Pending);
    states.insert(order_id.to_string(), next_queue_state(current, action)?);
    Ok(())
}
