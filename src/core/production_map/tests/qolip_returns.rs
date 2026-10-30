use super::fixtures::{canonical_apparatus_stage_map, service_with_default_apparatus};
use crate::core::production_map::*;
use crate::core::qolip::{QolipOrderStartPreparation, QolipProductSpec};
use queue_state::ApparatusQueueAction as Action;
use std::sync::Arc;

#[tokio::test]
async fn physical_qolip_return_requires_full_reacquisition_before_resuming() {
    const PRINT: &str = "apparatus:default:bosma_7";
    const ORDER: &str = "zakaz-returned-tooling";
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = service_with_default_apparatus(store.clone()).await;
    service
        .upsert_map(canonical_apparatus_stage_map(ORDER, PRINT, "Print"))
        .await
        .unwrap();
    let assigned = [PRINT.to_string()];
    let actor = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "printer".into(),
        display_name: "Printer".into(),
    };
    let codes = ["RETURNED-1".to_string(), "RETURNED-2".to_string()];
    let mut start = service
        .prepare_apparatus_queue_action_with_progress(
            PRINT,
            ORDER,
            Action::Start,
            &assigned,
            actor.clone(),
            Default::default(),
        )
        .await
        .unwrap();
    start.attach_qolip_set(&codes, "returned-set");
    service.commit_prepared_queue_action(start).await.unwrap();
    service
        .apply_apparatus_queue_action_with_progress(
            PRINT,
            ORDER,
            Action::DetachRoll,
            &assigned,
            actor.clone(),
            QueueProgressInput {
                produced_qty: Some(10.0),
                gross_qty: Some(2.0),
                bobina_kg: Some(0.5),
                uom: "m".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let mut paused = store
        .active_order_run_session(PRINT, ORDER)
        .await
        .unwrap()
        .unwrap();
    paused.release_returned_qolips("physical-checkout", 42);
    store.put_order_run_session(paused.clone()).await.unwrap();
    store
        .put_apparatus_queue_states(
            PRINT,
            std::collections::BTreeMap::from([(ORDER.into(), "paused".into())]),
        )
        .await
        .unwrap();
    assert!(
        service
            .qolip_validation_required_for_action(PRINT, ORDER, Action::Resume)
            .await
            .unwrap()
    );
    let snapshot = service.live_snapshot().await.unwrap();
    assert_eq!(
        snapshot.queue_action_controls[PRINT][ORDER]
            .interaction
            .qolip_mode,
        ApparatusQueueQolipMode::ScanRequired
    );
    assert_eq!(
        service
            .apply_apparatus_queue_action_with_progress(
                PRINT,
                ORDER,
                Action::Resume,
                &assigned,
                actor.clone(),
                Default::default()
            )
            .await,
        Err(ProductionMapError::QolipCodeMismatch)
    );

    let id = crate::core::apparatus_standard::ApparatusId::new(PRINT).unwrap();
    let preparations = codes
        .iter()
        .map(|code| QolipOrderStartPreparation {
            spec: QolipProductSpec {
                qolip_set_id: "returned-set".into(),
                qolip_code: code.clone(),
                ..Default::default()
            },
            checkout: None,
        })
        .collect::<Vec<_>>();
    let mut resume = service
        .prepare_apparatus_queue_action_with_material_scan_and_progress(
            MaterialScanProgressAction {
                apparatus: PRINT,
                order_id: ORDER,
                action: Action::Resume,
                assigned_apparatus: &assigned,
                actor,
                material_barcodes: &[],
                state_material_barcodes: &[],
                progress: Default::default(),
                qolip_validation: TrustedQolipStartValidation::from_preparations(
                    &id,
                    ORDER,
                    &preparations,
                ),
            },
        )
        .await
        .unwrap();
    resume.attach_qolip_set(&codes, "returned-set");
    let resumed = service
        .commit_prepared_queue_action(resume)
        .await
        .unwrap()
        .session
        .unwrap();
    assert_eq!(resumed.status, OrderRunStatus::Active);
    assert_eq!(resumed.payload_json["qolip_lock_owner"], true);
    assert!(!resumed.qolip_reacquisition_required());
    assert_eq!(
        ProductionQrSessionResources::for_session(&resumed).qolip_codes,
        codes
    );
    assert!(
        resumed.payload_json.get("_resume_pause").is_none(),
        "resource reacquisition cannot be reversed to a released snapshot"
    );
}
