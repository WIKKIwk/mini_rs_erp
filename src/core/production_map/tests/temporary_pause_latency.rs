use super::*;
use std::time::Duration;

const ORDER: &str = "zakaz-temporary-pause-guard";

async fn running_order() -> (
    ProductionMapService,
    Arc<MemoryProductionMapStore>,
    QueueActionActor,
    TemporaryPauseControl,
) {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let actor = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "pause-guard-worker".into(),
        display_name: "Pause guard worker".into(),
    };
    service
        .upsert_map(apparatus_stage_map(ORDER, FLOW_PECHAT_ID))
        .await
        .unwrap();
    start_first_stage(&service, ORDER, FLOW_PECHAT_ID, actor.clone())
        .await
        .unwrap();
    let control = service.queue_action_controls().await.unwrap()[FLOW_PECHAT_ID][ORDER]
        .temporary_pause
        .clone()
        .unwrap();
    (service, store, actor, control)
}

async fn pause(
    service: &ProductionMapService,
    actor: QueueActionActor,
    control: &TemporaryPauseControl,
) -> Result<(), ProductionMapError> {
    service
        .temporarily_pause_work(
            FLOW_PECHAT_ID,
            ORDER,
            &control.session_id,
            &control.token,
            &[FLOW_PECHAT_ID.into()],
            actor,
        )
        .await
}

#[tokio::test]
async fn temporary_pause_does_not_wait_for_another_apparatus() {
    let (service, store, actor, control) = running_order().await;
    let _other_machine = service.apparatus_action_guard(FLOW_REZKA_ID).await;
    tokio::time::timeout(Duration::from_secs(1), pause(&service, actor, &control))
        .await
        .expect("a busy independent apparatus must not delay detaching this roll")
        .unwrap();
    let session = store
        .order_run_session(&control.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(session.status, OrderRunStatus::Paused);
    assert!(
        store
            .progress_batches_for_order(ORDER)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn temporary_pause_still_waits_for_its_own_apparatus() {
    let (service, store, actor, control) = running_order().await;
    let same_machine = service.apparatus_action_guard(FLOW_PECHAT_ID).await;
    let pending = pause(&service, actor, &control);
    tokio::pin!(pending);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut pending)
            .await
            .is_err()
    );
    assert_eq!(
        store
            .order_run_session(&control.session_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        OrderRunStatus::Active
    );
    drop(same_machine);
    pending.await.unwrap();
    assert_eq!(
        store
            .order_run_session(&control.session_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        OrderRunStatus::Paused
    );
}

#[tokio::test]
async fn temporary_pause_still_waits_for_broad_map_or_freeze_mutations() {
    let (service, store, actor, control) = running_order().await;
    let broad = service.queue_action_guard().await;
    let pending = pause(&service, actor, &control);
    tokio::pin!(pending);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut pending)
            .await
            .is_err()
    );
    assert_eq!(
        store
            .order_run_session(&control.session_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        OrderRunStatus::Active
    );
    drop(broad);
    pending.await.unwrap();
    assert_eq!(
        store
            .order_run_session(&control.session_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        OrderRunStatus::Paused
    );
}
