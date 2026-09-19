use super::*;
use queue_state::ApparatusQueueAction as Action;
const ORDER: &str = "zakaz-resume-work";

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "worker-resume-session".into(),
        display_name: "Resume session worker".into(),
    }
}

fn output() -> QueueProgressInput {
    QueueProgressInput {
        produced_qty: Some(12.0),
        uom: "kg".into(),
        ..QueueProgressInput::default()
    }
}

fn completion() -> QueueProgressInput {
    QueueProgressInput {
        return_ink_kg: Some(0.1),
        lamination_film_leftover_rolls: Some(0.1),
        total_waste: Some(0.1),
        finished_goods_kg: Some(12.0),
        finished_goods_meter: Some(120.0),
        ..output()
    }
}

async fn fixture() -> (ProductionMapService, Arc<MemoryProductionMapStore>) {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    service
        .upsert_map(three_stage_map(
            ORDER,
            FLOW_PECHAT_ID,
            LAMINATION_1_ID,
            REZKA_ID,
            3,
        ))
        .await
        .unwrap();
    (service, store)
}

async fn run(
    service: &ProductionMapService,
    apparatus: &str,
    action: Action,
    input: QueueProgressInput,
) -> Result<ApparatusQueueActionResult, ProductionMapError> {
    service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            ORDER,
            action,
            &[apparatus.into()],
            actor(),
            input,
        )
        .await
}

async fn scan_start(service: &ProductionMapService, apparatus: &str, batch: &OrderProgressBatch) {
    run(
        service,
        apparatus,
        Action::Start,
        QueueProgressInput {
            qr_payload: batch.qr_payload.clone(),
            ..QueueProgressInput::default()
        },
    )
    .await
    .unwrap();
}

async fn assert_print_resume(downstream: OrderProgressBatchWipStatus) {
    let (service, store) = fixture().await;
    run(
        &service,
        FLOW_PECHAT_ID,
        Action::Start,
        QueueProgressInput::default(),
    )
    .await
    .unwrap();
    let detached = run(&service, FLOW_PECHAT_ID, Action::DetachRoll, output())
        .await
        .unwrap();
    let session_id = detached.session.unwrap().session_id;
    let first = detached.progress_batch.unwrap();
    if downstream != OrderProgressBatchWipStatus::Waiting {
        scan_start(&service, LAMINATION_1_ID, &first).await;
        if downstream == OrderProgressBatchWipStatus::Processed {
            run(&service, LAMINATION_1_ID, Action::Complete, completion())
                .await
                .unwrap();
        }
    }
    let before = store
        .progress_batch(&first.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before.wip_status, downstream);
    if downstream != OrderProgressBatchWipStatus::Waiting {
        assert!(
            run(
                &service,
                FLOW_PECHAT_ID,
                Action::Resume,
                QueueProgressInput {
                    qr_payload: first.qr_payload.clone(),
                    ..QueueProgressInput::default()
                }
            )
            .await
            .is_err(),
            "an explicit consumed output QR must not be reclaimed"
        );
    }
    let resumed = run(
        &service,
        FLOW_PECHAT_ID,
        Action::Resume,
        QueueProgressInput::default(),
    )
    .await
    .expect("producer can continue while the consumer uses its output");
    let session = resumed.session.unwrap();
    assert_eq!(session.status, OrderRunStatus::Active);
    assert_eq!(session.session_id, session_id);
    assert_eq!(session.payload_json["input_progress_batch_id"], "");
    assert!(resumed.progress_batch.is_none());
    assert!(resumed.progress_batches.is_empty());
    assert_eq!(
        store
            .progress_batch(&first.batch_id)
            .await
            .unwrap()
            .unwrap(),
        before
    );
    assert!(
        run(
            &service,
            FLOW_PECHAT_ID,
            Action::Resume,
            QueueProgressInput::default()
        )
        .await
        .is_err()
    );

    // Repeated output cycles must not fail when several older rolls are waiting
    // or already downstream. Finishing the job must not consume its own outputs.
    let second = run(&service, FLOW_PECHAT_ID, Action::DetachRoll, output())
        .await
        .unwrap()
        .progress_batch
        .unwrap();
    run(
        &service,
        FLOW_PECHAT_ID,
        Action::Resume,
        QueueProgressInput::default(),
    )
    .await
    .unwrap();
    let third = run(&service, FLOW_PECHAT_ID, Action::Complete, completion())
        .await
        .unwrap()
        .progress_batch
        .unwrap();
    assert_ne!(first.batch_id, second.batch_id);
    assert_ne!(second.batch_id, third.batch_id);
    assert_eq!(
        store
            .progress_batch(&first.batch_id)
            .await
            .unwrap()
            .unwrap(),
        before
    );
    assert_eq!(
        store
            .progress_batch(&second.batch_id)
            .await
            .unwrap()
            .unwrap(),
        second
    );
    assert!(third.parent_batch_id.is_empty());
}

