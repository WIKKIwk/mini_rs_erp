//! Disposable PostgreSQL transaction and scan-continuation oracles.
use mini_rs_erp::{
    core::production_map::*,
    db::{
        postgres::{
            apply_foundation_migration, apply_postgres_migrations_through_version,
            canonical_apparatus_service,
        },
        postgres_production_map::PostgresProductionMapStore,
    },
};
use serde_json::json;
use sqlx::{PgPool, postgres::PgConnectOptions};
use std::{collections::BTreeMap, sync::Arc};

const PRINT: &str = "apparatus:default:bosma_8";
const LAM: &str = "apparatus:default:asset-007";
const ORDER: &str = "zakaz-layer-remainder-order";
fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "lam-worker".into(),
        display_name: "Laminator".into(),
    }
}
fn session(id: &str, status: OrderRunStatus) -> OrderRunSession {
    OrderRunSession {
        session_id: id.into(),
        apparatus: LAM.into(),
        order_id: ORDER.into(),
        stage_node_id: "lam".into(),
        status,
        worker_role: "aparatchi".into(),
        worker_ref: "lam-worker".into(),
        worker_display_name: "Laminator".into(),
        started_at_unix: 10,
        updated_at_unix: 20,
        payload_json: json!({}),
    }
}
fn input(source: &LayerRemainderSource, kg: f64, key: &str) -> LayerRemainderInput {
    LayerRemainderInput {
        apparatus: LAM.into(),
        order_id: ORDER.into(),
        source_kind: source.source_kind.clone(),
        source_id: source.source_id.clone(),
        session_id: source.session_id.clone(),
        expected_revision: source.revision,
        remaining_net_kg: kg,
        request_id: key.into(),
    }
}

#[tokio::test]
#[ignore = "requires explicitly configured disposable MINI_ERP_LAYER_REMAINDER_TEST_DATABASE_URL"]
async fn exact_balance_identity_retry_and_reusable_wip_and_raw_stock() {
    let url = std::env::var("MINI_ERP_LAYER_REMAINDER_TEST_DATABASE_URL")
        .expect("disposable database URL");
    let admin = PgPool::connect(&url).await.unwrap();
    let name = format!("layer_remainder_{:032x}", rand::random::<u128>());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(url.parse::<PgConnectOptions>().unwrap().database(&name))
        .await
        .unwrap();
    let check_pool = pool.clone();
    // SQL recovery executes on one borrowed connection. Keep panic isolation
    // for database cleanup without moving that transaction across threads.
    let result = tokio::task::LocalSet::new()
        .run_until(async move {
            tokio::task::spawn_local(async move { checks(check_pool).await }).await
        })
        .await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}

