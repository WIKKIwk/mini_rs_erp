use super::*;
use std::sync::Arc;

const OWN: &str = "apparatus:default:bosma_7";
const PEER: &str = "apparatus:default:bosma_8";
const OTHER: &str = "apparatus:default:bosma_9";
const DOWNSTREAM: &str = "apparatus:default:asset-007";

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
