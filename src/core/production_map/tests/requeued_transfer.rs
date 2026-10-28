use super::super::fixtures::canonical_two_stage_map;
use super::*;

fn admin() -> QueueActionActor {
    QueueActionActor {
        role: "admin".into(),
        ref_: "transfer-admin".into(),
        display_name: "Admin".into(),
    }
}

fn worker() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "transfer-worker".into(),
        display_name: "Worker".into(),
    }
}

fn request(id: &str) -> ProductionMapApparatusTransferRequest {
    ProductionMapApparatusTransferRequest {
        order_id: id.into(),
        from_apparatus: PECHAT_7_ID.into(),
        to_apparatus: PECHAT_8_ID.into(),
        reason: "Muzdan chiqarilgan orderni boshqa bosmaga ko‘chirish".into(),
        idempotency_key: format!("requeued-transfer-{id}"),
    }
}

#[tokio::test]
async fn requeued_transfer_preserves_pending_session_output_materials_and_resume() {
    for scope in ["global", "stage", "worker-issue"] {
        let id = format!("zakaz-requeued-transfer-{scope}");
        let store = Arc::new(MemoryProductionMapStore::new());
        let service = default_service_with_store(store.clone()).await;
        let mut map = canonical_two_stage_map(
            &id,
            PECHAT_7_ID,
            "Bosma 7",
            LAMINATION_1_ID,
            "Laminatsiya 1",
        );
        map.roll_count = Some(7);
        map.width_mm = Some(650.0);
        service.upsert_map(map).await.unwrap();
        let target_before = format!("zakaz-target-before-{scope}");
        service
            .upsert_map(canonical_apparatus_stage_map(
                &target_before,
                PECHAT_8_ID,
                "Bosma 8",
            ))
            .await
            .unwrap();
        service
            .set_apparatus_sequence(PECHAT_8_ID, vec![target_before.clone()])
            .await
            .unwrap();
        let batch = if scope == "worker-issue" {
            start_first_stage(&service, &id, PECHAT_7_ID, worker())
                .await
                .unwrap();
            service
                .apply_apparatus_queue_action_with_progress(
                    PECHAT_7_ID,
                    &id,
                    queue_state::ApparatusQueueAction::Freeze,
                    &[PECHAT_7_ID.into()],
                    worker(),
                    QueueProgressInput {
                        freeze_with_issue: true,
                        description: "Bosmada rang muammosi".into(),
                        ..QueueProgressInput::default()
                    },
                )
                .await
                .unwrap();
            None
        } else {
            let batch = pause_first_stage_batch(&service, &id, PECHAT_7_ID, &worker(), 12.5)
                .await
                .unwrap();
            let frozen = if scope == "global" {
                service.request_order_freeze(&id, admin()).await.unwrap()
            } else {
                service
                    .request_stage_freeze(&id, PECHAT_7_ID, "apparatus", admin())
                    .await
                    .unwrap()
            };
            assert_eq!(frozen.state, OrderControlState::Frozen);
            Some(batch)
        };
        // Holds remain effective until the administrator explicitly releases them.
        assert_eq!(
            service
                .transfer_apparatus_order(request(&id), admin())
                .await,
            Err(ProductionMapError::OrderFrozen)
        );
        if scope == "global" {
            service.unfreeze_order(&id, admin()).await.unwrap();
        } else {
            service
                .unfreeze_stage(&id, PECHAT_7_ID, "apparatus", admin())
                .await
                .unwrap();
        }
        let original_session = service.order_run_sessions_for_order(&id).await.unwrap();
        assert_eq!(
            original_session.len(),
            1,
            "no downstream apparatus has started"
        );
        let original_session = &original_session[0];
        assert!(original_session.is_requeued_at_tail());
        store
            .put_raw_material_assignment(RawMaterialAssignment {
                order_id: id.clone(),
                apparatus_id: ApparatusId::new(PECHAT_7_ID).unwrap(),
                apparatus: "Bosma 7".into(),
                barcode: "REQUEUED-RAW".into(),
                item_code: "RAW".into(),
                item_name: "Raw material".into(),
                item_group: "Film".into(),
                assigned_by_role: "admin".into(),
                assigned_by_ref: "transfer-admin".into(),
                assigned_by_display_name: "Admin".into(),
                assigned_at: "now".into(),
            })
            .await
            .unwrap();
        let result = service
            .transfer_apparatus_order(request(&id), admin())
            .await
            .unwrap();
        assert_eq!(
            result.transfer.queue_state,
            queue_state::ApparatusQueueOrderState::Pending
        );
        assert_eq!(
            result.transfer.session.session_id,
            original_session.session_id
        );
        assert!(result.transfer.session.is_requeued_at_tail());
        assert_eq!(result.transfer.session.apparatus, PECHAT_8_ID);
        assert_eq!(result.transfer.session.stage_node_id, "apparatus");
        let states = store.apparatus_queue_states().await.unwrap();
        assert!(!states[PECHAT_7_ID].contains_key(&id));
        assert_eq!(states[PECHAT_8_ID][&id], "pending");
        assert_eq!(
            store.apparatus_sequences().await.unwrap()[PECHAT_8_ID],
            vec![target_before.clone(), id.clone()]
        );
        let batches = store.progress_batches_for_order(&id).await.unwrap();
        match batch {
            Some(batch) => {
                assert_eq!(batches.len(), 1);
                assert_eq!(batches[0].batch_id, batch.batch_id);
                assert_eq!(batches[0].produced_qty, batch.produced_qty);
                assert_eq!(batches[0].apparatus, PECHAT_8_ID);
                assert_eq!(result.transfer.progress_batch_id, batch.batch_id);
            }
            None => {
                assert!(batches.is_empty(), "no synthetic WIP or production output");
                assert!(result.transfer.progress_batch.is_none());
                assert!(result.transfer.progress_batch_id.is_empty());
            }
        }
        assert_eq!(
            store.raw_material_assignments().await.unwrap()[0]
                .apparatus_id
                .as_str(),
            PECHAT_8_ID
        );
        let audit = service.audit_production_workflow().await.unwrap();
        assert!(
            audit.violations.is_empty(),
            "{scope}: {:?}",
            audit.violations
        );
        let replay = service
            .transfer_apparatus_order(request(&id), admin())
            .await
            .unwrap();
        assert_eq!(replay.transfer, result.transfer);
        let snapshot = service.live_snapshot().await.unwrap();
        let control = &snapshot.queue_action_controls[PECHAT_8_ID][&id];
        assert_eq!(
            control.interaction.mode,
            ApparatusQueueInteractionMode::RequeuedWaiting
        );
        assert!(
            !control
                .allowed_actions
                .contains(&queue_state::ApparatusQueueAction::Resume)
        );
        service
            .set_apparatus_sequence(PECHAT_8_ID, vec![id.clone(), target_before])
            .await
            .unwrap();
        let target_id = ApparatusId::new(PECHAT_8_ID).unwrap();
        service
            .apply_apparatus_queue_action_with_material_scan_and_progress(
                MaterialScanProgressAction {
                    apparatus: PECHAT_8_ID,
                    order_id: &id,
                    action: queue_state::ApparatusQueueAction::Resume,
                    assigned_apparatus: &[PECHAT_8_ID.into()],
                    actor: worker(),
                    material_barcodes: &[],
                    state_material_barcodes: &[],
                    progress: QueueProgressInput::default(),
                    qolip_validation: TrustedQolipStartValidation::from_preparations(
                        &target_id,
                        &id,
                        &[QolipOrderStartPreparation {
                            spec: QolipProductSpec {
                                qolip_code: "TRANSFER-QOLIP".into(),
                                ..QolipProductSpec::default()
                            },
                            checkout: None,
                        }],
                    ),
                },
            )
            .await
            .unwrap();
        let sessions = service.order_run_sessions_for_order(&id).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, original_session.session_id);
        assert_eq!(sessions[0].status, OrderRunStatus::Active);
        assert_eq!(sessions[0].apparatus, PECHAT_8_ID);
    }
}

