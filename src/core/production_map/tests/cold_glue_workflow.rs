use super::*;
use queue_state::ApparatusQueueAction as A;

#[tokio::test]
async fn cold_glue_accepts_print_or_lamination_qr_and_keeps_its_own_output_identity() {
    for after_lamination in [false, true] {
        let store = Arc::new(MemoryProductionMapStore::new());
        let (service, apparatus_service) =
            service_with_apparatus_store(store.clone(), &[(FLOW_PECHAT_ID, "Bosma")]).await;
        let actor = QueueActionActor {
            role: "aparatchi".into(),
            ref_: "cold-glue-worker".into(),
            display_name: "Holodniy kley operatori".into(),
        };
        let order = "zakaz-cold-glue-qr-flow";
        let mut map = if after_lamination {
            three_stage_map(order, FLOW_PECHAT_ID, LAMINATION_1_ID, COLD_GLUE_ID, 1)
        } else {
            two_stage_map(order, FLOW_PECHAT_ID, COLD_GLUE_ID)
        };
        for node in &mut map.nodes {
            node.rezka_kadr_count = None;
        }
        service.upsert_map(map).await.unwrap();
        // A scanned upstream WIP reuses material exactly as on lamination.
        set_test_material_rule(
            &apparatus_service,
            ApparatusMaterialRuleUpsert {
                apparatus: COLD_GLUE_ID.into(),
                requires_material: true,
                start_policy: RawMaterialStartPolicy::StateAll,
                item_groups: vec!["Kley".into()],
                requirement_groups: Vec::new(),
            },
        )
        .await;
        let print_batch = pause_first_stage_batch(&service, order, FLOW_PECHAT_ID, &actor, 20.0)
            .await
            .unwrap();
        let input = if after_lamination {
            service
                .apply_apparatus_queue_action_with_progress(
                    LAMINATION_1_ID,
                    order,
                    A::Start,
                    &[LAMINATION_1_ID.into()],
                    actor.clone(),
                    QueueProgressInput {
                        qr_payload: print_batch.qr_payload.clone(),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            service
                .apply_apparatus_queue_action_with_progress(
                    LAMINATION_1_ID,
                    order,
                    A::Pause,
                    &[LAMINATION_1_ID.into()],
                    actor.clone(),
                    QueueProgressInput {
                        produced_qty: Some(20.0),
                        uom: "kg".into(),
                        ..Default::default()
                    },
                )
                .await
                .unwrap()
                .progress_batch
                .unwrap()
        } else {
            print_batch.clone()
        };
        let controls = service.queue_action_controls().await.unwrap();
        let control = &controls[COLD_GLUE_ID][order];
        assert_eq!(
            control.interaction.previous_wip_mode,
            ApparatusQueuePreviousWipMode::ScanRequired
        );
        assert_eq!(
            control.interaction.start_materials_mode,
            ApparatusQueueStartMaterialsMode::Hidden
        );
        assert_eq!(
            service
                .apply_apparatus_queue_action_with_progress(
                    COLD_GLUE_ID,
                    order,
                    A::Start,
                    &[COLD_GLUE_ID.into()],
                    actor.clone(),
                    QueueProgressInput::default(),
                )
                .await,
            Err(ProductionMapError::ProgressQrRequired)
        );
        if after_lamination {
            assert!(
                service
                    .apply_apparatus_queue_action_with_progress(
                        COLD_GLUE_ID,
                        order,
                        A::Start,
                        &[COLD_GLUE_ID.into()],
                        actor.clone(),
                        QueueProgressInput {
                            qr_payload: print_batch.qr_payload.clone(),
                            ..Default::default()
                        },
                    )
                    .await
                    .is_err(),
                "QR must match the immediately preceding stage"
            );
        }
        let started = service
            .apply_apparatus_queue_action_with_material_scan_and_progress(
                MaterialScanProgressAction {
                    apparatus: COLD_GLUE_ID,
                    order_id: order,
                    action: A::Start,
                    assigned_apparatus: &[COLD_GLUE_ID.into()],
                    actor: actor.clone(),
                    material_barcodes: &[],
                    state_material_barcodes: &[],
                    progress: QueueProgressInput {
                        qr_payload: input.qr_payload.clone(),
                        ..Default::default()
                    },
                    qolip_validation: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(started.states[order], "in_progress");
        assert_eq!(
            started.session.unwrap().payload_json["input_progress_batch_id"],
            input.batch_id
        );
        assert_eq!(
            store
                .progress_batch(&input.batch_id)
                .await
                .unwrap()
                .unwrap()
                .used_by_apparatus,
            COLD_GLUE_ID
        );
        assert_eq!(
            service
                .apply_apparatus_queue_action_with_progress(
                    COLD_GLUE_ID,
                    order,
                    A::Complete,
                    &[COLD_GLUE_ID.into()],
                    actor.clone(),
                    QueueProgressInput {
                        produced_qty: Some(20.0),
                        uom: "kg".into(),
                        force_full_completion_metrics: true,
                        ..Default::default()
                    },
                )
                .await,
            Err(ProductionMapError::LaminatsiyaCompletionMetricsRequired)
        );
        let completed = service
            .apply_apparatus_queue_action_with_progress(
                COLD_GLUE_ID,
                order,
                A::Complete,
                &[COLD_GLUE_ID.into()],
                actor,
                QueueProgressInput {
                    produced_qty: Some(20.0),
                    uom: "kg".into(),
                    finished_goods_kg: Some(20.0),
                    finished_goods_meter: Some(100.0),
                    lamination_print_leftover_rolls: Some(1.0),
                    lamination_film_leftover_rolls: Some(1.0),
                    total_waste: Some(1.0),
                    force_full_completion_metrics: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let output = completed.progress_batch.unwrap();
        assert_eq!(output.apparatus, COLD_GLUE_ID);
        assert_eq!(output.parent_batch_id, input.batch_id);
        assert_eq!(
            store
                .progress_batch(&input.batch_id)
                .await
                .unwrap()
                .unwrap()
                .wip_status,
            OrderProgressBatchWipStatus::Processed
        );
    }
}
