//! Uses a disposable PostgreSQL cluster with the `mini_rs_erp` runtime role.
use std::{collections::BTreeMap, sync::Arc};

use mini_rs_erp::{
    core::production_map::{queue_state::ApparatusQueueAction as Action, *},
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

const PRINT: &str = "apparatus:default:bosma_8";
const OTHER_PRINT: &str = "apparatus:default:bosma_7";
const LAM: &str = "apparatus:default:asset-007";
const ORDER: &str = "zakaz-qolip-scope";
const QR: &str = "400118DC12D6BE0591C9F010";

struct Fixture {
    admin: PgPool,
    pool: PgPool,
    database: String,
    store: Arc<PostgresProductionMapStore>,
    service: ProductionMapService,
}

impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .expect("explicit disposable PostgreSQL admin URL");
        let database = format!("qolip_lock_scope_{:032x}", rand::random::<u128>());
        let admin = PgPool::connect(&url).await.unwrap();
        sqlx::query(&format!("CREATE DATABASE \"{database}\""))
            .execute(&admin)
            .await
            .unwrap();
        let pool =
            PgPool::connect_with(url.parse::<PgConnectOptions>().unwrap().database(&database))
                .await
                .unwrap();
        apply_postgres_migrations_through_version(&pool, "0121")
            .await
            .unwrap();
        let canonical = canonical_apparatus_service(pool.clone());
        canonical.bootstrap_factory_defaults().await.unwrap();
        apply_foundation_migration(&pool).await.unwrap();
        let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
        let service = ProductionMapService::new(
            store.clone(),
            Arc::new(CanonicalServiceApparatusResolver::new(canonical)),
        );
        store.put_map(map(ORDER, PRINT)).await.unwrap();
        store
            .put_apparatus_sequence(LAM, vec![ORDER.into()])
            .await
            .unwrap();
        Self {
            admin,
            pool,
            database,
            store,
            service,
        }
    }

    async fn seed_print_wip(&self, status: OrderRunStatus, set_id: &str) -> OrderRunSession {
        let source = print_session("print-session", PRINT, ORDER, status, set_id);
        self.store
            .put_order_run_session(source.clone())
            .await
            .unwrap();
        self.store
            .put_apparatus_queue_states(
                PRINT,
                BTreeMap::from([(ORDER.into(), "in_progress".into())]),
            )
            .await
            .unwrap();
        let mut batch: OrderProgressBatch = serde_json::from_value(json!({
            "batch_id":"print-wip", "session_id":source.session_id,
            "started_at_unix":1, "completed_at_unix":2,
            "apparatus":PRINT, "order_id":ORDER, "action":"detach_roll",
            "status":"roll_detached", "produced_qty":8300, "uom":"m", "qr_payload":QR,
            "label_item_code":"PRODUCT", "label_item_name":"Printed product",
            "executor_name":"Printer", "worker_role":"aparatchi", "worker_ref":"print-worker",
            "worker_display_name":"Printer", "wip_status":"waiting",
            "current_apparatus":PRINT, "current_location":"Print output", "next_apparatus":LAM,
            "payload_json":{
                "stage_node_id":"print", "next_stage_node_id":"lam",
                "qolip_set_id":set_id, "qolip_code":"22091058-1", "qolip_codes":codes()
            }
        }))
        .unwrap();
        batch.refresh_status_detail();
        self.store.put_order_progress_batch(batch).await.unwrap();
        source
    }

    async fn start_lamination(&self) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        self.service
            .apply_apparatus_queue_action_with_progress(
                LAM,
                ORDER,
                Action::Start,
                &[LAM.into()],
                actor("lam-worker"),
                QueueProgressInput {
                    qr_payload: QR.into(),
                    ..Default::default()
                },
            )
            .await
    }

    async fn cleanup(self) {
        drop(self.service);
        drop(self.store);
        self.pool.close().await;
        sqlx::query(&format!("DROP DATABASE \"{}\" WITH (FORCE)", self.database))
            .execute(&self.admin)
            .await
            .unwrap();
        self.admin.close().await;
    }
}

fn codes() -> Vec<String> {
    (1..=8).map(|n| format!("22091058-{n}")).collect()
}

fn actor(id: &str) -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: id.into(),
        display_name: id.into(),
    }
}

