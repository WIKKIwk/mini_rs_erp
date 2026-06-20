use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;

use super::*;
use crate::core::production_map::ProductionMapError;

#[test]
fn first_actionable_skips_completed_orders() {
    let sequence = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let mut states = BTreeMap::from([("a".to_string(), ApparatusQueueOrderState::Completed)]);
    assert_eq!(
        first_actionable_order_id(&sequence, &states).as_deref(),
        Some("b")
    );
    states.insert("b".to_string(), ApparatusQueueOrderState::InProgress);
    assert_eq!(
        first_actionable_order_id(&sequence, &states).as_deref(),
        Some("b")
    );
}

#[test]
fn first_actionable_prioritizes_in_progress_order() {
    let sequence = vec!["a".to_string(), "b".to_string()];
    let states = BTreeMap::from([("b".to_string(), ApparatusQueueOrderState::InProgress)]);
    assert_eq!(
        first_actionable_order_id(&sequence, &states).as_deref(),
        Some("b")
    );
}

#[test]
fn effective_sequence_uses_visible_order_when_store_empty() {
    let visible = vec!["zakaz-1236".to_string(), "zakaz-6687".to_string()];
    assert_eq!(effective_apparatus_sequence(&[], &visible), visible,);
}

#[test]
fn effective_sequence_skips_missing_orders() {
    let stored = vec![
        "zakaz-old".to_string(),
        "zakaz-1236".to_string(),
        "zakaz-6687".to_string(),
    ];
    let visible = vec!["zakaz-1236".to_string(), "zakaz-6687".to_string()];
    assert_eq!(effective_apparatus_sequence(&stored, &visible), visible,);
}

#[test]
fn start_and_complete_flow() {
    let sequence = vec!["a".to_string(), "b".to_string()];
    let mut states = BTreeMap::new();
    apply_queue_action(&sequence, &mut states, "b", ApparatusQueueAction::Start)
        .expect_err("only first pending order");
    apply_queue_action(&sequence, &mut states, "a", ApparatusQueueAction::Start)
        .expect("start first");
    assert_eq!(states.get("a"), Some(&ApparatusQueueOrderState::InProgress));
    apply_queue_action(&sequence, &mut states, "a", ApparatusQueueAction::Complete)
        .expect("complete first");
    assert_eq!(states.get("a"), Some(&ApparatusQueueOrderState::Completed));
    apply_queue_action(&sequence, &mut states, "b", ApparatusQueueAction::Start)
        .expect("start second");
}

#[test]
fn progress_actions_pause_resume_and_complete_active_order() {
    let sequence = vec!["a".to_string()];
    let mut states = BTreeMap::new();

    apply_queue_action(&sequence, &mut states, "a", ApparatusQueueAction::Start).expect("start");
    assert_eq!(states.get("a"), Some(&ApparatusQueueOrderState::InProgress));

    apply_queue_action(&sequence, &mut states, "a", ApparatusQueueAction::Pause).expect("pause");
    assert_eq!(states.get("a"), Some(&ApparatusQueueOrderState::Paused));

    apply_queue_action(&sequence, &mut states, "a", ApparatusQueueAction::Resume).expect("resume");
    assert_eq!(states.get("a"), Some(&ApparatusQueueOrderState::InProgress));

    apply_queue_action(&sequence, &mut states, "a", ApparatusQueueAction::Complete)
        .expect("complete");
    assert_eq!(states.get("a"), Some(&ApparatusQueueOrderState::Completed));
}

#[test]
fn unordered_action_allows_any_pending_order() {
    let mut states = BTreeMap::new();
    apply_unordered_queue_action(&mut states, "b", ApparatusQueueAction::Start)
        .expect("free pick can start later order");
    assert_eq!(states.get("b"), Some(&ApparatusQueueOrderState::InProgress));
    apply_unordered_queue_action(&mut states, "b", ApparatusQueueAction::Complete)
        .expect("free pick completes started order");
    assert_eq!(states.get("b"), Some(&ApparatusQueueOrderState::Completed));
    apply_unordered_queue_action(&mut states, "b", ApparatusQueueAction::Start)
        .expect_err("completed order cannot restart");
}

#[test]
fn unordered_action_blocks_second_start_while_order_in_progress() {
    let mut states = BTreeMap::new();
    apply_unordered_queue_action(&mut states, "a", ApparatusQueueAction::Start)
        .expect("start first order");
    let result = apply_unordered_queue_action(&mut states, "b", ApparatusQueueAction::Start);
    assert_eq!(result, Err(ProductionMapError::QueueActionNotAllowed));
    assert_eq!(states.get("b"), None);
}

#[test]
fn resolve_apparatus_storage_key_matches_pechat_suffixes() {
    let keys = vec![
        "7 ta rangli pechat".to_string(),
        "Godex aparat - DEMO".to_string(),
    ];
    assert_eq!(
        resolve_apparatus_storage_key("7 ta rangli pechat - A", &keys),
        "7 ta rangli pechat"
    );
}

#[test]
fn apparatus_titles_match_warehouse_instance_suffixes() {
    assert!(apparatus_titles_match("Laminatsiya - A", "Laminatsiya"));
    assert!(apparatus_titles_match("Paket aparat - A", "Paket aparat"));
}

proptest! {
    #[test]
    fn effective_sequence_only_contains_visible_unique_ids(
        stored in proptest::collection::vec("[a-z]{1,8}", 0..24),
        visible_set in proptest::collection::btree_set("[a-z]{1,8}", 0..24),
    ) {
        let visible = visible_set.into_iter().collect::<Vec<_>>();
        let result = effective_apparatus_sequence(&stored, &visible);
        let visible_set = visible
            .iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect::<BTreeSet<_>>();
        let result_set = result.iter().cloned().collect::<BTreeSet<_>>();
        prop_assert_eq!(result.len(), result_set.len());
        prop_assert!(result.iter().all(|id| visible_set.contains(id)));
        prop_assert!(visible_set.iter().all(|id| result_set.contains(id)));
    }
}