#[tokio::test]
async fn print_resume_preserves_waiting_outputs_across_multiple_cycles() {
    assert_print_resume(OrderProgressBatchWipStatus::Waiting).await;
}

#[tokio::test]
async fn print_resume_while_lamination_is_using_its_output() {
    assert_print_resume(OrderProgressBatchWipStatus::InUse).await;
}

#[tokio::test]
async fn print_resume_after_lamination_processed_its_output() {
    assert_print_resume(OrderProgressBatchWipStatus::Processed).await;
}

async fn paused_lamination(
    service: &ProductionMapService,
) -> (OrderProgressBatch, OrderProgressBatch, OrderRunSession) {
    let input = pause_first_stage_batch(service, ORDER, FLOW_PECHAT_ID, &actor(), 30.0)
        .await
        .unwrap();
    scan_start(service, LAMINATION_1_ID, &input).await;
    let paused = run(service, LAMINATION_1_ID, Action::Pause, output())
        .await
        .unwrap();
    (
        input,
        paused.progress_batch.unwrap(),
        paused.session.unwrap(),
    )
}

#[tokio::test]
async fn lamination_resume_keeps_its_input_while_rezka_uses_its_output() {
    let (service, store) = fixture().await;
    let (input, output, session) = paused_lamination(&service).await;
    scan_start(&service, REZKA_ID, &output).await;
    let before_input = store
        .progress_batch(&input.batch_id)
        .await
        .unwrap()
        .unwrap();
    let before_output = store
        .progress_batch(&output.batch_id)
        .await
        .unwrap()
        .unwrap();
    let resumed = run(
        &service,
        LAMINATION_1_ID,
        Action::Resume,
        QueueProgressInput::default(),
    )
    .await
    .unwrap();
    let resumed_session = resumed.session.unwrap();
    assert_eq!(resumed_session.session_id, session.session_id);
    assert_eq!(
        resumed_session.payload_json["input_progress_batch_id"],
        input.batch_id
    );
    assert_eq!(
        resumed_session.payload_json["input_lineage"],
        session.payload_json["input_lineage"]
    );
    assert_eq!(
        store
            .progress_batch(&input.batch_id)
            .await
            .unwrap()
            .unwrap(),
        before_input
    );
    assert_eq!(
        store
            .progress_batch(&output.batch_id)
            .await
            .unwrap()
            .unwrap(),
        before_output
    );
    let completed = run(&service, LAMINATION_1_ID, Action::Complete, completion())
        .await
        .unwrap();
    assert_eq!(
        completed.progress_batch.unwrap().parent_batch_id,
        input.batch_id
    );
    assert_eq!(
        store
            .progress_batch(&input.batch_id)
            .await
            .unwrap()
            .unwrap()
            .wip_status,
        OrderProgressBatchWipStatus::Processed
    );
    assert_eq!(
        store
            .progress_batch(&output.batch_id)
            .await
            .unwrap()
            .unwrap(),
        before_output
    );
}

