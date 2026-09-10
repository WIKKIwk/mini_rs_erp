use super::*;
use queue_state::ApparatusQueueAction as A;

fn worker(id: &str) -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: id.into(),
        display_name: id.into(),
    }
}
async fn action(
    service: &ProductionMapService,
    apparatus: &str,
    action: A,
    progress: QueueProgressInput,
) -> ApparatusQueueActionResult {
    service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            "zakaz-cooperative",
            action,
            &[apparatus.into()],
            worker(apparatus),
            progress,
        )
        .await
        .unwrap_or_else(|e| panic!("{apparatus} {action:?}: {e:?}"))
}
async fn parallel_fixture() -> (
    ProductionMapService,
    Arc<MemoryProductionMapStore>,
    OrderProgressBatch,
) {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    service
        .upsert_map(unassigned_alternative_next_stage_map(
            "zakaz-cooperative",
            FLOW_PECHAT_ID,
            LAMINATION_1_ID,
            LAMINATION_2_ID,
        ))
        .await
        .unwrap();
    action(
        &service,
        FLOW_PECHAT_ID,
        A::Start,
        QueueProgressInput::default(),
    )
    .await;
    let first = action(
        &service,
        FLOW_PECHAT_ID,
        A::DetachRoll,
        QueueProgressInput {
            produced_qty: Some(50.0),
            uom: "m".into(),
            ..Default::default()
        },
    )
    .await
    .progress_batch
    .unwrap();
    action(
        &service,
        FLOW_PECHAT_ID,
        A::Resume,
        QueueProgressInput::default(),
    )
    .await;
    action(
        &service,
        LAMINATION_1_ID,
        A::Start,
        QueueProgressInput {
            qr_payload: first.qr_payload.clone(),
            ..Default::default()
        },
    )
    .await;
    assert!(
        service
            .apply_apparatus_queue_action_with_progress(
                LAMINATION_2_ID,
                "zakaz-cooperative",
                A::Start,
                &[LAMINATION_2_ID.into()],
                worker(LAMINATION_2_ID),
                QueueProgressInput {
                    qr_payload: first.qr_payload,
                    ..Default::default()
                }
            )
            .await
            .is_err()
    );
    let last = action(
        &service,
        FLOW_PECHAT_ID,
        A::DetachRoll,
        QueueProgressInput {
            produced_qty: Some(50.0),
            uom: "m".into(),
            ..Default::default()
        },
    )
    .await
    .progress_batch
    .unwrap();
    action(
        &service,
        LAMINATION_2_ID,
        A::Start,
        QueueProgressInput {
            qr_payload: last.qr_payload.clone(),
            ..Default::default()
        },
    )
    .await;
    let states = service.apparatus_queue_states().await.unwrap();
    for apparatus in [LAMINATION_1_ID, LAMINATION_2_ID] {
        assert_eq!(states[apparatus]["zakaz-cooperative"], "in_progress");
    }
    (service, store, last)
}
fn output(full: bool) -> QueueProgressInput {
    QueueProgressInput {
        produced_qty: Some(45.0),
        uom: "m".into(),
        finished_goods_meter: Some(45.0),
        finished_goods_kg: Some(10.0),
        lamination_print_leftover_rolls: full.then_some(1.0),
        lamination_film_leftover_rolls: full.then_some(1.0),
        total_waste: full.then_some(1.0),
        ..Default::default()
    }
}
async fn report(service: &ProductionMapService, apparatus: &str) {
    service
        .record_laminatsiya_astatka(
            apparatus,
            "zakaz-cooperative",
            worker(apparatus),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            None,
            None,
            None,
            "",
        )
        .await
        .unwrap();
}
#[tokio::test]
async fn cooperative_parallel_finish_requires_each_local_report_and_waits_for_active_peer() {
    let (service, _, last) = parallel_fixture().await;
    action(
        &service,
        FLOW_PECHAT_ID,
        A::Complete,
        bosma_closing_input(&last),
    )
    .await;
    assert!(
        service.queue_action_controls().await.unwrap()[LAMINATION_2_ID]["zakaz-cooperative"]
            .complete_requires_full_report
    );
    action(&service, LAMINATION_2_ID, A::Complete, output(true)).await;
    assert_eq!(
        service
            .order_status_detail("zakaz-cooperative")
            .await
            .unwrap()
            .lifecycle_status,
        ProductionOrderLifecycleStatus::InProgress
    );
    action(&service, LAMINATION_1_ID, A::Complete, output(true)).await;
    assert_eq!(
        service
            .order_status_detail("zakaz-cooperative")
            .await
            .unwrap()
            .lifecycle_status,
        ProductionOrderLifecycleStatus::ProductionCompleted
    );
    let controls = service.queue_action_controls().await.unwrap();
    let work = controls[LAMINATION_1_ID]["zakaz-cooperative"]
        .stage_work
        .as_ref()
        .unwrap();
    assert!(work.completed);
    assert_eq!(work.last_apparatus, LAMINATION_1_ID);
}
#[tokio::test]
async fn cooperative_late_source_close_reuses_both_manual_reports_without_extra_output() {
    let (service, store, last) = parallel_fixture().await;
    action(&service, LAMINATION_1_ID, A::Complete, output(false)).await;
    action(&service, LAMINATION_2_ID, A::Complete, output(false)).await;
    report(&service, LAMINATION_2_ID).await;
    report(&service, LAMINATION_1_ID).await;
    let before = store
        .progress_batches_for_order("zakaz-cooperative")
        .await
        .unwrap();
    action(
        &service,
        FLOW_PECHAT_ID,
        A::Complete,
        bosma_closing_input(&last),
    )
    .await;
    assert_eq!(
        before,
        store
            .progress_batches_for_order("zakaz-cooperative")
            .await
            .unwrap()
    );
    assert_eq!(
        service
            .order_status_detail("zakaz-cooperative")
            .await
            .unwrap()
            .lifecycle_status,
        ProductionOrderLifecycleStatus::ProductionCompleted
    );
    let controls = service.queue_action_controls().await.unwrap();
    for apparatus in [LAMINATION_1_ID, LAMINATION_2_ID] {
        let work = controls[apparatus]["zakaz-cooperative"]
            .stage_work
            .as_ref()
            .unwrap();
        assert!(!work.astatka_required);
        assert_eq!(work.last_apparatus, LAMINATION_1_ID);
    }
    assert_eq!(
        service.fully_completed_orders(10).await.unwrap()[0].closed_by_ref,
        LAMINATION_1_ID
    );
}
#[tokio::test]
async fn cooperative_late_source_close_prompts_only_missing_machine_report() {
    let (service, _, last) = parallel_fixture().await;
    action(&service, LAMINATION_1_ID, A::Complete, output(false)).await;
    action(&service, LAMINATION_2_ID, A::Complete, output(false)).await;
    report(&service, LAMINATION_2_ID).await;
    action(
        &service,
        FLOW_PECHAT_ID,
        A::Complete,
        bosma_closing_input(&last),
    )
    .await;
    let controls = service.queue_action_controls().await.unwrap();
    assert!(
        controls[LAMINATION_1_ID]["zakaz-cooperative"]
            .stage_work
            .as_ref()
            .unwrap()
            .astatka_required
    );
    assert!(
        !controls[LAMINATION_2_ID]["zakaz-cooperative"]
            .stage_work
            .as_ref()
            .unwrap()
            .astatka_required
    );
    report(&service, LAMINATION_1_ID).await;
    assert_eq!(
        service
            .order_status_detail("zakaz-cooperative")
            .await
            .unwrap()
            .lifecycle_status,
        ProductionOrderLifecycleStatus::ProductionCompleted
    );
}

