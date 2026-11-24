pub(super) fn nearest_allowed_sequence(
    current_sequence: &[String],
    requested_sequence: &[String],
    moved_order_id: &str,
    states: &BTreeMap<String, String>,
    frozen_order_ids: &BTreeSet<String>,
) -> Result<Vec<String>, ProductionMapError> {
    let Some(requested_index) = requested_sequence
        .iter()
        .position(|id| id == moved_order_id)
    else {
        return Err(ProductionMapError::QueueActionNotAllowed);
    };
    let Some(original_index) = current_sequence.iter().position(|id| id == moved_order_id) else {
        return Err(ProductionMapError::QueueActionNotAllowed);
    };
    if requested_sequence.iter().collect::<BTreeSet<_>>().len() != requested_sequence.len()
        || requested_sequence
            .iter()
            .any(|id| !current_sequence.contains(id))
    {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    // Only move the dragged order. Preserve orders hidden by search as well
    // as orders added/reordered by another client while this one was dragging.
    let mut remaining = current_sequence.to_vec();
    remaining.remove(original_index);
    let target_index = if let Some(next_id) = requested_sequence.get(requested_index + 1) {
        remaining.iter().position(|id| id == next_id).unwrap()
    } else if requested_index > 0 {
        remaining
            .iter()
            .position(|id| id == &requested_sequence[requested_index - 1])
            .unwrap()
            + 1
    } else {
        original_index
    };
    let mut slots = (0..=remaining.len()).collect::<Vec<_>>();
    slots.sort_by_key(|&index| (index.abs_diff(target_index), index.abs_diff(original_index)));
    for index in slots {
        let mut candidate = remaining.clone();
        candidate.insert(index, moved_order_id.to_string());
        if validate_active_sequence_barrier(current_sequence, &candidate, states, frozen_order_ids)
            .is_ok()
        {
            return Ok(candidate);
        }
    }
    Err(ProductionMapError::QueueActionNotAllowed)
}

pub(super) fn validate_active_sequence_barrier(
    current_sequence: &[String],
    next_sequence: &[String],
    states: &BTreeMap<String, String>,
    frozen_order_ids: &BTreeSet<String>,
) -> Result<(), ProductionMapError> {
    let mut current_positions = BTreeMap::new();
    for (index, id) in current_sequence.iter().enumerate() {
        current_positions.entry(id.trim()).or_insert(index);
    }
    let mut next_positions = BTreeMap::new();
    for (index, id) in next_sequence.iter().enumerate() {
        next_positions.entry(id.trim()).or_insert(index);
    }
    let mut prefix_max_current_position = Vec::with_capacity(next_sequence.len());
    let mut max_current_position: Option<usize> = None;
    let mut prefix_is_known = true;
    for id in next_sequence {
        if let Some(&current_position) = current_positions.get(id.trim()) {
            max_current_position = Some(
                max_current_position.map_or(current_position, |max| max.max(current_position)),
            );
        } else {
            prefix_is_known = false;
        }
        prefix_max_current_position.push(if prefix_is_known {
            max_current_position
        } else {
            None
        });
    }

    for (order_id, state) in states {
        let Some(parsed) = queue_state::ApparatusQueueOrderState::parse(state) else {
            continue;
        };
        if !parsed.is_active() {
            continue;
        }
        let order_id = order_id.trim();
        if frozen_order_ids.contains(order_id) {
            continue;
        }
        let Some(&next_index) = next_positions.get(order_id) else {
            return Err(ProductionMapError::QueueActionNotAllowed);
        };
        let current_index = current_positions.get(order_id).copied().unwrap_or(0);
        if next_index > current_index {
            return Err(ProductionMapError::QueueActionNotAllowed);
        }
        if next_index > 0 {
            match prefix_max_current_position[next_index - 1] {
                Some(max_position) if max_position < current_index => {}
                _ => return Err(ProductionMapError::QueueActionNotAllowed),
            }
        }
    }
    Ok(())
}
