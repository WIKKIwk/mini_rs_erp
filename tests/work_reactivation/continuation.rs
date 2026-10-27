use super::*;

async fn assert_batch_unchanged(f: &Fixture, id: &str, expected: &OrderProgressBatch) {
    let current = f.store.progress_batch(id).await.unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(current).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

async fn assert_session_unchanged(f: &Fixture, id: &str, expected: &OrderRunSession) {
    let current = f.store.order_run_session(id).await.unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(current).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

async fn exhaust_lamination_inputs(f: &Fixture) -> LaminatsiyaAstatkaCommitResult {
    let session = f
        .store
        .active_order_run_session(LAM, &f.order)
        .await
        .unwrap()
        .unwrap();
    let sources = f
        .service
        .layer_remainder_sources(LAM, &f.order)
        .await
        .unwrap();
    f.service
        .record_laminatsiya_astatka_shift(
            LaminatsiyaAstatkaShiftInput {
                action: Action::Complete,
                full_completion_report_required: true,
                apparatus: LAM.into(),
                order_id: f.order.clone(),
                session_id: session.session_id,
                request_id: format!("exhaust-{}", f.order),
                lamination_print_leftover_rolls: 0.0,
                lamination_film_leftover_rolls: 0.0,
                total_waste: 1.0,
                finished_goods_meter: Some(25.0),
                finished_goods_kg: Some(2.5),
                bobina_kg: Some(0.5),
                description: "Inputs physically exhausted".into(),
                layer_remainders: sources
                    .into_iter()
                    .map(|source| LaminatsiyaAstatkaLayerInput {
                        source_kind: source.source_kind,
                        source_id: source.source_id,
                        expected_revision: source.revision,
                        remaining_net_kg: 0.0,
                    })
                    .collect(),
            },
            actor(),
            &[LAM.into()],
        )
        .await
        .unwrap()
}

async fn mount_film(f: &Fixture) -> (String, String) {
    let mut active = f
        .store
        .active_order_run_session(LAM, &f.order)
        .await
        .unwrap()
        .unwrap();
    let stock_id = format!("zero-film-{}", f.order);
    let barcode = format!("ZERO-FILM-{}", f.order.to_uppercase());
    sqlx::query("INSERT INTO mini_items(code,name,uom,item_group) VALUES('ZERO_FILM','Zero film','kg','All Item Groups') ON CONFLICT(code) DO NOTHING")
        .execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty,length_m,uom,status,reserved_order_id,source_receipt_id,payload_json) VALUES($1,'Zero film warehouse','ZERO_FILM','Zero film',$2,11,110,'kg','in_use',$3,$1,'{}')")
        .bind(&stock_id).bind(&barcode).bind(&f.order).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_assignments(barcode,order_id,apparatus,canonical_apparatus_id,item_code,item_group,payload_json) VALUES($1,$2,$3,$3,'ZERO_FILM','All Item Groups',$4)")
        .bind(&barcode).bind(&f.order).bind(LAM)
        .bind(json!({"barcode":barcode,"order_id":f.order,"apparatus_id":LAM,"apparatus":LAM,
            "item_code":"ZERO_FILM","item_name":"Zero film","item_group":"All Item Groups",
            "assigned_by_role":"aparatchi","assigned_by_ref":actor().ref_,
            "assigned_by_display_name":"Worker","assigned_at":"2026-10-18"}))
        .execute(&f.pool).await.unwrap();
    let location = format!("zero-film-state-{}", f.order);
    sqlx::query("INSERT INTO mini_factory_locations(id,name) VALUES($1,$1)")
        .bind(&location)
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO mini_factory_location_apparatus_links(location_id,apparatus_id) VALUES($1,$2)",
    )
    .bind(&location)
    .bind(LAM)
    .execute(&f.pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO mini_inventory_placements(asset_kind,asset_ref,physical_location_id) VALUES('raw_material',$1,$2)")
        .bind(&stock_id).bind(format!("inventory_location:state:{location}")).execute(&f.pool).await.unwrap();
    active.payload_json["start_resources"] = json!({"session_id":active.session_id,
        "raw_materials":[{"barcode":barcode,"stock_id":stock_id,"source_qty":11.0,
            "uom":"kg","item_code":"ZERO_FILM","item_name":"Zero film"}],
        "raw_materials_available":true,"qolip_codes":[],"qolip_available":false});
    f.store.put_order_run_session(active).await.unwrap();
    (stock_id, barcode)
}

async fn runtime_service(f: &Fixture) -> ProductionMapService {
    let pool = sqlx::PgPool::connect_with(
        f.pool
            .connect_options()
            .as_ref()
            .clone()
            .username("mini_rs_erp"),
    )
    .await
    .unwrap();
    ProductionMapService::new(
        Arc::new(PostgresProductionMapStore::new(pool.clone())),
        Arc::new(CanonicalServiceApparatusResolver::new(
            postgres::canonical_apparatus_service(pool),
        )),
    )
}

async fn claim_output_downstream(f: &Fixture, status: &str) -> String {
    let id = format!("downstream-{}", f.order);
    let mut output = f.store.progress_batch(&f.output).await.unwrap().unwrap();
    output.wip_status = match status {
        "waiting" => OrderProgressBatchWipStatus::Waiting,
        "in_use" => OrderProgressBatchWipStatus::InUse,
        "processed" => OrderProgressBatchWipStatus::Processed,
        _ => panic!("unexpected downstream state"),
    };
    output.current_apparatus = CUT.into();
    output.current_location = CUT.into();
    output.used_by_session_id = if status == "in_use" {
        id.clone()
    } else {
        String::new()
    };
    output.used_by_apparatus = if status == "in_use" {
        CUT.into()
    } else {
        String::new()
    };
    output.processed_by_session_id = if status == "processed" {
        id.clone()
    } else {
        String::new()
    };
    output.processed_by_apparatus = if status == "processed" {
        CUT.into()
    } else {
        String::new()
    };
    f.store.put_order_progress_batch(output).await.unwrap();
    f.store
        .put_order_run_session(OrderRunSession {
            session_id: id.clone(),
            apparatus: CUT.into(),
            order_id: f.order.clone(),
            stage_node_id: "cut".into(),
            status: if status == "waiting" {
                OrderRunStatus::RollDetached
            } else {
                OrderRunStatus::Active
            },
            worker_role: "aparatchi".into(),
            worker_ref: "downstream-worker".into(),
            worker_display_name: "Downstream worker".into(),
            started_at_unix: 100,
            updated_at_unix: 100,
            payload_json: json!({
                "stage_work_protocol": 1,
                "input_progress_batch_id": f.output,
                "input_progress_qr_payload": format!("qr-{}", f.output),
                "input_progress_apparatus": LAM,
                "input_wip_source_kind": "progress_batch",
                "roll_removed_from_apparatus": status == "waiting"
            }),
        })
        .await
        .unwrap();
    f.store
        .put_apparatus_queue_states(
            CUT,
            [(
                f.order.clone(),
                if status == "waiting" {
                    "pending"
                } else {
                    "in_progress"
                }
                .into(),
            )]
            .into(),
        )
        .await
        .unwrap();
    id
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn lamination_reactivation_preserves_downstream_work_and_reopens_only_input_paused() {
    for status in ["waiting", "in_use", "processed"] {
        let f = Fixture::new(LAM, false).await;
        let downstream_id = claim_output_downstream(&f, status).await;
        let output = f.store.progress_batch(&f.output).await.unwrap().unwrap();
        let downstream = f
            .store
            .order_run_session(&downstream_id)
            .await
            .unwrap()
            .unwrap();
        let source = f
            .store
            .order_run_session(&f.source_session)
            .await
            .unwrap()
            .unwrap();
        let preview = f.preview().await;
        assert!(preview.available, "{status}: {preview:?}");
        let target = preview.target.unwrap();
        f.execute(&target).await.unwrap();
        f.execute(&target).await.unwrap();
        let continuation = f
            .store
            .active_order_run_session(LAM, &f.order)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(continuation.status, OrderRunStatus::Paused);
        assert_eq!(
            continuation.payload_json["input_progress_batch_id"],
            f.input
        );
        assert_eq!(
            continuation.payload_json["_reactivation_of_session_id"],
            f.source_session
        );
        let input = f.store.progress_batch(&f.input).await.unwrap().unwrap();
        assert_eq!(input.wip_status, OrderProgressBatchWipStatus::InUse);
        assert_eq!(input.used_by_session_id, continuation.session_id);
        assert_eq!(input.produced_qty, 100.0);
        assert!(input.processed_by_session_id.is_empty());
        assert_batch_unchanged(&f, &f.output, &output).await;
        assert_session_unchanged(&f, &downstream_id, &downstream).await;
        assert_session_unchanged(&f, &f.source_session, &source).await;
        let produced: (i64, f64) = sqlx::query_as("SELECT count(*)::bigint,COALESCE(sum(produced_qty),0)::float8 FROM mini_order_progress_events WHERE session_id=$1")
            .bind(&continuation.session_id).fetch_one(&f.pool).await.unwrap();
        assert_eq!(produced, (1, 0.0));

        // Closing an unstarted correction releases only its input. In particular,
        // it cannot pull a roll back from the downstream worker.
        f.reclose(&f.reclose_target().await).await.unwrap();
        let restored_input = f.store.progress_batch(&f.input).await.unwrap().unwrap();
        assert_eq!(
            restored_input.wip_status,
            OrderProgressBatchWipStatus::Processed
        );
        assert_eq!(restored_input.processed_by_session_id, f.source_session);
        assert_batch_unchanged(&f, &f.output, &output).await;
        assert_session_unchanged(&f, &downstream_id, &downstream).await;
        f.release().await;
    }
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn lamination_downstream_claim_after_preview_does_not_invalidate_input_continuation() {
    let f = Fixture::new(LAM, false).await;
    let target = f.preview().await.target.unwrap();
    let downstream_id = claim_output_downstream(&f, "in_use").await;
    let output = f.store.progress_batch(&f.output).await.unwrap().unwrap();
    let downstream = f
        .store
        .order_run_session(&downstream_id)
        .await
        .unwrap()
        .unwrap();
    f.execute(&target).await.unwrap();
    f.resume().await;
    let completed = f
        .service
        .apply_apparatus_queue_action_with_progress(
            LAM,
            &f.order,
            Action::Complete,
            &[LAM.into()],
            actor(),
            QueueProgressInput {
                produced_qty: Some(25.0),
                uom: "m".into(),
                finished_goods_meter: Some(25.0),
                finished_goods_kg: Some(2.5),
                bobina_kg: Some(0.5),
                lamination_print_leftover_rolls: Some(1.0),
                lamination_film_leftover_rolls: Some(1.0),
                total_waste: Some(1.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let new_output = completed.progress_batch.unwrap();
    assert_ne!(new_output.batch_id, output.batch_id);
    assert_eq!(new_output.produced_qty, 25.0);
    assert_batch_unchanged(&f, &f.output, &output).await;
    assert_session_unchanged(&f, &downstream_id, &downstream).await;
    let total: f64 = sqlx::query_scalar("SELECT sum(produced_qty)::float8 FROM mini_progress_batches WHERE order_id=$1 AND canonical_apparatus_id=$2")
        .bind(&f.order).bind(LAM).fetch_one(&f.pool).await.unwrap();
    assert_eq!(total, 125.0);
    f.release().await;
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn assigned_worker_can_reactivate_another_workers_completion_with_requester_bound_token() {
    for apparatus in [PRESS, LAM, CUT] {
        let f = Fixture::new(apparatus, false).await;
        let runtime = runtime_service(&f).await;
        let worker = QueueActionActor {
            ref_: "next-shift-worker".into(),
            display_name: "Next shift".into(),
            ..actor()
        };
        let history = runtime
            .completed_queue_orders_for_apparatuses(&[apparatus.into()], 200)
            .await
            .unwrap();
        assert!(history.iter().any(|row| row.order_id == f.order));
        assert!(history.iter().all(|row| row.apparatus == apparatus));
        assert!(
            runtime
                .completed_queue_orders_for_apparatuses(&[], 200)
                .await
                .unwrap()
                .is_empty()
        );
        let source = f
            .store
            .order_run_session(&f.source_session)
            .await
            .unwrap()
            .unwrap();
        let output = f.store.progress_batch(&f.output).await.unwrap().unwrap();
        let target = runtime
            .preview_reactivation(&f.selector(), &[apparatus.into()], &worker)
            .await
            .unwrap()
            .target
            .unwrap();
        assert_ne!(target.token, f.preview().await.target.unwrap().token);
        assert!(
            runtime
                .preview_reactivation(&f.selector(), &[], &worker)
                .await
                .is_err()
        );
        assert!(
            runtime
                .reactivate_completed_work(&target, &[], &worker, &[])
                .await
                .is_err()
        );
        assert!(
            runtime
                .reactivate_completed_work(&target, &[apparatus.into()], &actor(), &[])
                .await
                .is_err()
        );
        runtime
            .reactivate_completed_work(&target, &[apparatus.into()], &worker, &[])
            .await
            .unwrap();
        runtime
            .reactivate_completed_work(&target, &[apparatus.into()], &worker, &[])
            .await
            .unwrap();
        assert!(
            runtime
                .reactivate_completed_work(&target, &[apparatus.into()], &actor(), &[])
                .await
                .is_err()
        );
        let continuation = f
            .store
            .active_order_run_session(apparatus, &f.order)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(continuation.status, OrderRunStatus::Paused);
        assert_eq!(continuation.worker_ref, worker.ref_);
        assert_eq!(continuation.worker_display_name, worker.display_name);
        assert_session_unchanged(&f, &f.source_session, &source).await;
        let target = f.reclose_target().await;
        assert!(
            runtime
                .reclose_reactivated_work(&target, &[apparatus.into()], &actor())
                .await
                .is_err()
        );
        assert!(
            runtime
                .reclose_reactivated_work(&target, &[], &worker)
                .await
                .is_err()
        );
        runtime
            .reclose_reactivated_work(&target, &[apparatus.into()], &worker)
            .await
            .unwrap();
        assert_batch_unchanged(&f, &f.output, &output).await;
        f.release().await;
    }
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn lamination_continuation_rejects_substituted_output_and_input_taken_by_another_session() {
    let f = Fixture::new(LAM, false).await;
    let mut selector = f.selector();
    selector.batch_id = f.input.clone();
    let preview = f
        .service
        .preview_reactivation(&selector, &[LAM.into()], &actor())
        .await
        .unwrap();
    assert!(!preview.available);
    let target = f.preview().await.target.unwrap();
    sqlx::query("UPDATE mini_progress_batches SET processed_by_session_id='different-consumer' WHERE batch_id=$1")
        .bind(&f.input).execute(&f.pool).await.unwrap();
    let preview = f.preview().await;
    assert_eq!(preview.denial_reason.as_deref(), Some("source_not_free"));
    assert!(f.execute(&target).await.is_err());
    assert!(
        f.store
            .active_order_run_session(LAM, &f.order)
            .await
            .unwrap()
            .is_none()
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

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn lamination_returned_remainder_requires_qr_and_never_resurrects_its_parent() {
    let f = Fixture::new(LAM, false).await;
    let mut source = f
        .store
        .order_run_session(&f.source_session)
        .await
        .unwrap()
        .unwrap();
    source.payload_json["_completion_reactivation_payload"] = source.payload_json.clone();
    source.payload_json["_completion_queue_event_id"] =
        json!(format!("complete-{}", f.source_session));
    f.store.put_order_run_session(source).await.unwrap();
    let source = f
        .service
        .layer_remainder_sources(LAM, &f.order)
        .await
        .unwrap()
        .into_iter()
        .find(|source| source.source_id == f.input)
        .unwrap();
    let returned = f
        .service
        .record_layer_remainder(
            LayerRemainderInput {
                apparatus: LAM.into(),
                order_id: f.order.clone(),
                source_kind: "progress_batch".into(),
                source_id: f.input.clone(),
                session_id: f.source_session.clone(),
                expected_revision: source.revision,
                remaining_net_kg: 3.0,
                request_id: format!("returned-{}", f.order),
            },
            actor(),
        )
        .await
        .unwrap();
    let child = returned.progress_batch.unwrap();
    let parent = f.store.progress_batch(&f.input).await.unwrap().unwrap();
    let output = f.store.progress_batch(&f.output).await.unwrap().unwrap();
    let target = f.preview().await.target.unwrap();
    f.execute(&target).await.unwrap();
    let paused = f
        .store
        .active_order_run_session(LAM, &f.order)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(paused.status, OrderRunStatus::Paused);
    assert!(
        paused
            .payload_json
            .get("input_progress_batch_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .is_empty()
    );
    assert_batch_unchanged(&f, &f.input, &parent).await;
    assert_batch_unchanged(&f, &child.batch_id, &child).await;
    assert!(
        f.service
            .apply_apparatus_queue_action_with_progress(
                LAM,
                &f.order,
                Action::Resume,
                &[LAM.into()],
                actor(),
                QueueProgressInput::default()
            )
            .await
            .is_err()
    );
    let resumed = f
        .service
        .apply_apparatus_queue_action_with_progress(
            LAM,
            &f.order,
            Action::Resume,
            &[LAM.into()],
            actor(),
            QueueProgressInput {
                qr_payload: child.qr_payload.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let session = resumed.session.unwrap();
    assert_eq!(session.status, OrderRunStatus::Active);
    assert_eq!(
        session.payload_json["input_progress_batch_id"],
        child.batch_id
    );
    let claimed = f
        .store
        .progress_batch(&child.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.wip_status, OrderProgressBatchWipStatus::InUse);
    assert_eq!(claimed.used_by_session_id, session.session_id);
    assert_eq!(claimed.produced_qty, child.produced_qty);
    assert_batch_unchanged(&f, &f.input, &parent).await;
    assert_batch_unchanged(&f, &f.output, &output).await;
    f.release().await;
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn lamination_continuation_retains_machine_and_order_closure_guards() {
    let f = Fixture::new(LAM, false).await;
    let busy = Fixture::new(LAM, false).await;
    sqlx::query("UPDATE mini_order_run_sessions SET status='active' WHERE session_id=$1")
        .bind(&busy.source_session)
        .execute(&busy.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE mini_queue_states SET state='in_progress' WHERE order_id=$1 AND canonical_apparatus_id=$2")
        .bind(&busy.order).bind(LAM).execute(&busy.pool).await.unwrap();
    assert_eq!(
        f.preview().await.denial_reason.as_deref(),
        Some("apparatus_busy")
    );
    busy.release().await;
    assert!(f.preview().await.available);
    sqlx::query(
        "INSERT INTO mini_order_control_states(order_id,state) VALUES($1,'freeze_requested')",
    )
    .bind(&f.order)
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        f.preview().await.denial_reason.as_deref(),
        Some("order_closed")
    );
    sqlx::query("UPDATE mini_order_control_states SET state='active' WHERE order_id=$1")
        .bind(&f.order)
        .execute(&f.pool)
        .await
        .unwrap();
    for state in ["cancelled", "closed"] {
        sqlx::query("UPDATE mini_production_maps SET lifecycle_status=$2,production_completed_at=now(),closed_at=now() WHERE id=$1")
            .bind(&f.order).bind(state).execute(&f.pool).await.unwrap();
        assert_eq!(
            f.preview().await.denial_reason.as_deref(),
            Some("order_closed")
        );
    }
    assert!(
        f.store
            .active_order_run_session(LAM, &f.order)
            .await
            .unwrap()
            .is_none()
    );
    f.release().await;
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn lamination_zero_leftover_completion_requires_a_new_input_without_restoring_consumed_roll()
{
    let f = Fixture::new(LAM, false).await;
    f.execute(&f.preview().await.target.unwrap()).await.unwrap();
    f.resume().await;
    let completed = exhaust_lamination_inputs(&f).await;
    let output = completed.progress_batches.into_iter().next().unwrap();
    let source = completed.session;
    let parent = f.store.progress_batch(&f.input).await.unwrap().unwrap();
    assert_eq!(parent.wip_status, OrderProgressBatchWipStatus::Processed);
    assert_eq!(parent.processed_by_session_id, source.session_id);
    let selector = ReactivationSelector {
        batch_id: output.batch_id.clone(),
        ..f.selector()
    };
    let target = f
        .service
        .preview_reactivation(&selector, &[LAM.into()], &actor())
        .await
        .unwrap()
        .target
        .unwrap();
    f.execute(&target).await.unwrap();
    let paused = f
        .store
        .active_order_run_session(LAM, &f.order)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(paused.status, OrderRunStatus::Paused);
    assert!(
        paused
            .payload_json
            .get("input_progress_batch_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .is_empty()
    );
    assert_batch_unchanged(&f, &f.input, &parent).await;
    let runtime = runtime_service(&f).await;
    assert!(
        runtime
            .apply_apparatus_queue_action_with_progress(
                LAM,
                &f.order,
                Action::Resume,
                &[LAM.into()],
                actor(),
                QueueProgressInput::default()
            )
            .await
            .is_err()
    );
    assert!(
        runtime
            .apply_apparatus_queue_action_with_progress(
                LAM,
                &f.order,
                Action::Resume,
                &[LAM.into()],
                actor(),
                QueueProgressInput {
                    qr_payload: output.qr_payload.clone(),
                    ..Default::default()
                }
            )
            .await
            .is_err()
    );
    let mut next_input = parent.clone();
    next_input.batch_id = format!("new-input-{}", f.order);
    next_input.qr_payload = format!("qr-{}", next_input.batch_id);
    next_input.wip_status = OrderProgressBatchWipStatus::Waiting;
    next_input.used_by_session_id.clear();
    next_input.used_by_apparatus.clear();
    next_input.processed_by_session_id.clear();
    next_input.processed_by_apparatus.clear();
    next_input.current_apparatus = PRESS.into();
    next_input.current_location = PRESS.into();
    f.store
        .put_order_progress_batch(next_input.clone())
        .await
        .unwrap();
    let resumed = runtime
        .apply_apparatus_queue_action_with_progress(
            LAM,
            &f.order,
            Action::Resume,
            &[LAM.into()],
            actor(),
            QueueProgressInput {
                qr_payload: next_input.qr_payload.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let resumed = resumed.session.unwrap();
    assert_eq!(resumed.status, OrderRunStatus::Active);
    assert_eq!(
        resumed.payload_json["input_progress_batch_id"],
        next_input.batch_id
    );
    assert_batch_unchanged(&f, &f.input, &parent).await;
    assert_eq!(
        f.store
            .progress_batch(&next_input.batch_id)
            .await
            .unwrap()
            .unwrap()
            .used_by_session_id,
        resumed.session_id
    );
    f.release().await;
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn lamination_zero_film_leftover_does_not_reverse_consumed_stock_on_reopen_or_reclose() {
    let f = Fixture::new(LAM, false).await;
    f.execute(&f.preview().await.target.unwrap()).await.unwrap();
    f.resume().await;
    let (stock_id, barcode) = mount_film(&f).await;
    let completed = exhaust_lamination_inputs(&f).await;
    let output = completed.progress_batches.into_iter().next().unwrap();
    let stock: (String, f64) =
        sqlx::query_as("SELECT status,qty::float8 FROM mini_raw_material_stock WHERE id=$1")
            .bind(&stock_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(stock, ("consumed".into(), 11.0));
    let ledger: (i64, f64) = sqlx::query_as("SELECT count(*)::bigint,COALESCE(sum(qty_delta),0)::float8 FROM mini_raw_material_events WHERE barcode=$1 AND event_type IN ('consumption_posted','consumption_reversed')")
        .bind(&barcode).fetch_one(&f.pool).await.unwrap();
    assert_eq!(ledger.1, -11.0);
    let selector = ReactivationSelector {
        batch_id: output.batch_id.clone(),
        ..f.selector()
    };
    let target = f
        .service
        .preview_reactivation(&selector, &[LAM.into()], &actor())
        .await
        .unwrap()
        .target
        .unwrap();
    f.execute(&target).await.unwrap();
    let paused = f
        .store
        .active_order_run_session(LAM, &f.order)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(paused.status, OrderRunStatus::Paused);
    assert_eq!(
        paused.payload_json["start_resources"]["raw_materials_available"],
        false
    );
    for reclose in [false, true] {
        if reclose {
            f.reclose(&f.reclose_target().await).await.unwrap();
        }
        let current: (String, f64) =
            sqlx::query_as("SELECT status,qty::float8 FROM mini_raw_material_stock WHERE id=$1")
                .bind(&stock_id)
                .fetch_one(&f.pool)
                .await
                .unwrap();
        assert_eq!(current, stock);
        let current: (i64, f64) = sqlx::query_as("SELECT count(*)::bigint,COALESCE(sum(qty_delta),0)::float8 FROM mini_raw_material_events WHERE barcode=$1 AND event_type IN ('consumption_posted','consumption_reversed')")
            .bind(&barcode).fetch_one(&f.pool).await.unwrap();
        assert_eq!(current, ledger);
        assert_batch_unchanged(&f, &output.batch_id, &output).await;
    }
    f.release().await;
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn lamination_missing_remainder_evidence_reopens_inputless_without_reclaiming_old_stock() {
    let f = Fixture::new(LAM, false).await;
    let parent = f.store.progress_batch(&f.input).await.unwrap().unwrap();
    sqlx::query("DELETE FROM mini_order_progress_events WHERE session_id=$1 AND action='complete'")
        .bind(&f.source_session)
        .execute(&f.pool)
        .await
        .unwrap();
    let target = f.preview().await.target.unwrap();
    f.execute(&target).await.unwrap();
    let paused = f
        .store
        .active_order_run_session(LAM, &f.order)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(paused.status, OrderRunStatus::Paused);
    assert!(
        paused
            .payload_json
            .get("input_progress_batch_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .is_empty()
    );
    assert_batch_unchanged(&f, &f.input, &parent).await;
    assert!(
        f.service
            .apply_apparatus_queue_action_with_progress(
                LAM,
                &f.order,
                Action::Resume,
                &[LAM.into()],
                actor(),
                QueueProgressInput::default()
            )
            .await
            .is_err()
    );
    f.reclose(&f.reclose_target().await).await.unwrap();
    assert_batch_unchanged(&f, &f.input, &parent).await;
    f.release().await;
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn lamination_standalone_film_return_preserves_parent_and_residual_stock_on_reactivation() {
    let f = Fixture::new(LAM, false).await;
    f.execute(&f.preview().await.target.unwrap()).await.unwrap();
    f.resume().await;
    let (stock_id, barcode) = mount_film(&f).await;
    let completed = f
        .service
        .apply_apparatus_queue_action_with_progress(
            LAM,
            &f.order,
            Action::Complete,
            &[LAM.into()],
            actor(),
            QueueProgressInput {
                produced_qty: Some(25.0),
                uom: "m".into(),
                finished_goods_meter: Some(25.0),
                finished_goods_kg: Some(2.5),
                bobina_kg: Some(0.5),
                total_waste: Some(1.0),
                lamination_print_leftover_rolls: Some(1.0),
                lamination_film_leftover_rolls: Some(1.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let output = completed.progress_batch.unwrap();
    let source = completed.session.unwrap();
    let raw = f
        .service
        .layer_remainder_sources(LAM, &f.order)
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.source_id == stock_id)
        .unwrap();
    let record = f
        .service
        .record_layer_remainder(
            LayerRemainderInput {
                apparatus: LAM.into(),
                order_id: f.order.clone(),
                source_kind: "raw_material".into(),
                source_id: stock_id.clone(),
                session_id: source.session_id,
                expected_revision: raw.revision,
                remaining_net_kg: 3.0,
                request_id: format!("standalone-film-{}", f.order),
            },
            actor(),
        )
        .await
        .unwrap();
    let child = record.raw_material_stock.unwrap();
    let selector = ReactivationSelector {
        batch_id: output.batch_id.clone(),
        ..f.selector()
    };
    let target = f
        .service
        .preview_reactivation(&selector, &[LAM.into()], &actor())
        .await
        .unwrap()
        .target
        .unwrap();
    f.execute(&target).await.unwrap();
    let paused = f
        .store
        .active_order_run_session(LAM, &f.order)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(paused.status, OrderRunStatus::Paused);
    assert_eq!(paused.payload_json["input_progress_batch_id"], f.input);
    assert_eq!(
        paused.payload_json["start_resources"]["raw_materials_available"],
        false
    );
    for reclose in [false, true] {
        if reclose {
            f.reclose(&f.reclose_target().await).await.unwrap();
        }
        let parent: (String, f64) =
            sqlx::query_as("SELECT status,qty::float8 FROM mini_raw_material_stock WHERE id=$1")
                .bind(&stock_id)
                .fetch_one(&f.pool)
                .await
                .unwrap();
        assert_eq!(parent, ("consumed".into(), 11.0));
        let residual: (String, f64) =
            sqlx::query_as("SELECT status,qty::float8 FROM mini_raw_material_stock WHERE id=$1")
                .bind(&child.id)
                .fetch_one(&f.pool)
                .await
                .unwrap();
        assert_eq!(residual, ("available".into(), 3.0));
        let ledger: f64 = sqlx::query_scalar("SELECT sum(qty_delta)::float8 FROM mini_raw_material_events WHERE (barcode=$1 OR barcode=$2) AND event_type IN ('consumption_posted','consumption_reversed','layer_remainder_returned')")
            .bind(&barcode).bind(&child.barcode).fetch_one(&f.pool).await.unwrap();
        assert_eq!(ledger, -8.0);
    }
    f.release().await;
}
