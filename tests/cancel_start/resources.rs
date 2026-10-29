use super::*;
use sha2::{Digest, Sha256};

// Exercise the production store's exact Start/material transaction in addition
// to the public service flow tested above.
async fn start_with_material(f: &Fixture) -> CancelStartControl {
    let original = f.batch().await;
    let map = f
        .store
        .maps()
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.id == f.order)
        .unwrap();
    let fingerprint = format!("{:x}", Sha256::digest(serde_json::to_vec(&map).unwrap()));
    let route = json!({"source_stage_node_id":"print", "stage_node_id":"work",
        "consumer_apparatus_ids":[f.machine], "map_fingerprint":fingerprint, "remapped":false});
    let session_id = format!("run-{:032x}", rand::random::<u128>());
    let event_id = format!("start-{session_id}");
    let session = OrderRunSession {
        session_id: session_id.clone(),
        apparatus: f.machine.clone(),
        order_id: f.order.clone(),
        stage_node_id: "work".into(),
        status: OrderRunStatus::Active,
        worker_role: actor().role,
        worker_ref: actor().ref_,
        worker_display_name: actor().display_name,
        started_at_unix: 1000,
        updated_at_unix: 1000,
        payload_json: json!({"stage_work_protocol":1,"input_progress_batch_id":original.batch_id,
            "input_progress_qr_payload":original.qr_payload,"input_progress_apparatus":PRESS,"input_wip_source_kind":"progress_batch"}),
    };
    let mut input = original.clone();
    input.wip_status = OrderProgressBatchWipStatus::InUse;
    input.current_apparatus = f.machine.clone();
    input.current_location = f.machine.clone();
    input.used_by_apparatus = f.machine.clone();
    input.used_by_session_id = session_id.clone();
    input.payload_json["wip_route_binding"] = serde_json::to_value(route).unwrap();
    let progress=serde_json::from_value(json!({"event_id":format!("progress-{session_id}"),"session_id":session_id,
        "apparatus":f.machine,"order_id":f.order,"action":"start","produced_qty":0,"uom":"",
        "worker_role":actor().role,"worker_ref":actor().ref_,"worker_display_name":actor().display_name,"payload_json":{"event":"start"}})).unwrap();
    let write = QueueActionProgressWrite {
        apparatus: f.machine.clone(),
        states: [(f.order.clone(), "in_progress".into())].into(),
        event: ApparatusQueueActionEvent {
            event_id,
            apparatus: f.machine.clone(),
            order_id: f.order.clone(),
            stage_node_id: "work".into(),
            action: Action::Start,
            from_state: State::Pending,
            to_state: State::InProgress,
            policy: ApparatusQueuePolicy::FreePick,
            actor: actor(),
            assigned_apparatus: vec![f.machine.clone()],
            payload_json: json!({"cancel_start_supported":true,
                "wip_input_map_fingerprint":fingerprint,"wip_input_expected_batch":original}),
        },
        session: Some(session),
        progress_event: Some(progress),
        map_update: None,
        sequence_updates: Default::default(),
        progress_batch: None,
        progress_batches: vec![],
        progress_batch_updates: vec![input],
        opening_wip_batch_updates: vec![],
        raw_material_stock_transitions: vec![RawMaterialStockTransition::new(
            RawMaterialStockTransitionKind::InUse,
            vec![f.order.clone()],
            &f.order,
        )],
        qolip_checkouts: vec![],
        returned_paint_report: None,
        order_control_update: None,
        schedule_reservation_status: None,
        print_preflight_hold_id: None,
        print_preflight_cancel_hold_id: None,
    };
    f.store
        .put_apparatus_queue_states_with_event_and_progress(&write)
        .await
        .unwrap();
    // This deliberately minimal storage fixture bypasses operator presentation;
    // the normal service/control path is exercised for all four machine types.
    let stored = f
        .store
        .active_order_run_session(&f.machine, &f.order)
        .await
        .unwrap()
        .unwrap();
    serde_json::from_value(stored.payload_json["_cancel_start"]["control"].clone()).unwrap()
}

