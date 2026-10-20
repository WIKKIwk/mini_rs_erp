use super::*;
use std::sync::Arc;

const OWN: &str = "apparatus:default:bosma_7";
const PEER: &str = "apparatus:default:bosma_8";
const OTHER: &str = "apparatus:default:bosma_9";
const DOWNSTREAM: &str = "apparatus:default:asset-007";

#[tokio::test]
async fn admin_order_page_snapshot_preserves_controls_without_building_global_details() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new_for_test(store.clone());
    for (id, route) in [
        ("zakaz-own", vec![OWN, DOWNSTREAM]),
        ("zakaz-peer", vec![PEER, DOWNSTREAM]),
        ("zakaz-other", vec![OTHER]),
    ] {
        service.upsert_map(map(id, &route)).await.unwrap();
    }
    service
        .set_apparatus_sequence(DOWNSTREAM, vec!["zakaz-peer".into(), "zakaz-own".into()])
        .await
        .unwrap();
    store
        .put_apparatus_queue_states(
            DOWNSTREAM,
            BTreeMap::from([("zakaz-peer".into(), "in_progress".into())]),
        )
        .await
        .unwrap();
    service.notify_live();
    let (page, revision) = service
        .order_snapshot_shared_with_revision(&["zakaz-own".into()])
        .await
        .unwrap();
    assert!(service.snapshot_cache.snapshot.read().await.is_none());
    assert_eq!(page.maps.len(), 1);
    assert_eq!(page.maps[0].map.id, "zakaz-own");
    assert!(!page.stage_states.contains_key("zakaz-peer"));
    let (full, full_revision) = service.live_snapshot_shared_with_revision().await.unwrap();
    assert_eq!(revision, full_revision);
    for apparatus in [OWN, DOWNSTREAM] {
        assert_eq!(
            page.queue_action_controls[apparatus]["zakaz-own"],
            full.queue_action_controls[apparatus]["zakaz-own"]
        );
        assert_eq!(
            page.sequence_versions[apparatus],
            full.sequence_versions[apparatus]
        );
    }
    assert_eq!(
        page.stage_states["zakaz-own"],
        full.stage_states["zakaz-own"]
    );
}

fn map(id: &str, route: &[&str]) -> ProductionMapDefinition {
    let mut nodes = vec![serde_json::json!({"id":"start", "kind":"start", "title":"Start"})];
    let mut edges = Vec::new();
    let mut previous = "start".to_string();
    for (index, apparatus) in route.iter().enumerate() {
        let id = format!("stage-{index}");
        nodes.push(serde_json::json!({"id":id, "kind":"apparatus", "title":apparatus, "apparatus_id":apparatus}));
        edges.push(serde_json::json!({"from":previous, "to":id}));
        previous = id;
    }
    nodes.push(serde_json::json!({"id":"end", "kind":"end", "title":"End"}));
    edges.push(serde_json::json!({"from":previous, "to":"end"}));
    serde_json::from_value(serde_json::json!({"id":id, "product_code":id,
        "title":id, "nodes":nodes, "edges":edges}))
    .unwrap()
}

#[tokio::test]
async fn worker_scoped_snapshot_preserves_competing_queue_and_route_controls() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new_for_test(store.clone());
    for (id, route) in [
        ("zakaz-own", vec![OWN, DOWNSTREAM]),
        ("zakaz-peer", vec![PEER, DOWNSTREAM]),
        ("zakaz-other", vec![OTHER]),
        ("zakaz-history", vec![OTHER]),
    ] {
        service.upsert_map(map(id, &route)).await.unwrap();
    }
    service
        .set_apparatus_sequence(DOWNSTREAM, vec!["zakaz-peer".into(), "zakaz-own".into()])
        .await
        .unwrap();
    store
        .put_apparatus_queue_states(
            DOWNSTREAM,
            BTreeMap::from([("zakaz-peer".into(), "in_progress".into())]),
        )
        .await
        .unwrap();
    service.notify_live();
    let assigned = vec![OWN.into()];
    let history = vec!["zakaz-history".into()];
    let (worker, revision) = service
        .worker_snapshot_shared_with_revision(&assigned, &history)
        .await
        .unwrap();
    assert!(
        service.snapshot_cache.snapshot.read().await.is_none(),
        "worker must not build the global snapshot"
    );
    let ids = worker
        .maps
        .iter()
        .map(|saved| saved.map.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids, BTreeSet::from(["zakaz-own", "zakaz-history"]));
    assert!(!worker.stage_states.contains_key("zakaz-peer"));
    assert_eq!(
        worker.sequences.keys().collect::<Vec<_>>(),
        vec![&OWN.to_string()]
    );
    let (cached, cached_revision) = service
        .worker_snapshot_shared_with_revision(&assigned, &history)
        .await
        .unwrap();
    assert_eq!(revision, cached_revision);
    assert!(Arc::ptr_eq(&worker, &cached));

    let (global, global_revision) = service.live_snapshot_shared_with_revision().await.unwrap();
    assert_eq!(revision, global_revision);
    for apparatus in [OWN, DOWNSTREAM] {
        assert_eq!(
            worker.queue_action_controls[apparatus]["zakaz-own"],
            global.queue_action_controls[apparatus]["zakaz-own"]
        );
    }
    assert_eq!(
        worker.stage_states["zakaz-own"],
        global.stage_states["zakaz-own"]
    );
    assert_eq!(worker.sequence_versions[OWN], global.sequence_versions[OWN]);
    assert_eq!(worker.sequences[OWN], global.sequences[OWN]);
}

