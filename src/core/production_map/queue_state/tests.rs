use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;

use super::*;
use crate::core::production_map::ProductionMapError;

#[test]
fn action_encoding_preserves_persisted_and_wire_names() {
    use ApparatusQueueAction::*;
    for (action, name) in [
        (Start, "start"),
        (Pause, "pause"),
        (Freeze, "freeze"),
        (DetachRoll, "detach_roll"),
        (Resume, "resume"),
        (Merge, "merge"),
        (RollComplete, "roll_complete"),
        (Complete, "complete"),
    ] {
        assert_eq!(action.as_str(), name);
        assert_eq!(ApparatusQueueAction::parse(name), Some(action));
        assert_eq!(
            ApparatusQueueAction::parse(&format!(" {} ", name.to_uppercase())),
            Some(action)
        );
        assert_eq!(serde_json::to_value(action).unwrap(), name);
    }
    assert_eq!(ApparatusQueueAction::parse(""), None);
    assert_eq!(ApparatusQueueAction::parse("unknown"), None);
}

#[test]
fn progress_output_actions_have_one_canonical_classification() {
    for action in [
        ApparatusQueueAction::Pause,
        ApparatusQueueAction::DetachRoll,
        ApparatusQueueAction::RollComplete,
        ApparatusQueueAction::Complete,
    ] {
        assert!(action.records_progress_output());
    }
    assert!(ApparatusQueueAction::Pause.creates_resumable_output());
    assert!(ApparatusQueueAction::DetachRoll.creates_resumable_output());
    for action in [
        ApparatusQueueAction::Start,
        ApparatusQueueAction::Freeze,
        ApparatusQueueAction::Resume,
        ApparatusQueueAction::Merge,
    ] {
        assert!(!action.records_progress_output());
        assert!(!action.creates_resumable_output());
    }
    assert!(!ApparatusQueueAction::RollComplete.creates_resumable_output());
    assert!(!ApparatusQueueAction::Complete.creates_resumable_output());
}

#[test]
fn first_actionable_skips_completed_orders() {
    let sequence = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let mut states = BTreeMap::from([("a".to_string(), ApparatusQueueOrderState::Completed)]);
    assert_eq!(first_actionable_order_id(&sequence, &states), Some("b"));
    states.insert("b".to_string(), ApparatusQueueOrderState::InProgress);
    assert_eq!(first_actionable_order_id(&sequence, &states), Some("b"));
}

#[test]
fn first_actionable_prioritizes_in_progress_order() {
    let sequence = vec!["a".to_string(), "b".to_string()];
    let states = BTreeMap::from([("b".to_string(), ApparatusQueueOrderState::InProgress)]);
    assert_eq!(first_actionable_order_id(&sequence, &states), Some("b"));
}

#[test]
fn effective_sequence_appends_unsequenced_orders_oldest_first() {
    let visible = vec!["zakaz-new".to_string(), "zakaz-old".to_string()];
    assert_eq!(
        effective_apparatus_sequence(&[], &visible),
        vec!["zakaz-old".to_string(), "zakaz-new".to_string()],
    );
}

#[test]
fn effective_sequence_appends_new_orders_after_saved_sequence() {
    let stored = vec!["zakaz-old".to_string()];
    let visible = vec![
        "zakaz-newer".to_string(),
        "zakaz-new".to_string(),
        "zakaz-old".to_string(),
    ];
    assert_eq!(
        effective_apparatus_sequence(&stored, &visible),
        vec![
            "zakaz-old".to_string(),
            "zakaz-new".to_string(),
            "zakaz-newer".to_string(),
        ],
    );
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
fn effective_sequence_excludes_frozen_orders_without_readding_them() {
    let stored = vec!["order-1".to_string(), "order-2".to_string()];
    let visible = vec!["order-2".to_string(), "order-1".to_string()];
    let excluded = BTreeSet::from(["order-1".to_string()]);

    assert_eq!(
        effective_apparatus_sequence_excluding(&stored, &visible, &excluded),
        vec!["order-2".to_string()],
    );
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
fn merge_keeps_only_an_in_progress_order_in_progress() {
    let sequence = vec!["a".to_string()];
    let mut states = BTreeMap::new();

    apply_queue_action(&sequence, &mut states, "a", ApparatusQueueAction::Merge)
        .expect_err("pending order cannot merge");
    apply_queue_action(&sequence, &mut states, "a", ApparatusQueueAction::Start).expect("start");
    apply_queue_action(&sequence, &mut states, "a", ApparatusQueueAction::Merge).expect("merge");
    assert_eq!(states.get("a"), Some(&ApparatusQueueOrderState::InProgress));
}

#[test]
fn frozen_order_cannot_resume_until_admin_unfreezes_it() {
    let sequence = vec!["frozen".to_string(), "next".to_string()];
    let mut states = BTreeMap::from([
        ("frozen".to_string(), ApparatusQueueOrderState::Frozen),
        ("next".to_string(), ApparatusQueueOrderState::Pending),
    ]);

    apply_queue_action(
        &sequence,
        &mut states,
        "frozen",
        ApparatusQueueAction::Resume,
    )
    .expect_err("frozen order must wait for admin unfreeze");
    assert_eq!(
        states.get("frozen"),
        Some(&ApparatusQueueOrderState::Frozen)
    );
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
fn unordered_action_allows_start_when_other_order_is_paused() {
    let mut states = BTreeMap::new();
    apply_unordered_queue_action(&mut states, "a", ApparatusQueueAction::Start)
        .expect("start first order");
    apply_unordered_queue_action(&mut states, "a", ApparatusQueueAction::Pause)
        .expect("pause first order");

    apply_unordered_queue_action(&mut states, "b", ApparatusQueueAction::Start)
        .expect("start second order after pause");

    assert_eq!(states.get("a"), Some(&ApparatusQueueOrderState::Paused));
    assert_eq!(states.get("b"), Some(&ApparatusQueueOrderState::InProgress));
}

#[test]
fn resolve_apparatus_storage_key_requires_exact_canonical_id() {
    let keys = vec![
        "apparatus:catalog:press-007".to_string(),
        "apparatus:catalog:godex-demo".to_string(),
    ];
    assert_eq!(
        resolve_apparatus_storage_key("apparatus:catalog:press-007", &keys),
        "apparatus:catalog:press-007"
    );
    assert_eq!(
        resolve_apparatus_storage_key("7 ta rangli pechat - A", &keys),
        ""
    );
}

#[test]
fn apparatus_ids_match_requires_canonical_id() {
    assert!(!apparatus_ids_match("Laminatsiya - A", "Laminatsiya"));
    assert!(apparatus_ids_match(
        "apparatus:catalog:lam-001",
        "apparatus:catalog:lam-001"
    ));
}

#[test]
fn next_stage_identity_match_requires_canonical_id() {
    assert!(next_stage_apparatus_matches(
        "apparatus:catalog:lam-001",
        "apparatus:catalog:lam-001"
    ));
    assert!(!next_stage_apparatus_matches(
        "apparatus:catalog:lam-001",
        "apparatus:catalog:lam-002"
    ));
}

#[test]
fn apparatus_search_key_accepts_canonical_id_only() {
    assert_eq!(
        apparatus_search_key("apparatus:catalog:press-007"),
        "apparatus:catalog:press-007"
    );
    assert_eq!(apparatus_search_key("Laminatsiya - A"), "");
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
