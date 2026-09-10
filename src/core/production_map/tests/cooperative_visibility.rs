use super::*;
use queue_state::{ApparatusQueueAction as A, ApparatusQueueOrderState as S};
const ORDER: &str = "zakaz-visibility";

async fn fixture(protocol: bool) -> (ProductionMapService, Arc<MemoryProductionMapStore>) {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let map = serde_json::from_value(serde_json::json!({
        "id":ORDER,"product_code":"VIS","title":"Visibility",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
        {"id":"before","kind":"apparatus","title":"Rezka","apparatus_id":REZKA_ID,"rezka_kadr_count":1},
        {"id":"a","kind":"apparatus","title":"Laminatsiya 1","apparatus_id":LAMINATION_1_ID,"alternative_group_id":"shared"},
        {"id":"b","kind":"apparatus","title":"Laminatsiya 2","apparatus_id":LAMINATION_2_ID,"alternative_group_id":"shared"},
        {"id":"after","kind":"apparatus","title":"Rezka","apparatus_id":REZKA_ID,"rezka_kadr_count":1},
        {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"before"},{"from":"before","to":"a"},
        {"from":"a","to":"after"},{"from":"after","to":"end"}]
    })).unwrap();
    service.upsert_map(map).await.unwrap();
    record(&store, REZKA_ID, "before", protocol, S::Completed).await;
    record(&store, LAMINATION_1_ID, "a", protocol, S::Completed).await;
    (service, store)
}

async fn record(store: &MemoryProductionMapStore, apparatus: &str, node: &str, protocol: bool, state: S) {
    let actor = QueueActionActor { role: "aparatchi".into(), ref_: "same-worker".into(), display_name: "Worker".into() };
    let mut session = OrderRunSession {
        session_id: format!("session-{node}"), apparatus: apparatus.into(), order_id: ORDER.into(),
        stage_node_id: node.into(), status: if state == S::Completed { OrderRunStatus::Completed } else { OrderRunStatus::Active },
        worker_role: actor.role.clone(), worker_ref: actor.ref_.clone(), worker_display_name: actor.display_name.clone(),
        started_at_unix: 1, updated_at_unix: 2, payload_json: serde_json::json!({}),
    };
    if protocol {
        session.payload_json["stage_work_protocol"] = serde_json::json!(1);
        crate::core::production_map::stage_execution::stamp_work_report(&mut session, node, 1, &actor, 2);
    }
    store.put_order_run_session(session).await.unwrap();
    store.put_apparatus_queue_states_with_event(apparatus, BTreeMap::from([(ORDER.into(), state.as_str().into())]),
        ApparatusQueueActionEvent { event_id: format!("event-{node}"), apparatus: apparatus.into(), order_id: ORDER.into(),
            stage_node_id: node.into(), action: if state == S::Completed { A::Complete } else { A::Start },
            from_state: S::InProgress, to_state: state, policy: ApparatusQueuePolicy::FreePick, actor,
            assigned_apparatus: vec![apparatus.into()], payload_json: serde_json::json!({}) }).await.unwrap();
}

#[tokio::test]
async fn shared_closure_hides_unused_alternative_without_faking_history_including_legacy() {
    for protocol in [false, true] {
        let (service, store) = fixture(protocol).await;
        let snapshot = service.live_snapshot_shared().await.unwrap();
        assert_eq!(snapshot.order_statuses[ORDER].lifecycle_status, ProductionOrderLifecycleStatus::InProgress);
        for machine in [LAMINATION_1_ID, LAMINATION_2_ID] {
            assert!(!snapshot.visible_order_ids[machine].contains(&ORDER.into()));
        }
        assert!(snapshot.visible_order_ids[REZKA_ID].contains(&ORDER.into()), "later occurrence stays visible");
        assert_eq!(snapshot.stage_states[ORDER]["b"], "completed");
        assert_eq!(snapshot.queue_states[LAMINATION_2_ID][ORDER], "pending", "shared closure is not own execution");
        assert!(store.apparatus_queue_states().await.unwrap().get(LAMINATION_2_ID).is_none());
        let history = service.completed_queue_orders_for_actor("same-worker", 20).await.unwrap();
        assert!(history.iter().any(|h| h.apparatus == LAMINATION_1_ID));
        assert!(!history.iter().any(|h| h.apparatus == LAMINATION_2_ID));
        record(&store, REZKA_ID, "after", protocol, S::Completed).await;
        service.notify_live();
        let done = service.live_snapshot_shared().await.unwrap();
        assert_eq!(done.order_statuses[ORDER].lifecycle_status, ProductionOrderLifecycleStatus::ProductionCompleted);
        assert!(done.visible_order_ids.values().all(|ids| !ids.contains(&ORDER.into())));
        assert!(done.maps.iter().any(|saved| saved.map.id == ORDER), "retain map for actual history");
    }
}

#[tokio::test]
async fn active_peer_keeps_shared_stage_visible_and_each_participant_keeps_own_history() {
    let (service, store) = fixture(true).await;
    record(&store, LAMINATION_2_ID, "b", true, S::InProgress).await;
    let snapshot = service.live_snapshot_shared().await.unwrap();
    assert!(snapshot.visible_order_ids[LAMINATION_2_ID].contains(&ORDER.into()));
    assert!(!snapshot.queue_action_controls[LAMINATION_2_ID][ORDER].stage_work.as_ref().unwrap().completed);
    record(&store, LAMINATION_2_ID, "b", true, S::Completed).await;
    service.notify_live();
    let snapshot = service.live_snapshot_shared().await.unwrap();
    assert!(!snapshot.visible_order_ids[LAMINATION_2_ID].contains(&ORDER.into()));
    let history = service.completed_queue_orders_for_actor("same-worker", 20).await.unwrap();
    for machine in [LAMINATION_1_ID, LAMINATION_2_ID] {
        assert_eq!(history.iter().filter(|h| h.apparatus == machine).count(), 1);
    }
    assert!(service.completed_queue_orders_for_actor("other-worker", 20).await.unwrap().is_empty());
}

#[tokio::test]
async fn local_report_on_repeated_machine_uses_latest_occurrence_without_closing_upstream() {
    let (service, store) = fixture(true).await;
    record(&store, LAMINATION_2_ID, "b", true, S::InProgress).await;
    record(&store, REZKA_ID, "after", true, S::Pending).await;
    let mut last = store.order_run_sessions_for_order(ORDER).await.unwrap().into_iter()
        .find(|s| s.stage_node_id == "after").unwrap();
    last.started_at_unix = 10;
    last.status = OrderRunStatus::Completed;
    crate::core::production_map::stage_execution::stamp_work_report(
        &mut last, "after-report", 3, &QueueActionActor::default(), 11);
    store.put_order_run_session(last).await.unwrap();
    let snapshot = service.live_snapshot_shared().await.unwrap();
    let own = &snapshot.queue_action_controls[REZKA_ID][ORDER];
    assert_eq!(own.stage_node_id, "after");
    assert!(own.stage_work.as_ref().unwrap().local_completed);
    assert!(!own.stage_work.as_ref().unwrap().completed);
    assert!(!snapshot.visible_order_ids[REZKA_ID].contains(&ORDER.into()));
    assert!(snapshot.visible_order_ids[LAMINATION_2_ID].contains(&ORDER.into()));
}
