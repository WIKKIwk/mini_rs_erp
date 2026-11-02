use super::super::fixtures::canonical_two_stage_map;
use super::*;

fn worker() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "print-transfer-worker".into(),
        display_name: "Worker".into(),
    }
}

fn request(order: &str, from: &str, to: &str) -> ProductionMapApparatusTransferRequest {
    ProductionMapApparatusTransferRequest {
        order_id: order.into(),
        from_apparatus: from.into(),
        to_apparatus: to.into(),
        reason: "Apparat buzildi".into(),
        idempotency_key: format!("{order}:{from}:{to}"),
    }
}

async fn fixture(order: &str) -> (ProductionMapService, Arc<MemoryProductionMapStore>) {
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let mut map = canonical_two_stage_map(
        order,
        PECHAT_7_ID,
        "Bosma 7",
        LAMINATION_1_ID,
        "Laminatsiya 1",
    );
    map.roll_count = Some(7);
    map.width_mm = Some(800.0);
    service.upsert_map(map).await.unwrap();
    (service, store)
}

async fn resume(service: &ProductionMapService, order: &str, apparatus: &str) {
    let result = service
        .apply_apparatus_queue_action_with_progress(
            apparatus,
            order,
            queue_state::ApparatusQueueAction::Resume,
            &[apparatus.into()],
            worker(),
            QueueProgressInput::default(),
        )
        .await
        .unwrap();
    assert!(
        result.progress_batch.is_none(),
        "Resume must not rewrite a historical QR"
    );
}