#[tokio::test]
async fn cooperative_each_manual_report_hides_only_its_machine_before_shared_closure() {
    let (service, store, last) = parallel_fixture().await;
    action(&service, LAMINATION_1_ID, A::Complete, output(false)).await;
    action(&service, LAMINATION_2_ID, A::Complete, output(false)).await;
    action(&service, FLOW_PECHAT_ID, A::Complete, bosma_closing_input(&last)).await;
    let before = service.live_snapshot_shared().await.unwrap();
    for machine in [LAMINATION_1_ID, LAMINATION_2_ID] {
        assert!(before.visible_order_ids[machine].contains(&"zakaz-cooperative".into()));
    }
    let states = store.apparatus_queue_states().await.unwrap();
    let batches = store.progress_batches_for_order("zakaz-cooperative").await.unwrap();
    report(&service, LAMINATION_1_ID).await;
    let first = service.live_snapshot_shared().await.unwrap();
    assert!(!first.visible_order_ids[LAMINATION_1_ID].contains(&"zakaz-cooperative".into()),
        "the accepted local report must not wait for the peer's report");
    assert!(first.visible_order_ids[LAMINATION_2_ID].contains(&"zakaz-cooperative".into()));
    assert_eq!(first.order_statuses["zakaz-cooperative"].lifecycle_status, ProductionOrderLifecycleStatus::InProgress);
    assert!(!first.queue_action_controls[LAMINATION_1_ID]["zakaz-cooperative"].stage_work.as_ref().unwrap().completed);
    report(&service, LAMINATION_2_ID).await;
    let both = service.live_snapshot_shared().await.unwrap();
    for machine in [LAMINATION_1_ID, LAMINATION_2_ID] {
        assert!(!both.visible_order_ids[machine].contains(&"zakaz-cooperative".into()));
    }
    assert_eq!(both.order_statuses["zakaz-cooperative"].lifecycle_status, ProductionOrderLifecycleStatus::ProductionCompleted);
    assert_eq!(both.queue_action_controls[LAMINATION_1_ID]["zakaz-cooperative"].stage_work.as_ref().unwrap().last_apparatus, LAMINATION_2_ID);
    assert_eq!(store.apparatus_queue_states().await.unwrap(), states, "visibility must not manufacture queue completions");
    assert_eq!(store.progress_batches_for_order("zakaz-cooperative").await.unwrap(), batches, "reports must not create WIP");
}