fn map(id: &str, printer: &str) -> ProductionMapDefinition {
    serde_json::from_value(json!({
        "id":id, "product_code":"PRODUCT", "title":"Qolip scope regression",
        "nodes":[
            {"id":"start","kind":"start","title":"Start"},
            {"id":"print","kind":"apparatus","title":"Bosma","apparatus_id":printer},
            {"id":"lam","kind":"apparatus","title":"Laminatsiya 1","apparatus_id":LAM},
            {"id":"end","kind":"end","title":"End"}
        ],
        "edges":[{"from":"start","to":"print"},{"from":"print","to":"lam"},{"from":"lam","to":"end"}]
    })).unwrap()
}

fn print_session(
    id: &str,
    apparatus: &str,
    order: &str,
    status: OrderRunStatus,
    set: &str,
) -> OrderRunSession {
    OrderRunSession {
        session_id: id.into(),
        apparatus: apparatus.into(),
        order_id: order.into(),
        stage_node_id: "print".into(),
        status,
        worker_role: "aparatchi".into(),
        worker_ref: "print-worker".into(),
        worker_display_name: "Printer".into(),
        started_at_unix: 1,
        updated_at_unix: 2,
        payload_json: json!({"qolip_lock_owner":true,"qolip_set_id":set,"qolip_code":"22091058-1","qolip_codes":codes()}),
    }
}

fn print_start_write(session: OrderRunSession) -> QueueActionProgressWrite {
    QueueActionProgressWrite {
        apparatus: session.apparatus.clone(),
        map_update: None,
        states: BTreeMap::from([(session.order_id.clone(), "in_progress".into())]),
        sequence_updates: BTreeMap::new(),
        event: ApparatusQueueActionEvent {
            event_id: format!("start:{}", session.session_id),
            apparatus: session.apparatus.clone(),
            order_id: session.order_id.clone(),
            stage_node_id: session.stage_node_id.clone(),
            action: Action::Start,
            from_state: queue_state::ApparatusQueueOrderState::Pending,
            to_state: queue_state::ApparatusQueueOrderState::InProgress,
            policy: ApparatusQueuePolicy::StrictSequence,
            actor: actor("other-printer"),
            assigned_apparatus: vec![session.apparatus.clone()],
            payload_json: json!({}),
        },
        session: Some(session),
        progress_event: None,
        progress_batch: None,
        progress_batches: vec![],
        progress_batch_updates: vec![],
        opening_wip_batch_updates: vec![],
        raw_material_stock_transitions: vec![],
        qolip_checkouts: vec![],
        returned_paint_report: None,
        order_control_update: None,
        schedule_reservation_status: None,
        print_preflight_hold_id: None,
        print_preflight_cancel_hold_id: None,
    }
}

