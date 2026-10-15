use std::sync::Arc;

use super::fixtures::{canonical_apparatus_stage_map, service_with_default_apparatus};
use crate::core::production_map::*;

const SOURCE: &str = "apparatus:default:bosma_7";
const TARGET: &str = "apparatus:default:bosma_8";
const A: &str = "zakaz-colour-a";
const B: &str = "zakaz-aaa-waiting-b";
const C: &str = "zakaz-target-c";
const D: &str = "zakaz-target-d";
const E: &str = "zakaz-source-e";

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|id| id.to_string()).collect()
}

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "admin".into(),
        ref_: "pending-move".into(),
        display_name: "Admin".into(),
    }
}

async fn fixture() -> (Arc<MemoryProductionMapStore>, ProductionMapService) {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = service_with_default_apparatus(store.clone()).await;
    for (id, apparatus) in [
        (A, SOURCE),
        (B, SOURCE),
        (E, SOURCE),
        (C, TARGET),
        (D, TARGET),
    ] {
        let mut map = canonical_apparatus_stage_map(id, apparatus, "Bosma");
        map.roll_count = Some(7);
        map.width_mm = Some(650.0);
        service.upsert_map(map).await.unwrap();
    }
    store
        .put_apparatus_sequence(SOURCE, ids(&[A, B, E]))
        .await
        .unwrap();
    // D is visible but has not been persisted in an explicit sequence yet.
    store
        .put_apparatus_sequence(TARGET, ids(&[C]))
        .await
        .unwrap();
    (store, service)
}

async fn move_b(service: &ProductionMapService, batch: bool) -> Result<(), ProductionMapError> {
    if batch {
        service
            .move_apparatus_batch(ProductionMapBatchMoveRequest {
                from_apparatus: SOURCE.into(),
                to_apparatus: TARGET.into(),
                map_ids: ids(&[B, B]),
            })
            .await
            .map(|_| ())
    } else {
        service
            .move_apparatus(ProductionMapMoveRequest {
                map_id: B.into(),
                from_apparatus: SOURCE.into(),
                to_apparatus: TARGET.into(),
            })
            .await
            .map(|_| ())
    }
}

async fn assert_moved(store: &MemoryProductionMapStore, service: &ProductionMapService) {
    let sequences = store.apparatus_sequences().await.unwrap();
    assert_eq!(sequences[SOURCE], ids(&[A, E]));
    assert_eq!(
        sequences[TARGET],
        ids(&[C, D, B]),
        "append after existing implicit orders, exactly once"
    );
    let refreshed = service.live_snapshot().await.unwrap();
    assert_eq!(refreshed.sequences[SOURCE], ids(&[A, E]));
    assert_eq!(refreshed.sequences[TARGET], ids(&[C, D, B]));
    assert_eq!(
        refreshed.queue_action_controls[TARGET][B].state,
        queue_state::ApparatusQueueOrderState::Pending
    );
    assert!(
        store
            .order_run_sessions_for_order(B)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .progress_batches_for_order(B)
            .await
            .unwrap()
            .is_empty()
    );
    let map = store.map_by_id(B).await.unwrap().unwrap();
    assert_eq!(
        map.nodes
            .iter()
            .find(|node| node.kind == ProductionMapNodeKind::Apparatus)
            .unwrap()
            .apparatus_id,
        TARGET
    );
}