#[tokio::test]
#[ignore = "requires CANCEL_START_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn raw_material_claims_restore_atomically_and_repeat_usage_is_audited() {
    let f = Fixture::new(MACHINES[1]).await;
    sqlx::query("INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty,uom,status,reserved_order_id,payload_json)
        VALUES($1,'Original warehouse','FILM','Film',$1,23.125,'kg','available',$1,'{\"original\":true}')")
        .bind(&f.order).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_assignments(barcode,order_id,apparatus,canonical_apparatus_id,item_code,item_group,payload_json)
        VALUES($1,$1,$2,$2,'FILM','film','{}')").bind(&f.order).bind(&f.machine).execute(&f.pool).await.unwrap();
    let before: Value = sqlx::query_scalar(
        "SELECT to_jsonb(s)-'updated_at' FROM mini_raw_material_stock s WHERE barcode=$1",
    )
    .bind(&f.order)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    for _ in 0..2 {
        let c = start_with_material(&f).await;
        let runtime_pool = sqlx::PgPool::connect_with(
            f.pool
                .connect_options()
                .as_ref()
                .clone()
                .username("mini_rs_erp"),
        )
        .await
        .unwrap();
        let runtime_service = ProductionMapService::new(
            Arc::new(PostgresProductionMapStore::new(runtime_pool.clone())),
            Arc::new(CanonicalServiceApparatusResolver::new(
                postgres::canonical_apparatus_service(runtime_pool.clone()),
            )),
        );
        runtime_service
            .cancel_started_work(
                &f.machine,
                &f.order,
                &c.session_id,
                &c.token,
                &[f.machine.clone()],
                actor(),
            )
            .await
            .unwrap();
        runtime_pool.close().await;
        let after: Value = sqlx::query_scalar(
            "SELECT to_jsonb(s)-'updated_at' FROM mini_raw_material_stock s WHERE barcode=$1",
        )
        .bind(&f.order)
        .fetch_one(&f.pool)
        .await
        .unwrap();
        assert_eq!(
            after, before,
            "status, reservation, quantity, placement and payload must be preserved"
        );
    }
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM mini_raw_material_events WHERE barcode_key=$1 AND event_type='usage_started'")
        .bind(&f.order).fetch_one(&f.pool).await.unwrap(),2);
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM mini_raw_material_events WHERE barcode_key=$1 AND event_type='usage_cancelled' AND qty_delta=0")
        .bind(&f.order).fetch_one(&f.pool).await.unwrap(),2);
    let c = start_with_material(&f).await;
    sqlx::query("UPDATE mini_raw_material_stock SET qty=qty-1 WHERE barcode=$1")
        .bind(&f.order)
        .execute(&f.pool)
        .await
        .unwrap();
    let held = f.batch().await;
    assert!(f.cancel(&c).await.is_err());
    assert_eq!(
        f.batch().await,
        held,
        "stale stock must not partially release WIP"
    );
    f.release().await;
}

#[tokio::test]
#[ignore = "requires CANCEL_START_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn opening_wip_keeps_its_original_label_quantity_and_history() {
    let f = Fixture::new(MACHINES[1]).await;
    let record=f.service.create_opening_wip(serde_json::from_value(json!({"idempotency_key":f.order,"order_id":f.order,
        "source_apparatus":PRESS,"source_stage_node_id":"print","source_operation":"print",
        "batches":[{"quantity":80,"uom":"m","quantity_basis":"measured","finished_goods_meter":80,"finished_goods_kg":10,"bobina_kg":1}]})).unwrap(),actor()).await.unwrap();
    let original = &record.batches[0];
    f.service
        .apply_apparatus_queue_action_with_progress(
            &f.machine,
            &f.order,
            Action::Start,
            &[f.machine.clone()],
            actor(),
            QueueProgressInput {
                qr_payload: original.qr_payload.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let c = f.control().await.unwrap();
    f.cancel(&c).await.unwrap();
    let restored = f
        .store
        .opening_wip_batch(&original.batch_id, &original.qr_payload)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored.intake, record.intake);
    let mut batch = restored.batch;
    batch.updated_at_unix = original.updated_at_unix;
    assert_eq!(&batch, original);
}
