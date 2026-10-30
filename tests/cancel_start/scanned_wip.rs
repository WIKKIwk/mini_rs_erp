use super::*;

#[tokio::test]
#[ignore = "requires CANCEL_START_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn scanned_wip_opening_input_returns_original_qr_quantity_and_intake() {
    let f = Fixture::new_scanned(MACHINES[1]).await;
    let record = f
        .service
        .create_opening_wip(
            serde_json::from_value(json!({"idempotency_key":f.order,"order_id":f.order,
        "source_apparatus":PRESS,"source_stage_node_id":"print","source_operation":"print",
        "batches":[{"quantity":80,"uom":"m","quantity_basis":"measured","finished_goods_meter":80,
          "finished_goods_kg":10,"bobina_kg":1}]}))
            .unwrap(),
            actor(),
        )
        .await
        .unwrap();
    let original = &record.batches[0];
    f.service
        .apply_apparatus_queue_action_with_progress(
            &f.machine,
            &f.order,
            Action::Start,
            &[f.machine.clone()],
            actor(),
            QueueProgressInput {
                qr_payload: original.qr_payload.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let rows = inputs(&f).await;
    let item = rows
        .iter()
        .find(|r| r.batch_id == original.batch_id)
        .unwrap();
    return_input(&f, item).await.unwrap();
    let after = f
        .store
        .opening_wip_batch(&original.batch_id, "")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.batch.qr_payload, original.qr_payload);
    assert_eq!(after.batch.quantity, original.quantity);
    assert_eq!(after.batch.intake_id, original.intake_id);
    assert_eq!(after.batch.wip_status, OpeningWipBatchStatus::Waiting);
}

#[tokio::test]
#[ignore = "requires CANCEL_START_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn scanned_wip_ambiguous_legacy_output_cannot_release_a_merge_contributor() {
    let f = Fixture::new_scanned(MACHINES[0]).await;
    let started = f.start().await;
    let second = another(&f, "second").await;
    merge(&f, &second).await;
    let mut out = output(&f, &started.session_id, &[second.batch_id.clone()]).await;
    out.payload_json = json!({"stage_node_id":"work","source_input_links":[]});
    out.parent_batch_id = second.batch_id;
    f.store.put_order_progress_batch(out).await.unwrap();
    assert!(
        inputs(&f)
            .await
            .iter()
            .all(|row| row.consume_state == "unknown" && row.return_token.is_none())
    );
    f.release().await;
}