#[tokio::test]
async fn pending_move_ignores_other_orders_running_and_passed_colour_trials() {
    for outcome in ["running", "passed"] {
        for batch in [true, false] {
            let (store, service) = fixture().await;
            let mut hold = service
                .begin_print_preflight(SOURCE, A, "colour-a", "colour-a", actor())
                .await
                .unwrap();
            if outcome == "passed" {
                hold = service
                    .advance_print_preflight(SOURCE, A, &hold.hold_id, "passed", actor())
                    .await
                    .unwrap();
            }
            let states = store.apparatus_queue_states().await.unwrap();
            move_b(&service, batch)
                .await
                .expect("another order's colour trial must not block a queue move");
            assert_moved(&store, &service).await;
            assert_eq!(
                store
                    .print_preflight_hold_by_idempotency_key("colour-a")
                    .await
                    .unwrap(),
                Some(hold)
            );
            assert_eq!(
                store.apparatus_queue_states().await.unwrap(),
                states,
                "moving must not change production states"
            );
            assert!(
                store
                    .order_run_sessions_for_order(A)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
    }
}

#[tokio::test]
async fn pending_move_persists_tail_after_all_existing_target_orders() {
    for batch in [true, false] {
        let (store, service) = fixture().await;
        move_b(&service, batch).await.unwrap();
        assert_moved(&store, &service).await;
        let reloaded = service_with_default_apparatus(store.clone()).await;
        assert_eq!(
            reloaded.live_snapshot().await.unwrap().sequences[TARGET],
            ids(&[C, D, B])
        );
    }
}

#[tokio::test]
async fn pending_move_preserves_own_preflight_and_batch_atomicity() {
    for outcome in ["running", "passed"] {
        let (store, service) = fixture().await;
        let hold = service
            .begin_print_preflight(SOURCE, A, "own-colour", "own-colour", actor())
            .await
            .unwrap();
        if outcome == "passed" {
            service
                .advance_print_preflight(SOURCE, A, &hold.hold_id, "passed", actor())
                .await
                .unwrap();
        }
        let before_maps = store.maps().await.unwrap();
        let before_sequences = store.apparatus_sequences().await.unwrap();
        for map_ids in [ids(&[A]), ids(&[B, A])] {
            assert!(matches!(
                service
                    .move_apparatus_batch(ProductionMapBatchMoveRequest {
                        from_apparatus: SOURCE.into(),
                        to_apparatus: TARGET.into(),
                        map_ids,
                    })
                    .await,
                Err(ProductionMapError::PrintPreflightActive)
            ));
            assert_eq!(store.maps().await.unwrap(), before_maps);
            assert_eq!(store.apparatus_sequences().await.unwrap(), before_sequences);
        }
    }
}

#[tokio::test]
async fn pending_move_still_rejects_started_paused_frozen_and_completed_orders() {
    for state in ["in_progress", "paused", "frozen", "completed"] {
        for batch in [true, false] {
            let (store, service) = fixture().await;
            store
                .put_apparatus_queue_states(SOURCE, [(B.into(), state.into())].into())
                .await
                .unwrap();
            let before_maps = store.maps().await.unwrap();
            let before_sequences = store.apparatus_sequences().await.unwrap();
            assert!(matches!(
                move_b(&service, batch).await,
                Err(ProductionMapError::StartedOrderMoveRequiresTransfer)
            ));
            assert_eq!(store.maps().await.unwrap(), before_maps);
            assert_eq!(store.apparatus_sequences().await.unwrap(), before_sequences);
        }
    }
}

#[tokio::test]
async fn pending_move_rejects_live_session_even_if_queue_row_is_missing() {
    for status in [OrderRunStatus::Active, OrderRunStatus::Paused] {
        let (store, service) = fixture().await;
        let session = OrderRunSession {
            session_id: "existing-run".into(),
            apparatus: SOURCE.into(),
            order_id: B.into(),
            stage_node_id: "apparatus".into(),
            status,
            worker_role: "aparatchi".into(),
            worker_ref: "operator".into(),
            worker_display_name: "Operator".into(),
            started_at_unix: 1,
            updated_at_unix: 2,
            payload_json: serde_json::json!({}),
        };
        store.put_order_run_session(session.clone()).await.unwrap();
        assert!(matches!(
            move_b(&service, true).await,
            Err(ProductionMapError::StartedOrderMoveRequiresTransfer)
        ));
        assert_eq!(
            store.active_order_run_session(SOURCE, B).await.unwrap(),
            Some(session)
        );
    }
}

#[tokio::test]
async fn pending_move_rejects_stale_queue_without_changing_maps_or_sequences() {
    let (store, _) = fixture().await;
    let before_maps = store.maps().await.unwrap();
    let before_sequences = store.apparatus_sequences().await.unwrap();
    let expected = [
        (SOURCE.to_string(), ids(&[A, B])),
        (TARGET.to_string(), ids(&[C])),
    ]
    .into();
    let next = [
        (SOURCE.to_string(), ids(&[A])),
        (TARGET.to_string(), ids(&[C, D, B])),
    ]
    .into();
    let mut map = store.map_by_id(B).await.unwrap().unwrap();
    map.nodes
        .iter_mut()
        .find(|node| node.kind == ProductionMapNodeKind::Apparatus)
        .unwrap()
        .apparatus_id = TARGET.into();
    assert!(matches!(
        store
            .put_maps_with_sequences(&[map], &expected, &next)
            .await,
        Err(ProductionMapError::QueueReorderConflict)
    ));
    assert_eq!(store.maps().await.unwrap(), before_maps);
    assert_eq!(store.apparatus_sequences().await.unwrap(), before_sequences);
}