#[tokio::test]
async fn worker_scoped_snapshot_invalidates_on_commit_and_supports_multiple_assignments() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new_for_test(store.clone());
    service.upsert_map(map("zakaz-own", &[OWN])).await.unwrap();
    service
        .upsert_map(map("zakaz-other", &[OTHER]))
        .await
        .unwrap();
    let (before, revision) = service
        .worker_snapshot_shared_with_revision(&[OWN.into()], &[])
        .await
        .unwrap();
    ProductionMapStorePort::put_order_control_state(
        store.as_ref(),
        OrderControlRecord {
            order_id: "zakaz-own".into(),
            state: OrderControlState::Frozen,
            actor: QueueActionActor {
                role: "admin".into(),
                ref_: "worker-snapshot-test".into(),
                display_name: "Test".into(),
            },
            requested_at_unix: 1,
            frozen_at_unix: Some(1),
            freeze_request: None,
            early_close: None,
        },
    )
    .await
    .unwrap();
    service.notify_live();
    let (after, next_revision) = service
        .worker_snapshot_shared_with_revision(&[OWN.into()], &[])
        .await
        .unwrap();
    assert!(next_revision > revision);
    assert!(!Arc::ptr_eq(&before, &after));
    assert_eq!(
        after.queue_action_controls[OWN]["zakaz-own"].state,
        queue_state::ApparatusQueueOrderState::Frozen
    );
    assert!(
        after.queue_action_controls[OWN]["zakaz-own"]
            .allowed_actions
            .is_empty()
    );
    let (both, _) = service
        .worker_snapshot_shared_with_revision(&[OTHER.into(), OWN.into(), OWN.into()], &[])
        .await
        .unwrap();
    assert!(both.maps.iter().any(|saved| saved.map.id == "zakaz-other"));
    assert_eq!(both.sequences.len(), 2);
    assert!(service.snapshot_cache.snapshot.read().await.is_none());
}

#[tokio::test]
async fn worker_scoped_snapshot_cache_has_a_fixed_upper_bound() {
    let service = ProductionMapService::new_for_test(Arc::new(MemoryProductionMapStore::new()));
    for id in 0..=MAX_WORKER_SNAPSHOTS {
        service
            .worker_snapshot_shared_with_revision(&[OWN.into()], &[format!("history-{id}")])
            .await
            .unwrap();
    }
    assert_eq!(
        service.snapshot_cache.worker_snapshots.lock().await.len(),
        MAX_WORKER_SNAPSHOTS
    );
}

async fn history_fixture() -> (Arc<MemoryProductionMapStore>, ProductionMapService) {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = ProductionMapService::new_for_test(store.clone());
    for (id, route) in [
        ("zakaz-own", vec![OWN, DOWNSTREAM]),
        ("zakaz-second", vec![OWN]),
        ("zakaz-peer", vec![PEER, DOWNSTREAM]),
        ("zakaz-history", vec![OTHER, DOWNSTREAM]),
    ] {
        service.upsert_map(map(id, &route)).await.unwrap();
    }
    service
        .set_apparatus_sequence(DOWNSTREAM, vec!["zakaz-peer".into(), "zakaz-own".into()])
        .await
        .unwrap();
    store
        .put_apparatus_queue_states(
            DOWNSTREAM,
            BTreeMap::from([("zakaz-peer".into(), "in_progress".into())]),
        )
        .await
        .unwrap();
    service.notify_live();
    (store, service)
}

