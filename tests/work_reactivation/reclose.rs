use super::*;

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn reclose_keeps_unfinished_stage_work_startable() {
    for pending_work in ["existing_input", "new_input", "upstream_reopened"] {
        let apparatus = if pending_work == "existing_input" {
            CUT
        } else {
            LAM
        };
        let f = Fixture::new(apparatus, false).await;
        f.execute(&f.preview().await.target.unwrap()).await.unwrap();
        let close = f.reclose_target().await;
        if pending_work == "new_input" {
            let mut incoming = f.store.progress_batch(&f.input).await.unwrap().unwrap();
            incoming.batch_id = format!("incoming-{}", f.order);
            incoming.qr_payload = format!("qr-{}", incoming.batch_id);
            incoming.wip_status = OrderProgressBatchWipStatus::Waiting;
            incoming.used_by_session_id.clear();
            incoming.used_by_apparatus.clear();
            incoming.processed_by_session_id.clear();
            incoming.processed_by_apparatus.clear();
            f.store.put_order_progress_batch(incoming).await.unwrap();
        } else if pending_work == "upstream_reopened" {
            let target = f
                .service
                .preview_reactivation(
                    &ReactivationSelector {
                        apparatus: PRESS.into(),
                        order_id: f.order.clone(),
                        batch_id: String::new(),
                    },
                    &[PRESS.into()],
                    &actor(),
                )
                .await
                .unwrap()
                .target
                .unwrap();
            f.service
                .reactivate_completed_work(&target, &[PRESS.into()], &actor(), &[])
                .await
                .unwrap();
        }
        f.reclose(&close).await.unwrap();
        let state: String = sqlx::query_scalar(
            "SELECT state FROM mini_queue_states WHERE order_id=$1 AND canonical_apparatus_id=$2",
        )
        .bind(&f.order)
        .bind(apparatus)
        .fetch_one(&f.pool)
        .await
        .unwrap();
        assert_eq!(state, "pending", "{pending_work}");
        let session = f
            .store
            .order_run_session(&close.session_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.status, OrderRunStatus::Completed);
        let control = &f.service.queue_action_controls().await.unwrap()[apparatus][&f.order];
        if pending_work != "upstream_reopened" {
            assert!(
                control.allowed_actions.contains(&Action::Start),
                "{pending_work}: {control:?}"
            );
        }
        assert!(control.reclose.is_none());
    }
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn reclose_preserves_report_only_astatka_but_rejects_legacy_new_output() {
    for has_output in [false, true] {
        let f = Fixture::new(PRESS, false).await;
        f.execute(&f.preview().await.target.unwrap()).await.unwrap();
        let close = f.reclose_target().await;
        let report = f.service.record_bosma_astatka(serde_json::from_value(json!({
            "report_id":"", "order_id":f.order,"apparatus":PRESS,"from_at_unix":0,"to_at_unix":0,
            "total_waste":0.0,"finished_goods_meter":if has_output { Some(10.0) } else { None },"description":"Optional astatka",
            "returned_paint":{"id":"", "order_id":f.order,"order_code":"R","order_name":"Order",
                "apparatus":PRESS,"sender_role":"aparatchi","sender_ref":actor().ref_,"sender_display_name":"Worker",
                "items":[],"status":"waiting_for_boyoqchi_input","created_at_unix":0}
        })).unwrap()).await.unwrap();
        if has_output {
            assert!(f.reclose(&close).await.is_err());
        } else {
            f.reclose(&close).await.unwrap();
        }
        assert_eq!(
            f.store
                .bosma_astatka_reports_for_order(&f.order)
                .await
                .unwrap(),
            vec![report]
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

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn unstarted_reclose_preserves_output_reports_and_can_repeat() {
    for apparatus in [PRESS, LAM, CUT] {
        let f = Fixture::new(apparatus, false).await;
        let original = f
            .store
            .order_run_session(&f.source_session)
            .await
            .unwrap()
            .unwrap();
        let output = f.store.progress_batch(&f.output).await.unwrap().unwrap();
        let initial_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM mini_progress_batches WHERE order_id=$1")
                .bind(&f.order)
                .fetch_one(&f.pool)
                .await
                .unwrap();
        let mut previous_token = String::new();
        for _ in 0..2 {
            let preview = f.preview().await;
            assert!(preview.available, "{apparatus}: {preview:?}");
            let target = preview.target.unwrap();
            assert_ne!(target.token, previous_token);
            previous_token = target.token.clone();
            f.execute(&target).await.unwrap();
            let close = f.reclose_target().await;
            assert_eq!(
                f.store
                    .order_run_session(&close.session_id)
                    .await
                    .unwrap()
                    .unwrap()
                    .status,
                OrderRunStatus::Paused
            );
            let controls = f.service.queue_action_controls().await.unwrap();
            let control = &controls[apparatus][&f.order];
            assert_eq!(control.state, queue_state::ApparatusQueueOrderState::Paused);
            assert_eq!(
                control.reclose.as_ref().unwrap().session_id,
                close.session_id
            );
            assert!(control.allowed_actions.contains(&Action::Resume));
            let (one, two) = tokio::join!(f.reclose(&close), f.reclose(&close));
            one.unwrap();
            two.unwrap();
            let state: String = sqlx::query_scalar("SELECT state FROM mini_queue_states WHERE order_id=$1 AND canonical_apparatus_id=$2")
                .bind(&f.order).bind(apparatus).fetch_one(&f.pool).await.unwrap();
            assert_eq!(
                state,
                if apparatus == CUT {
                    "pending"
                } else {
                    "completed"
                }
            );
            assert_eq!(
                f.store
                    .order_run_session(&f.source_session)
                    .await
                    .unwrap()
                    .unwrap(),
                original
            );
            let saved = f.store.progress_batch(&f.output).await.unwrap().unwrap();
            assert_eq!(saved.produced_qty, output.produced_qty);
            assert_eq!(saved.qr_payload, output.qr_payload);
            assert_eq!(saved.total_waste, output.total_waste);
            assert_eq!(saved.wip_status, OrderProgressBatchWipStatus::Waiting);
            let closed = f
                .store
                .order_run_session(&close.session_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(closed.status, OrderRunStatus::Completed);
            assert!(closed.payload_json.get("stage_work_report").is_some());
            assert_eq!(
                closed.payload_json["_reactivation_reclosed_without_work"],
                true
            );
            if !f.input.is_empty() {
                let input = f.store.progress_batch(&f.input).await.unwrap().unwrap();
                assert_eq!(input.wip_status, OrderProgressBatchWipStatus::Processed);
                assert_eq!(input.processed_by_session_id, f.source_session);
            }
            let count: i64 =
                sqlx::query_scalar("SELECT count(*) FROM mini_progress_batches WHERE order_id=$1")
                    .bind(&f.order)
                    .fetch_one(&f.pool)
                    .await
                    .unwrap();
            assert_eq!(count, initial_count);
            let writes: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_order_progress_events WHERE session_id=$1 AND action='complete'")
                .bind(&close.session_id).fetch_one(&f.pool).await.unwrap();
            assert_eq!(writes, 0);
        }
    }
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn reclose_rejects_owner_stage_and_work_started_even_after_repause() {
    for apparatus in [PRESS, LAM, CUT] {
        let f = Fixture::new(apparatus, false).await;
        f.execute(&f.preview().await.target.unwrap()).await.unwrap();
        let close = f.reclose_target().await;
        let mut other = actor();
        other.ref_ = "other-worker".into();
        assert!(
            f.service
                .reclose_reactivated_work(&close, &[apparatus.into()], &other)
                .await
                .is_err()
        );
        let mut wrong_stage = close.clone();
        wrong_stage.stage_node_id = "another-occurrence".into();
        assert!(f.reclose(&wrong_stage).await.is_err());
        assert!(
            f.service
                .reclose_reactivated_work(&close, &[], &actor())
                .await
                .is_err()
        );
        f.resume().await;
        assert!(f.reclose(&close).await.is_err());
        let resume_id: String = sqlx::query_scalar("SELECT event_id FROM mini_queue_action_events WHERE order_id=$1 AND canonical_apparatus_id=$2 ORDER BY id DESC LIMIT 1")
            .bind(&f.order).bind(apparatus).fetch_one(&f.pool).await.unwrap();
        f.service
            .re_pause_resumed_work(
                apparatus,
                &f.order,
                &resume_id,
                &[apparatus.into()],
                actor(),
            )
            .await
            .unwrap();
        let paused = f
            .store
            .order_run_session(&close.session_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(paused.status, OrderRunStatus::Paused);
        assert_eq!(paused.payload_json["_reactivation_started"], true);
        assert!(paused.payload_json.get("_reactivation_reclose").is_none());
        assert!(
            f.service.queue_action_controls().await.unwrap()[apparatus][&f.order]
                .reclose
                .is_none()
        );
        assert!(f.reclose(&close).await.is_err());
        f.release().await;
    }
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn reclose_preserves_a_physical_wip_remainder() {
    let f = Fixture::new(LAM, false).await;
    let raw_id = format!("reclose-raw-{}", f.order);
    let barcode = format!("RECLOSE-RAW-{}", f.order.to_uppercase());
    sqlx::raw_sql("INSERT INTO mini_warehouses(id,name) VALUES('reclose-raw-warehouse','Reclose Raw');
        INSERT INTO mini_items(code,name,uom,item_group) VALUES('RECLOSE_FILM','Film','kg','All Item Groups');
        INSERT INTO mini_factory_locations(id,name) VALUES('reclose-lam-state','Reclose Lamination State');")
        .execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO mini_factory_location_apparatus_links(location_id,apparatus_id) VALUES('reclose-lam-state',$1)")
        .bind(LAM).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty,length_m,uom,status,reserved_order_id,source_receipt_id,payload_json) VALUES($1,'Reclose Raw','RECLOSE_FILM','Film',$2,11,110,'kg','in_use',$3,$1,'{}')")
        .bind(&raw_id).bind(&barcode).bind(&f.order).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO mini_inventory_placements(asset_kind,asset_ref,physical_location_id) VALUES('raw_material',$1,'inventory_location:state:reclose-lam-state')")
        .bind(&raw_id).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_assignments(barcode,order_id,apparatus,canonical_apparatus_id,item_code,item_group,payload_json) VALUES($1,$2,$3,$3,'RECLOSE_FILM','All Item Groups',$4)")
        .bind(&barcode).bind(&f.order).bind(LAM)
        .bind(json!({"barcode":barcode,"order_id":f.order,"apparatus_id":LAM,"apparatus":LAM,"item_code":"RECLOSE_FILM","item_name":"Film","item_group":"All Item Groups","assigned_by_role":"aparatchi","assigned_by_ref":actor().ref_,"assigned_by_display_name":"Worker","assigned_at":"2026-10-18"}))
        .execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE mini_order_run_sessions SET payload_json=jsonb_set(payload_json,'{start_resources}',$2) WHERE session_id=$1")
        .bind(&f.source_session)
        .bind(json!({"session_id":f.source_session,"raw_materials":[{"barcode":barcode,"stock_id":raw_id,"source_qty":11.0,"uom":"kg","item_code":"RECLOSE_FILM","item_name":"Film"}],"raw_materials_available":true,"qolip_codes":[],"qolip_available":false}))
        .execute(&f.pool).await.unwrap();
    f.execute(&f.preview().await.target.unwrap()).await.unwrap();
    let close = f.reclose_target().await;
    let sources = f
        .service
        .layer_remainder_sources(LAM, &f.order)
        .await
        .unwrap();
    assert_eq!(sources.len(), 2);
    let result = f
        .service
        .record_laminatsiya_astatka_shift(
            LaminatsiyaAstatkaShiftInput {
                action: Action::Pause,
                full_completion_report_required: false,
                apparatus: LAM.into(),
                order_id: f.order.clone(),
                session_id: close.session_id.clone(),
                request_id: "reclose-remainder".into(),
                lamination_print_leftover_rolls: 1.0,
                lamination_film_leftover_rolls: 1.0,
                total_waste: 0.0,
                finished_goods_meter: None,
                finished_goods_kg: None,
                bobina_kg: None,
                description: "Optional return before reclose".into(),
                layer_remainders: sources
                    .iter()
                    .map(|source| LaminatsiyaAstatkaLayerInput {
                        source_kind: source.source_kind.clone(),
                        source_id: source.source_id.clone(),
                        expected_revision: source.revision,
                        remaining_net_kg: 3.0,
                    })
                    .collect(),
            },
            actor(),
            &[LAM.into()],
        )
        .await
        .unwrap();
    assert!(result.progress_batches.is_empty());
    assert!(
        f.service.queue_action_controls().await.unwrap()[LAM][&f.order]
            .reclose
            .is_some()
    );
    let returned = result.remainders.first().unwrap();
    let remainder = returned.progress_batch.clone().unwrap();
    let raw_remainder = result
        .remainders
        .iter()
        .find_map(|r| r.raw_material_stock.as_ref())
        .unwrap();
    let raw_before: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM mini_raw_material_stock s WHERE id=$1")
            .bind(&raw_remainder.id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let ledger_before: (i64, f64) = sqlx::query_as(
        "SELECT count(*),sum(qty_delta)::float8 FROM mini_raw_material_events WHERE order_id=$1",
    )
    .bind(&f.order)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    f.reclose(&close).await.unwrap();
    f.reclose(&close).await.unwrap();
    let raw_after: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM mini_raw_material_stock s WHERE id=$1")
            .bind(&raw_remainder.id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let ledger_after: (i64, f64) = sqlx::query_as(
        "SELECT count(*),sum(qty_delta)::float8 FROM mini_raw_material_events WHERE order_id=$1",
    )
    .bind(&f.order)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(raw_after, raw_before);
    assert_eq!(ledger_after, ledger_before);
    assert_eq!(raw_remainder.qty, 3.0);
    assert_eq!(ledger_after, (2, -8.0));
    assert_eq!(
        f.store
            .laminatsiya_astatka_reports_for_order(&f.order)
            .await
            .unwrap(),
        vec![result.report.unwrap()]
    );
    assert_eq!(
        f.store
            .progress_batch(&remainder.batch_id)
            .await
            .unwrap()
            .unwrap(),
        remainder
    );
    let source = f.store.progress_batch(&f.input).await.unwrap().unwrap();
    assert_eq!(source.processed_by_session_id, close.session_id);
    assert_eq!(
        source.payload_json["layer_remainder_operation_id"],
        returned.operation_id
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
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn resume_and_reclose_from_independent_services_cannot_both_commit() {
    for apparatus in [PRESS, LAM, CUT] {
        let f = Fixture::new(apparatus, false).await;
        f.execute(&f.preview().await.target.unwrap()).await.unwrap();
        let close = f.reclose_target().await;
        let assigned = [apparatus.into()];
        let (closing, starting) = tokio::join!(
            f.reclose(&close),
            f.service.apply_apparatus_queue_action_with_progress(
                apparatus,
                &f.order,
                Action::Resume,
                &assigned,
                actor(),
                QueueProgressInput::default()
            )
        );
        assert_ne!(
            closing.is_ok(),
            starting.is_ok(),
            "close={closing:?}, resume={starting:?}"
        );
        let session = f
            .store
            .order_run_session(&close.session_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            session.status,
            if closing.is_ok() {
                OrderRunStatus::Completed
            } else {
                OrderRunStatus::Active
            }
        );
        f.release().await;
    }
}
