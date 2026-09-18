use super::*;

#[tokio::test]
async fn queue_snapshot_preserves_machine_stage_work_recency() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let cases = [
        ("untouched", "pending", None, 0),
        ("running", "in_progress", Some(OrderRunStatus::Active), 10),
        ("paused", "paused", Some(OrderRunStatus::Paused), 20),
        ("released", "pending", Some(OrderRunStatus::Paused), 30),
    ];
    for (id, _, status, updated) in cases {
        service
            .upsert_map(apparatus_stage_map(id, FLOW_PECHAT_ID))
            .await
            .unwrap();
        if let Some(status) = status {
            let session = OrderRunSession {
                session_id: format!("run-{id}"),
                apparatus: FLOW_PECHAT_ID.into(),
                order_id: id.into(),
                stage_node_id: "apparatus".into(),
                status,
                worker_role: "aparatchi".into(),
                worker_ref: "worker".into(),
                worker_display_name: "Worker".into(),
                started_at_unix: 1,
                updated_at_unix: updated,
                payload_json: serde_json::json!({"requeued_at_tail": id == "released"}),
            };
            store.put_order_run_session(session.clone()).await.unwrap();
            // Neither another machine nor an earlier occurrence may change
            // this machine/stage's ordering, even if its event is newer.
            for (suffix, apparatus, stage) in [
                ("peer", FLOW_ALT_PECHAT_ID, "apparatus"),
                ("stage", FLOW_PECHAT_ID, "old-stage"),
            ] {
                store
                    .put_order_run_session(OrderRunSession {
                        session_id: format!("{suffix}-{id}"),
                        apparatus: apparatus.into(),
                        stage_node_id: stage.into(),
                        status: OrderRunStatus::Completed,
                        updated_at_unix: 999,
                        ..session.clone()
                    })
                    .await
                    .unwrap();
            }
        }
    }
    store
        .put_apparatus_queue_states(
            FLOW_PECHAT_ID,
            cases
                .iter()
                .map(|(id, state, _, _)| (id.to_string(), state.to_string()))
                .collect(),
        )
        .await
        .unwrap();
    let sequence = cases
        .iter()
        .map(|(id, _, _, _)| id.to_string())
        .collect::<Vec<_>>();
    store
        .put_apparatus_sequence(FLOW_PECHAT_ID, sequence.clone())
        .await
        .unwrap();

    // A new service reads the same persisted sessions; no client/session cache
    // or new per-order request is needed after app re-entry.
    for reader in [service, default_service_with_store(store.clone()).await] {
        let snapshot = reader.live_snapshot().await.unwrap();
        for (id, _, _, updated) in cases {
            let control = &snapshot.queue_action_controls[FLOW_PECHAT_ID][id];
            assert_eq!(control.last_worked_at_unix, updated, "{id}");
            assert_eq!(
                serde_json::to_value(control).unwrap()["last_worked_at_unix"],
                updated
            );
        }
        assert_eq!(snapshot.sequences[FLOW_PECHAT_ID], sequence);
    }
}