#[tokio::test]
async fn requeued_transfer_does_not_accept_an_unmarked_pending_session() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let id = "zakaz-unmarked-pending-transfer";
    service
        .upsert_map(canonical_apparatus_stage_map(id, PECHAT_7_ID, "Bosma 7"))
        .await
        .unwrap();
    pause_first_stage_batch(&service, id, PECHAT_7_ID, &worker(), 10.0)
        .await
        .unwrap();
    store
        .put_apparatus_queue_states(PECHAT_7_ID, BTreeMap::from([(id.into(), "pending".into())]))
        .await
        .unwrap();
    assert_eq!(
        service.transfer_apparatus_order(request(id), admin()).await,
        Err(ProductionMapError::ApparatusTransferOrderNotPaused)
    );
    assert!(
        store
            .apparatus_transfers_for_audit()
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        service.order_run_sessions_for_order(id).await.unwrap()[0].apparatus,
        PECHAT_7_ID
    );
}

#[tokio::test]
async fn requeued_transfer_keeps_legacy_paused_receipts_readable() {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store).await;
    let id = "zakaz-legacy-paused-transfer";
    service
        .upsert_map(canonical_apparatus_stage_map(id, PECHAT_7_ID, "Bosma 7"))
        .await
        .unwrap();
    pause_first_stage_batch(&service, id, PECHAT_7_ID, &worker(), 10.0)
        .await
        .unwrap();
    let result = service
        .transfer_apparatus_order(request(id), admin())
        .await
        .unwrap();
    let mut legacy = serde_json::to_value(&result.transfer).unwrap();
    legacy.as_object_mut().unwrap().remove("queue_state");
    let restored: ProductionMapApparatusTransferRecord = serde_json::from_value(legacy).unwrap();
    assert_eq!(
        restored.queue_state,
        queue_state::ApparatusQueueOrderState::Paused
    );
    assert_eq!(restored.progress_batch, result.transfer.progress_batch);
}