async fn assert_original_history_snapshot(
    service: &ProductionMapService,
    snapshot: &ProductionMapLiveSnapshot,
    history: &[String],
) {
    let original = service
        .build_production_snapshot_in_scope(
            Some(&WorkerSnapshotScope {
                apparatus: vec![OWN.into()],
                history: normalized(history),
                only_orders: false,
            }),
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(snapshot).unwrap(),
        serde_json::to_value(original).unwrap(),
        "Every field must match the original explicit-history builder"
    );
}

#[tokio::test]
async fn redundant_histories_share_one_concurrent_apparatus_snapshot() {
    use std::future::Future;
    use std::task::Poll;

    let (_, service) = history_fixture().await;
    let assigned = [OWN.into()];
    let action_history = ["zakaz-own".into()];
    let stream_history = [
        " zakaz-second ".into(),
        "zakaz-own".into(),
        "zakaz-own".into(),
    ];
    // Park the first builder so all three callers reach the shared entry.
    let global_guard = service.snapshot_cache.snapshot.write().await;
    let mut reads = Box::pin(async {
        tokio::join!(
            service.worker_snapshot_shared_with_revision(&assigned, &[]),
            service.worker_snapshot_shared_with_revision(&assigned, &action_history),
            service.worker_snapshot_shared_with_revision(&assigned, &stream_history),
        )
    });
    std::future::poll_fn(|cx| {
        assert!(reads.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(global_guard);
    let (base, action, stream) = reads.await;
    let (base, revision) = base.unwrap();
    let (action, action_revision) = action.unwrap();
    let (stream, stream_revision) = stream.unwrap();
    assert_eq!((action_revision, stream_revision), (revision, revision));
    assert!(Arc::ptr_eq(&base, &action));
    assert!(Arc::ptr_eq(&base, &stream));
    assert_eq!(
        service.snapshot_cache.worker_snapshots.lock().await.len(),
        1
    );
    assert_original_history_snapshot(&service, &base, &action_history).await;
    assert_original_history_snapshot(&service, &base, &stream_history).await;
    assert!(!base.maps.iter().any(|saved| saved.map.id == "zakaz-peer"));
    assert!(base.queue_action_controls[DOWNSTREAM].contains_key("zakaz-own"));
}

#[tokio::test]
async fn outside_and_dependency_only_history_keep_exact_scope_across_revisions() {
    let (_, service) = history_fixture().await;
    let assigned = [OWN.into()];
    // The peer is a dependency of the base, but its own details are not outputs.
    let history = [
        "zakaz-own".into(),
        "zakaz-peer".into(),
        "zakaz-history".into(),
    ];
    let (with_history, revision) = service
        .worker_snapshot_shared_with_revision(&assigned, &history)
        .await
        .unwrap();
    let base_entry = service
        .snapshot_cache
        .worker_snapshots
        .lock()
        .await
        .get(&WorkerSnapshotScope {
            apparatus: assigned.to_vec(),
            history: Vec::new(),
            only_orders: false,
        })
        .unwrap()
        .clone();
    let base = base_entry.lock().await.as_ref().unwrap().snapshot.clone();
    assert!(!Arc::ptr_eq(&base, &with_history));
    assert!(
        !base
            .maps
            .iter()
            .any(|saved| saved.map.id == "zakaz-history")
    );
    assert!(with_history.queue_action_controls[OTHER].contains_key("zakaz-history"));
    assert!(with_history.queue_action_controls[PEER].contains_key("zakaz-peer"));
    assert_original_history_snapshot(&service, &with_history, &history).await;

    for _ in 0..3 {
        service.notify_live();
        let (next, next_revision) = service
            .worker_snapshot_shared_with_revision(&assigned, &history)
            .await
            .unwrap();
        assert_eq!(next_revision, service.snapshot_revision());
        assert!(next_revision > revision);
        assert_original_history_snapshot(&service, &next, &history).await;
        let base_cached = base_entry.lock().await;
        let base_cached = base_cached.as_ref().unwrap();
        assert_eq!(base_cached.revision, revision, "Must not rebuild the base");
        assert!(Arc::ptr_eq(&base_cached.snapshot, &base));
    }
    assert_eq!(
        service.snapshot_cache.worker_snapshots.lock().await.len(),
        2
    );
}

#[tokio::test]
async fn missing_or_removed_history_preserves_original_fallback() {
    let (store, service) = history_fixture().await;
    let history = ["zakaz-history".into(), "missing-order".into()];
    let (_, previous_revision) = service
        .worker_snapshot_shared_with_revision(&[OWN.into()], &history)
        .await
        .unwrap();
    ProductionMapStorePort::delete_map(store.as_ref(), "zakaz-history")
        .await
        .unwrap();
    service.notify_live();
    let (snapshot, revision) = service
        .worker_snapshot_shared_with_revision(&[OWN.into()], &history)
        .await
        .unwrap();
    assert!(revision > previous_revision);
    assert!(
        !snapshot
            .maps
            .iter()
            .any(|saved| saved.map.id == "zakaz-history")
    );
    assert_original_history_snapshot(&service, &snapshot, &history).await;
    assert!(
        service
            .snapshot_cache
            .worker_snapshots
            .lock()
            .await
            .contains_key(&WorkerSnapshotScope {
                apparatus: vec![OWN.into()],
                history: normalized(&history),
                only_orders: false,
            })
    );
}

#[tokio::test]
async fn shared_history_observes_revision_change_while_waiting_for_build() {
    use std::future::Future;
    use std::task::Poll;

    let (store, service) = history_fixture().await;
    let old_revision = service.snapshot_revision();
    let global_guard = service.snapshot_cache.snapshot.write().await;
    let assigned = [OWN.into()];
    let history = ["zakaz-own".into()];
    let mut read = Box::pin(service.worker_snapshot_shared_with_revision(&assigned, &history));
    std::future::poll_fn(|cx| {
        assert!(read.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let mut changed = map("zakaz-own", &[OWN, DOWNSTREAM]);
    changed.title = "Committed revision".into();
    ProductionMapStorePort::put_map(store.as_ref(), changed)
        .await
        .unwrap();
    service.notify_live();
    drop(global_guard);
    let (snapshot, revision) = read.await.unwrap();
    assert!(revision > old_revision);
    assert_eq!(revision, service.snapshot_revision());
    assert_eq!(
        snapshot
            .maps
            .iter()
            .find(|saved| saved.map.id == "zakaz-own")
            .unwrap()
            .map
            .title,
        "Committed revision"
    );
    assert_original_history_snapshot(&service, &snapshot, &history).await;
}

#[tokio::test]
async fn cancelled_shared_history_builder_releases_entry_for_other_callers() {
    use std::future::Future;
    use std::task::Poll;

    let (_, service) = history_fixture().await;
    let assigned = [OWN.into()];
    let history = ["zakaz-own".into()];
    let global_guard = service.snapshot_cache.snapshot.write().await;
    let mut read = Box::pin(service.worker_snapshot_shared_with_revision(&assigned, &history));
    std::future::poll_fn(|cx| {
        assert!(read.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(read);
    drop(global_guard);
    let (snapshot, revision) = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        service.worker_snapshot_shared_with_revision(&assigned, &history),
    )
    .await
    .expect("Cancellation must release the shared builder lock")
    .unwrap();
    assert_eq!(revision, service.snapshot_revision());
    assert_original_history_snapshot(&service, &snapshot, &history).await;
}

#[tokio::test]
async fn admin_order_only_scope_does_not_reuse_worker_apparatus_outputs() {
    let (_, service) = history_fixture().await;
    let (worker, _) = service
        .worker_snapshot_shared_with_revision(&[OWN.into()], &["zakaz-own".into()])
        .await
        .unwrap();
    let (page, _) = service
        .order_snapshot_shared_with_revision(&["zakaz-own".into()])
        .await
        .unwrap();
    assert!(!Arc::ptr_eq(&worker, &page));
    assert_eq!(page.maps.len(), 1);
    assert!(page.sequences.contains_key(DOWNSTREAM));
    assert!(!worker.sequences.contains_key(DOWNSTREAM));
}

#[tokio::test]
async fn completed_history_can_share_materialized_map_outside_visible_queue() {
    let (store, service) = history_fixture().await;
    store
        .put_apparatus_queue_states(
            OWN,
            BTreeMap::from([("zakaz-second".into(), "completed".into())]),
        )
        .await
        .unwrap();
    service.notify_live();
    let (base, _) = service
        .worker_snapshot_shared_with_revision(&[OWN.into()], &[])
        .await
        .unwrap();
    assert!(base.maps.iter().any(|saved| saved.map.id == "zakaz-second"));
    assert!(!base.visible_order_ids[OWN].contains(&"zakaz-second".to_string()));
    let history = ["zakaz-second".into()];
    let (with_history, _) = service
        .worker_snapshot_shared_with_revision(&[OWN.into()], &history)
        .await
        .unwrap();
    assert!(Arc::ptr_eq(&base, &with_history));
    assert_original_history_snapshot(&service, &with_history, &history).await;
}
