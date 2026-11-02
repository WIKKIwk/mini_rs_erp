use super::*;

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn two_kg_fields_commit_pause_and_complete_with_original_wip_and_stable_retry() {
    for action in [Action::Pause, Action::Complete] {
        let f = Fixture::new(LAM, false).await;
        f.execute(&f.preview().await.target.unwrap()).await.unwrap();
        f.resume().await;
        let snapshot = f
            .service
            .laminatsiya_astatka_snapshot(LAM, &f.order, &actor().ref_, "")
            .await
            .unwrap();
        assert_eq!(snapshot.sources.len(), 1);
        let source = snapshot.sources[0].clone();
        assert_eq!(source.source_kind, "progress_batch");
        let session = snapshot.session.unwrap();
        let before = f.store.progress_batches_for_order(&f.order).await.unwrap();
        let input = LaminatsiyaAstatkaShiftInput {
            action,
            full_completion_report_required: action == Action::Complete,
            apparatus: LAM.into(),
            order_id: f.order.clone(),
            session_id: session.session_id,
            request_id: format!("kg-fields-{}", f.order),
            lamination_print_leftover_rolls: 999.0,
            lamination_film_leftover_rolls: 999.0,
            total_waste: 0.0,
            finished_goods_meter: Some(25.0),
            finished_goods_kg: Some(2.5),
            bobina_kg: Some(0.5),
            description: "Existing two kg fields".into(),
            layer_remainders: vec![],
            layer_remainder_weights: Some(LaminationRemainderWeights {
                wip_net_kg: 3.25,
                film_net_kg: 0.0,
                wip_source_id: None,
                film_source_id: None,
                expected_sources: vec![LaminationRemainderSourceRevision {
                    source_kind: source.source_kind.clone(),
                    source_id: source.source_id.clone(),
                    expected_revision: source.revision,
                }],
            }),
        };
        let mut stale = input.clone();
        stale
            .layer_remainder_weights
            .as_mut()
            .unwrap()
            .expected_sources[0]
            .expected_revision += 1;
        assert_eq!(
            f.service
                .record_laminatsiya_astatka_shift(stale, actor(), &[LAM.into()])
                .await
                .unwrap_err(),
            ProductionMapError::ProgressBatchCorrectionConflict
        );
        assert_eq!(
            f.store.progress_batches_for_order(&f.order).await.unwrap(),
            before
        );
        let result = f
            .service
            .record_laminatsiya_astatka_shift(input.clone(), actor(), &[LAM.into()])
            .await
            .unwrap();
        assert_eq!(result.progress_batches.len(), 1);
        assert_eq!(result.progress_batches[0].produced_qty, 25.0);
        assert_eq!(result.remainders.len(), 1);
        let remainder = &result.remainders[0];
        assert_eq!(remainder.source_id, source.source_id);
        assert_eq!(remainder.producer_name, source.producer_name);
        assert_eq!(remainder.tare_kg, source.tare_kg);
        assert_eq!(remainder.remaining_net_kg, 3.25);
        assert_ne!(remainder.qr_payload, source.qr_payload);
        let returned = remainder.progress_batch.as_ref().unwrap();
        assert_eq!(returned.finished_goods_kg, Some(3.25 + source.tare_kg));
        if action == Action::Pause {
            assert_eq!(result.session.status, OrderRunStatus::Paused);
            assert_eq!(
                result
                    .report
                    .as_ref()
                    .unwrap()
                    .lamination_print_leftover_rolls,
                3.25
            );
            assert_eq!(
                result
                    .report
                    .as_ref()
                    .unwrap()
                    .lamination_film_leftover_rolls,
                0.0
            );
        } else {
            assert_eq!(result.session.status, OrderRunStatus::Completed);
            assert!(result.report.is_none());
        }
        let after = f.store.progress_batches_for_order(&f.order).await.unwrap();
        assert_eq!(
            f.service
                .record_laminatsiya_astatka_shift(input, actor(), &[LAM.into()])
                .await
                .unwrap(),
            result
        );
        assert_eq!(
            f.store.progress_batches_for_order(&f.order).await.unwrap(),
            after
        );
        assert_eq!(
            f.store
                .progress_batch(&f.output)
                .await
                .unwrap()
                .unwrap()
                .produced_qty,
            100.0
        );
        f.release().await;
    }
}
