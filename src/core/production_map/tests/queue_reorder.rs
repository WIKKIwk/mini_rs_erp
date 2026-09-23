use std::collections::BTreeMap;
use std::sync::Arc;

use super::fixtures::{canonical_apparatus_stage_map, service_with_default_apparatus};
use crate::core::production_map::*;

const PRINT: &str = "apparatus:default:bosma_7";
const LAMINATION: &str = "apparatus:default:asset-007";

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|id| id.to_string()).collect()
}

async fn fixture(apparatus: &str) -> (ProductionMapService, Arc<MemoryProductionMapStore>) {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = service_with_default_apparatus(store.clone()).await;
    for id in ["a", "b", "c", "d"] {
        service
            .upsert_map(canonical_apparatus_stage_map(id, apparatus, "Apparat"))
            .await
            .unwrap();
    }
    service
        .set_apparatus_sequence(apparatus, ids(&["a", "b", "c", "d"]))
        .await
        .unwrap();
    (service, store)
}

#[tokio::test]
async fn reorder_respects_every_queue_state_and_print_pause_policy() {
    for apparatus in [PRINT, LAMINATION] {
        for state in [
            "pending",
            "print_preflight",
            "in_progress",
            "paused",
            "completed",
            "frozen",
        ] {
            let (service, store) = fixture(apparatus).await;
            store
                .put_apparatus_queue_states(apparatus, BTreeMap::from([("a".into(), state.into())]))
                .await
                .unwrap();
            let saved = service
                .reorder_apparatus_sequence(apparatus, ids(&["d", "a", "b", "c"]), "d")
                .await
                .unwrap();
            let blocked = matches!(state, "in_progress" | "print_preflight")
                || (state == "paused" && apparatus == PRINT);
            assert_eq!(
                saved,
                if state == "frozen" {
                    ids(&["d", "b", "c", "a"])
                } else if blocked {
                    ids(&["a", "d", "b", "c"])
                } else {
                    ids(&["d", "a", "b", "c"])
                },
                "{apparatus}: {state}"
            );
            assert_eq!(
                service.apparatus_sequences().await.unwrap()[apparatus],
                if state == "frozen" {
                    ids(&["d", "b", "c"])
                } else {
                    saved
                }
            );
        }
    }
}

#[tokio::test]
async fn reorder_clamps_around_multiple_barriers_in_both_directions() {
    let (service, store) = fixture(PRINT).await;
    store
        .put_apparatus_queue_states(
            PRINT,
            BTreeMap::from([
                ("a".into(), "paused".into()),
                ("c".into(), "in_progress".into()),
            ]),
        )
        .await
        .unwrap();
    assert_eq!(
        service
            .reorder_apparatus_sequence(PRINT, ids(&["d", "a", "b", "c"]), "d")
            .await
            .unwrap(),
        ids(&["a", "b", "c", "d"])
    );
    assert_eq!(
        service
            .reorder_apparatus_sequence(PRINT, ids(&["a", "c", "d", "b"]), "b")
            .await
            .unwrap(),
        ids(&["a", "c", "d", "b"])
    );
    // Moving the running order down must not allow another order to pass it.
    assert_eq!(
        service
            .reorder_apparatus_sequence(PRINT, ids(&["a", "d", "b", "c"]), "c")
            .await
            .unwrap(),
        ids(&["a", "c", "d", "b"])
    );
}

#[tokio::test]
async fn reorder_preserves_hidden_orders_and_rejects_invalid_input_without_writing() {
    let (service, store) = fixture(PRINT).await;
    store
        .put_apparatus_queue_states(PRINT, BTreeMap::from([("a".into(), "in_progress".into())]))
        .await
        .unwrap();
    assert_eq!(
        service
            .reorder_apparatus_sequence(PRINT, ids(&["d", "b"]), "d")
            .await
            .unwrap(),
        ids(&["a", "d", "b", "c"])
    );
    let before = service.apparatus_sequences().await.unwrap();
    for (requested, moved) in [
        (ids(&["a", "d", "d", "b", "c"]), "d"),
        (ids(&["a", "b", "c"]), "d"),
        (ids(&["missing", "a", "b", "c", "d"]), "missing"),
    ] {
        assert!(
            service
                .reorder_apparatus_sequence(PRINT, requested, moved)
                .await
                .is_err()
        );
        assert_eq!(service.apparatus_sequences().await.unwrap(), before);
    }
}

