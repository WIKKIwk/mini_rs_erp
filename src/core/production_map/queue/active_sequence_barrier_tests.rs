use super::*;

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn active(order_id: &str) -> BTreeMap<String, String> {
    BTreeMap::from([(
        order_id.to_string(),
        queue_state::ApparatusQueueOrderState::InProgress
            .as_str()
            .to_string(),
    )])
}

#[test]
fn nearest_slot_is_legal_and_minimal_for_every_drag_and_active_barrier_combination() {
    let current = ids(&["a", "b", "c", "d", "e"]);
    for mask in 0..32 {
        let states = current
            .iter()
            .enumerate()
            .filter(|(index, _)| mask & (1 << index) != 0)
            .map(|(_, id)| (id.clone(), "in_progress".to_string()))
            .collect::<BTreeMap<_, _>>();
        for old_index in 0..current.len() {
            for requested_index in 0..current.len() {
                let mut requested = current.clone();
                let moved = requested.remove(old_index);
                requested.insert(requested_index, moved.clone());
                let saved = nearest_allowed_sequence(
                    &current,
                    &requested,
                    &moved,
                    &states,
                    &BTreeSet::new(),
                )
                .unwrap();
                validate_active_sequence_barrier(&current, &saved, &states, &BTreeSet::new())
                    .unwrap();
                let saved_index = saved.iter().position(|id| id == &moved).unwrap();
                let remaining = current
                    .iter()
                    .filter(|id| *id != &moved)
                    .cloned()
                    .collect::<Vec<_>>();
                assert_eq!(
                    saved
                        .iter()
                        .filter(|id| *id != &moved)
                        .cloned()
                        .collect::<Vec<_>>(),
                    remaining
                );
                for index in 0..current.len() {
                    let mut candidate = remaining.clone();
                    candidate.insert(index, moved.clone());
                    if validate_active_sequence_barrier(
                        &current,
                        &candidate,
                        &states,
                        &BTreeSet::new(),
                    )
                    .is_ok()
                    {
                        assert!(
                            saved_index.abs_diff(requested_index)
                                <= index.abs_diff(requested_index)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn paused_order_does_not_block_sequence_changes() {
    let states = BTreeMap::from([("a".to_string(), "paused".to_string())]);
    assert_eq!(
        validate_active_sequence_barrier(
            &ids(&["a", "b", "c"]),
            &ids(&["b", "c", "a"]),
            &states,
            &BTreeSet::new(),
        ),
        Ok(())
    );
}

#[test]
fn unchanged_active_prefix_is_allowed() {
    assert_eq!(
        validate_active_sequence_barrier(
            &ids(&["a", "b", "c"]),
            &ids(&["a", "b", "c"]),
            &active("b"),
            &BTreeSet::new(),
        ),
        Ok(())
    );
}

#[test]
fn unknown_order_before_active_order_is_rejected() {
    assert_eq!(
        validate_active_sequence_barrier(
            &ids(&["a", "b", "c"]),
            &ids(&["unknown", "b", "a", "c"]),
            &active("b"),
            &BTreeSet::new(),
        ),
        Err(ProductionMapError::QueueActionNotAllowed)
    );
}

#[test]
fn later_order_moved_before_active_order_is_rejected() {
    assert_eq!(
        validate_active_sequence_barrier(
            &ids(&["a", "b", "c"]),
            &ids(&["c", "b", "a"]),
            &active("b"),
            &BTreeSet::new(),
        ),
        Err(ProductionMapError::QueueActionNotAllowed)
    );
}