#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL; creates an isolated database"]
async fn postgres_lamination_starts_from_wip_while_print_molds_remain_locked() {
    let f = Fixture::new().await;
    let source = f
        .seed_print_wip(OrderRunStatus::Active, "original-print-set")
        .await;
    let result = f
        .start_lamination()
        .await
        .expect("ready WIP can start lamination while printing continues");
    let session = result.session.expect("lamination session");
    assert_eq!(session.status, OrderRunStatus::Active);
    assert_eq!(session.apparatus, LAM);
    assert_eq!(session.payload_json["qolip_codes"], json!(codes()));
    assert_eq!(session.payload_json["qolip_set_id"], "original-print-set");
    assert_ne!(session.payload_json["qolip_lock_owner"], true);
    let saved = f
        .store
        .order_run_session(&session.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.payload_json, session.payload_json);
    let batch = f.store.progress_batch("print-wip").await.unwrap().unwrap();
    assert_eq!(batch.wip_status, OrderProgressBatchWipStatus::InUse);
    assert_eq!(batch.used_by_session_id, session.session_id);
    assert_eq!(batch.used_by_apparatus, LAM);
    assert_eq!(batch.payload_json["qolip_codes"], json!(codes()));
    for (action, progress, expected_status) in [
        (
            Action::Pause,
            QueueProgressInput {
                worker_handoff: true,
                lamination_print_leftover_rolls: Some(0.0),
                lamination_film_leftover_rolls: Some(0.0),
                total_waste: Some(0.0),
                ..Default::default()
            },
            OrderRunStatus::Paused,
        ),
        (
            Action::Resume,
            QueueProgressInput::default(),
            OrderRunStatus::Active,
        ),
    ] {
        let changed = f
            .service
            .apply_apparatus_queue_action_with_progress(
                LAM,
                ORDER,
                action,
                &[LAM.into()],
                actor("lam-worker"),
                progress,
            )
            .await
            .expect("lamination handoff and resume do not claim physical molds");
        let changed = changed.session.unwrap();
        assert_eq!(changed.session_id, session.session_id);
        assert_eq!(changed.status, expected_status);
        assert_eq!(changed.payload_json["qolip_codes"], json!(codes()));
        assert_ne!(changed.payload_json["qolip_lock_owner"], true);
    }
    assert_eq!(
        f.store
            .order_run_session(&source.session_id)
            .await
            .unwrap()
            .unwrap(),
        source
    );
    assert_eq!(
        f.store
            .active_order_run_session_for_qolip("22091058-1")
            .await
            .unwrap()
            .unwrap()
            .session_id,
        source.session_id
    );
    f.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL; creates an isolated database"]
async fn postgres_lamination_preserves_historical_set_without_revalidating_the_tooling_catalog() {
    let f = Fixture::new().await;
    f.seed_print_wip(
        OrderRunStatus::Completed,
        "historical-set-no-longer-in-catalog",
    )
    .await;
    let result = f
        .start_lamination()
        .await
        .expect("downstream lineage does not claim a physical mold set");
    let session = result.session.unwrap();
    assert_eq!(
        session.payload_json["qolip_set_id"],
        "historical-set-no-longer-in-catalog"
    );
    assert_ne!(session.payload_json["qolip_lock_owner"], true);
    f.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL; creates an isolated database"]
async fn postgres_physical_print_claims_still_enforce_mold_locks_and_set_membership() {
    let f = Fixture::new().await;
    f.store
        .put_map(map("competing-order", OTHER_PRINT))
        .await
        .unwrap();
    let contender = print_start_write(print_session(
        "competing-session",
        OTHER_PRINT,
        "competing-order",
        OrderRunStatus::Active,
        "missing-set",
    ));
    for status in [
        OrderRunStatus::Active,
        OrderRunStatus::Paused,
        OrderRunStatus::RollDetached,
    ] {
        f.store
            .put_order_run_session(print_session(
                "print-session",
                PRINT,
                ORDER,
                status,
                "original-print-set",
            ))
            .await
            .unwrap();
        assert_eq!(
            f.store
                .put_apparatus_queue_states_with_event_and_progress(&contender)
                .await,
            Err(ProductionMapError::QolipAlreadyInUse),
            "physical owner in {status:?}"
        );
        assert!(
            f.store
                .order_run_session("competing-session")
                .await
                .unwrap()
                .is_none()
        );
    }
    f.store
        .put_order_run_session(print_session(
            "print-session",
            PRINT,
            ORDER,
            OrderRunStatus::Frozen,
            "original-print-set",
        ))
        .await
        .unwrap();
    assert!(
        f.store
            .active_order_run_session_for_qolip("22091058-1")
            .await
            .unwrap()
            .is_none(),
        "fully Frozen releases physical claims"
    );
    assert_eq!(
        f.store
            .put_apparatus_queue_states_with_event_and_progress(&contender)
            .await,
        Err(ProductionMapError::QolipCodeMismatch),
        "released molds still require a valid set"
    );
    f.store
        .put_order_run_session(print_session(
            "print-session",
            PRINT,
            ORDER,
            OrderRunStatus::Completed,
            "original-print-set",
        ))
        .await
        .unwrap();
    assert_eq!(
        f.store
            .put_apparatus_queue_states_with_event_and_progress(&contender)
            .await,
        Err(ProductionMapError::QolipCodeMismatch),
        "a physical claim must still validate its set"
    );
    assert!(
        f.store
            .order_run_session("competing-session")
            .await
            .unwrap()
            .is_none()
    );
    f.cleanup().await;
}

#[tokio::test]
#[ignore = "requires MINI_ERP_TEST_ADMIN_DATABASE_URL; creates an isolated database"]
async fn postgres_resume_after_freeze_reacquires_the_complete_physical_set_atomically() {
    use mini_rs_erp::core::qolip::QolipCheckout;
    let f = Fixture::new().await;
    let set = "freeze-resume-set";
    let active = print_session("print-session", PRINT, ORDER, OrderRunStatus::Active, set);
    f.store.put_order_run_session(active.clone()).await.unwrap();
    for (index, code) in codes().iter().enumerate() {
        sqlx::query(
            "INSERT INTO mini_qolip_product_specs (item_code, item_name, qolip_code, size, payload_json)
             VALUES ('PRODUCT', 'Printed product', $1, 40, $2)",
        ).bind(code).bind(json!({"qolip_set_id":set,"warehouse":"Qolip ombori"}))
            .execute(&f.pool).await.unwrap();
        sqlx::query(
            "INSERT INTO mini_qolip_checkouts (id, location_id, block, warehouse, item_code, item_name,
                qolip_code, size, quantity, row_letter, column_number, location_label,
                issued_to_ref, issued_to_name, status, issued_by_role, issued_by_ref, issued_by_name)
             VALUES ($1, $2, 'A', 'Qolip ombori', 'PRODUCT', 'Printed product', $3, 40, 1,
                'A', 1, 'A1', 'print-worker', 'Printer', 'open', 'qolipchi', 'owner', 'Owner')",
        ).bind(format!("old-{index}")).bind(format!("qolip:a:product:22091058_{}:40:a:1", index + 1)).bind(code)
            .execute(&f.pool).await.unwrap();
    }
    let mut frozen = active.clone();
    frozen.status = OrderRunStatus::Frozen;
    f.store.put_order_run_session(frozen).await.unwrap();
    let mut paused = f
        .store
        .order_run_session("print-session")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(paused.payload_json["qolip_returned_checkout_count"], 8);
    paused.status = OrderRunStatus::Paused;
    f.store.put_order_run_session(paused.clone()).await.unwrap();
    f.store
        .put_apparatus_queue_states(PRINT, BTreeMap::from([(ORDER.into(), "pending".into())]))
        .await
        .unwrap();
    let mut resume = print_start_write(active);
    resume.event.action = Action::Resume;
    resume.event.event_id = "resume-after-freeze".into();
    resume.event.actor = actor("print-worker");
    resume.event.payload_json = json!({"qolip_reacquired_after_freeze":true});
    assert_eq!(
        f.store
            .put_apparatus_queue_states_with_event_and_progress(&resume)
            .await,
        Err(ProductionMapError::QolipCodeMismatch),
        "codes alone cannot reacquire released stock"
    );
    assert_eq!(
        f.store
            .order_run_session("print-session")
            .await
            .unwrap()
            .unwrap()
            .status,
        OrderRunStatus::Paused
    );
    assert_eq!(
        f.store.apparatus_queue_states().await.unwrap()[PRINT][ORDER],
        "pending"
    );
    let quantity: i64 =
        sqlx::query_scalar("SELECT sum(quantity)::bigint FROM mini_qolip_locations")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(quantity, 8);

    resume.qolip_checkouts = codes()
        .iter()
        .enumerate()
        .map(|(index, code)| QolipCheckout {
            id: format!("new-{index}"),
            location_id: format!("qolip:a:product:22091058_{}:40:a:1", index + 1),
            block: "A".into(),
            warehouse: "Qolip ombori".into(),
            item_code: "PRODUCT".into(),
            item_name: "Printed product".into(),
            qolip_code: code.clone(),
            size: 40,
            quantity: 1,
            row_letter: "A".into(),
            column_number: Some(1),
            location_label: "A1".into(),
            issued_to_ref: "print-worker".into(),
            issued_to_name: "Printer".into(),
            status: "open".into(),
            issued_by_role: "aparatchi".into(),
            issued_by_ref: "print-worker".into(),
            issued_by_name: "Printer".into(),
            issued_at: "2026-10-07T00:00:00Z".into(),
            ..Default::default()
        })
        .collect();
    f.store
        .put_map(map("other-order", OTHER_PRINT))
        .await
        .unwrap();
    let mut competitor = print_session(
        "other-session",
        OTHER_PRINT,
        "other-order",
        OrderRunStatus::Active,
        set,
    );
    competitor.worker_ref = "other-worker".into();
    f.store
        .put_order_run_session(competitor.clone())
        .await
        .unwrap();
    assert_eq!(
        f.store
            .put_apparatus_queue_states_with_event_and_progress(&resume)
            .await,
        Err(ProductionMapError::QolipAlreadyInUse),
        "another order owns the released molds"
    );
    competitor.status = OrderRunStatus::Completed;
    f.store.put_order_run_session(competitor).await.unwrap();
    f.store
        .put_apparatus_queue_states_with_event_and_progress(&resume)
        .await
        .expect("reacquire all molds");
    let resumed = f
        .store
        .order_run_session("print-session")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resumed.status, OrderRunStatus::Active);
    assert_eq!(resumed.payload_json["qolip_lock_owner"], true);
    assert!(!resumed.qolip_reacquisition_required());
    let open: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mini_qolip_checkouts WHERE status = 'open'")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(open, 8);
    let stock: i64 =
        sqlx::query_scalar("SELECT COALESCE(sum(quantity),0)::bigint FROM mini_qolip_locations")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(stock, 0);
    f.store
        .put_apparatus_queue_states_with_event_and_progress(&resume)
        .await
        .expect("replay resume");
    let open: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mini_qolip_checkouts WHERE status = 'open'")
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(open, 8, "resume replay cannot issue duplicate checkouts");
    f.cleanup().await;
}