#[tokio::test]
async fn reorder_running_and_passed_preflight_only_locks_the_reserved_order() {
    let ids = |values: &[&str]| {
        values
            .iter()
            .map(|id| format!("zakaz-{id}"))
            .collect::<Vec<_>>()
    };
    for outcome in ["running", "passed", "failed", "cancel"] {
        let service =
            service_with_default_apparatus(Arc::new(MemoryProductionMapStore::new())).await;
        for id in ids(&["a", "b", "c", "d"]) {
            service
                .upsert_map(canonical_apparatus_stage_map(&id, PRINT, "Apparat"))
                .await
                .unwrap();
        }
        service
            .set_apparatus_sequence(PRINT, ids(&["a", "b", "c", "d"]))
            .await
            .unwrap();
        let actor = QueueActionActor {
            role: "aparatchi".into(),
            ref_: "worker".into(),
            display_name: "Worker".into(),
        };
        let hold = service
            .begin_print_preflight(PRINT, "zakaz-a", "trial", "trial", actor.clone())
            .await
            .unwrap();
        if outcome != "running" {
            service
                .advance_print_preflight(PRINT, "zakaz-a", &hold.hold_id, outcome, actor.clone())
                .await
                .unwrap();
        }
        let saved = service
            .reorder_apparatus_sequence(PRINT, ids(&["d", "a", "b", "c"]), "zakaz-d")
            .await
            .unwrap();
        assert_eq!(
            saved,
            if matches!(outcome, "failed" | "cancel") {
                ids(&["d", "a", "b", "c"])
            } else {
                ids(&["a", "d", "b", "c"])
            }
        );
        assert_eq!(service.apparatus_sequences().await.unwrap()[PRINT], saved);
    }
}

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "admin".into(),
        ref_: "queue-test".into(),
        display_name: "Test".into(),
    }
}

async fn command(service: &ProductionMapService, key: &str) -> SequenceMove {
    let version = service
        .live_snapshot_shared()
        .await
        .unwrap()
        .sequence_versions[PRINT]
        .clone();
    SequenceMove {
        apparatus: PRINT.into(),
        order_id: "d".into(),
        before_order_id: Some("b".into()),
        after_order_id: None,
        expected_version: version,
        idempotency_key: key.into(),
    }
}

#[tokio::test]
async fn reorder_frozen_control_does_not_block_other_orders_or_reenter_persisted_queue() {
    let (service, store) = fixture(PRINT).await;
    store
        .put_order_control_state(OrderControlRecord {
            order_id: "c".into(),
            state: OrderControlState::Frozen,
            actor: actor(),
            requested_at_unix: 1,
            frozen_at_unix: Some(1),
            freeze_request: None,
            early_close: None,
        })
        .await
        .unwrap();
    // Reproduces the old app's full-list payload, including the frozen row.
    let old_reply = service
        .reorder_apparatus_sequence(PRINT, ids(&["d", "a", "b", "c"]), "d")
        .await
        .unwrap();
    assert!(
        old_reply.contains(&"c".into()),
        "old mobile response contract"
    );
    assert_eq!(
        store.apparatus_sequences().await.unwrap()[PRINT],
        ids(&["d", "a", "b"])
    );
    assert_eq!(
        store.order_control_states().await.unwrap()["c"].state,
        OrderControlState::Frozen
    );
    assert_eq!(
        service
            .reorder_apparatus_sequence(PRINT, ids(&["c", "d", "a", "b"]), "c")
            .await,
        Err(ProductionMapError::QueueReorderFrozen)
    );
    service.notify_live();
    let mut move_command = command(&service, "frozen-neighbor").await;
    move_command.before_order_id = Some("a".into());
    let result = service
        .move_apparatus_sequence(move_command, actor())
        .await
        .unwrap();
    assert_eq!(result.order_ids, ids(&["d", "a", "b"]));
}

#[tokio::test]
async fn reorder_command_retry_is_durable_and_does_not_undo_a_later_move() {
    let (service, store) = fixture(PRINT).await;
    let first = command(&service, "first").await;
    let first_result = service
        .move_apparatus_sequence(first.clone(), actor())
        .await
        .unwrap();
    assert_eq!(first_result.order_ids, ids(&["a", "d", "b", "c"]));
    let mut second = command(&service, "second").await;
    second.before_order_id = None;
    service
        .move_apparatus_sequence(second, actor())
        .await
        .unwrap();
    let before_retry = store.apparatus_sequences().await.unwrap();
    // A new service instance models loss of all process-local caches.
    let restarted = service_with_default_apparatus(store.clone()).await;
    assert_eq!(
        restarted
            .move_apparatus_sequence(first.clone(), actor())
            .await
            .unwrap(),
        first_result
    );
    assert_eq!(store.apparatus_sequences().await.unwrap(), before_retry);
    let mut reused_key = first;
    reused_key.before_order_id = Some("a".into());
    assert_eq!(
        restarted.move_apparatus_sequence(reused_key, actor()).await,
        Err(ProductionMapError::QueueReorderIdempotencyConflict)
    );
}