async fn inputs(f: &Fixture) -> Vec<ScannedWipInput> {
    f.service
        .scanned_wip_inputs(&f.machine, &f.order, "work", &[f.machine.clone()], actor())
        .await
        .unwrap()
}
fn target(f: &Fixture, item: &ScannedWipInput) -> ScannedWipReturn {
    ScannedWipReturn {
        apparatus: f.machine.clone(),
        order_id: f.order.clone(),
        stage_node_id: "work".into(),
        session_id: item.session_id.clone(),
        batch_id: item.batch_id.clone(),
        token: item.return_token.clone().unwrap(),
    }
}
async fn return_input(f: &Fixture, item: &ScannedWipInput) -> Result<(), ProductionMapError> {
    f.service
        .return_scanned_wip(&target(f, item), &[f.machine.clone()], actor())
        .await
}
async fn another(f: &Fixture, suffix: &str) -> OrderProgressBatch {
    let mut batch = f.original.clone();
    batch.batch_id = format!("{}-{suffix}", batch.batch_id);
    batch.qr_payload = format!("{}-{suffix}", batch.qr_payload);
    f.store
        .put_order_progress_batch(batch.clone())
        .await
        .unwrap();
    batch
}
async fn merge(f: &Fixture, batch: &OrderProgressBatch) {
    f.service
        .apply_apparatus_queue_action_with_progress(
            &f.machine,
            &f.order,
            Action::Merge,
            &[f.machine.clone()],
            actor(),
            QueueProgressInput {
                qr_payload: batch.qr_payload.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
}
async fn output(f: &Fixture, session: &str, sources: &[String]) -> OrderProgressBatch {
    let mut batch = f.original.clone();
    batch.batch_id = format!("out-{}", f.order);
    batch.qr_payload = format!("OUT-{}", f.order);
    batch.session_id = session.into();
    batch.apparatus = f.machine.clone();
    batch.next_apparatus.clear();
    batch.current_apparatus = f.machine.clone();
    batch.payload_json = json!({"stage_node_id":"work","source_input_batch_ids":sources});
    f.store
        .put_order_progress_batch(batch.clone())
        .await
        .unwrap();
    batch
}

#[tokio::test]
#[ignore = "requires CANCEL_START_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn scanned_wip_returns_wrong_first_input_after_merge_and_then_releases_last_input() {
    for machine in MACHINES {
        let f = Fixture::new_scanned(machine).await;
        f.start().await;
        let second = another(&f, "second").await;
        merge(&f, &second).await;
        let rows = inputs(&f).await;
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter()
                .all(|r| r.consume_state == "unused" && r.return_token.is_some())
        );
        let first = rows
            .iter()
            .find(|r| r.batch_id == f.original.batch_id)
            .unwrap();
        assert_eq!(
            first.state, "unprocessed",
            "Merge is not evidence of physical consumption"
        );
        let request = target(&f, first);
        let (a, b) = tokio::join!(return_input(&f, first), return_input(&f, first));
        a.unwrap();
        b.unwrap();
        let restored = f.batch().await;
        assert_eq!(restored.wip_status, OrderProgressBatchWipStatus::Waiting);
        assert_eq!(restored.qr_payload, f.original.qr_payload);
        assert_eq!(restored.produced_qty, f.original.produced_qty);
        assert_eq!(restored.current_location, f.original.current_location);
        assert_eq!(restored.payload_json, f.original.payload_json);
        let session = f
            .store
            .active_order_run_session(machine, &f.order)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            session.payload_json["input_progress_batch_id"],
            second.batch_id
        );
        assert_eq!(
            session.payload_json["input_lineage"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let rows = inputs(&f).await;
        assert_eq!(
            rows.iter()
                .find(|r| r.batch_id == f.original.batch_id)
                .unwrap()
                .state,
            "returned"
        );
        let last = rows.iter().find(|r| r.batch_id == second.batch_id).unwrap();
        return_input(&f, last).await.unwrap();
        assert!(
            f.store
                .active_order_run_session(machine, &f.order)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            f.store.apparatus_queue_states().await.unwrap()[machine][&f.order],
            "pending"
        );
        let fresh = f.start().await;
        f.service
            .return_scanned_wip(&request, &[machine.into()], actor())
            .await
            .unwrap();
        assert_eq!(
            f.batch().await.used_by_session_id,
            fresh.session_id,
            "late replay must not return a new claim"
        );
        f.cancel(&fresh).await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires CANCEL_START_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn scanned_wip_output_blocks_both_history_action_and_stale_return_without_partial_changes() {
    for machine in [MACHINES[0], MACHINES[1]] {
        let f = Fixture::new_scanned(machine).await;
        let started = f.start().await;
        let second = another(&f, "second").await;
        merge(&f, &second).await;
        let before = inputs(&f).await;
        let stale = target(
            &f,
            before
                .iter()
                .find(|r| r.batch_id == f.original.batch_id)
                .unwrap(),
        );
        let out = output(
            &f,
            &started.session_id,
            &[f.original.batch_id.clone(), second.batch_id.clone()],
        )
        .await;
        let original = f.batch().await;
        let rows = inputs(&f).await;
        assert!(
            rows.iter()
                .all(|r| r.consume_state == "consumed" && r.return_token.is_none())
        );
        assert!(
            f.service
                .return_scanned_wip(&stale, &[machine.into()], actor())
                .await
                .is_err()
        );
        assert_eq!(f.batch().await, original);
        assert_eq!(
            f.store
                .progress_batch(&out.batch_id)
                .await
                .unwrap()
                .unwrap()
                .qr_payload,
            out.qr_payload
        );
        f.release().await;
    }
}

#[tokio::test]
#[ignore = "requires CANCEL_START_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn scanned_wip_can_return_new_unused_input_after_an_earlier_input_produced_output() {
    for machine in [MACHINES[0], MACHINES[1]] {
        let f = Fixture::new_scanned(machine).await;
        let started = f.start().await;
        let out = output(&f, &started.session_id, &[f.original.batch_id.clone()]).await;
        let second = another(&f, "second").await;
        merge(&f, &second).await;
        let rows = inputs(&f).await;
        assert_eq!(
            rows.iter()
                .find(|r| r.batch_id == f.original.batch_id)
                .unwrap()
                .consume_state,
            "consumed"
        );
        let unused = rows.iter().find(|r| r.batch_id == second.batch_id).unwrap();
        assert_eq!(unused.consume_state, "unused");
        return_input(&f, unused).await.unwrap();
        assert_eq!(
            f.batch().await.wip_status,
            OrderProgressBatchWipStatus::InUse
        );
        assert_eq!(
            f.store
                .progress_batch(&second.batch_id)
                .await
                .unwrap()
                .unwrap()
                .wip_status,
            OrderProgressBatchWipStatus::Waiting
        );
        assert_eq!(
            f.store
                .progress_batch(&out.batch_id)
                .await
                .unwrap()
                .unwrap()
                .payload_json,
            out.payload_json
        );
        f.release().await;
    }
}

#[tokio::test]
#[ignore = "requires CANCEL_START_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn scanned_wip_rejects_other_workers_wrong_stages_and_corrected_inputs() {
    let f = Fixture::new_scanned(MACHINES[0]).await;
    f.start().await;
    let rows = inputs(&f).await;
    let request = target(&f, &rows[0]);
    let mut stranger = actor();
    stranger.ref_ = "stranger".into();
    assert!(
        f.service
            .return_scanned_wip(&request, &[f.machine.clone()], stranger)
            .await
            .is_err()
    );
    assert!(
        f.service
            .return_scanned_wip(&request, &[], actor())
            .await
            .is_err()
    );
    let mut wrong = request.clone();
    wrong.stage_node_id = "print".into();
    assert!(
        f.service
            .return_scanned_wip(&wrong, &[f.machine.clone()], actor())
            .await
            .is_err()
    );
    let mut corrected = f.batch().await;
    corrected.description = "stock correction after scan".into();
    f.store
        .put_order_progress_batch(corrected.clone())
        .await
        .unwrap();
    assert!(inputs(&f).await[0].return_token.is_none());
    assert!(
        f.service
            .return_scanned_wip(&request, &[f.machine.clone()], actor())
            .await
            .is_err()
    );
    assert_eq!(f.batch().await.description, corrected.description);
    f.release().await;
}