#[tokio::test]
async fn lamination_resume_rejects_missing_unowned_or_finished_input() {
    for invalid in [
        "missing-link",
        "other-session",
        "other-apparatus",
        "processed",
        "waiting",
    ] {
        let (service, store) = fixture().await;
        let (input, output, mut session) = paused_lamination(&service).await;
        if invalid == "missing-link" {
            session.payload_json["input_progress_batch_id"] = serde_json::json!("");
            store.put_order_run_session(session.clone()).await.unwrap();
        } else {
            let mut input = store
                .progress_batch(&input.batch_id)
                .await
                .unwrap()
                .unwrap();
            match invalid {
                "other-session" => input.used_by_session_id = "another-session".into(),
                "other-apparatus" => input.used_by_apparatus = LAMINATION_2_ID.into(),
                "processed" => input.wip_status = OrderProgressBatchWipStatus::Processed,
                "waiting" => input.wip_status = OrderProgressBatchWipStatus::Waiting,
                _ => unreachable!(),
            }
            store.put_order_progress_batch(input).await.unwrap();
        }
        assert!(
            run(
                &service,
                LAMINATION_1_ID,
                Action::Resume,
                QueueProgressInput::default()
            )
            .await
            .is_err(),
            "must reject {invalid}"
        );
        assert_eq!(
            store
                .order_run_session(&session.session_id)
                .await
                .unwrap()
                .unwrap()
                .status,
            OrderRunStatus::Paused
        );
        assert_eq!(
            service.apparatus_queue_states().await.unwrap()[LAMINATION_1_ID][ORDER],
            "paused"
        );
        assert_eq!(
            store
                .progress_batch(&output.batch_id)
                .await
                .unwrap()
                .unwrap(),
            output
        );
    }
}

#[tokio::test]
async fn opening_wip_resume_preserves_input_and_downstream_output() {
    let (service, store) = fixture().await;
    let opening = service
        .create_opening_wip(
            OpeningWipCreateInput {
                idempotency_key: "resume-opening".into(),
                order_id: ORDER.into(),
                entry_apparatus: String::new(),
                source_operation: "Bosma".into(),
                source_apparatus: FLOW_PECHAT_ID.into(),
                source_stage_node_id: "apparatus".into(),
                current_location: String::new(),
                note: String::new(),
                batches: vec![OpeningWipBatchInput {
                    quantity_basis: OpeningWipQuantityBasis::Measured,
                    finished_goods_meter: Some(100.0),
                    finished_goods_kg: Some(10.0),
                    bobina_kg: Some(1.0),
                    diameter: None,
                }],
            },
            actor(),
        )
        .await
        .unwrap();
    let input = &opening.batches[0];
    run(
        &service,
        LAMINATION_1_ID,
        Action::Start,
        QueueProgressInput {
            qr_payload: input.qr_payload.clone(),
            ..QueueProgressInput::default()
        },
    )
    .await
    .unwrap();
    let paused = run(&service, LAMINATION_1_ID, Action::Pause, output())
        .await
        .unwrap();
    let output = paused.progress_batch.unwrap();
    scan_start(&service, REZKA_ID, &output).await;
    let before_output = store
        .progress_batch(&output.batch_id)
        .await
        .unwrap()
        .unwrap();
    let before_input = store
        .opening_wip_batch(&input.batch_id, &input.qr_payload)
        .await
        .unwrap()
        .unwrap();
    let resumed = run(
        &service,
        LAMINATION_1_ID,
        Action::Resume,
        QueueProgressInput::default(),
    )
    .await
    .unwrap();
    let session = resumed.session.unwrap();
    assert_eq!(session.status, OrderRunStatus::Active);
    assert_eq!(
        session.payload_json["input_progress_batch_id"],
        input.batch_id
    );
    assert_eq!(session.payload_json["input_wip_source_kind"], "opening_wip");
    assert!(resumed.progress_batch.is_none());
    assert_eq!(
        store
            .opening_wip_batch(&input.batch_id, &input.qr_payload)
            .await
            .unwrap()
            .unwrap(),
        before_input
    );
    assert_eq!(
        store
            .progress_batch(&output.batch_id)
            .await
            .unwrap()
            .unwrap(),
        before_output
    );
}
