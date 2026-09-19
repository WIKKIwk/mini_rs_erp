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
                if blocked {
                    ids(&["a", "d", "b", "c"])
                } else {
                    ids(&["d", "a", "b", "c"])
                },
                "{apparatus}: {state}"
            );
            assert_eq!(
                service.apparatus_sequences().await.unwrap()[apparatus],
                saved
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