#[tokio::test]
async fn reorder_two_clients_with_same_version_only_one_wins() {
    let (service, store) = fixture(PRINT).await;
    let first = command(&service, "client-1").await;
    let mut second = first.clone();
    second.idempotency_key = "client-2".into();
    second.before_order_id = Some("a".into());
    let other = service_with_default_apparatus(store.clone()).await;
    let (one, two) = tokio::join!(
        service.move_apparatus_sequence(first, actor()),
        other.move_apparatus_sequence(second, actor())
    );
    assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
    let error = if one.is_err() { one } else { two };
    assert_eq!(error, Err(ProductionMapError::QueueReorderConflict));
    assert_eq!(store.apparatus_sequences().await.unwrap()[PRINT].len(), 4);
}

#[tokio::test]
async fn reorder_version_is_apparatus_scoped_and_freeze_or_start_invalidates_it() {
    let (service, store) = fixture(PRINT).await;
    let first = command(&service, "scope").await;
    store
        .put_apparatus_queue_states(
            LAMINATION,
            BTreeMap::from([("other".into(), "in_progress".into())]),
        )
        .await
        .unwrap();
    service
        .move_apparatus_sequence(first, actor())
        .await
        .unwrap();
    let stale = command(&service, "stale").await;
    store
        .put_apparatus_queue_states(PRINT, BTreeMap::from([("a".into(), "in_progress".into())]))
        .await
        .unwrap();
    let before = store.apparatus_sequences().await.unwrap();
    assert_eq!(
        service.move_apparatus_sequence(stale, actor()).await,
        Err(ProductionMapError::QueueReorderConflict)
    );
    assert_eq!(store.apparatus_sequences().await.unwrap(), before);
}

#[test]
fn reorder_command_rejects_ambiguous_or_self_anchors() {
    let base = SequenceMove {
        apparatus: PRINT.into(),
        order_id: "b".into(),
        before_order_id: Some("a".into()),
        after_order_id: None,
        expected_version: "0".repeat(64),
        idempotency_key: "test".into(),
    };
    for command in [
        SequenceMove {
            after_order_id: Some("c".into()),
            ..base.clone()
        },
        SequenceMove {
            before_order_id: Some("b".into()),
            ..base.clone()
        },
        SequenceMove {
            expected_version: String::new(),
            ..base.clone()
        },
        SequenceMove {
            idempotency_key: String::new(),
            ..base.clone()
        },
    ] {
        assert_eq!(
            command.validate(),
            Err(ProductionMapError::QueueReorderInvalid)
        );
    }
}

proptest::proptest! {
    #[test]
    fn reorder_never_loses_duplicates_or_reorders_unmoved_orders(
        count in 2usize..60, moved_seed in 0usize..60, target_seed in 0usize..60,
        barrier_seed in 0usize..60,
    ) {
        let orders = (0..count).map(|i| format!("order-{i}")).collect::<Vec<_>>();
        let moved = orders[moved_seed % count].clone();
        let mut remaining = orders.clone();
        remaining.retain(|id| id != &moved);
        let anchor = remaining[target_seed % remaining.len()].clone();
        let state = SequenceMoveState { apparatus: PRINT.into(), order_ids: orders.clone(),
            states: BTreeMap::from([(orders[barrier_seed % count].clone(), "in_progress".into())]),
            frozen: Default::default() };
        let command = SequenceMove { apparatus: PRINT.into(), order_id: moved.clone(),
            before_order_id: Some(anchor), after_order_id: None,
            expected_version: state.version(), idempotency_key: "property".into() };
        let result = state.apply(&command).unwrap();
        proptest::prop_assert_eq!(result.order_ids.len(), count);
        proptest::prop_assert_eq!(result.order_ids.iter().collect::<std::collections::BTreeSet<_>>().len(), count);
        let untouched = result.order_ids.iter().filter(|id| **id != moved).cloned().collect::<Vec<_>>();
        proptest::prop_assert_eq!(untouched, remaining);
        proptest::prop_assert!(super::super::service_queue_support::validate_active_sequence_barrier(
            &orders, &result.order_ids, &state.states, &state.frozen).is_ok());
    }
}
