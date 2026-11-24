use super::*;
use crate::core::production_map::MemoryProductionMapStore;

#[test]
fn committed_preflight_events_are_published_once_in_either_arrival_order() {
    let service = ProductionMapService::new_for_test(std::sync::Arc::new(
        MemoryProductionMapStore::new(),
    ));
    let mut receiver = service.subscribe_live();
    let clone = service.clone();
    for event_id in ["local-first", "listener-first"] {
        assert!(service.notify_print_preflight_once(
            Some(event_id),
            "order",
            "apparatus:default:bosma_9",
        ));
        assert!(!clone.notify_print_preflight_once(
            Some(event_id),
            "order",
            "apparatus:default:bosma_9",
        ));
        assert!(matches!(
            receiver.try_recv().unwrap(),
            ProductionMapLiveEvent::PrintPreflight { .. }
        ));
        assert!(receiver.try_recv().is_err());
    }
    assert_eq!(service.snapshot_revision(), 2);
    // Memory and old writers have no exact transaction ID. Never suppress
    // them using order/status/timing heuristics.
    assert!(service.notify_print_preflight_once(None, "order", "apparatus"));
    assert!(service.notify_print_preflight_once(None, "order", "apparatus"));
    assert_eq!(service.snapshot_revision(), 4);
}

#[test]
fn concurrent_committed_publishers_share_one_bounded_registry() {
    let service = ProductionMapService::new_for_test(std::sync::Arc::new(
        MemoryProductionMapStore::new(),
    ));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    std::thread::scope(|scope| {
        for _ in 0..2 {
            let service = service.clone();
            let barrier = barrier.clone();
            scope.spawn(move || {
                barrier.wait();
                service.notify_print_preflight_once(Some("same-commit"), "order", "apparatus");
            });
        }
    });
    assert_eq!(service.snapshot_revision(), 1);
    let mut cache = CommittedLiveEvents::default();
    for index in 0..=COMMITTED_LIVE_EVENTS_LIMIT {
        assert!(cache.remember(&index.to_string()));
    }
    assert_eq!(cache.order.len(), COMMITTED_LIVE_EVENTS_LIMIT);
    assert_eq!(cache.seen.len(), COMMITTED_LIVE_EVENTS_LIMIT);
    assert!(!cache.remember(&COMMITTED_LIVE_EVENTS_LIMIT.to_string()));
    assert!(cache.remember("0"), "Evicted IDs may publish again safely");
}

#[test]
fn coalesced_preflight_dedup_precedes_scope_selection() {
    let service = ProductionMapService::new_for_test(std::sync::Arc::new(
        MemoryProductionMapStore::new(),
    ));
    let mut receiver = service.subscribe_live();
    service.notify_print_preflight_once(Some("old"), "old-order", "apparatus");
    receiver.try_recv().unwrap();
    let changes = [
        (Some("old"), "old-order", "apparatus"),
        (Some("new"), "new-order", "apparatus"),
    ];
    service.notify_print_preflight_batch(&changes, false);
    assert!(matches!(receiver.try_recv().unwrap(),
        ProductionMapLiveEvent::PrintPreflight { order_id, .. }
            if order_id == "new-order"));
    service.notify_print_preflight_batch(&changes, false);
    assert!(receiver.try_recv().is_err());
    service.notify_print_preflight_batch(&changes, true);
    assert_eq!(
        receiver.try_recv().unwrap(),
        ProductionMapLiveEvent::Invalidate
    );
    service.notify_print_preflight_batch(
        &[
            (Some("another-1"), "a", "apparatus"),
            (Some("another-2"), "b", "apparatus"),
        ],
        false,
    );
    assert_eq!(
        receiver.try_recv().unwrap(),
        ProductionMapLiveEvent::Invalidate
    );
    // Two new transitions of the same order can share one fresh projection.
    service.notify_print_preflight_batch(
        &[
            (Some("same-order-1"), "a", "apparatus"),
            (Some("same-order-2"), "a", "apparatus"),
        ],
        false,
    );
    assert!(matches!(
        receiver.try_recv().unwrap(),
        ProductionMapLiveEvent::PrintPreflight { .. }
    ));
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn live_snapshot_cache_reuses_shared_arc_until_notification() {
    let service = ProductionMapService::new_for_test(std::sync::Arc::new(
        MemoryProductionMapStore::new(),
    ));

    let initial = service
        .live_snapshot_shared()
        .await
        .expect("initial snapshot");
    let reused = service
        .live_snapshot_shared()
        .await
        .expect("cached snapshot");
    assert!(std::sync::Arc::ptr_eq(&initial, &reused));

    service.notify_live();
    let refreshed = service
        .live_snapshot_shared()
        .await
        .expect("invalidated snapshot");
    assert!(!std::sync::Arc::ptr_eq(&initial, &refreshed));
}
