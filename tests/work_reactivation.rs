#![cfg(feature = "verification")]
//! Integration checks require an explicitly supplied disposable PostgreSQL server.
use mini_rs_erp::{
    core::production_map::{queue_state::ApparatusQueueAction as Action, *},
    db::{postgres, postgres_production_map::PostgresProductionMapStore},
};
use serde_json::json;
use std::sync::Arc;

const PRESS: &str = "apparatus:default:bosma_7";
const LAM: &str = "apparatus:default:asset-007";
const CUT: &str = "apparatus:default:asset-010";
#[path = "work_reactivation/reclose.rs"]
mod reclose;
#[path = "work_reactivation/continuation.rs"]
mod continuation;

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "reopen-worker".into(),
        display_name: "Worker".into(),
    }
}

async fn pool() -> sqlx::PgPool {
    static DATABASE: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();
    let admin_url = std::env::var("REACTIVATION_TEST_DATABASE_URL")
        .expect("REACTIVATION_TEST_DATABASE_URL must point at a disposable PostgreSQL server");
    let database = DATABASE
        .get_or_init(|| async {
            let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
            let name = format!("work_reactivation_{:032x}", rand::random::<u128>());
            sqlx::query(&format!("CREATE DATABASE {name}"))
                .execute(&admin)
                .await
                .unwrap();
            let pool = sqlx::PgPool::connect_with(
                admin_url
                    .parse::<sqlx::postgres::PgConnectOptions>()
                    .unwrap()
                    .database(&name),
            )
            .await
            .unwrap();
            postgres::apply_postgres_migrations_through_version(&pool, "0121")
                .await
                .unwrap();
            postgres::canonical_apparatus_service(pool.clone())
                .bootstrap_factory_defaults()
                .await
                .unwrap();
            postgres::apply_foundation_migration(&pool).await.unwrap();
            pool.close().await;
            admin.close().await;
            name
        })
        .await;
    sqlx::PgPool::connect_with(
        admin_url
            .parse::<sqlx::postgres::PgConnectOptions>()
            .unwrap()
            .database(database),
    )
    .await
    .unwrap()
}

struct Fixture {
    pool: sqlx::PgPool,
    store: Arc<PostgresProductionMapStore>,
    service: ProductionMapService,
    order: String,
    apparatus: &'static str,
    source_session: String,
    output: String,
    input: String,
}