async fn checks(pool: PgPool) {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .try_init();
    sqlx::raw_sql("DO $$ BEGIN IF NOT EXISTS(SELECT 1 FROM pg_roles WHERE rolname='mini_rs_erp') THEN CREATE ROLE mini_rs_erp NOLOGIN; END IF; END $$;").execute(&pool).await.unwrap();
    apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    let canonical = canonical_apparatus_service(pool.clone());
    canonical.bootstrap_factory_defaults().await.unwrap();
    apply_postgres_migrations_through_version(&pool, "0141")
        .await
        .unwrap();
    // Existing delivery receipts must survive the extended ledger allowlist.
    sqlx::raw_sql("INSERT INTO mini_raw_material_events(event_id,idempotency_key,event_type,warehouse,barcode,item_code,item_name,qty_delta,uom,actor_role,actor_ref,source_type,source_id) VALUES('historical-delivery','historical-delivery','delivery_received','Raw W','HISTORICAL','FILM','Film',0,'kg','aparatchi','lam-worker','material_delivery','delivery')").execute(&pool).await.unwrap();
    apply_foundation_migration(&pool).await.unwrap();
    // Real installations configure material tracking; bare factory defaults
    // intentionally omit it. Enable the actual policy via the canonical API.
    use mini_rs_erp::core::apparatus_standard::{
        ApparatusId, ApparatusOperationalPolicies, CanonicalApparatusPatch,
        CanonicalCommandMetadata, MaterialExecutionPolicy,
    };
    let id = ApparatusId::new(LAM).unwrap();
    let current = canonical.current_configuration(&id).await.unwrap().unwrap();
    canonical
        .patch(
            id,
            current.runtime.source_revision,
            CanonicalApparatusPatch {
                policies: Some(ApparatusOperationalPolicies {
                    queue: current.queue.discipline,
                    material: MaterialExecutionPolicy::AllRequired {
                        item_group_ids: vec!["All Item Groups".into()],
                    },
                    tooling: current.material.tooling.clone(),
                }),
                ..Default::default()
            },
            CanonicalCommandMetadata::new("layer-fixture", "required-film-layer"),
        )
        .await
        .unwrap();
    let runtime = sqlx::postgres::PgPoolOptions::new()
        .after_connect(|conn, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE mini_rs_erp").execute(conn).await?;
                Ok(())
            })
        })
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let store = Arc::new(PostgresProductionMapStore::new(runtime.clone()));
    let service = ProductionMapService::new(
        store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(canonical)),
    );
    let map:ProductionMapDefinition=serde_json::from_value(json!({"id":ORDER,"product_code":"PRODUCT","title":"Layer remainder","nodes":[{"id":"start","kind":"start","title":"Start"},{"id":"print","kind":"apparatus","title":"Bosma","apparatus_id":PRINT},{"id":"lam","kind":"apparatus","title":"Lamination","apparatus_id":LAM},{"id":"end","kind":"end","title":"End"}],"edges":[{"from":"start","to":"print"},{"from":"print","to":"lam"},{"from":"lam","to":"end"}]})).unwrap();
    store.put_map(map).await.unwrap();
    store
        .put_apparatus_sequence(LAM, vec![ORDER.into()])
        .await
        .unwrap();
    store
        .put_apparatus_queue_states(LAM, BTreeMap::from([(ORDER.into(), "completed".into())]))
        .await
        .unwrap();
    let batch:OrderProgressBatch=serde_json::from_value(json!({"batch_id":"original50","session_id":"print-session","started_at_unix":1,"completed_at_unix":2,"apparatus":PRINT,"order_id":ORDER,"action":"complete","status":"completed","produced_qty":1500,"uom":"m","qr_payload":"40010000000000000000AA01","label_item_code":"PRODUCT","label_item_name":"Printed film","executor_name":"Original Printer","worker_role":"aparatchi","worker_ref":"print-worker","worker_display_name":"Original Printer","wip_status":"processed","current_apparatus":LAM,"current_location":LAM,"next_apparatus":LAM,"processed_by_session_id":"lam-session","processed_by_apparatus":LAM,"finished_goods_kg":52,"bobina_kg":2,"finished_goods_meter":1500,"payload_json":{"stage_node_id":"print","next_stage_node_id":"lam","gross_qty":52}})).unwrap();
    store.put_order_progress_batch(batch.clone()).await.unwrap();
    let mut run = session("lam-session", OrderRunStatus::Completed);
    run.payload_json = json!({"input_progress_batch_id":batch.batch_id,"input_progress_qr_payload":batch.qr_payload,"input_progress_apparatus":PRINT,"input_wip_source_kind":"progress_batch","input_lineage":[{"input_batch_id":batch.batch_id,"input_qr_payload":batch.qr_payload,"source_apparatus":PRINT,"source_kind":"progress_batch","stage_node_id":"lam","sequence_no":1,"status":"processed","linked_at_unix":10,"processed_at_unix":20}]});
    store.put_order_run_session(run).await.unwrap();
    let sources = service.layer_remainder_sources(LAM, ORDER).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].source_net_kg, 50.0);
    let request = input(&sources[0], 20.0, "remainder20-request");
    let mut invalid = request.clone();
    invalid.request_id = "invalid-weight-request".into();
    invalid.remaining_net_kg = 51.0;
    assert!(
        service
            .record_layer_remainder(invalid, actor())
            .await
            .is_err()
    );
    let mut stale_revision = request.clone();
    stale_revision.request_id = "stale-revision-request".into();
    stale_revision.expected_revision += 1;
    assert_eq!(
        service
            .record_layer_remainder(stale_revision, actor())
            .await
            .unwrap_err(),
        ProductionMapError::ProgressBatchCorrectionConflict
    );
    let record = service
        .record_layer_remainder(request.clone(), actor())
        .await
        .unwrap();
    assert_eq!(record.remaining_meter, 600.0);
    assert_eq!(record.tare_kg, 2.0);
    assert_eq!(record.producer_name, "Original Printer");
    assert_eq!(record.recorded_by.display_name, "Laminator");
    assert_eq!(
        service
            .record_layer_remainder(request.clone(), actor())
            .await
            .unwrap(),
        record
    );
    let mut stale = request.clone();
    stale.request_id = "different-request".into();
    assert!(
        service
            .record_layer_remainder(stale, actor())
            .await
            .is_err()
    );
    let original = store.progress_batch("original50").await.unwrap().unwrap();
    assert_eq!(original.produced_qty, 1500.0);
    assert_eq!(original.finished_goods_kg, Some(52.0));
    assert_eq!(original.wip_status, OrderProgressBatchWipStatus::Processed);
    let child = record.progress_batch.as_ref().unwrap();
    assert!(child.is_layer_remainder());
    assert_eq!(child.worker_ref, "print-worker");
    assert_eq!(child.finished_goods_kg, Some(22.0));
    assert_eq!(child.finished_goods_meter, Some(600.0));
    assert_ne!(child.qr_payload, original.qr_payload);
    assert_eq!(
        store
            .progress_batches_for_worker(&["print-worker".into()], "", 20)
            .await
            .unwrap()
            .len(),
        1
    );
    let events: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mini_order_progress_events WHERE order_id=$1")
            .bind(ORDER)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(events, 0);
    let pending:i64=sqlx::query_scalar("SELECT COALESCE(sum(batch_count),0)::bigint FROM mini_progress_batch_work_inputs WHERE order_id=$1").bind(ORDER).fetch_one(&pool).await.unwrap();
    assert_eq!(pending, 0);

    // Real scan uses the normal authoritative Start route/claim transaction.
    let started = service
        .apply_apparatus_queue_action_with_progress(
            LAM,
            ORDER,
            queue_state::ApparatusQueueAction::Start,
            &[LAM.into()],
            actor(),
            QueueProgressInput {
                qr_payload: child.qr_payload.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(started.progress_batch.is_none());
    let active = store
        .active_order_run_session(LAM, ORDER)
        .await
        .unwrap()
        .unwrap();
    let mut child_used = store
        .progress_batch(&child.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(child_used.wip_status, OrderProgressBatchWipStatus::InUse);
    // Closing without producing output isolates the residual-return balance.
    child_used.wip_status = OrderProgressBatchWipStatus::Processed;
    child_used.used_by_session_id.clear();
    child_used.used_by_apparatus.clear();
    child_used.processed_by_session_id = active.session_id.clone();
    child_used.processed_by_apparatus = LAM.into();
    child_used.refresh_status_detail();
    store.put_order_progress_batch(child_used).await.unwrap();
    let mut completed = active;
    completed.status = OrderRunStatus::Completed;
    store.put_order_run_session(completed).await.unwrap();
    let next = service
        .layer_remainder_sources(LAM, ORDER)
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.source_id == child.batch_id)
        .unwrap();
    let tiny = service
        .record_layer_remainder(input(&next, 5.0, "remainder5-request"), actor())
        .await
        .unwrap();
    assert_eq!(tiny.remaining_meter, 150.0);
    assert_eq!(
        tiny.progress_batch
            .as_ref()
            .unwrap()
            .payload_json
            .pointer("/layer_remainder/root_batch_id")
            .and_then(serde_json::Value::as_str),
        Some("original50")
    );
    assert_eq!(
        store
            .progress_batch("original50")
            .await
            .unwrap()
            .unwrap()
            .finished_goods_kg,
        Some(52.0)
    );
    assert!(
        sqlx::query(
            "UPDATE mini_progress_batches SET wip_status='waiting' WHERE batch_id='original50'"
        )
        .execute(&pool)
        .await
        .is_err()
    );

    sqlx::raw_sql("INSERT INTO mini_warehouses(id,name) VALUES('raw-w','Raw W'); INSERT INTO mini_items(code,name,uom,item_group) VALUES('FILM','Film','kg','All Item Groups'); INSERT INTO mini_factory_locations(id,name) VALUES('lam-state','Lamination State');")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO mini_factory_location_apparatus_links(location_id,apparatus_id) VALUES('lam-state',$1)").bind(LAM).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty,width_mm,micron,length_m,uom,status,reserved_order_id,source_receipt_id,payload_json) VALUES('raw50','Raw W','FILM','Film','RAW50',50,1000,20,1500,'kg','consumed',$1,'original-receipt','{\"gross_qty\":52,\"tare_kg\":2}')").bind(ORDER).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO mini_inventory_placements(asset_kind,asset_ref,physical_location_id) VALUES('raw_material','raw50','inventory_location:state:lam-state')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_assignments(barcode,order_id,apparatus,canonical_apparatus_id,item_code,item_group,payload_json) VALUES('RAW50',$1,$2,$2,'FILM','All Item Groups',$3)")
        .bind(ORDER).bind(LAM).bind(json!({"order_id":ORDER,"apparatus_id":LAM,"apparatus":LAM,"barcode":"RAW50","item_code":"FILM","item_name":"Film","item_group":"All Item Groups","assigned_by_role":"material_taminotchi","assigned_by_ref":"original-supplier","assigned_by_display_name":"Original Supplier","assigned_at":"2026-10-12"})).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_events(event_id,idempotency_key,event_type,warehouse,barcode,item_code,item_name,qty_delta,uom,stock_status_before,stock_status_after,order_id,apparatus,canonical_apparatus_id,actor_role,actor_ref,actor_display_name,owner_role,owner_ref,owner_display_name,source_type,source_id) VALUES('consume-raw','consume-raw','consumption_posted','Raw W','RAW50','FILM','Film',-50,'kg','in_use','consumed',$1,$2,$2,'aparatchi','lam-worker','Laminator','material_taminotchi','original-supplier','Original Supplier','consumption',$1)").bind(ORDER).bind(LAM).execute(&pool).await.unwrap();
    let mut raw_run = session("raw-session", OrderRunStatus::Completed);
    raw_run.started_at_unix = store
        .order_run_sessions_for_order(ORDER)
        .await
        .unwrap()
        .into_iter()
        .map(|session| session.started_at_unix)
        .max()
        .unwrap_or(0) + 1;
    raw_run.payload_json = json!({"start_resources":{"session_id":"raw-session","raw_materials_available":true,"raw_materials":[{"barcode":"RAW50","item_code":"FILM","item_name":"Film","stock_id":"raw50","source_qty":50,"uom":"kg"}],"qolip_codes":[],"qolip_available":false}});
    store.put_order_run_session(raw_run).await.unwrap();
    let raw_source = service
        .layer_remainder_sources(LAM, ORDER)
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.source_id == "raw50")
        .unwrap();
    let raw_request = input(&raw_source, 20.0, "raw-remainder-request");
    let raw = service
        .record_layer_remainder(raw_request.clone(), actor())
        .await
        .unwrap();
    assert_eq!(raw.remaining_meter, 600.0);
    assert_eq!(raw.producer_name, "Original Supplier");
    assert_eq!(
        service
            .record_layer_remainder(raw_request, actor())
            .await
            .unwrap(),
        raw
    );
    let stock = raw.raw_material_stock.unwrap();
    assert_eq!(stock.source_receipt_id, "original-receipt");
    assert_eq!(stock.qty, 20.0);
    assert_eq!(stock.status, "available");
    assert_eq!(stock.payload_json["owner_ref"], "original-supplier");
    let net:f64=sqlx::query_scalar("SELECT sum(qty_delta)::float8 FROM mini_raw_material_events WHERE order_id=$1 AND (event_type='consumption_posted' OR event_type='layer_remainder_returned')").bind(ORDER).fetch_one(&pool).await.unwrap();
    assert_eq!(net, -30.0);
    let assigned:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_raw_material_assignments WHERE barcode=$1 AND order_id=$2 AND canonical_apparatus_id=$3)").bind(&stock.barcode).bind(ORDER).bind(LAM).fetch_one(&pool).await.unwrap();
    assert!(assigned);
    let location:String=sqlx::query_scalar("SELECT physical_location_id FROM mini_inventory_placements WHERE asset_kind='raw_material' AND asset_ref=$1").bind(&stock.id).fetch_one(&pool).await.unwrap();
    assert_eq!(location, "inventory_location:state:lam-state");
    let requirements = service
        .raw_material_start_requirements_for_stock(
            LAM,
            ORDER,
            &[stock.barcode.clone()],
            &stock.barcode,
            Some(&[stock.barcode.clone()]),
        )
        .await
        .unwrap();
    assert!(
        requirements
            .eligible_barcodes
            .iter()
            .any(|barcode| barcode.eq_ignore_ascii_case(&stock.barcode)),
        "{requirements:?}"
    );
    assert!(requirements.scan_satisfied);
    assert!(
        sqlx::query("UPDATE mini_raw_material_stock SET status='available' WHERE id='raw50'")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM mini_layer_remainder_operations")
            .execute(&pool)
            .await
            .is_err()
    );
    sqlx::query(
        "INSERT INTO mini_order_control_states(order_id,state) VALUES($1,'freeze_requested')",
    )
    .bind(ORDER)
    .execute(&pool)
    .await
    .unwrap();
    let mut frozen_request = request;
    frozen_request.request_id = "pending-freeze-request".into();
    assert_eq!(
        service
            .record_layer_remainder(frozen_request, actor())
            .await
            .unwrap_err(),
        ProductionMapError::OrderFrozen
    );
    additional_intake_and_legacy_mounted_source(&pool, &store, &service).await;
    runtime.close().await;
    pool.close().await;
}