#[tokio::test]
async fn printing_transfer_accepts_detached_rolls_and_preserves_all_output_history() {
    for action in [
        queue_state::ApparatusQueueAction::Pause,
        queue_state::ApparatusQueueAction::DetachRoll,
    ] {
        let order = format!("zakaz-print-transfer-{action:?}").to_lowercase();
        let (service, store) = fixture(&order).await;
        pause_first_stage_batch(&service, &order, PECHAT_7_ID, &worker(), 12.0)
            .await
            .unwrap();
        resume(&service, &order, PECHAT_7_ID).await;
        service
            .apply_apparatus_queue_action_with_progress(
                PECHAT_7_ID,
                &order,
                action,
                &[PECHAT_7_ID.into()],
                worker(),
                QueueProgressInput {
                    produced_qty: Some(7.0),
                    uom: "kg".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let before = store.progress_batches_for_order(&order).await.unwrap();
        assert_eq!(before.len(), 2);
        let result = service
            .transfer_apparatus_order(request(&order, PECHAT_7_ID, PECHAT_8_ID), worker())
            .await
            .unwrap();
        assert!(result.transfer.progress_batch.is_none());
        assert_eq!(
            store.progress_batches_for_order(&order).await.unwrap(),
            before
        );
        assert_eq!(result.transfer.session.status, OrderRunStatus::Paused);
        assert_eq!(
            result
                .saved
                .map
                .nodes
                .iter()
                .find(|n| n.id == "apparatus")
                .unwrap()
                .apparatus_id,
            PECHAT_7_ID
        );
        resume(&service, &order, PECHAT_8_ID).await;
        let new_output = service
            .apply_apparatus_queue_action_with_progress(
                PECHAT_8_ID,
                &order,
                queue_state::ApparatusQueueAction::Pause,
                &[PECHAT_8_ID.into()],
                worker(),
                QueueProgressInput {
                    produced_qty: Some(3.0),
                    uom: "kg".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
            .progress_batch
            .unwrap();
        assert_eq!(new_output.apparatus, PECHAT_8_ID);
        assert_ne!(
            new_output.payload_json["stage_node_id"],
            before[0].payload_json["stage_node_id"]
        );
        for old in &before {
            assert_eq!(
                store.progress_batch(&old.batch_id).await.unwrap().as_ref(),
                Some(old)
            );
        }
        let audit = service.audit_production_workflow().await.unwrap();
        assert!(audit.violations.is_empty(), "{:?}", audit.violations);
    }
}

#[tokio::test]
async fn printing_transfer_resumes_temporary_pause_without_synthetic_output() {
    let order = "zakaz-print-transfer-temporary";
    let (service, store) = fixture(order).await;
    start_first_stage(&service, order, PECHAT_7_ID, worker())
        .await
        .unwrap();
    let control = service.queue_action_controls().await.unwrap()[PECHAT_7_ID][order]
        .temporary_pause
        .clone()
        .unwrap();
    service
        .temporarily_pause_work(
            PECHAT_7_ID,
            order,
            &control.session_id,
            &control.token,
            &[PECHAT_7_ID.into()],
            worker(),
        )
        .await
        .unwrap();
    let stopped = store
        .order_run_session(&control.session_id)
        .await
        .unwrap()
        .unwrap();
    let result = service
        .transfer_apparatus_order(request(order, PECHAT_7_ID, PECHAT_8_ID), worker())
        .await
        .unwrap();
    assert_eq!(
        result.transfer.session.payload_json["_temporary_pause"],
        stopped.payload_json["_temporary_pause"]
    );
    resume(&service, order, PECHAT_8_ID).await;
    assert!(
        store
            .progress_batches_for_order(order)
            .await
            .unwrap()
            .is_empty()
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
    assert!(
        service
            .audit_production_workflow()
            .await
            .unwrap()
            .violations
            .is_empty()
    );
}

#[tokio::test]
async fn printing_transfer_can_fail_over_again_and_replay_without_changing_newer_work() {
    let order = "zakaz-print-transfer-again";
    let (service, store) = fixture(order).await;
    let output = pause_first_stage_batch(&service, order, PECHAT_7_ID, &worker(), 12.0)
        .await
        .unwrap();
    let first = service
        .transfer_apparatus_order(request(order, PECHAT_7_ID, PECHAT_8_ID), worker())
        .await
        .unwrap();
    let second = service
        .transfer_apparatus_order(request(order, PECHAT_8_ID, PECHAT_9_ID), worker())
        .await
        .unwrap();
    let replay = service
        .transfer_apparatus_order(request(order, PECHAT_7_ID, PECHAT_8_ID), worker())
        .await
        .unwrap();
    assert_eq!(replay.transfer, first.transfer);
    assert_eq!(
        store
            .order_run_session(&second.transfer.session_id)
            .await
            .unwrap()
            .unwrap()
            .apparatus,
        PECHAT_9_ID
    );
    assert_eq!(
        store.progress_batch(&output.batch_id).await.unwrap(),
        Some(output)
    );
    resume(&service, order, PECHAT_9_ID).await;
    let audit = service.audit_production_workflow().await.unwrap();
    assert!(audit.violations.is_empty(), "{:?}", audit.violations);
}

#[tokio::test]
async fn printing_transfer_leaves_downstream_waiting_in_use_and_processed_rolls_untouched() {
    for status in [
        OrderProgressBatchWipStatus::Waiting,
        OrderProgressBatchWipStatus::InUse,
        OrderProgressBatchWipStatus::Processed,
    ] {
        let order = format!("zakaz-print-transfer-downstream-{status:?}").to_lowercase();
        let (service, store) = fixture(&order).await;
        let output = pause_first_stage_batch(&service, &order, PECHAT_7_ID, &worker(), 12.0)
            .await
            .unwrap();
        if status != OrderProgressBatchWipStatus::Waiting {
            service
                .apply_apparatus_queue_action_with_progress(
                    LAMINATION_1_ID,
                    &order,
                    queue_state::ApparatusQueueAction::Start,
                    &[LAMINATION_1_ID.into()],
                    worker(),
                    QueueProgressInput {
                        qr_payload: output.qr_payload.clone(),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            if status == OrderProgressBatchWipStatus::Processed {
                service
                    .apply_apparatus_queue_action_with_progress(
                        LAMINATION_1_ID,
                        &order,
                        queue_state::ApparatusQueueAction::Complete,
                        &[LAMINATION_1_ID.into()],
                        worker(),
                        QueueProgressInput {
                            produced_qty: Some(12.0),
                            uom: "kg".into(),
                            total_waste: Some(0.1),
                            return_ink_kg: Some(0.1),
                            lamination_film_leftover_rolls: Some(0.1),
                            finished_goods_kg: Some(12.0),
                            finished_goods_meter: Some(120.0),
                            ..Default::default()
                        },
                    )
                    .await
                    .unwrap();
            }
        }
        let before = store
            .progress_batch(&output.batch_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(before.wip_status, status);
        service
            .transfer_apparatus_order(request(&order, PECHAT_7_ID, PECHAT_8_ID), worker())
            .await
            .unwrap();
        let audit = service.audit_production_workflow().await.unwrap();
        assert!(audit.violations.is_empty(), "{:?}", audit.violations);
        resume(&service, &order, PECHAT_8_ID).await;
        assert_eq!(
            store.progress_batch(&output.batch_id).await.unwrap(),
            Some(before)
        );
    }
}

#[tokio::test]
async fn printing_transfer_selects_existing_alternative_and_preserves_other_occurrences() {
    let order = "zakaz-print-transfer-alternatives";
    let (service, store) = fixture(order).await;
    let mut map = store.maps().await.unwrap().pop().unwrap();
    let source = map
        .nodes
        .iter_mut()
        .find(|node| node.id == "apparatus")
        .unwrap();
    source.alternative_group_id = "print-choice".into();
    source.alternative_assigned_apparatus_id = PECHAT_7_ID.into();
    let mut peer = source.clone();
    peer.id = "print-peer".into();
    peer.apparatus_id = PECHAT_8_ID.into();
    map.nodes.push(peer);
    let mut repeated = map
        .nodes
        .iter()
        .find(|node| node.id == "apparatus")
        .unwrap()
        .clone();
    repeated.id = "later-print".into();
    repeated.alternative_group_id.clear();
    repeated.alternative_assigned_apparatus_id.clear();
    map.nodes.push(repeated.clone());
    let edge = map.edges.iter_mut().find(|edge| edge.to == "end").unwrap();
    edge.to = repeated.id.clone();
    map.edges.push(ProductionMapEdge {
        from: repeated.id.clone(),
        to: "end".into(),
        branch: String::new(),
    });
    service.upsert_map(map).await.unwrap();
    let output = pause_first_stage_batch(&service, order, PECHAT_7_ID, &worker(), 12.0)
        .await
        .unwrap();
    let result = service
        .transfer_apparatus_order(request(order, PECHAT_7_ID, PECHAT_8_ID), worker())
        .await
        .unwrap();
    assert_eq!(result.transfer.session.stage_node_id, "print-peer");
    assert_eq!(
        result
            .saved
            .map
            .nodes
            .iter()
            .find(|node| node.id == repeated.id),
        Some(&repeated)
    );
    assert_eq!(
        store.progress_batch(&output.batch_id).await.unwrap(),
        Some(output)
    );
}

#[tokio::test]
async fn printing_transfer_moves_only_its_mounted_input_ownership() {
    let order = "zakaz-print-transfer-mounted-input";
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let mut map = canonical_two_stage_map(
        order,
        FLOW_PECHAT_ID,
        "First print",
        PECHAT_7_ID,
        "Second print",
    );
    map.roll_count = Some(7);
    map.width_mm = Some(800.0);
    service.upsert_map(map).await.unwrap();
    let input = pause_first_stage_batch(&service, order, FLOW_PECHAT_ID, &worker(), 12.0)
        .await
        .unwrap();
    service
        .apply_apparatus_queue_action_with_progress(
            PECHAT_7_ID,
            order,
            queue_state::ApparatusQueueAction::Start,
            &[PECHAT_7_ID.into()],
            worker(),
            QueueProgressInput {
                qr_payload: input.qr_payload.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let control = service.queue_action_controls().await.unwrap()[PECHAT_7_ID][order]
        .temporary_pause
        .clone()
        .unwrap();
    service
        .temporarily_pause_work(
            PECHAT_7_ID,
            order,
            &control.session_id,
            &control.token,
            &[PECHAT_7_ID.into()],
            worker(),
        )
        .await
        .unwrap();
    let before = store
        .progress_batch(&input.batch_id)
        .await
        .unwrap()
        .unwrap();
    let result = service
        .transfer_apparatus_order(request(order, PECHAT_7_ID, PECHAT_8_ID), worker())
        .await
        .unwrap();
    let after = store
        .progress_batch(&input.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.apparatus, before.apparatus);
    assert_eq!(after.qr_payload, before.qr_payload);
    assert_eq!(after.used_by_session_id, before.used_by_session_id);
    assert_eq!(after.used_by_apparatus, PECHAT_8_ID);
    assert_eq!(result.transfer.progress_batch_updates.len(), 1);
    resume(&service, order, PECHAT_8_ID).await;
}

#[tokio::test]
async fn printing_transfer_revalidates_target_tooling_and_required_materials_before_resume() {
    let order = "zakaz-print-transfer-resources";
    let store = Arc::new(MemoryProductionMapStore::new());
    let (service, canonical) = service_with_apparatus_store(store.clone(), &[]).await;
    seed_apparatus(
        &canonical,
        TestApparatusSpec::print(
            FLOW_ALT_PECHAT_ID,
            "Target",
            ProcessTechnology::Rotogravure,
            Some(8),
        )
        .requiring_tooling(),
    )
    .await
    .unwrap();
    set_test_material_rule(
        &canonical,
        ApparatusMaterialRuleUpsert {
            apparatus: FLOW_ALT_PECHAT_ID.into(),
            requires_material: true,
            start_policy: RawMaterialStartPolicy::StateAll,
            item_groups: vec!["Film".into()],
            requirement_groups: vec![],
        },
    )
    .await;
    let mut map =
        canonical_two_stage_map(order, PECHAT_7_ID, "Source", LAMINATION_1_ID, "Lamination");
    map.roll_count = Some(7);
    map.width_mm = Some(800.0);
    service.upsert_map(map).await.unwrap();
    let output = pause_first_stage_batch(&service, order, PECHAT_7_ID, &worker(), 12.0)
        .await
        .unwrap();
    store
        .put_raw_material_assignment(RawMaterialAssignment {
            order_id: order.into(),
            apparatus_id: ApparatusId::new(PECHAT_7_ID).unwrap(),
            apparatus: "Source".into(),
            barcode: "TRANSFER-FILM".into(),
            item_code: "FILM".into(),
            item_name: "Film".into(),
            item_group: "Film".into(),
            assigned_by_role: "admin".into(),
            assigned_by_ref: "admin".into(),
            assigned_by_display_name: "Admin".into(),
            assigned_at: "now".into(),
        })
        .await
        .unwrap();
    service
        .transfer_apparatus_order(request(order, PECHAT_7_ID, FLOW_ALT_PECHAT_ID), worker())
        .await
        .unwrap();
    let action = queue_state::ApparatusQueueAction::Resume;
    assert!(
        service
            .qolip_validation_required_for_action(FLOW_ALT_PECHAT_ID, order, action)
            .await
            .unwrap()
    );
    assert!(
        service
            .requires_material_reacquisition_for_action(FLOW_ALT_PECHAT_ID, order, action)
            .await
            .unwrap()
    );
    assert_eq!(
        service
            .apply_apparatus_queue_action_with_progress(
                FLOW_ALT_PECHAT_ID,
                order,
                action,
                &[FLOW_ALT_PECHAT_ID.into()],
                worker(),
                Default::default()
            )
            .await,
        Err(ProductionMapError::QolipCodeMismatch)
    );
    let target_id = ApparatusId::new(FLOW_ALT_PECHAT_ID).unwrap();
    let validation = || {
        TrustedQolipStartValidation::from_preparations(
            &target_id,
            order,
            &[QolipOrderStartPreparation {
                spec: QolipProductSpec {
                    qolip_code: "TARGET-TOOL".into(),
                    ..Default::default()
                },
                checkout: None,
            }],
        )
    };
    assert!(matches!(
        service
            .prepare_apparatus_queue_action_with_material_scan_and_progress(
                MaterialScanProgressAction {
                    apparatus: FLOW_ALT_PECHAT_ID,
                    order_id: order,
                    action,
                    assigned_apparatus: &[FLOW_ALT_PECHAT_ID.into()],
                    actor: worker(),
                    material_barcodes: &[],
                    state_material_barcodes: &["TRANSFER-FILM".into()],
                    progress: Default::default(),
                    qolip_validation: validation(),
                }
            )
            .await,
        Err(ProductionMapError::RawMaterialScanRequired)
    ));
    let mut prepared = service
        .prepare_apparatus_queue_action_with_material_scan_and_progress(
            MaterialScanProgressAction {
                apparatus: FLOW_ALT_PECHAT_ID,
                order_id: order,
                action,
                assigned_apparatus: &[FLOW_ALT_PECHAT_ID.into()],
                actor: worker(),
                material_barcodes: &["TRANSFER-FILM".into()],
                state_material_barcodes: &["TRANSFER-FILM".into()],
                progress: Default::default(),
                qolip_validation: validation(),
            },
        )
        .await
        .unwrap();
    prepared.attach_qolip_set(&["TARGET-TOOL".into()], "target-set");
    service
        .commit_prepared_queue_action(prepared)
        .await
        .unwrap();
    assert!(
        !service
            .qolip_validation_required_for_action(FLOW_ALT_PECHAT_ID, order, action)
            .await
            .unwrap()
    );
    assert!(
        !service
            .requires_material_reacquisition_for_action(FLOW_ALT_PECHAT_ID, order, action)
            .await
            .unwrap()
    );
    assert_eq!(
        store.progress_batch(&output.batch_id).await.unwrap(),
        Some(output)
    );
}

#[tokio::test]
async fn printing_transfer_stays_paused_when_target_is_busy_and_preserves_its_queue() {
    let order = "zakaz-print-transfer-busy";
    let (service, store) = fixture(order).await;
    let other = "zakaz-print-target-running";
    service
        .upsert_map(canonical_apparatus_stage_map(other, PECHAT_8_ID, "Target"))
        .await
        .unwrap();
    start_first_stage(
        &service,
        other,
        PECHAT_8_ID,
        QueueActionActor {
            ref_: "other-worker".into(),
            ..worker()
        },
    )
    .await
    .unwrap();
    pause_first_stage_batch(&service, order, PECHAT_7_ID, &worker(), 12.0)
        .await
        .unwrap();
    service
        .transfer_apparatus_order(request(order, PECHAT_7_ID, PECHAT_8_ID), worker())
        .await
        .unwrap();
    let states = store.apparatus_queue_states().await.unwrap();
    assert_eq!(states[PECHAT_8_ID][other], "in_progress");
    assert_eq!(states[PECHAT_8_ID][order], "paused");
    assert!(
        service
            .apply_apparatus_queue_action_with_progress(
                PECHAT_8_ID,
                order,
                queue_state::ApparatusQueueAction::Resume,
                &[PECHAT_8_ID.into()],
                worker(),
                Default::default()
            )
            .await
            .is_err()
    );
    assert_eq!(store.apparatus_queue_states().await.unwrap(), states);
    assert_eq!(
        store.apparatus_sequences().await.unwrap()[PECHAT_8_ID],
        vec![other.to_string(), order.to_string()]
    );
}

#[tokio::test]
async fn printing_transfer_preserves_opening_wip_intake_and_moves_its_mounted_owner() {
    let order = "zakaz-print-transfer-opening-input";
    let store = Arc::new(MemoryProductionMapStore::new());
    let service = default_service_with_store(store.clone()).await;
    let mut map = canonical_two_stage_map(
        order,
        FLOW_PECHAT_ID,
        "First print",
        PECHAT_7_ID,
        "Second print",
    );
    map.roll_count = Some(7);
    map.width_mm = Some(800.0);
    service.upsert_map(map).await.unwrap();
    let opening = service
        .create_opening_wip(
            OpeningWipCreateInput {
                idempotency_key: "print-transfer-opening".into(),
                order_id: order.into(),
                entry_apparatus: PECHAT_7_ID.into(),
                source_operation: "Print".into(),
                source_apparatus: FLOW_PECHAT_ID.into(),
                source_stage_node_id: "apparatus".into(),
                current_location: PECHAT_7_ID.into(),
                note: "Imported roll".into(),
                batches: vec![OpeningWipBatchInput {
                    quantity_basis: OpeningWipQuantityBasis::Measured,
                    finished_goods_meter: Some(100.0),
                    finished_goods_kg: Some(10.0),
                    bobina_kg: Some(1.0),
                    diameter: None,
                }],
            },
            worker(),
        )
        .await
        .unwrap();
    let input = &opening.batches[0];
    service
        .apply_apparatus_queue_action_with_progress(
            PECHAT_7_ID,
            order,
            queue_state::ApparatusQueueAction::Start,
            &[PECHAT_7_ID.into()],
            worker(),
            QueueProgressInput {
                qr_payload: input.qr_payload.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let control = service.queue_action_controls().await.unwrap()[PECHAT_7_ID][order]
        .temporary_pause
        .clone()
        .unwrap();
    service
        .temporarily_pause_work(
            PECHAT_7_ID,
            order,
            &control.session_id,
            &control.token,
            &[PECHAT_7_ID.into()],
            worker(),
        )
        .await
        .unwrap();
    let before = store
        .opening_wip_batch(&input.batch_id, "")
        .await
        .unwrap()
        .unwrap();
    service
        .transfer_apparatus_order(request(order, PECHAT_7_ID, PECHAT_8_ID), worker())
        .await
        .unwrap();
    let after = store
        .opening_wip_batch(&input.batch_id, "")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.intake, before.intake);
    assert_eq!(after.batch.qr_payload, before.batch.qr_payload);
    assert_eq!(
        after.batch.used_by_session_id,
        before.batch.used_by_session_id
    );
    assert_eq!(after.batch.used_by_apparatus, PECHAT_8_ID);
    resume(&service, order, PECHAT_8_ID).await;
}