impl Fixture {
    async fn new(apparatus: &'static str, final_complete: bool) -> Self {
        let pool = pool().await;
        let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
        let service = ProductionMapService::new(
            store.clone(),
            Arc::new(CanonicalServiceApparatusResolver::new(
                postgres::canonical_apparatus_service(pool.clone()),
            )),
        );
        let order = format!("zakaz-reopen-{:032x}", rand::random::<u128>());
        service.upsert_map(serde_json::from_value(json!({
            "id":order,"title":"Reactivation fixture","product_code":"REOPEN","roll_count":4,"width_mm":650,
            "nodes":[{"id":"start","kind":"start","title":"Start"},
                {"id":"press","kind":"apparatus","title":"Bosma","apparatus_id":PRESS},
                {"id":"lam","kind":"apparatus","title":"Laminatsiya","apparatus_id":LAM},
                {"id":"cut","kind":"apparatus","title":"Rezka","apparatus_id":CUT,"rezka_kadr_count":1},
                {"id":"end","kind":"end","title":"End"}],
            "edges":[{"from":"start","to":"press"},{"from":"press","to":"lam"},{"from":"lam","to":"cut"},{"from":"cut","to":"end"}]
        })).unwrap()).await.unwrap();
        let mut f = Self {
            pool,
            store,
            service,
            order,
            apparatus,
            source_session: String::new(),
            output: String::new(),
            input: String::new(),
        };
        let depth = if final_complete || apparatus == CUT {
            3
        } else if apparatus == LAM {
            2
        } else {
            1
        };
        for (index, (station, node, next, next_node)) in [
            (PRESS, "press", LAM, "lam"),
            (LAM, "lam", CUT, "cut"),
            (CUT, "cut", "", ""),
        ]
        .into_iter()
        .enumerate()
        {
            f.store
                .put_apparatus_sequence(station, vec![f.order.clone()])
                .await
                .unwrap();
            f.store
                .put_apparatus_queue_states(
                    station,
                    [(
                        f.order.clone(),
                        if index < depth {
                            "completed"
                        } else {
                            "pending"
                        }
                        .into(),
                    )]
                    .into(),
                )
                .await
                .unwrap();
            if index >= depth {
                continue;
            }
            let id = format!("run-{}-{node}", f.order);
            let output = format!("output-{}-{node}", f.order);
            let input = if index == 0 {
                String::new()
            } else {
                format!(
                    "output-{}-{}",
                    f.order,
                    if index == 1 { "press" } else { "lam" }
                )
            };
            let mut payload = json!({"stage_work_protocol":1,"input_progress_batch_id":input,
                "input_progress_qr_payload":if input.is_empty(){String::new()}else{format!("qr-{input}")},
                "input_progress_apparatus":if index==1 {PRESS}else if index==2 {LAM}else{""},
                "input_wip_source_kind":if input.is_empty(){""}else{"progress_batch"},
                "stage_work_report":{"report_id":format!("complete-{id}"),"sequence":index+1,"submitted_at_unix":10+index,"worker_ref":actor().ref_,"worker_role":actor().role,"worker_display_name":"Worker"}});
            if !input.is_empty() {
                payload["input_lineage"] = json!([{"input_batch_id":input,"input_qr_payload":format!("qr-{input}"),"source_apparatus":if index==1{PRESS}else{LAM},"source_kind":"progress_batch","stage_node_id":node,"sequence_no":1,"status":"processed","linked_at_unix":1,"processed_at_unix":10+index}]);
            }
            f.store
                .put_order_run_session(OrderRunSession {
                    session_id: id.clone(),
                    apparatus: station.into(),
                    order_id: f.order.clone(),
                    stage_node_id: node.into(),
                    status: OrderRunStatus::Completed,
                    worker_role: actor().role,
                    worker_ref: actor().ref_,
                    worker_display_name: "Worker".into(),
                    started_at_unix: 1 + index as i64,
                    updated_at_unix: 10 + index as i64,
                    payload_json: payload,
                })
                .await
                .unwrap();
            let processed = index + 1 < depth;
            f.store.put_order_progress_batch(serde_json::from_value(json!({
                "batch_id":output,"session_id":id,"started_at_unix":1,"completed_at_unix":10,
                "apparatus":station,"order_id":f.order,"action":"complete","status":"completed",
                "produced_qty":100.0,"uom":"m","qr_payload":format!("qr-{output}"),"label_item_code":"REOPEN","label_item_name":"Film","executor_name":"Worker",
                "worker_role":"aparatchi","worker_ref":actor().ref_,"worker_display_name":"Worker",
                "wip_status":if processed{"processed"}else{"waiting"},"next_apparatus":next,
                "current_apparatus":if processed{next}else{station},"current_location":station,
                "processed_by_session_id":if processed{format!("run-{}-{next_node}",f.order)}else{String::new()},
                "processed_by_apparatus":if processed{next}else{""},"parent_batch_id":input,
                "finished_goods_meter":100.0,"finished_goods_kg":10.0,"bobina_kg":1.0,
                "payload_json":{"stage_node_id":node,"next_stage_node_id":next_node,"contained_kadr_count":1}
            })).unwrap()).await.unwrap();
            if station == LAM {
                // Restoring a physical input requires explicit completion
                // evidence that the printed roll and film still had remainder.
                f.store.put_order_progress_event(serde_json::from_value(json!({
                    "event_id":format!("progress-complete-{id}"),"session_id":id,
                    "batch_id":output,"apparatus":station,"order_id":f.order,
                    "action":"complete","produced_qty":100.0,"uom":"m",
                    "worker_role":"aparatchi","worker_ref":actor().ref_,
                    "worker_display_name":"Worker","lamination_print_leftover_rolls":1.0,
                    "lamination_film_leftover_rolls":1.0,"finished_goods_meter":100.0,
                    "finished_goods_kg":10.0,"bobina_kg":1.0,"total_waste":0.0,"payload_json":{}
                })).unwrap()).await.unwrap();
            }
            f.store
                .append_apparatus_queue_action_event(ApparatusQueueActionEvent {
                    event_id: format!("complete-{id}"),
                    apparatus: station.into(),
                    order_id: f.order.clone(),
                    stage_node_id: node.into(),
                    action: Action::Complete,
                    from_state: queue_state::ApparatusQueueOrderState::InProgress,
                    to_state: queue_state::ApparatusQueueOrderState::Completed,
                    policy: ApparatusQueuePolicy::FreePick,
                    actor: actor(),
                    assigned_apparatus: vec![station.into()],
                    payload_json: json!({}),
                })
                .await
                .unwrap();
            if station == apparatus {
                f.source_session = id;
                f.output = output;
                f.input = input;
            }
        }
        if apparatus == CUT && !final_complete {
            // A final machine can finish one input without completing its operation.
            let mut extra = f.store.progress_batch(&f.input).await.unwrap().unwrap();
            extra.batch_id = format!("extra-{}", f.order);
            extra.qr_payload = format!("qr-{}", extra.batch_id);
            extra.wip_status = OrderProgressBatchWipStatus::Waiting;
            extra.processed_by_apparatus.clear();
            extra.processed_by_session_id.clear();
            f.store.put_order_progress_batch(extra).await.unwrap();
            f.store
                .put_apparatus_queue_states(CUT, [(f.order.clone(), "pending".into())].into())
                .await
                .unwrap();
        }
        f
    }
    fn selector(&self) -> ReactivationSelector {
        ReactivationSelector {
            apparatus: self.apparatus.into(),
            order_id: self.order.clone(),
            batch_id: if self.apparatus == PRESS {
                String::new()
            } else {
                self.output.clone()
            },
        }
    }
    async fn preview(&self) -> ReactivationPreview {
        self.service
            .preview_reactivation(&self.selector(), &[self.apparatus.into()], &actor())
            .await
            .unwrap()
    }
    async fn execute(&self, target: &ReactivationTarget) -> Result<(), ProductionMapError> {
        // Seed with the database owner, but execute every correction using the
        // restricted runtime login that serves real worker requests.
        let runtime_pool = sqlx::PgPool::connect_with(
            self.pool
                .connect_options()
                .as_ref()
                .clone()
                .username("mini_rs_erp"),
        )
        .await
        .unwrap();
        let runtime = ProductionMapService::new(
            Arc::new(PostgresProductionMapStore::new(runtime_pool.clone())),
            Arc::new(CanonicalServiceApparatusResolver::new(
                postgres::canonical_apparatus_service(runtime_pool),
            )),
        );
        runtime
            .reactivate_completed_work(target, &[self.apparatus.into()], &actor(), &[])
            .await
    }
    async fn release(&self) {
        sqlx::query("UPDATE mini_queue_states SET state='paused' WHERE order_id=$1 AND state IN ('in_progress','print_preflight')").bind(&self.order).execute(&self.pool).await.unwrap();
        sqlx::query("UPDATE mini_order_run_sessions SET status='paused' WHERE order_id=$1 AND status='active'").bind(&self.order).execute(&self.pool).await.unwrap();
    }
    async fn resume(&self) {
        self.service.apply_apparatus_queue_action_with_progress(self.apparatus, &self.order,
            Action::Resume, &[self.apparatus.into()], actor(), QueueProgressInput::default()).await.unwrap();
    }
    async fn reclose_target(&self) -> ReactivationRecloseTarget {
        let session = self.store.active_order_run_session(self.apparatus, &self.order).await.unwrap().unwrap();
        ReactivationRecloseTarget { apparatus: self.apparatus.into(), order_id: self.order.clone(),
            session_id: session.session_id, stage_node_id: session.stage_node_id,
            token: session.payload_json["reactivation_token"].as_str().unwrap().into() }
    }
    async fn reclose(&self, target: &ReactivationRecloseTarget) -> Result<(), ProductionMapError> {
        let pool = sqlx::PgPool::connect_with(self.pool.connect_options().as_ref().clone().username("mini_rs_erp")).await.unwrap();
        let service = ProductionMapService::new(Arc::new(PostgresProductionMapStore::new(pool.clone())),
            Arc::new(CanonicalServiceApparatusResolver::new(postgres::canonical_apparatus_service(pool))));
        service.reclose_reactivated_work(target, &[self.apparatus.into()], &actor()).await
    }
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn reactivation_preserves_history_reserves_wip_and_retries_once() {
    for apparatus in [PRESS, LAM, CUT] {
        let f = Fixture::new(apparatus, false).await;
        if apparatus == PRESS {
            // A stale automatic status is recoverable when the real final
            // operation has never closed.
            sqlx::query("UPDATE mini_production_maps SET lifecycle_status='production_completed',production_completed_at=now() WHERE id=$1")
                .bind(&f.order).execute(&f.pool).await.unwrap();
        }
        let preview = f.preview().await;
        assert!(preview.available, "{apparatus}: {preview:?}");
        let target = preview.target.unwrap();
        let runtime_pool = sqlx::PgPool::connect_with(
            f.pool
                .connect_options()
                .as_ref()
                .clone()
                .username("mini_rs_erp"),
        )
        .await
        .unwrap();
        let restarted = ProductionMapService::new(
            Arc::new(PostgresProductionMapStore::new(runtime_pool.clone())),
            Arc::new(CanonicalServiceApparatusResolver::new(
                postgres::canonical_apparatus_service(runtime_pool),
            )),
        );
        let assigned = [apparatus.into()];
        let worker = actor();
        let (first, duplicate) = tokio::join!(
            restarted.reactivate_completed_work(&target, &assigned, &worker, &[]),
            f.execute(&target)
        );
        first.unwrap();
        duplicate.unwrap();
        let active = f
            .store
            .active_order_run_session(apparatus, &f.order)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(active.session_id, f.source_session);
        assert_eq!(active.status, OrderRunStatus::Paused);
        assert_eq!(active.stage_node_id, target.stage_node_id);
        assert_eq!(
            f.service
                .production_order_lifecycle(&f.order)
                .await
                .unwrap()
                .status,
            ProductionOrderLifecycleStatus::InProgress
        );
        assert!(active.payload_json.get("stage_work_report").is_none());
        assert_eq!(
            f.store
                .order_run_session(&f.source_session)
                .await
                .unwrap()
                .unwrap()
                .status,
            OrderRunStatus::Completed
        );
        let output = f.store.progress_batch(&f.output).await.unwrap().unwrap();
        assert_eq!(output.produced_qty, 100.0);
        if apparatus != PRESS {
            if apparatus == CUT {
                assert_eq!(output.wip_status, OrderProgressBatchWipStatus::InUse);
                assert_eq!(output.used_by_session_id, active.session_id);
            } else {
                assert_eq!(output.wip_status, OrderProgressBatchWipStatus::Waiting);
                assert!(output.used_by_session_id.is_empty());
            }
            let input = f.store.progress_batch(&f.input).await.unwrap().unwrap();
            assert_eq!(input.wip_status, OrderProgressBatchWipStatus::InUse);
            assert_eq!(input.used_by_session_id, active.session_id);
        }
        f.execute(&target).await.unwrap();
        let mut tampered = target.clone();
        tampered.stage_node_id = "different-stage".into();
        assert!(f.execute(&tampered).await.is_err());
        assert_eq!(
            f.store
                .active_order_run_session(apparatus, &f.order)
                .await
                .unwrap()
                .unwrap()
                .session_id,
            active.session_id
        );
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM mini_order_progress_events WHERE order_id=$1 AND payload_json->>'event'='reactivation' AND produced_qty=0").bind(&f.order).fetch_one(&f.pool).await.unwrap();
        assert_eq!(count, 1);
        let history = f
            .service
            .completed_queue_orders_for_actor(&actor().ref_, 500)
            .await
            .unwrap();
        assert_eq!(
            history
                .iter()
                .find(|h| h.order_id == f.order && h.apparatus == apparatus)
                .unwrap()
                .status,
            CompletedQueueOrderStatus::InProgress
        );
        f.resume().await;
        f.service
            .apply_apparatus_queue_action_with_progress(
                apparatus,
                &f.order,
                Action::Freeze,
                &[apparatus.into()],
                actor(),
                QueueProgressInput {
                    description: "Safety stop".into(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        if apparatus != PRESS {
            let released = f.store.progress_batch(&f.output).await.unwrap().unwrap();
            assert_eq!(released.wip_status, OrderProgressBatchWipStatus::Waiting);
            assert!(released.used_by_session_id.is_empty());
        }
        // A delayed retry acknowledges the receipt without undoing later work.
        f.execute(&target).await.unwrap();
        assert_eq!(
            f.store
                .order_run_session(&active.session_id)
                .await
                .unwrap()
                .unwrap()
                .status,
            OrderRunStatus::Frozen
        );
        f.release().await;
    }
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn reactivation_rejects_stale_unassigned_busy_and_final_closure() {
    let f = Fixture::new(LAM, false).await;
    let target = f.preview().await.target.unwrap();
    assert!(
        f.service
            .preview_reactivation(&f.selector(), &[], &actor())
            .await
            .is_err()
    );
    sqlx::query("UPDATE mini_progress_batches SET revision=revision+1 WHERE batch_id=$1")
        .bind(&f.input)
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(f.execute(&target).await.is_err());
    f.release().await;
    let f = Fixture::new(CUT, false).await;
    // A downstream worker can release a roll back to Waiting while retaining
    // resumable work. Waiting alone must not authorize producer recovery.
    f.store.put_order_run_session(OrderRunSession {
        session_id: format!("downstream-{}", f.order), apparatus: LAM.into(), order_id: f.order.clone(), stage_node_id: "lam".into(),
        status: OrderRunStatus::RollDetached, worker_role: "aparatchi".into(), worker_ref: "downstream-worker".into(), worker_display_name: "Downstream".into(),
        started_at_unix: 100, updated_at_unix: 100,
        payload_json: json!({"stage_work_protocol":1,"input_progress_batch_id":f.output,"roll_removed_from_apparatus":true}),
    }).await.unwrap();
    assert_eq!(
        f.preview().await.denial_reason.as_deref(),
        Some("source_not_free")
    );
    sqlx::query("UPDATE mini_progress_batches SET wip_status='in_use',used_by_session_id='downstream',used_by_apparatus=$2 WHERE batch_id=$1").bind(&f.output).bind(LAM).execute(&f.pool).await.unwrap();
    assert_eq!(
        f.preview().await.denial_reason.as_deref(),
        Some("source_not_free")
    );
    f.release().await;
    let f = Fixture::new(PRESS, false).await;
    sqlx::query("UPDATE mini_queue_states SET state='print_preflight' WHERE order_id=$1 AND canonical_apparatus_id=$2").bind(&f.order).bind(PRESS).execute(&f.pool).await.unwrap();
    assert_eq!(
        f.preview().await.denial_reason.as_deref(),
        Some("apparatus_busy")
    );
    f.release().await;
    let f = Fixture::new(CUT, true).await;
    assert_eq!(
        f.preview().await.denial_reason.as_deref(),
        Some("final_stage_closed")
    );
    for apparatus in [PRESS, LAM] {
        let selector = ReactivationSelector {
            apparatus: apparatus.into(),
            order_id: f.order.clone(),
            batch_id: if apparatus == PRESS {
                String::new()
            } else {
                format!("output-{}-lam", f.order)
            },
        };
        assert_eq!(
            f.service
                .preview_reactivation(&selector, &[apparatus.into()], &actor())
                .await
                .unwrap()
                .denial_reason
                .as_deref(),
            Some("final_stage_closed")
        );
    }
    f.release().await;
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn reactivation_compensates_material_consumption_without_duplicate_stock() {
    use mini_rs_erp::db::postgres_raw_material_events::{
        RawMaterialEventDraft, insert_raw_material_event_tx,
    };
    let f = Fixture::new(LAM, false).await;
    let stock_id = format!("raw-{}", f.order);
    let barcode = format!("RAW-{}", f.order.to_uppercase());
    sqlx::query("INSERT INTO mini_item_groups(name,parent_item_group,is_group,payload_json) VALUES('film','All Item Groups',false,'{}') ON CONFLICT(name) DO NOTHING")
        .execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO mini_items(code,name,uom,item_group,payload_json) VALUES('FILM','Film','kg','film','{}') ON CONFLICT(code) DO NOTHING")
        .execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty,uom,status,reserved_order_id,source_receipt_id,payload_json) VALUES($1,'Reactivation warehouse','FILM','Film',$2,11,'kg','consumed',$3,$1,'{}')")
        .bind(&stock_id).bind(&barcode).bind(&f.order).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_assignments(barcode,order_id,apparatus,canonical_apparatus_id,item_code,item_group,payload_json) VALUES($1,$2,$3,$3,'FILM','film',$4)")
        .bind(&barcode).bind(&f.order).bind(LAM)
        .bind(json!({"barcode":barcode,"order_id":f.order,"apparatus_id":LAM,"apparatus":LAM,"item_code":"FILM","item_name":"Film","item_group":"film","assigned_by_role":"aparatchi","assigned_by_ref":actor().ref_,"assigned_by_display_name":"Worker","assigned_at":"2026-10-18"}))
        .execute(&f.pool).await.unwrap();
    sqlx::query("UPDATE mini_order_run_sessions SET started_at=now()-interval '1 second',updated_at=now(),payload_json=jsonb_set(payload_json,'{start_resources}',$2) WHERE session_id=$1")
        .bind(&f.source_session).bind(json!({"session_id":f.source_session,"raw_materials":[{"barcode":barcode,"stock_id":stock_id,"source_qty":11.0,"uom":"kg","item_code":"FILM","item_name":"Film"}],"raw_materials_available":true,"qolip_codes":[],"qolip_available":false}))
        .execute(&f.pool).await.unwrap();
    let draft = RawMaterialEventDraft {
        idempotency_key: format!("consumption_posted:{barcode}:{}:{LAM}", f.order),
        event_type: "consumption_posted".into(),
        warehouse: "Reactivation warehouse".into(),
        barcode: barcode.clone(),
        item_code: "FILM".into(),
        item_name: "Film".into(),
        qty_delta: -11.0,
        uom: "kg".into(),
        stock_status_before: Some("in_use".into()),
        stock_status_after: Some("consumed".into()),
        order_id: Some(f.order.clone()),
        apparatus: Some(LAM.into()),
        actor_role: actor().role,
        actor_ref: actor().ref_,
        actor_display_name: actor().display_name,
        owner_role: String::new(),
        owner_ref: String::new(),
        owner_display_name: String::new(),
        source_type: "consumption".into(),
        source_id: f.order.clone(),
        source_line_ref: Some(barcode.clone()),
        correlation_id: None,
        payload_json: json!({"stock_id":stock_id}),
    };
    let mut tx = f.pool.begin().await.unwrap();
    insert_raw_material_event_tx(&mut tx, draft.clone())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let preview = f.preview().await;
    assert!(preview.available, "{preview:?}");
    let target = preview.target.unwrap();
    f.execute(&target).await.unwrap();
    f.execute(&target).await.unwrap();
    let state: (String, f64) =
        sqlx::query_as("SELECT status,qty::float8 FROM mini_raw_material_stock WHERE id=$1")
            .bind(&stock_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(state, ("in_use".into(), 11.0));
    let ledger:(i64,f64)=sqlx::query_as("SELECT count(*)::bigint,sum(qty_delta)::float8 FROM mini_raw_material_events WHERE barcode=$1 AND event_type IN ('consumption_posted','consumption_reversed')").bind(&barcode).fetch_one(&f.pool).await.unwrap();
    assert_eq!(ledger, (2, 0.0));
    let close = f.reclose_target().await;
    f.reclose(&close).await.unwrap();
    f.reclose(&close).await.unwrap();
    let ledger:(i64,f64)=sqlx::query_as("SELECT count(*)::bigint,sum(qty_delta)::float8 FROM mini_raw_material_events WHERE barcode=$1 AND event_type IN ('consumption_posted','consumption_reversed')").bind(&barcode).fetch_one(&f.pool).await.unwrap();
    assert_eq!(ledger, (3, -11.0));
    let next = f.preview().await;
    assert!(next.available, "{next:?}");
    f.execute(&next.target.unwrap()).await.unwrap();
    f.resume().await;
    // The next ordinary settlement uses the compensation-aware stable key.
    let mut tx = f.pool.begin().await.unwrap();
    sqlx::query("UPDATE mini_raw_material_stock SET status='consumed' WHERE id=$1")
        .bind(&stock_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    insert_raw_material_event_tx(&mut tx, draft).await.unwrap();
    tx.commit().await.unwrap();
    let ledger:(i64,f64)=sqlx::query_as("SELECT count(*)::bigint,sum(qty_delta)::float8 FROM mini_raw_material_events WHERE barcode=$1 AND event_type IN ('consumption_posted','consumption_reversed')").bind(&barcode).fetch_one(&f.pool).await.unwrap();
    assert_eq!(ledger, (5, -11.0));
    f.release().await;
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn continued_completion_records_only_additional_output_and_can_reopen_again() {
    let f = Fixture::new(LAM, false).await;
    let target = f.preview().await.target.unwrap();
    f.execute(&target).await.unwrap();
    f.resume().await;
    let result = f
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
    let new_batch = result.progress_batch.unwrap();
    assert_eq!(new_batch.produced_qty, 25.0);
    assert_ne!(new_batch.batch_id, f.output);
    let old = f.store.progress_batch(&f.output).await.unwrap().unwrap();
    assert_eq!(old.produced_qty, 100.0);
    assert_eq!(old.wip_status, OrderProgressBatchWipStatus::Waiting);
    let total:f64=sqlx::query_scalar("SELECT sum(produced_qty)::float8 FROM mini_progress_batches WHERE order_id=$1 AND canonical_apparatus_id=$2")
        .bind(&f.order).bind(LAM).fetch_one(&f.pool).await.unwrap();
    assert_eq!(total, 125.0);
    let completed = result.session.unwrap();
    assert!(completed.updated_at_unix >= completed.started_at_unix);
    let stored = f
        .store
        .order_run_session(&completed.session_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        stored
            .payload_json
            .get("_completion_reactivation_payload")
            .is_some()
    );
    assert!(
        stored
            .payload_json
            .get("_completion_queue_event_id")
            .is_some()
    );
    let preview = f
        .service
        .preview_reactivation(
            &ReactivationSelector {
                apparatus: LAM.into(),
                order_id: f.order.clone(),
                batch_id: new_batch.batch_id,
            },
            &[LAM.into()],
            &actor(),
        )
        .await
        .unwrap();
    assert!(preview.available, "{preview:?}");
    f.execute(&preview.target.unwrap()).await.unwrap();
    f.release().await;
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn opening_wip_reactivation_validates_intake_status_and_exact_route() {
    let f = Fixture::new(LAM, false).await;
    let opening = f
        .service
        .create_opening_wip(
            OpeningWipCreateInput {
                idempotency_key: format!("opening-{}", f.order),
                order_id: f.order.clone(),
                entry_apparatus: LAM.into(),
                source_operation: "print".into(),
                source_apparatus: PRESS.into(),
                source_stage_node_id: "press".into(),
                current_location: PRESS.into(),
                note: String::new(),
                batches: vec![OpeningWipBatchInput {
                    quantity_basis: OpeningWipQuantityBasis::Measured,
                    finished_goods_meter: Some(100.0),
                    finished_goods_kg: Some(10.0),
                    bobina_kg: Some(1.0),
                    diameter: None,
                }],
            },
            QueueActionActor {
                role: "admin".into(),
                ref_: "admin".into(),
                display_name: "Admin".into(),
            },
        )
        .await
        .unwrap();
    let batch = &opening.batches[0];
    sqlx::query("UPDATE mini_opening_wip_batches SET wip_status='processed',processed_by_session_id=$2,processed_by_apparatus=$3 WHERE batch_id=$1")
        .bind(&batch.batch_id).bind(&f.source_session).bind(LAM).execute(&f.pool).await.unwrap();
    let mut session = f
        .store
        .order_run_session(&f.source_session)
        .await
        .unwrap()
        .unwrap();
    session.payload_json["input_progress_batch_id"] = json!(batch.batch_id);
    session.payload_json["input_progress_qr_payload"] = json!(batch.qr_payload);
    session.payload_json["input_wip_source_kind"] = json!("opening_wip");
    session.payload_json["input_lineage"] = json!([{"input_batch_id":batch.batch_id,"input_qr_payload":batch.qr_payload,"source_apparatus":PRESS,"source_kind":"opening_wip","stage_node_id":"lam","sequence_no":1,"status":"processed","linked_at_unix":1,"processed_at_unix":11}]);
    f.store.put_order_run_session(session).await.unwrap();
    let preview = f.preview().await;
    assert!(preview.available, "{preview:?}");
    let target = preview.target.unwrap();
    sqlx::query("UPDATE mini_opening_wip_intakes SET status='cancelled' WHERE intake_id=$1")
        .bind(&opening.intake.intake_id)
        .execute(&f.pool)
        .await
        .unwrap();
    assert_eq!(
        f.preview().await.denial_reason.as_deref(),
        Some("source_not_free")
    );
    assert!(f.execute(&target).await.is_err());
    sqlx::query("UPDATE mini_opening_wip_intakes SET status='confirmed',resume_stage_node_id='missing-stage' WHERE intake_id=$1").bind(&opening.intake.intake_id).execute(&f.pool).await.unwrap();
    assert_eq!(
        f.preview().await.denial_reason.as_deref(),
        Some("stale_target")
    );
    sqlx::query("UPDATE mini_opening_wip_intakes SET resume_stage_node_id=$2 WHERE intake_id=$1")
        .bind(&opening.intake.intake_id)
        .bind(&opening.intake.resume_stage_node_id)
        .execute(&f.pool)
        .await
        .unwrap();
    f.execute(&f.preview().await.target.unwrap()).await.unwrap();
    let restored = f
        .service
        .opening_wip_batch(&batch.batch_id, "")
        .await
        .unwrap();
    assert_eq!(restored.batch.wip_status, OpeningWipBatchStatus::InUse);
    assert_ne!(restored.batch.used_by_session_id, f.source_session);
    f.reclose(&f.reclose_target().await).await.unwrap();
    let closed = f.service.opening_wip_batch(&batch.batch_id, "").await.unwrap();
    assert_eq!(closed.batch.wip_status, OpeningWipBatchStatus::Processed);
    assert_eq!(closed.batch.processed_by_session_id, f.source_session);
    f.release().await;
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn cutting_continuation_records_frames_and_releases_previous_output_once() {
    let f = Fixture::new(CUT, false).await;
    f.execute(&f.preview().await.target.unwrap()).await.unwrap();
    f.resume().await;
    let session = f
        .store
        .active_order_run_session(CUT, &f.order)
        .await
        .unwrap()
        .unwrap();
    let cycle = session.payload_json["rezka_output_cycle"]
        .as_str()
        .unwrap()
        .to_owned();
    let frame = serde_json::from_value(
        json!({"produced_qty":25.0,"gross_qty":2.5,"bobina_kg":0.5,"diameter":45.0}),
    )
    .unwrap();
    let single = QueueProgressInput {
        rezka_record_frame_index: Some(1),
        rezka_output_cycle: cycle.clone(),
        rezka_frames: vec![frame],
        ..Default::default()
    };
    let recorded = f
        .service
        .apply_apparatus_queue_action_with_progress(
            CUT,
            &f.order,
            Action::RollComplete,
            &[CUT.into()],
            actor(),
            single.clone(),
        )
        .await
        .unwrap();
    assert_eq!(recorded.progress_batch.unwrap().produced_qty, 25.0);
    assert_eq!(
        f.store
            .progress_batch(&f.output)
            .await
            .unwrap()
            .unwrap()
            .wip_status,
        OrderProgressBatchWipStatus::InUse
    );
    let closing = QueueProgressInput {
        rezka_record_frame_index: None,
        ..single
    };
    f.service
        .apply_apparatus_queue_action_with_progress(
            CUT,
            &f.order,
            Action::RollComplete,
            &[CUT.into()],
            actor(),
            closing,
        )
        .await
        .unwrap();
    assert_eq!(
        f.store
            .progress_batch(&f.output)
            .await
            .unwrap()
            .unwrap()
            .wip_status,
        OrderProgressBatchWipStatus::Waiting
    );
    let total:(i64,f64)=sqlx::query_as("SELECT count(*)::bigint,sum(produced_qty)::float8 FROM mini_progress_batches WHERE order_id=$1 AND canonical_apparatus_id=$2").bind(&f.order).bind(CUT).fetch_one(&f.pool).await.unwrap();
    assert_eq!(total, (2, 125.0));
    f.release().await;
}

#[tokio::test]
#[ignore = "requires REACTIVATION_TEST_DATABASE_URL on a disposable server"]
async fn http_reactivation_retry_after_another_completion_acknowledges_original_receipt() {
    use mini_rs_erp::{app::AppState, config::AppConfig,
        core::{auth::models::{Principal, PrincipalRole}, authz::RoleAssignmentUpsert},
        http::router::build_router};
    use axum::{body::Body, http::{Request, StatusCode}};
    use tower::ServiceExt;
    let f = Fixture::new(PRESS, false).await;
    let dir = tempfile::tempdir().unwrap();
    struct RestoreDirectory(std::path::PathBuf);
    impl Drop for RestoreDirectory {
        fn drop(&mut self) { std::env::set_current_dir(&self.0).unwrap(); }
    }
    let _restore = RestoreDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(dir.path()).unwrap();
    let mut state = AppState::verification(AppConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(), default_target_warehouse: String::new(),
        http_timeout: std::time::Duration::from_secs(5),
        session_store_path: dir.path().join("sessions"), profile_store_path: dir.path().join("profiles"),
        push_token_store_path: dir.path().join("push"), session_ttl_seconds: Some(3600),
        supplier_prefix: "10".into(), werka_prefix: "20".into(), werka_code: String::new(),
        werka_name: String::new(), werka_phone: String::new(), material_taminotchi_code: String::new(),
        material_taminotchi_name: String::new(), material_taminotchi_phone: String::new(),
        admin_phone: String::new(), admin_name: String::new(), admin_code: String::new(),
    });
    state.apparatus = postgres::canonical_apparatus_service(f.pool.clone());
    state.production_maps = f.service.clone();
    state.admin.upsert_role_assignment(RoleAssignmentUpsert {
        principal_role: PrincipalRole::Aparatchi, principal_ref: actor().ref_,
        role_id: "aparatchi".into(), assigned_apparatus: vec![PRESS.into()], assigned_item_groups: vec![],
    }).await.unwrap();
    let token = state.sessions.create(Principal {
        role: PrincipalRole::Aparatchi, ref_: actor().ref_, display_name: "Worker".into(),
        legal_name: String::new(), avatar_url: String::new(), phone: String::new(),
    }).await.unwrap();
    let router = build_router(state);
    let target = f.preview().await.target.unwrap();
    let post = |body: &ReactivationTarget| Request::builder().method("POST")
        .uri("/v1/mobile/admin/production-maps/reactivation")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap())).unwrap();
    assert_eq!(router.clone().oneshot(post(&target)).await.unwrap().status(), StatusCode::OK);
    let reclose = f.reclose_target().await;
    let reclose_post = |payload: serde_json::Value| Request::builder().method("POST")
        .uri("/v1/mobile/admin/production-maps/reactivation/reclose")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap())).unwrap();
    let mut invalid = json!(reclose);
    invalid["total_waste"] = json!(0);
    assert_eq!(router.clone().oneshot(reclose_post(invalid)).await.unwrap().status(), StatusCode::BAD_REQUEST);
    assert_eq!(router.clone().oneshot(reclose_post(json!(reclose))).await.unwrap().status(), StatusCode::OK);
    let reopened = f.preview().await.target.unwrap();
    assert_eq!(router.clone().oneshot(post(&reopened)).await.unwrap().status(), StatusCode::OK);
    // An old reclose receipt cannot close this newer continuation.
    assert_eq!(router.clone().oneshot(reclose_post(json!(reclose))).await.unwrap().status(), StatusCode::OK);
    f.resume().await;
    f.service.apply_apparatus_queue_action_with_progress(PRESS, &f.order, Action::Complete,
        &[PRESS.into()], actor(), QueueProgressInput {
            produced_qty: Some(25.0), uom: "m".into(), finished_goods_meter: Some(25.0),
            finished_goods_kg: Some(2.5), bobina_kg: Some(0.5), return_ink_kg: Some(0.5),
            total_waste: Some(1.0), ..Default::default()
        }).await.unwrap();
    let next = f.preview().await;
    assert!(next.available, "{next:?}");
    assert_ne!(next.target.unwrap().token, target.token);
    let before = json!({
        "sessions": f.store.order_run_sessions_for_order(&f.order).await.unwrap(),
        "batches": f.store.progress_batches_for_order(&f.order).await.unwrap(),
        "events": f.service.queue_action_logs_for_order(&f.order).await.unwrap(),
    });
    assert_eq!(router.clone().oneshot(post(&target)).await.unwrap().status(), StatusCode::OK);
    for field in ["stage", "session", "batch", "token"] {
        let mut tampered = target.clone();
        match field {
            "stage" => tampered.stage_node_id = "different-stage".into(),
            "session" => tampered.session_id = "different-session".into(),
            "batch" => tampered.batch_id = "different-batch".into(),
            _ => tampered.token = "0".repeat(64),
        }
        assert!(!router.clone().oneshot(post(&tampered)).await.unwrap().status().is_success(), "{field}");
    }
    assert_eq!(before, json!({
        "sessions": f.store.order_run_sessions_for_order(&f.order).await.unwrap(),
        "batches": f.store.progress_batches_for_order(&f.order).await.unwrap(),
        "events": f.service.queue_action_logs_for_order(&f.order).await.unwrap(),
    }));
    f.release().await;
}