async fn additional_intake_and_legacy_mounted_source(
    pool: &PgPool,
    store: &PostgresProductionMapStore,
    service: &ProductionMapService,
) {
    const EXTRA_ORDER: &str = "zakaz-additional-layer-intake";
    let map: ProductionMapDefinition = serde_json::from_value(json!({
        "id": EXTRA_ORDER, "product_code": "PRODUCT", "title": "Additional layer intake",
        "nodes": [{"id":"start","kind":"start","title":"Start"},
            {"id":"lam","kind":"apparatus","title":"Lamination","apparatus_id":LAM},
            {"id":"end","kind":"end","title":"End"}],
        "edges": [{"from":"start","to":"lam"},{"from":"lam","to":"end"}]
    }))
    .unwrap();
    store.put_map(map).await.unwrap();
    store
        .put_apparatus_sequence(LAM, vec![EXTRA_ORDER.into()])
        .await
        .unwrap();
    store
        .put_apparatus_queue_states(
            LAM,
            BTreeMap::from([(EXTRA_ORDER.into(), "in_progress".into())]),
        )
        .await
        .unwrap();
    let mut run = session("additional-intake-session", OrderRunStatus::Active);
    run.order_id = EXTRA_ORDER.into();
    run.started_at_unix = 1000;
    store.put_order_run_session(run.clone()).await.unwrap();
    for (id, status, reserved) in [
        ("extra25", "available", ""),
        ("unreceived25", "available", ""),
        ("unaudited25", "available", ""),
    ] {
        sqlx::query("INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty,width_mm,micron,length_m,uom,status,reserved_order_id,source_receipt_id,payload_json) VALUES($1,'Raw W','FILM','CPP',$1,25,1000,20,750,'kg',$2,$3,'additional-receipt','{\"gross_qty\":27,\"tare_kg\":2}')")
            .bind(id).bind(status).bind(reserved).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO mini_inventory_placements(asset_kind,asset_ref,physical_location_id) VALUES('raw_material',$1,'inventory_location:state:lam-state')")
            .bind(id).execute(pool).await.unwrap();
        store
            .put_raw_material_assignment(RawMaterialAssignment {
                order_id: EXTRA_ORDER.into(),
                apparatus_id: mini_rs_erp::core::apparatus_standard::ApparatusId::new(LAM).unwrap(),
                apparatus: LAM.into(),
                barcode: id.into(),
                item_code: "FILM".into(),
                item_name: "CPP".into(),
                item_group: "All Item Groups".into(),
                assigned_by_role: "material_taminotchi".into(),
                assigned_by_ref: "original-supplier".into(),
                assigned_by_display_name: "Original Supplier".into(),
                assigned_at: "2026-10-13".into(),
            })
            .await
            .unwrap();
    }
    sqlx::query("UPDATE mini_raw_material_stock SET status='in_use',reserved_order_id=$1 WHERE id='unaudited25'")
        .bind(EXTRA_ORDER).execute(pool).await.unwrap();
    let assignment: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json FROM mini_raw_material_assignments WHERE barcode='extra25'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    store
        .receive_raw_material_assignment(
            serde_json::from_value(assignment.clone()).unwrap(),
            &actor(),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .receive_raw_material_assignment(serde_json::from_value(assignment).unwrap(), &actor())
            .await
            .unwrap_err(),
        ProductionMapError::RawMaterialStockUnavailable
    );
    let saved = store
        .order_run_session(&run.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        saved
            .payload_json
            .pointer("/start_resources/raw_materials/0/stock_id"),
        Some(&json!("extra25"))
    );
    assert_eq!(
        saved
            .payload_json
            .pointer("/start_resources/raw_materials/0/source_qty"),
        Some(&json!(25.0))
    );
    assert_eq!(
        saved
            .payload_json
            .pointer("/start_resources/raw_materials_available"),
        Some(&json!(true))
    );
    let sources = service
        .layer_remainder_sources(LAM, EXTRA_ORDER)
        .await
        .unwrap();
    assert_eq!(
        sources.len(),
        1,
        "unreceived or unaudited stock must be excluded"
    );
    assert_eq!(sources[0].source_id, "extra25");
    assert_eq!(sources[0].source_net_kg, 25.0);
    assert_eq!(sources[0].source_meter, 750.0);

    // The user's existing execution has no start_resources. Its actual intake
    // event and mounted stock must still supply the same layer in Astatka.
    sqlx::query("UPDATE mini_order_run_sessions SET payload_json=payload_json-'start_resources' WHERE session_id=$1")
        .bind(&run.session_id).execute(pool).await.unwrap();
    let legacy = service
        .layer_remainder_sources(LAM, EXTRA_ORDER)
        .await
        .unwrap();
    assert_eq!(legacy, sources);
    run.status = OrderRunStatus::Completed;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    run.updated_at_unix = now;
    run.payload_json["last_action"] = json!("complete");
    store.put_order_run_session(run.clone()).await.unwrap();
    let mut next_run = run.clone();
    next_run.session_id = "additional-next-wip-session".into();
    next_run.status = OrderRunStatus::Active;
    next_run.started_at_unix = now + 1;
    next_run.updated_at_unix = now + 1;
    next_run.payload_json = json!({});
    store.put_order_run_session(next_run.clone()).await.unwrap();
    let continued = service.layer_remainder_sources(LAM, EXTRA_ORDER).await.unwrap();
    assert_eq!(continued.len(), 1);
    assert_eq!(continued[0].source_id, "extra25");
    assert_eq!(continued[0].session_id, next_run.session_id);
    assert_eq!(continued[0].source_net_kg, 25.0);

    // Reproduce the historical bug, then run the same guarded audit recovery
    // used for local 0054. The original debit must remain immutable.
    sqlx::query("INSERT INTO mini_raw_material_events(event_id,idempotency_key,event_type,warehouse,barcode,item_code,item_name,qty_delta,uom,stock_status_before,stock_status_after,order_id,apparatus,canonical_apparatus_id,actor_role,actor_ref,actor_display_name,owner_role,owner_ref,owner_display_name,source_type,source_id,occurred_at) VALUES('premature-source-event',$1,'consumption_posted','Raw W','EXTRA25','FILM','CPP',-25,'kg','in_use','consumed',$2,$3,$3,'aparatchi','lam-worker','Laminator','material_taminotchi','original-supplier','Original Supplier','consumption',$2,to_timestamp($4))")
        .bind(format!("consumption_posted:EXTRA25:{EXTRA_ORDER}:{LAM}"))
        .bind(EXTRA_ORDER).bind(LAM).bind(now as f64).execute(pool).await.unwrap();
    sqlx::query("UPDATE mini_raw_material_stock SET status='consumed' WHERE id='extra25'")
        .execute(pool).await.unwrap();
    let recovery = include_str!("../tools/db/repairs/0054_lamination_raw_consumption.sql")
        .replace("zakaz-0054", EXTRA_ORDER)
        .replace("30DDE6C27B4A9230F0A13CA9", "EXTRA25")
        .replace("rme_47f878e2c7b58a2accf76db6c208329c", "premature-source-event")
        .replace(&format!("order-run:1791844578597507000:apparatus-default-asset-007:{EXTRA_ORDER}:worker-d5f3bafec0949a7e26073f5d"), &run.session_id);
    let mut connection = pool.acquire().await.unwrap();
    sqlx::query("UPDATE mini_order_run_sessions SET payload_json=payload_json || '{\"stage_work_report\":{}}' WHERE session_id=$1")
        .bind(&run.session_id).execute(&mut *connection).await.unwrap();
    assert!(sqlx::raw_sql(&recovery).execute(&mut *connection).await.is_err());
    sqlx::query("ROLLBACK").execute(&mut *connection).await.unwrap();
    let still_consumed: String = sqlx::query_scalar("SELECT status FROM mini_raw_material_stock WHERE id='extra25'")
        .fetch_one(&mut *connection).await.unwrap();
    assert_eq!(still_consumed, "consumed");
    sqlx::query("UPDATE mini_order_run_sessions SET payload_json=payload_json-'stage_work_report' WHERE session_id=$1")
        .bind(&run.session_id).execute(&mut *connection).await.unwrap();
    sqlx::raw_sql(&recovery).execute(&mut *connection).await.unwrap();
    sqlx::raw_sql(&recovery).execute(&mut *connection).await.unwrap();
    let recovery_events: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_raw_material_events WHERE event_type='consumption_reversed' AND barcode='EXTRA25'")
        .fetch_one(&mut *connection).await.unwrap();
    assert_eq!(recovery_events, 1);
    let restored: f64 = sqlx::query_scalar("SELECT sum(qty_delta)::float8 FROM mini_raw_material_events WHERE barcode='EXTRA25' AND event_type IN ('consumption_posted','consumption_reversed')")
        .fetch_one(&mut *connection).await.unwrap();
    assert_eq!(restored, 0.0);
    drop(connection);
    next_run.status = OrderRunStatus::Completed;
    next_run.updated_at_unix = now + 2;
    store.put_order_run_session(next_run).await.unwrap();
    store
        .put_apparatus_queue_states(
            LAM,
            BTreeMap::from([(EXTRA_ORDER.into(), "completed".into())]),
        )
        .await
        .unwrap();
    let source = service
        .layer_remainder_sources(LAM, EXTRA_ORDER)
        .await
        .unwrap()
        .remove(0);
    let mut request = input(&source, 10.0, "additional-raw-remainder");
    request.order_id = EXTRA_ORDER.into();
    let record = service
        .record_layer_remainder(request.clone(), actor())
        .await
        .unwrap();
    assert_eq!(record.remaining_meter, 300.0);
    assert_eq!(record.producer_name, "Original Supplier");
    assert_eq!(record.recorded_by.ref_, "lam-worker");
    let stock = record.raw_material_stock.as_ref().unwrap();
    assert_eq!(stock.qty, 10.0);
    assert_eq!(stock.status, "available");
    assert_eq!(stock.source_receipt_id, "additional-receipt");
    assert_eq!(
        service
            .record_layer_remainder(request, actor())
            .await
            .unwrap(),
        record
    );
    let status: String =
        sqlx::query_scalar("SELECT status FROM mini_raw_material_stock WHERE id='extra25'")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(status, "consumed");
    let net_consumption: f64 = sqlx::query_scalar("SELECT sum(qty_delta)::float8 FROM mini_raw_material_events WHERE order_id=$1 AND event_type IN ('consumption_posted','consumption_reversed','layer_remainder_returned')")
        .bind(EXTRA_ORDER).fetch_one(pool).await.unwrap();
    assert_eq!(net_consumption, -15.0);
    let debits: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_raw_material_events WHERE barcode='EXTRA25' AND event_type='consumption_posted'")
        .fetch_one(pool).await.unwrap();
    assert_eq!(debits, 2, "the corrected debit cannot suppress the actual final settlement");
    assert!(
        service
            .layer_remainder_sources(LAM, EXTRA_ORDER)
            .await
            .unwrap()
            .is_empty()
    );
}
