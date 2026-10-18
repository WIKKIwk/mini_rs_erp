#![cfg(feature = "verification")]
//! Runs only against an explicitly supplied disposable PostgreSQL database.
use mini_rs_erp::{
    app::AppState,
    config::AppConfig,
    core::{
        auth::models::{Principal, PrincipalRole},
        authz::RoleAssignmentUpsert,
        production_map::{queue_state::ApparatusQueueAction as Action, *},
    },
    db::{postgres, postgres_production_map::PostgresProductionMapStore},
    http::router::build_router,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

const MACHINE: &str = "apparatus:default:bosma_7";
fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "repause-worker".into(),
        display_name: "Worker".into(),
    }
}

async fn pool() -> sqlx::PgPool {
    static DATABASE: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();
    let admin_url = std::env::var("RE_PAUSE_TEST_DATABASE_URL")
        .expect("explicit disposable PostgreSQL database URL");
    let database = DATABASE
        .get_or_init(|| async {
            let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
            let database = format!("resume_pause_{:032x}", rand::random::<u128>());
            sqlx::query(&format!("CREATE DATABASE {database}"))
                .execute(&admin)
                .await
                .unwrap();
            let options = admin_url
                .parse::<sqlx::postgres::PgConnectOptions>()
                .unwrap()
                .database(&database);
            let pool = sqlx::PgPool::connect_with(options).await.unwrap();
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
            database
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
    service: ProductionMapService,
    store: Arc<PostgresProductionMapStore>,
    order: String,
}
impl Fixture {
    async fn new() -> Self {
        let pool = pool().await;
        let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
        let service = ProductionMapService::new(
            store.clone(),
            Arc::new(CanonicalServiceApparatusResolver::new(
                postgres::canonical_apparatus_service(pool.clone()),
            )),
        );
        let order = format!("zakaz-repause-{:032x}", rand::random::<u128>());
        let map = serde_json::from_value(json!({
            "id":order,"title":order,"product_code":"REP","roll_count":7,"width_mm":650,
            "nodes":[{"id":"start","kind":"start","title":"Start"},
                {"id":"print","kind":"apparatus","title":"Print","apparatus_id":MACHINE},
                {"id":"end","kind":"end","title":"End"}],
            "edges":[{"from":"start","to":"print"},{"from":"print","to":"end"}]
        }))
        .unwrap();
        service.upsert_map(map).await.unwrap();
        store
            .put_apparatus_sequence(MACHINE, vec![order.clone()])
            .await
            .unwrap();
        let fixture = Self {
            pool,
            service,
            store,
            order,
        };
        // Seed a validated running session; tooling provisioning is outside this
        // regression. Detach, Resume and re-pause use the production services.
        fixture
            .store
            .put_apparatus_queue_states(
                MACHINE,
                [(fixture.order.clone(), "in_progress".into())].into(),
            )
            .await
            .unwrap();
        fixture
            .store
            .put_order_run_session(OrderRunSession {
                session_id: format!("run-{}", fixture.order),
                apparatus: MACHINE.into(),
                order_id: fixture.order.clone(),
                stage_node_id: "print".into(),
                status: OrderRunStatus::Active,
                worker_role: actor().role,
                worker_ref: actor().ref_,
                worker_display_name: actor().display_name,
                started_at_unix: 1,
                updated_at_unix: 1,
                payload_json: json!({"stage_work_protocol":1}),
            })
            .await
            .unwrap();
        fixture.run(Action::DetachRoll).await.unwrap();
        fixture.run(Action::Resume).await.unwrap();
        fixture
    }
    async fn run(&self, action: Action) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        self.service
            .apply_apparatus_queue_action_with_progress(
                MACHINE,
                &self.order,
                action,
                &[MACHINE.into()],
                actor(),
                if action == Action::DetachRoll {
                    QueueProgressInput {
                        produced_qty: Some(12.0),
                        uom: "kg".into(),
                        finished_goods_kg: Some(12.0),
                        finished_goods_meter: Some(120.0),
                        bobina_kg: Some(1.0),
                        ..Default::default()
                    }
                } else {
                    Default::default()
                },
            )
            .await
    }
    async fn control(&self) -> Option<ResumePauseControl> {
        let controls = self.service.queue_action_controls().await.unwrap();
        controls
            .get(MACHINE)
            .and_then(|orders| orders.get(&self.order))
            .and_then(|c| c.re_pause.clone())
    }
    async fn cancel(&self, resume_id: &str) -> Result<(), ProductionMapError> {
        self.service
            .re_pause_resumed_work(MACHINE, &self.order, resume_id, &[MACHINE.into()], actor())
            .await
    }
    async fn current(&self) -> OrderRunSession {
        self.store
            .active_order_run_session(MACHINE, &self.order)
            .await
            .unwrap()
            .unwrap()
    }
    async fn batches(&self) -> Vec<OrderProgressBatch> {
        self.store
            .progress_batches_for_order(&self.order)
            .await
            .unwrap()
    }
    async fn batch_rows(&self) -> Value {
        // Compare every durable field, including the immutable production times.
        sqlx::query_scalar("SELECT COALESCE(jsonb_agg(to_jsonb(b) ORDER BY batch_id),'[]') FROM mini_progress_batches b WHERE order_id=$1")
            .bind(&self.order).fetch_one(&self.pool).await.unwrap()
    }
    async fn release(&self) {
        // Every scenario leaves the shared machine available to the next one.
        self.store
            .put_apparatus_queue_states(MACHINE, [(self.order.clone(), "paused".into())].into())
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
#[ignore = "requires RE_PAUSE_TEST_DATABASE_URL pointing at a disposable PostgreSQL database"]
async fn wip_production_times_survive_session_updates_and_all_history_reads() {
    let f = Fixture::new().await;
    let first = f.batches().await.remove(0);
    assert_eq!(first.started_at_unix, 1);
    let checkpoint = f
        .control()
        .await
        .expect("timing fields preserve the reversible resume");
    f.cancel(&checkpoint.resume_id).await.unwrap();
    let resumed = f.run(Action::Resume).await.unwrap().session.unwrap();
    let second = f
        .run(Action::DetachRoll)
        .await
        .unwrap()
        .progress_batch
        .unwrap();
    assert_eq!(second.started_at_unix, resumed.updated_at_unix);
    assert!(second.started_at_unix > first.started_at_unix);
    let closed = f
        .service
        .apply_apparatus_queue_action_with_progress(
            MACHINE,
            &f.order,
            Action::Complete,
            &[MACHINE.into()],
            actor(),
            QueueProgressInput {
                complete_without_output: true,
                total_waste: Some(0.0),
                progress_batch_id: second.batch_id.clone(),
                return_ink_kg: Some(0.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(closed.progress_batch.is_none());
    let mut later = closed.session.unwrap();
    later.updated_at_unix += 86_400;
    f.store.put_order_run_session(later).await.unwrap();

    let mut correction = first.clone();
    correction.started_at_unix = 2;
    correction.completed_at_unix += 172_800;
    correction.description = "Later correction".into();
    f.store.put_order_progress_batch(correction).await.unwrap();
    let all = f
        .store
        .progress_batches_for_orders(&[f.order.clone()])
        .await
        .unwrap();
    let reads = vec![
        vec![
            f.store
                .progress_batch(&first.batch_id)
                .await
                .unwrap()
                .unwrap(),
        ],
        vec![
            f.store
                .progress_batch_by_qr(&first.qr_payload)
                .await
                .unwrap()
                .unwrap(),
        ],
        f.batches().await,
        all[&f.order].clone(),
        f.store
            .progress_batches_for_worker(&[actor().ref_], "", 100)
            .await
            .unwrap(),
        f.store.progress_batches_for_audit().await.unwrap(),
        f.store
            .wip_progress_batches(WipProgressBatchQuery::new(
                "", "", "", None, true, &f.order, 100,
            ))
            .await
            .unwrap(),
    ];
    for batches in reads {
        let saved = batches
            .iter()
            .find(|batch| batch.batch_id == first.batch_id)
            .unwrap();
        assert_eq!(
            (saved.started_at_unix, saved.completed_at_unix),
            (first.started_at_unix, first.completed_at_unix)
        );
        assert_eq!(saved.description, "Later correction");
    }
    let stored_second = f
        .store
        .progress_batch(&second.batch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            stored_second.started_at_unix,
            stored_second.completed_at_unix
        ),
        (second.started_at_unix, second.completed_at_unix),
        "first-print and reprint values agree"
    );
    let rejected = sqlx::query("UPDATE mini_progress_batches SET production_completed_at=production_completed_at+interval '1 day' WHERE batch_id=$1")
        .bind(&first.batch_id).execute(&f.pool).await.unwrap_err();
    assert_eq!(
        rejected.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    f.release().await;
}

#[tokio::test]
#[ignore = "requires RE_PAUSE_TEST_DATABASE_URL pointing at a disposable PostgreSQL database"]
async fn re_pause_preserves_outputs_retries_and_repeated_resumes() {
    let f = Fixture::new().await;
    let before = f.batch_rows().await;
    let session_id = f.current().await.session_id;
    let first = f
        .control()
        .await
        .expect("ordinary resume exposes additive control");
    // Independent services/processes share the durable checkpoint.
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
            postgres::canonical_apparatus_service(runtime_pool.clone()),
        )),
    );
    restarted
        .re_pause_resumed_work(
            MACHINE,
            &f.order,
            &first.resume_id,
            &[MACHINE.into()],
            actor(),
        )
        .await
        .unwrap();
    let assigned = [MACHINE.into()];
    let (a, b) = tokio::join!(
        f.cancel(&first.resume_id),
        restarted.re_pause_resumed_work(MACHINE, &f.order, &first.resume_id, &assigned, actor()),
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(f.current().await.status, OrderRunStatus::RollDetached);
    assert_eq!(f.current().await.session_id, session_id);
    assert_eq!(f.batch_rows().await, before);
    assert!(f.control().await.is_none());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_order_progress_events WHERE order_id=$1 AND payload_json->>'event'='re_pause'")
        .bind(&f.order).fetch_one(&f.pool).await.unwrap();
    assert_eq!(count, 1);
    f.run(Action::Resume).await.unwrap();
    let second = f.control().await.unwrap();
    assert_ne!(first.resume_id, second.resume_id);
    f.cancel(&first.resume_id).await.unwrap();
    assert_eq!(f.current().await.status, OrderRunStatus::Active);
    f.cancel(&second.resume_id).await.unwrap();
    f.run(Action::Resume).await.unwrap();
    f.cancel(&f.control().await.unwrap().resume_id)
        .await
        .unwrap();
    assert_eq!(f.batch_rows().await, before);
    f.release().await;
}

#[tokio::test]
#[ignore = "requires RE_PAUSE_TEST_DATABASE_URL pointing at a disposable PostgreSQL database"]
async fn re_pause_rejects_work_materials_reports_admin_changes_and_wrong_owner() {
    for activity in [
        "output",
        "material",
        "report",
        "map",
        "freeze",
        "same-second-session",
    ] {
        let f = Fixture::new().await;
        let checkpoint = f.control().await.unwrap();
        let mut other = actor();
        other.ref_ = "another-worker".into();
        assert!(
            f.service
                .re_pause_resumed_work(
                    MACHINE,
                    &f.order,
                    &checkpoint.resume_id,
                    &[MACHINE.into()],
                    other
                )
                .await
                .is_err()
        );
        assert!(
            f.service
                .re_pause_resumed_work(MACHINE, &f.order, &checkpoint.resume_id, &[], actor())
                .await
                .is_err()
        );
        match activity {
            "output" => {
                f.run(Action::DetachRoll).await.unwrap();
            }
            "material" => {
                sqlx::query("INSERT INTO mini_raw_material_events(event_id,idempotency_key,event_type,warehouse,barcode,item_code,order_id,apparatus,canonical_apparatus_id,actor_role,actor_ref,source_type,source_id,stock_status_before,stock_status_after) VALUES($1,$1,'delivery_received','Warehouse','ROLL','FILM',$1,$2,$2,'aparatchi','repause-worker','material_delivery',$1,'in_use','in_use')")
                    .bind(&f.order).bind(MACHINE).execute(&f.pool).await.unwrap();
            }
            "report" => {
                let id = format!("report-{}", f.order);
                sqlx::query("INSERT INTO mini_bosma_astatka_reports(report_id,order_id,apparatus,from_at_unix,to_at_unix,report_json) VALUES($1,$2,$3,0,1,$4)")
                    .bind(&id).bind(&f.order).bind(MACHINE).bind(json!({"report_id":id,"order_id":f.order,"apparatus":MACHINE}))
                    .execute(&f.pool).await.unwrap();
            }
            "map" => {
                sqlx::query("UPDATE mini_production_maps SET map_json=jsonb_set(map_json,'{title}','\"Edited\"') WHERE id=$1")
                .bind(&f.order).execute(&f.pool).await.unwrap();
            }
            "freeze" => {
                sqlx::query("INSERT INTO mini_order_control_states(order_id,state) VALUES($1,'freeze_requested') ON CONFLICT(order_id) DO UPDATE SET state='freeze_requested'")
                    .bind(&f.order).execute(&f.pool).await.unwrap();
            }
            _ => {
                let mut session = f.current().await;
                session.payload_json["recorded_work"] = json!(true);
                f.store.put_order_run_session(session).await.unwrap();
            }
        }
        let before = f.batch_rows().await;
        assert!(f.control().await.is_none(), "{activity}");
        assert!(f.cancel(&checkpoint.resume_id).await.is_err(), "{activity}");
        assert_eq!(f.batch_rows().await, before);
        f.release().await;
    }
}

#[tokio::test]
#[ignore = "requires RE_PAUSE_TEST_DATABASE_URL pointing at a disposable PostgreSQL database"]
async fn re_pause_and_output_race_commits_exactly_one_effect() {
    let f = Fixture::new().await;
    let token = f.control().await.unwrap().resume_id;
    let peer = ProductionMapService::new(
        f.store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(
            postgres::canonical_apparatus_service(f.pool.clone()),
        )),
    );
    let assigned = [MACHINE.into()];
    let (cancel, output) = tokio::join!(
        f.cancel(&token),
        peer.apply_apparatus_queue_action_with_progress(
            MACHINE,
            &f.order,
            Action::DetachRoll,
            &assigned,
            actor(),
            QueueProgressInput {
                produced_qty: Some(13.0),
                uom: "kg".into(),
                ..Default::default()
            }
        )
    );
    assert_ne!(cancel.is_ok(), output.is_ok(), "{cancel:?}, {output:?}");
    assert_eq!(f.batches().await.len(), if output.is_ok() { 2 } else { 1 });
    assert_eq!(f.current().await.status, OrderRunStatus::RollDetached);
    f.release().await;
}

#[tokio::test]
#[ignore = "requires RE_PAUSE_TEST_DATABASE_URL pointing at a disposable PostgreSQL database"]
async fn re_pause_http_authorizes_worker_and_rejects_client_metrics() {
    let f = Fixture::new().await;
    let dir = tempfile::tempdir().unwrap();
    // AppState's verification builder has additional relative local stores.
    // Keep this test process in its own directory, away from live SQLite data.
    struct RestoreDirectory(std::path::PathBuf);
    impl Drop for RestoreDirectory {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.0).unwrap();
        }
    }
    let _restore_directory = RestoreDirectory(std::env::current_dir().unwrap());
    std::env::set_current_dir(dir.path()).unwrap();
    let mut state = AppState::verification(AppConfig {
        bind_addr: "127.0.0.1:0".parse().unwrap(),
        default_target_warehouse: String::new(),
        http_timeout: Duration::from_secs(5),
        session_store_path: dir.path().join("sessions"),
        profile_store_path: dir.path().join("profiles"),
        push_token_store_path: dir.path().join("push"),
        session_ttl_seconds: Some(3600),
        supplier_prefix: "10".into(),
        werka_prefix: "20".into(),
        werka_code: String::new(),
        werka_name: String::new(),
        werka_phone: String::new(),
        material_taminotchi_code: String::new(),
        material_taminotchi_name: String::new(),
        material_taminotchi_phone: String::new(),
        admin_phone: String::new(),
        admin_name: String::new(),
        admin_code: String::new(),
    });
    state.apparatus = postgres::canonical_apparatus_service(f.pool.clone());
    state.production_maps = f.service.clone();
    state
        .admin
        .upsert_role_assignment(RoleAssignmentUpsert {
            principal_role: PrincipalRole::Aparatchi,
            principal_ref: actor().ref_,
            role_id: "aparatchi".into(),
            assigned_apparatus: vec![MACHINE.into()],
            assigned_item_groups: vec![],
        })
        .await
        .unwrap();
    let mut tokens = Vec::new();
    for (role, who) in [
        (PrincipalRole::Aparatchi, actor().ref_),
        (PrincipalRole::Admin, "admin".into()),
    ] {
        tokens.push(
            state
                .sessions
                .create(Principal {
                    role,
                    ref_: who,
                    display_name: "Worker".into(),
                    legal_name: String::new(),
                    avatar_url: String::new(),
                    phone: String::new(),
                })
                .await
                .unwrap(),
        );
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/v1/mobile/admin/production-maps/re-pause",
        listener.local_addr().unwrap()
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, build_router(state)).await.unwrap();
    });
    let client = reqwest::Client::new();
    let body = json!({"apparatus":MACHINE,"order_id":f.order,"resume_id":f.control().await.unwrap().resume_id});
    assert_eq!(
        client.post(&url).json(&body).send().await.unwrap().status(),
        401
    );
    assert_eq!(
        client
            .post(&url)
            .bearer_auth(&tokens[1])
            .json(&body)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let mut extra = body.clone();
    extra["produced_qty"] = json!(0);
    assert_eq!(
        client
            .post(&url)
            .bearer_auth(&tokens[0])
            .json(&extra)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let response = client
        .post(&url)
        .bearer_auth(&tokens[0])
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let response: Value = response.json().await.unwrap();
    assert_eq!(status, 200, "{response}");
    assert_eq!(response["ok"], true);
    assert_eq!(f.batches().await.len(), 1);
    task.abort();
    let _ = task.await;
    f.release().await;
}
