#![cfg(feature = "verification")]
//! Every mutation runs against an explicitly supplied disposable database.
use mini_rs_erp::{
    core::{apparatus_standard::*, production_map::*},
    db::{postgres, postgres_production_map::PostgresProductionMapStore},
};
use queue_state::{ApparatusQueueAction as Action, ApparatusQueueOrderState as State};
#[path = "cancel_start/http.rs"]
mod http;
#[path = "cancel_start/resources.rs"]
mod resources;
use serde_json::{Value, json};
use std::sync::Arc;

const PRESS: &str = "apparatus:default:bosma_9";
const MACHINES: [&str; 4] = [
    "apparatus:default:asset-010",
    "apparatus:default:asset-007",
    "apparatus:default:asset-004",
    "apparatus:default:holodniy_kley",
];
fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "cancel-worker".into(),
        display_name: "Worker".into(),
    }
}

async fn pool() -> sqlx::PgPool {
    static DATABASE: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();
    let url = std::env::var("CANCEL_START_TEST_DATABASE_URL")
        .expect("explicit disposable PostgreSQL URL");
    let database = DATABASE
        .get_or_init(|| async {
            let admin = sqlx::PgPool::connect(&url).await.unwrap();
            let name = format!("cancel_start_{:032x}", rand::random::<u128>());
            sqlx::query(&format!("CREATE DATABASE {name}"))
                .execute(&admin)
                .await
                .unwrap();
            let pool = sqlx::PgPool::connect_with(
                url.parse::<sqlx::postgres::PgConnectOptions>()
                    .unwrap()
                    .database(&name),
            )
            .await
            .unwrap();
            postgres::apply_postgres_migrations_through_version(&pool, "0121")
                .await
                .unwrap();
            let canonical = postgres::canonical_apparatus_service(pool.clone());
            canonical.bootstrap_factory_defaults().await.unwrap();
            postgres::apply_foundation_migration(&pool).await.unwrap();
            sqlx::raw_sql("INSERT INTO mini_item_groups(name,parent_item_group) VALUES('film','All Item Groups') ON CONFLICT DO NOTHING;
                INSERT INTO mini_items(code,name,item_group) VALUES('FILM','Film','film') ON CONFLICT DO NOTHING;")
                .execute(&pool).await.unwrap();
            for machine in MACHINES {
                let id = ApparatusId::new(machine).unwrap();
                let current = canonical.current_configuration(&id).await.unwrap().unwrap();
                canonical
                    .patch(
                        id,
                        current.runtime.source_revision,
                        CanonicalApparatusPatch {
                            policies: Some(ApparatusOperationalPolicies {
                                queue: QueueDiscipline::FreePick,
                                material: MaterialExecutionPolicy::NotRequired {
                                    item_group_ids: vec![],
                                },
                                tooling: ToolingExecutionPolicy::NotRequired,
                                worker_material_assignment_allowed: false,
                            }),
                            ..Default::default()
                        },
                        CanonicalCommandMetadata::new("test", format!("cancel-start-{machine}")),
                    )
                    .await
                    .unwrap();
            }
            pool.close().await;
            admin.close().await;
            name
        })
        .await;
    sqlx::PgPool::connect_with(
        url.parse::<sqlx::postgres::PgConnectOptions>()
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
    machine: String,
    original: OrderProgressBatch,
}
impl Fixture {
    async fn new(machine: &str) -> Self {
        let pool = pool().await;
        let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
        let service = ProductionMapService::new(
            store.clone(),
            Arc::new(CanonicalServiceApparatusResolver::new(
                postgres::canonical_apparatus_service(pool.clone()),
            )),
        );
        let order = format!("zakaz-cancel-{:032x}", rand::random::<u128>());
        service.upsert_map(serde_json::from_value(json!({"id":order,"title":order,"product_code":"CANCEL","roll_count":2,"width_mm":650,
            "nodes":[{"id":"start","kind":"start","title":"Start"},
                {"id":"print","kind":"apparatus","title":"Print","apparatus_id":PRESS},
                {"id":"work","kind":"apparatus","title":"Work","apparatus_id":machine,"rezka_kadr_count":2},
                {"id":"end","kind":"end","title":"End"}],
            "edges":[{"from":"start","to":"print"},{"from":"print","to":"work"},{"from":"work","to":"end"}]})).unwrap()).await.unwrap();
        store
            .put_apparatus_sequence(machine, vec![order.clone()])
            .await
            .unwrap();
        let original:OrderProgressBatch=serde_json::from_value(json!({
            "batch_id":format!("source-{order}"),"session_id":"producer","started_at_unix":100,"completed_at_unix":110,
            "apparatus":PRESS,"order_id":order,"action":"roll_complete","status":"completed","produced_qty":100,
            "uom":"m","qr_payload":format!("QR-{order}"),"label_item_code":"FILM","label_item_name":"Original roll",
            "executor_name":"Printer","worker_role":"aparatchi","worker_ref":"printer","worker_display_name":"Printer",
            "wip_status":"waiting","current_apparatus":PRESS,"current_location":"Original place","next_apparatus":machine,
            "payload_json":{"stage_node_id":"print","next_stage_node_id":"work","contained_kadr_count":2,"original":"preserve"}
        })).unwrap();
        store
            .put_order_progress_batch(original.clone())
            .await
            .unwrap();
        Self {
            pool,
            store,
            service,
            order,
            machine: machine.into(),
            original,
        }
    }
    async fn start(&self) -> CancelStartControl {
        self.service
            .apply_apparatus_queue_action_with_progress(
                &self.machine,
                &self.order,
                Action::Start,
                &[self.machine.clone()],
                actor(),
                QueueProgressInput {
                    qr_payload: self.original.qr_payload.clone(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        self.control()
            .await
            .expect("a fresh WIP Start has rollback authority")
    }
    async fn control(&self) -> Option<CancelStartControl> {
        self.service
            .queue_action_controls_for_apparatus(&self.machine)
            .await
            .unwrap()
            .get(&self.machine)
            .and_then(|orders| orders.get(&self.order))
            .and_then(|c| c.cancel_start.clone())
    }
    async fn cancel(&self, c: &CancelStartControl) -> Result<(), ProductionMapError> {
        self.service
            .cancel_started_work(
                &self.machine,
                &self.order,
                &c.session_id,
                &c.token,
                &[self.machine.clone()],
                actor(),
            )
            .await
    }
    async fn batch(&self) -> OrderProgressBatch {
        self.store
            .progress_batch(&self.original.batch_id)
            .await
            .unwrap()
            .unwrap()
    }
    async fn release(&self) {
        self.store
            .put_apparatus_queue_states(
                &self.machine,
                [(self.order.clone(), "paused".into())].into(),
            )
            .await
            .unwrap();
        sqlx::query("UPDATE mini_order_run_sessions SET status='paused' WHERE order_id=$1")
            .bind(&self.order)
            .execute(&self.pool)
            .await
            .unwrap();
    }
}

#[tokio::test]
#[ignore = "requires CANCEL_START_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn all_wip_operators_can_undo_only_their_untouched_start_and_retry_safely() {
    for machine in MACHINES {
        let f = Fixture::new(machine).await;
        let c = f.start().await;
        let held = f.batch().await;
        assert_eq!(held.wip_status, OrderProgressBatchWipStatus::InUse);
        let mut stranger = actor();
        stranger.ref_ = "other-worker".into();
        assert!(
            f.service
                .cancel_started_work(
                    machine,
                    &f.order,
                    &c.session_id,
                    &c.token,
                    &[machine.into()],
                    stranger
                )
                .await
                .is_err()
        );
        assert!(
            f.service
                .cancel_started_work(machine, &f.order, &c.session_id, &c.token, &[], actor())
                .await
                .is_err()
        );
        for print in [PRESS, "apparatus:default:asset-005"] {
            assert!(
                f.service
                    .cancel_started_work(
                        print,
                        &f.order,
                        &c.session_id,
                        &c.token,
                        &[print.into()],
                        actor()
                    )
                    .await
                    .is_err()
            );
        }
        assert_eq!(f.batch().await, held);
        let (a, b) = tokio::join!(f.cancel(&c), f.cancel(&c));
        a.unwrap();
        b.unwrap();
        let restored = f.batch().await;
        assert_eq!(restored.wip_status, OrderProgressBatchWipStatus::Waiting);
        assert_eq!(restored.qr_payload, f.original.qr_payload);
        assert_eq!(restored.produced_qty, f.original.produced_qty);
        assert_eq!(restored.current_location, f.original.current_location);
        assert_eq!(restored.current_apparatus, f.original.current_apparatus);
        assert_eq!(restored.payload_json, f.original.payload_json);
        assert!(restored.used_by_session_id.is_empty());
        assert!(restored.revision > held.revision);
        assert!(
            f.store
                .active_order_run_session(machine, &f.order)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            f.store.apparatus_queue_states().await.unwrap()[machine][&f.order],
            "pending"
        );
        let fresh = f.start().await;
        f.cancel(&c).await.unwrap(); // Receipt acknowledgement must not touch the fresh claim.
        assert_eq!(f.batch().await.used_by_session_id, fresh.session_id);
        assert_eq!(f.control().await, Some(fresh.clone()));
        f.cancel(&fresh).await.unwrap();
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM mini_queue_action_events WHERE order_id=$1 AND payload_json->>'event'='cancel_start'")
            .bind(&f.order).fetch_one(&f.pool).await.unwrap(),2);
    }
}

#[tokio::test]
#[ignore = "requires CANCEL_START_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn later_work_stale_resources_and_missing_checkpoints_cannot_be_undone() {
    for change in ["session", "output", "input", "waste", "freeze", "legacy"] {
        let f = Fixture::new(MACHINES[0]).await;
        let c = f.start().await;
        match change {
            "session" => {
                sqlx::query("UPDATE mini_order_run_sessions SET payload_json=payload_json||'{\"total_waste\":1}' WHERE session_id=$1")
                .bind(&c.session_id).execute(&f.pool).await.unwrap();
            }
            "output" => {
                let mut output = f.original.clone();
                output.batch_id = format!("output-{}", f.order);
                output.session_id = c.session_id.clone();
                output.qr_payload = format!("OUT-{}", f.order);
                f.store.put_order_progress_batch(output).await.unwrap();
            }
            "input" => {
                sqlx::query("UPDATE mini_progress_batches SET revision=revision+1,description='correction' WHERE batch_id=$1")
                .bind(&f.original.batch_id).execute(&f.pool).await.unwrap();
            }
            "freeze" => {
                sqlx::query("INSERT INTO mini_order_control_states(order_id,state,frozen_at_unix) VALUES($1,'frozen',1) ON CONFLICT(order_id) DO UPDATE SET state='frozen',frozen_at_unix=1")
                .bind(&f.order).execute(&f.pool).await.unwrap();
            }
            "waste" => {
                f.store
                    .put_rezka_astatka_report(RezkaAstatkaReport {
                        report_id: f.order.clone(),
                        order_id: f.order.clone(),
                        apparatus: f.machine.clone(),
                        from_at_unix: 100,
                        to_at_unix: 110,
                        total_waste: 1.0,
                        rezka_bosma_waste: 1.0,
                        rezka_lamination_waste: 0.0,
                        rezka_edge_waste: 0.0,
                        finished_goods_meter: None,
                        finished_goods_kg: None,
                        bobina_kg: None,
                        worker_role: actor().role,
                        worker_ref: actor().ref_,
                        worker_display_name: actor().display_name,
                        description: String::new(),
                        created_at_unix: 111,
                    })
                    .await
                    .unwrap();
            }
            _ => {
                sqlx::query("UPDATE mini_order_run_sessions SET payload_json=payload_json-'_cancel_start' WHERE session_id=$1")
                .bind(&c.session_id).execute(&f.pool).await.unwrap();
            }
        }
        let before = f.batch().await;
        assert!(
            f.cancel(&c).await.is_err(),
            "{change} must invalidate cancellation"
        );
        assert_eq!(
            f.batch().await,
            before,
            "rejection must not partially release WIP"
        );
        assert_eq!(
            f.store.apparatus_queue_states().await.unwrap()[&f.machine][&f.order],
            "in_progress"
        );
        f.release().await;
    }
}
