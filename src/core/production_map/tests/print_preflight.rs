use std::sync::Arc;

use crate::core::production_map::*;

use super::fixtures::{canonical_apparatus_stage_map, service_with_default_apparatus};

const PRINT_ID: &str = "apparatus:default:bosma_7";

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".to_string(),
        ref_: "print-preflight-test".to_string(),
        display_name: "Print preflight test".to_string(),
    }
}

#[tokio::test]
async fn another_orders_preflight_blocks_start_with_a_distinct_reason() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = service_with_default_apparatus(store.clone()).await;
    let first = "zakaz-preflight-first";
    let second = "zakaz-preflight-second";
    for order_id in [first, second] {
        service
            .upsert_map(canonical_apparatus_stage_map(
                order_id,
                PRINT_ID,
                "7 ta rangli bosma aparat",
            ))
            .await
            .unwrap();
    }
    service
        .set_apparatus_sequence(PRINT_ID, vec![first.into(), second.into()])
        .await
        .unwrap();
    let hold = service
        .begin_print_preflight(
            PRINT_ID,
            first,
            "other-order-hold",
            "other-order-hold",
            actor(),
        )
        .await
        .unwrap();
    for status in ["running", "passed"] {
        if status == "passed" {
            service
                .advance_print_preflight(PRINT_ID, first, &hold.hold_id, "passed", actor())
                .await
                .unwrap();
        }
        let snapshot = service.live_snapshot().await.unwrap();
        let controls = &snapshot.queue_action_controls[PRINT_ID];
        assert_eq!(
            controls[first].print_preflight.as_ref().unwrap().status.as_str(),
            status
        );
        assert_eq!(
            controls[second].interaction.blocking_reason_code,
            "print_preflight_other_order_active"
        );
        assert!(!controls[second].print_preflight_allowed);
        assert!(controls[second].print_preflight.is_none());
        assert!(controls[second].allowed_actions.is_empty());
        assert!(matches!(
            service
                .begin_print_preflight(PRINT_ID, second, "blocked", "blocked", actor())
                .await,
            Err(ProductionMapError::PrintPreflightActive)
        ));
    }
}

#[tokio::test]
async fn color_button_starts_preflight_and_locks_order_operations() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = service_with_default_apparatus(store.clone()).await;
    let order_id = "zakaz-print-preflight-running";
    service
        .upsert_map(canonical_apparatus_stage_map(
            order_id,
            PRINT_ID,
            "7 ta rangli bosma aparat",
        ))
        .await
        .expect("map");

    let before = service.live_snapshot_shared_with_revision().await.unwrap();
    let mut stream = service.subscribe_live();

    let hold = service
        .begin_print_preflight(
            PRINT_ID,
            order_id,
            "print-preflight-running",
            "print-preflight-running-idempotency",
            actor(),
        )
        .await
        .expect("preflight start");
    assert_eq!(hold.status, PrintPreflightStatus::Running);
    assert!(hold.is_live_at(i64::MAX), "a running trial cannot expire");
    stream
        .try_recv()
        .expect("ordinary order-list stream notification");
    let (after, revision) = service.live_snapshot_shared_with_revision().await.unwrap();
    assert!(revision > before.1);
    assert_eq!(after.queue_states[PRINT_ID][order_id], "print_preflight");
    let status = &after.order_statuses[order_id];
    assert_eq!(status.order_status, "print_preflight");
    assert_eq!(status.work_status, "print_preflight");
    assert_eq!(status.flow_status, "print_preflight");
    assert_eq!(
        status.lifecycle_status,
        ProductionOrderLifecycleStatus::Released
    );
    assert!(
        store
            .order_run_sessions_for_order(order_id)
            .await
            .unwrap()
            .is_empty()
    );
    let reopened = service_with_default_apparatus(store.clone()).await;
    let restored = reopened.live_snapshot().await.unwrap();
    assert_eq!(restored.order_statuses[order_id], *status);
    assert_eq!(restored.queue_states[PRINT_ID][order_id], "print_preflight");

    let snapshot = service.live_snapshot().await.expect("snapshot");
    let control = snapshot
        .queue_action_controls
        .get(PRINT_ID)
        .and_then(|orders| orders.get(order_id))
        .expect("queue control");
    assert_eq!(
        control
            .print_preflight
            .as_ref()
            .expect("active preflight")
            .status,
        PrintPreflightStatus::Running
    );
    assert!(control.allowed_actions.is_empty());
    assert_eq!(
        control.interaction.blocking_reason_code,
        "print_preflight_active"
    );

    assert!(matches!(
        service
            .set_apparatus_sequence(PRINT_ID, vec![order_id.to_string()])
            .await,
        Err(ProductionMapError::PrintPreflightActive)
    ));

    let Err(ProductionMapError::OrderDeleteBlocked(blockers)) =
        service.delete_order(order_id).await
    else {
        panic!("active color preflight must block deletion");
    };
    assert!(
        blockers
            .iter()
            .any(|blocker| blocker.code == "print_preflight_active"),
        "preflight blocker missing: {blockers:?}"
    );

    service
        .advance_print_preflight(PRINT_ID, order_id, &hold.hold_id, "failed", actor())
        .await
        .expect("failed trial");
    stream.try_recv().expect("failure uses the same stream");
    let reset = service.live_snapshot().await.unwrap();
    assert_eq!(
        reset.order_statuses[order_id],
        before.0.order_statuses[order_id]
    );
    assert_eq!(
        reset
            .queue_states
            .get(PRINT_ID)
            .and_then(|s| s.get(order_id)),
        before
            .0
            .queue_states
            .get(PRINT_ID)
            .and_then(|s| s.get(order_id))
    );
    service
        .set_apparatus_sequence(PRINT_ID, vec![order_id.to_string()])
        .await
        .expect("unlocked after failure");

    let retry = service
        .begin_print_preflight(PRINT_ID, order_id, "retry", "retry", actor())
        .await
        .expect("another colour trial");
    service
        .advance_print_preflight(PRINT_ID, order_id, &retry.hold_id, "passed", actor())
        .await
        .expect("colour matched");
    let ready = service.live_snapshot().await.unwrap();
    assert_eq!(
        ready.order_statuses[order_id].order_status,
        "print_preflight"
    );
    assert!(
        ready.queue_action_controls[PRINT_ID][order_id]
            .allowed_actions
            .contains(&queue_state::ApparatusQueueAction::Start)
    );
    service
        .validate_print_preflight_start(PRINT_ID, order_id, &retry.hold_id)
        .await
        .unwrap();
    let mut start = service
        .prepare_apparatus_queue_action_with_progress(
            PRINT_ID,
            order_id,
            queue_state::ApparatusQueueAction::Start,
            &[PRINT_ID.to_string()],
            actor(),
            QueueProgressInput::default(),
        )
        .await
        .expect("ordinary start from colour-matching status");
    start.attach_print_preflight_hold_id(&retry.hold_id);
    service
        .commit_prepared_queue_action(start)
        .await
        .expect("formal start");
    let started = service.live_snapshot().await.unwrap();
    assert_eq!(started.queue_states[PRINT_ID][order_id], "in_progress");
    assert_eq!(started.order_statuses[order_id].order_status, "in_progress");
    assert!(
        started.queue_action_controls[PRINT_ID][order_id]
            .print_preflight
            .is_none()
    );
}
