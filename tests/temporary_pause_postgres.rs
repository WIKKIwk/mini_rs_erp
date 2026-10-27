#![cfg(feature = "verification")]
//! Uses only an explicitly configured disposable PostgreSQL server.
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use mini_rs_erp::{
    app::AppState,
    config::AppConfig,
    core::{
        apparatus_standard::{
            ApparatusId, ApparatusOperationalPolicies, CanonicalApparatusPatch,
            CanonicalApparatusService, CanonicalCommandMetadata, MaterialExecutionPolicy,
            QueueDiscipline, ToolingExecutionPolicy,
        },
        auth::models::{Principal, PrincipalRole},
        authz::RoleAssignmentUpsert,
        production_map::{queue_state::ApparatusQueueAction as Action, *},
        session::manager::SessionManager,
    },
    db::{postgres, postgres_production_map::PostgresProductionMapStore},
    http::router::build_router,
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;

const MACHINE: &str = "apparatus:default:bosma_7";

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "temporary-worker".into(),
        display_name: "Worker".into(),
    }
}

struct Fixture {
    pool: sqlx::PgPool,
    apparatus: CanonicalApparatusService,
    store: Arc<PostgresProductionMapStore>,
    service: ProductionMapService,
    a: String,
    b: String,
    c: String,
}

impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("TEMPORARY_PAUSE_TEST_DATABASE_URL").expect(
            "TEMPORARY_PAUSE_TEST_DATABASE_URL must point at a disposable PostgreSQL server",
        );
        let admin = sqlx::PgPool::connect(&url).await.unwrap();
        let name = format!("temporary_pause_{:032x}", rand::random::<u128>());
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
        let apparatus = postgres::canonical_apparatus_service(pool.clone());
        apparatus.bootstrap_factory_defaults().await.unwrap();
        postgres::apply_foundation_migration(&pool).await.unwrap();
        admin.close().await;
        let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
        let service = ProductionMapService::new(
            store.clone(),
            Arc::new(CanonicalServiceApparatusResolver::new(apparatus.clone())),
        );
        let fixture = Self {
            pool,
            apparatus,
            store,
            service,
            a: format!("zakaz-{name}-a"),
            b: format!("zakaz-{name}-b"),
            c: format!("zakaz-{name}-c"),
        };
        fixture.policy(QueueDiscipline::StrictSequence).await;
        for order in [&fixture.a, &fixture.b, &fixture.c] {
            fixture.service.upsert_map(serde_json::from_value(json!({
                "id":order,"title":order,"product_code":"TEMP","roll_count":1,"width_mm":650,
                "nodes":[{"id":"start","kind":"start","title":"Start"},
                    {"id":"print","kind":"apparatus","title":"Print","apparatus_id":MACHINE},
                    {"id":"end","kind":"end","title":"End"}],
                "edges":[{"from":"start","to":"print"},{"from":"print","to":"end"}]
            })).unwrap()).await.unwrap();
        }
        fixture
            .store
            .put_apparatus_sequence(
                MACHINE,
                vec![fixture.a.clone(), fixture.b.clone(), fixture.c.clone()],
            )
            .await
            .unwrap();
        // Begin from validated running work; no output roll has been recorded.
        fixture
            .store
            .put_apparatus_queue_states(MACHINE, [(fixture.a.clone(), "in_progress".into())].into())
            .await
            .unwrap();
        fixture.store.put_order_run_session(OrderRunSession {
            session_id: format!("run-{}", fixture.a), apparatus: MACHINE.into(), order_id: fixture.a.clone(),
            stage_node_id: "print".into(), status: OrderRunStatus::Active,
            worker_role: actor().role, worker_ref: actor().ref_, worker_display_name: actor().display_name,
            started_at_unix: 100, updated_at_unix: 100,
            payload_json: json!({"stage_work_protocol":1,"wip_started_at_unix":100,"wip_waiting_for_resume":false}),
        }).await.unwrap();
        fixture
    }

    async fn policy(&self, queue: QueueDiscipline) {
        let service = &self.apparatus;
        let id = ApparatusId::new(MACHINE).unwrap();
        let current = service.current_configuration(&id).await.unwrap().unwrap();
        service
            .patch(
                id,
                current.runtime.source_revision,
                CanonicalApparatusPatch {
                    policies: Some(ApparatusOperationalPolicies {
                        queue,
                        worker_material_assignment_allowed: false,
                        material: MaterialExecutionPolicy::NotRequired {
                            item_group_ids: vec![],
                        },
                        tooling: ToolingExecutionPolicy::NotRequired,
                    }),
                    ..Default::default()
                },
                CanonicalCommandMetadata::new(
                    "user:test",
                    format!("temporary-policy-{}", current.runtime.source_revision),
                ),
            )
            .await
            .unwrap();
    }

    async fn control(&self) -> TemporaryPauseControl {
        self.service.queue_action_controls().await.unwrap()[MACHINE][&self.a]
            .temporary_pause
            .clone()
            .expect("active work may be temporarily removed")
    }

    async fn current(&self) -> OrderRunSession {
        self.store
            .active_order_run_session(MACHINE, &self.a)
            .await
            .unwrap()
            .unwrap()
    }

    async fn stop(&self, control: &TemporaryPauseControl) -> Result<(), ProductionMapError> {
        self.service
            .temporarily_pause_work(
                MACHINE,
                &self.a,
                &control.session_id,
                &control.token,
                &[MACHINE.into()],
                actor(),
            )
            .await
    }

    async fn run(
        &self,
        order: &str,
        action: Action,
        input: QueueProgressInput,
    ) -> Result<ApparatusQueueActionResult, ProductionMapError> {
        self.service
            .apply_apparatus_queue_action_with_progress(
                MACHINE,
                order,
                action,
                &[MACHINE.into()],
                actor(),
                input,
            )
            .await
    }
}

fn output(meters: f64) -> QueueProgressInput {
    QueueProgressInput {
        produced_qty: Some(meters),
        uom: "m".into(),
        finished_goods_meter: Some(meters),
        finished_goods_kg: Some(12.0),
        bobina_kg: Some(1.0),
        ..Default::default()
    }
}

#[tokio::test]
#[ignore = "requires TEMPORARY_PAUSE_TEST_DATABASE_URL on a disposable PostgreSQL server"]
async fn queue_snapshot_logs_keep_current_states_and_freeze_facts_without_audit_history() {
    let f = Fixture::new().await;
    sqlx::query(
        "INSERT INTO mini_queue_action_events
         (event_id,apparatus,canonical_apparatus_id,order_id,stage_node_id,action,from_state,to_state,policy,created_at)
         SELECT 'old-pause-'||n,$1,$1,$2,'print','pause','in_progress','paused','strict_sequence',to_timestamp(100)
         FROM generate_series(1,600) AS n",
    ).bind(MACHINE).bind(&f.a).execute(&f.pool).await.unwrap();
    let events = [
        ("start", "print", "start", "in_progress"),
        ("old-freeze", "print", "freeze", "frozen"),
        ("resume", "print", "resume", "in_progress"),
        ("new-freeze", "print", "freeze", "frozen"),
        ("latest-print", "print", "resume", "in_progress"),
        ("freeze-cut", "cut", "freeze", "frozen"),
        ("latest-cut", "cut", "pause", "paused"),
        ("global-freeze", "", "freeze", "frozen"),
        ("latest-legacy", "", "resume", "in_progress"),
    ];
    for (id,stage,action,to_state) in events {
        sqlx::query(
            "INSERT INTO mini_queue_action_events
             (event_id,apparatus,canonical_apparatus_id,order_id,stage_node_id,action,from_state,to_state,policy,
              actor_role,actor_ref,actor_display_name,payload_json,created_at)
             VALUES ($1,$2,$2,$3,$4,$5,'in_progress',$6,'strict_sequence','aparatchi',$1,$1,$7,to_timestamp(2000))",
        ).bind(id).bind(MACHINE).bind(&f.a).bind(stage).bind(action).bind(to_state)
            .bind(json!({"issue_note":format!("note-{id}")})).execute(&f.pool).await.unwrap();
    }
    // A later event outside the requested orders must never affect selection.
    sqlx::query(
        "INSERT INTO mini_queue_action_events
         (event_id,apparatus,canonical_apparatus_id,order_id,stage_node_id,action,from_state,to_state,policy,created_at)
         VALUES ('other-order',$1,$1,$2,'print','freeze','in_progress','frozen','strict_sequence',to_timestamp(3000))",
    ).bind(MACHINE).bind(&f.b).execute(&f.pool).await.unwrap();
    let orders = vec![format!(" {} ", f.a), "absent-order".into(), String::new()];
    let full = f.store.queue_action_logs_for_orders(&orders).await.unwrap();
    let snapshot = f.store.queue_snapshot_logs_for_orders(&orders).await.unwrap();
    assert_eq!(full.len(),1);
    assert_eq!(full[&f.a].len(),609);
    assert_eq!(snapshot.len(),1);
    assert_eq!(snapshot[&f.a], full[&f.a][603..]);
    assert_eq!(snapshot[&f.a][0].issue_note,"note-new-freeze");
    assert_eq!(snapshot[&f.a][0].actor_ref,"new-freeze");
    // Both the latest global freeze and stage-specific freeze survive later resumes.
    let frozen = snapshot[&f.a].iter().rev()
        .filter(|log| log.action == Action::Freeze && log.to_state.as_str() == "frozen")
        .map(|log| log.event_id.as_str()).collect::<Vec<_>>();
    assert_eq!(frozen, ["global-freeze","freeze-cut","new-freeze"]);
    assert!(f.store.queue_snapshot_logs_for_orders(&[" ".into(),"absent-order".into()])
        .await.unwrap().is_empty());
    assert_eq!(f.store.queue_action_logs_for_orders(&orders).await.unwrap(),full);
}

#[tokio::test]
#[ignore = "requires TEMPORARY_PAUSE_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn temporary_pause_persists_one_receipt_and_resumes_the_same_unfinished_roll() {
    let f = Fixture::new().await;
    let original = f.current().await;
    let control = f.control().await;
    let second_service = ProductionMapService::new(
        Arc::new(PostgresProductionMapStore::new(f.pool.clone())),
        Arc::new(CanonicalServiceApparatusResolver::new(
            postgres::canonical_apparatus_service(f.pool.clone()),
        )),
    );
    let assigned = [MACHINE.into()];
    let (first, second) = tokio::join!(
        f.stop(&control),
        second_service.temporarily_pause_work(
            MACHINE,
            &f.a,
            &control.session_id,
            &control.token,
            &assigned,
            actor()
        )
    );
    first.unwrap();
    second.unwrap();
    let paused = f.current().await;
    assert_eq!(paused.status, OrderRunStatus::Paused);
    assert_eq!(paused.session_id, original.session_id);
    assert_eq!(paused.started_at_unix, original.started_at_unix);
    assert_eq!(paused.worker_ref, original.worker_ref);
    assert_eq!(paused.payload_json["wip_started_at_unix"], json!(100));
    assert!(
        f.store
            .progress_batches_for_order(&f.a)
            .await
            .unwrap()
            .is_empty()
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mini_queue_action_events WHERE order_id=$1")
            .bind(&f.a)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(count, 1, "independent services share the durable receipt");
    let event: (f64, String) = sqlx::query_as("SELECT produced_qty::double precision,qr_payload FROM mini_order_progress_events WHERE order_id=$1").bind(&f.a).fetch_one(&f.pool).await.unwrap();
    assert_eq!(event, (0.0, String::new()));
    let controls = f.service.queue_action_controls().await.unwrap();
    assert!(controls[MACHINE][&f.a].temporarily_paused);
    assert!(
        controls[MACHINE][&f.b]
            .allowed_actions
            .contains(&Action::Start)
    );
    assert!(
        !controls[MACHINE][&f.c]
            .allowed_actions
            .contains(&Action::Start)
    );
    assert!(
        f.run(&f.c, Action::Start, Default::default())
            .await
            .is_err()
    );
    f.run(&f.b, Action::Start, Default::default())
        .await
        .unwrap();
    assert!(
        f.run(&f.a, Action::Resume, Default::default())
            .await
            .is_err()
    );
    let b_output = f
        .run(&f.b, Action::DetachRoll, output(80.0))
        .await
        .unwrap()
        .progress_batch
        .unwrap();
    f.run(
        &f.b,
        Action::Complete,
        QueueProgressInput {
            complete_without_output: true,
            progress_batch_id: b_output.batch_id,
            total_waste: Some(0.0),
            return_ink_kg: Some(0.0),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let resumed = f
        .run(&f.a, Action::Resume, Default::default())
        .await
        .unwrap()
        .session
        .unwrap();
    assert_eq!(resumed.session_id, original.session_id);
    assert_eq!(resumed.payload_json["wip_started_at_unix"], json!(100));
    assert_eq!(resumed.payload_json["wip_waiting_for_resume"], json!(false));
    f.stop(&control).await.unwrap();
    assert_eq!(
        f.current().await,
        resumed,
        "an old receipt never rewinds resumed work"
    );
    let next_control = f.control().await;
    assert_ne!(
        next_control.token, control.token,
        "same-second stop/resume cycles receive fresh tokens"
    );
    f.stop(&next_control).await.unwrap();
    f.policy(QueueDiscipline::FreePick).await;
    let controls = f.service.queue_action_controls().await.unwrap();
    assert!(
        controls[MACHINE][&f.c]
            .allowed_actions
            .contains(&Action::Start)
    );
    f.run(&f.c, Action::Start, Default::default())
        .await
        .unwrap();
    f.run(&f.c, Action::DetachRoll, output(30.0)).await.unwrap();
    f.run(&f.a, Action::Resume, Default::default())
        .await
        .unwrap();
    let a_output = f
        .run(&f.a, Action::DetachRoll, output(200.0))
        .await
        .unwrap()
        .progress_batch
        .unwrap();
    assert_eq!(a_output.started_at_unix, 100);
    assert_eq!(a_output.produced_qty, 200.0);
    assert_eq!(
        f.store
            .progress_batches_for_order(&f.a)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        f.store
            .progress_batches_for_order(&f.b)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
#[ignore = "requires TEMPORARY_PAUSE_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn temporary_pause_backfills_legacy_qolip_capture_with_runtime_permissions() {
    let f = Fixture::new().await;
    let original = f.current().await;
    // Existing work predates mileage capture. Seed it directly so the pause is
    // the first session write to inspect its original started_at column.
    sqlx::query(
        "UPDATE mini_order_run_sessions SET updated_at=to_timestamp(200),
            payload_json=payload_json || $2::jsonb WHERE session_id=$1",
    )
    .bind(&original.session_id)
    .bind(json!({
        "qolip_code":"7222-1",
        "qolip_codes":["7222-1"],
        "qolip_lock_owner":true
    }))
    .execute(&f.pool)
    .await
    .unwrap();
    let original = f.current().await;
    assert!(original.payload_json.get("qolip_mileage_cycles").is_none());
    // A cycle returned after this session began must not be attributed to old
    // work, even though it predates the session's latest update.
    sqlx::query(
        "INSERT INTO mini_qolip_mileage_cycles(id,qolip_code,rework_id,started_at)
         VALUES($1,'7222-1',$1,to_timestamp(150))",
    )
    .bind(format!("later-cycle-{}", f.a))
    .execute(&f.pool)
    .await
    .unwrap();

    // Migrations and fixtures use the owner; worker requests use this login.
    let runtime_pool = sqlx::PgPool::connect_with(
        f.pool
            .connect_options()
            .as_ref()
            .clone()
            .username("mini_rs_erp"),
    )
    .await
    .unwrap();
    let runtime_role: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&runtime_pool)
        .await
        .unwrap();
    assert_eq!(runtime_role, "mini_rs_erp");
    let runtime_store = Arc::new(PostgresProductionMapStore::new(runtime_pool.clone()));
    let runtime = ProductionMapService::new(
        runtime_store.clone(),
        Arc::new(CanonicalServiceApparatusResolver::new(
            postgres::canonical_apparatus_service(runtime_pool.clone()),
        )),
    );
    let control = runtime.queue_action_controls().await.unwrap()[MACHINE][&f.a]
        .temporary_pause
        .clone()
        .unwrap();
    runtime
        .temporarily_pause_work(
            MACHINE,
            &f.a,
            &control.session_id,
            &control.token,
            &[MACHINE.into()],
            actor(),
        )
        .await
        .unwrap();

    let paused = runtime_store
        .active_order_run_session(MACHINE, &f.a)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(paused.status, OrderRunStatus::Paused);
    assert_eq!(paused.session_id, original.session_id);
    assert_eq!(paused.started_at_unix, original.started_at_unix);
    for key in [
        "wip_started_at_unix",
        "wip_waiting_for_resume",
        "qolip_code",
        "qolip_codes",
        "qolip_lock_owner",
    ] {
        assert_eq!(
            paused.payload_json[key], original.payload_json[key],
            "{key}"
        );
    }
    assert_eq!(
        paused.payload_json["qolip_mileage_cycles"],
        json!({"7222-1":null}),
        "legacy work without a tracked cycle keeps that unknown lineage"
    );
    assert_eq!(
        paused.payload_json["qolip_mileage_apparatus_id"],
        json!(MACHINE)
    );
    assert!(
        runtime_store
            .progress_batches_for_order(&f.a)
            .await
            .unwrap()
            .is_empty()
    );
    let progress: (f64, String) = sqlx::query_as(
        "SELECT produced_qty::double precision,qr_payload
         FROM mini_order_progress_events WHERE order_id=$1",
    )
    .bind(&f.a)
    .fetch_one(&runtime_pool)
    .await
    .unwrap();
    assert_eq!(progress, (0.0, String::new()));
    let contributions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mini_qolip_mileage_contributions")
            .fetch_one(&runtime_pool)
            .await
            .unwrap();
    assert_eq!(
        contributions, 0,
        "temporary removal creates no plate mileage"
    );
}

#[tokio::test]
#[ignore = "requires TEMPORARY_PAUSE_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn temporary_pause_rejects_stale_tokens_and_stale_prepared_resumes() {
    let f = Fixture::new().await;
    let stale = f.control().await;
    let mut changed = f.current().await;
    changed.payload_json["received_material_revision"] = json!(1);
    f.store.put_order_run_session(changed).await.unwrap();
    assert!(f.stop(&stale).await.is_err());
    let control = f.control().await;
    f.stop(&control).await.unwrap();
    // Exercise the public persistence boundary with a write prepared from the
    // old held session, then mutate that session through another writer.
    let expected = f.current().await;
    let mut proposed = expected.clone();
    proposed.status = OrderRunStatus::Active;
    proposed
        .payload_json
        .as_object_mut()
        .unwrap()
        .remove("_temporary_pause");
    let prepared = QueueActionProgressWrite {
        apparatus: MACHINE.into(),
        states: [(f.a.clone(), "in_progress".into())].into(),
        event: ApparatusQueueActionEvent {
            event_id: format!("stale-temporary-resume-{:032x}", rand::random::<u128>()),
            apparatus: MACHINE.into(),
            order_id: f.a.clone(),
            stage_node_id: "print".into(),
            action: Action::Resume,
            from_state: queue_state::ApparatusQueueOrderState::Paused,
            to_state: queue_state::ApparatusQueueOrderState::InProgress,
            policy: ApparatusQueuePolicy::StrictSequence,
            actor: actor(),
            assigned_apparatus: vec![MACHINE.into()],
            payload_json: json!({"temporary_pause_expected_session":expected}),
        },
        session: Some(proposed),
        progress_event: None,
        map_update: None,
        sequence_updates: Default::default(),
        progress_batch: None,
        progress_batches: vec![],
        progress_batch_updates: vec![],
        opening_wip_batch_updates: vec![],
        raw_material_stock_transitions: vec![],
        qolip_checkouts: vec![],
        returned_paint_report: None,
        order_control_update: None,
        schedule_reservation_status: Some(ApparatusScheduleStatus::Active),
        print_preflight_hold_id: None,
        print_preflight_cancel_hold_id: None,
    };
    let mut changed = f.current().await;
    changed.payload_json["received_material_revision"] = json!(2);
    f.store
        .put_order_run_session(changed.clone())
        .await
        .unwrap();
    assert_eq!(
        f.store
            .put_apparatus_queue_states_with_event_and_progress(&prepared)
            .await,
        Err(ProductionMapError::QueueActionNotAllowed)
    );
    assert_eq!(f.current().await, changed);
    f.run(&f.a, Action::Resume, Default::default())
        .await
        .unwrap();
    assert_eq!(
        f.current().await.payload_json["received_material_revision"],
        json!(2)
    );
}

#[tokio::test]
#[ignore = "requires TEMPORARY_PAUSE_TEST_DATABASE_URL pointing at a disposable PostgreSQL server; run with --test-threads=1"]
async fn temporary_pause_http_checks_assignment_and_rejects_measurements() {
    let f = Fixture::new().await;
    let dir = tempfile::tempdir().unwrap();
    struct RestoreDirectory(std::path::PathBuf);
    impl Drop for RestoreDirectory {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.0).unwrap();
        }
    }
    let _restore = RestoreDirectory(std::env::current_dir().unwrap());
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
    state.sessions = SessionManager::memory(Some(3600));
    state.production_maps = f.service.clone();
    state.apparatus = postgres::canonical_apparatus_service(f.pool.clone());
    for who in ["temporary-worker", "unassigned-worker"] {
        state
            .admin
            .upsert_role_assignment(RoleAssignmentUpsert {
                principal_role: PrincipalRole::Aparatchi,
                principal_ref: who.into(),
                role_id: "aparatchi".into(),
                assigned_apparatus: if who == "temporary-worker" {
                    vec![MACHINE.into()]
                } else {
                    vec![]
                },
                assigned_item_groups: vec![],
            })
            .await
            .unwrap();
    }
    let mut tokens = Vec::new();
    for (role, who) in [
        (PrincipalRole::Aparatchi, "temporary-worker"),
        (PrincipalRole::Aparatchi, "unassigned-worker"),
        (PrincipalRole::Admin, "admin"),
    ] {
        tokens.push(
            state
                .sessions
                .create(Principal {
                    role,
                    ref_: who.into(),
                    display_name: "Worker".into(),
                    legal_name: String::new(),
                    phone: String::new(),
                    avatar_url: String::new(),
                })
                .await
                .unwrap(),
        );
    }
    let router = build_router(state);
    let control = f.control().await;
    let body = json!({"apparatus":MACHINE,"order_id":f.a,"session_id":control.session_id,"token":control.token});
    for (token, payload, expected) in [
        (None, body.clone(), StatusCode::UNAUTHORIZED),
        (Some(&tokens[2]), body.clone(), StatusCode::FORBIDDEN),
        (Some(&tokens[1]), body.clone(), StatusCode::BAD_REQUEST),
        (
            Some(&tokens[0]),
            {
                let mut p = body.clone();
                p["finished_goods_meter"] = json!(0);
                p
            },
            StatusCode::BAD_REQUEST,
        ),
        (Some(&tokens[0]), body.clone(), StatusCode::OK),
        (Some(&tokens[0]), body.clone(), StatusCode::OK),
    ] {
        let mut request = Request::builder()
            .method("POST")
            .uri("/v1/mobile/admin/production-maps/temporary-pause")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = router
            .clone()
            .oneshot(request.body(Body::from(payload.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let response: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(status, expected, "{response}");
        if status == StatusCode::OK {
            assert_eq!(response["ok"], json!(true));
        }
    }
    assert!(
        f.store
            .progress_batches_for_order(&f.a)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
#[ignore = "requires TEMPORARY_PAUSE_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn temporary_pause_and_reorder_preserve_raw_stock_and_recheck_reservation_on_resume() {
    let f = Fixture::new().await;
    let stock_id = format!("stock-{}", f.a);
    let barcode = format!("RAW-{}", f.a);
    sqlx::query("INSERT INTO mini_item_groups(name,parent_item_group,is_group,payload_json) VALUES('film','All Item Groups',false,'{}') ON CONFLICT(name) DO NOTHING")
        .execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO mini_items(code,name,uom,item_group,payload_json) VALUES('FILM','Film','kg','film','{}') ON CONFLICT(code) DO NOTHING")
        .execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty,uom,status,reserved_order_id,source_receipt_id,payload_json) VALUES($1,'Temporary pause warehouse','FILM','Film',$2,11,'kg','in_use',$3,$1,'{}')")
        .bind(&stock_id).bind(&barcode).bind(&f.a).execute(&f.pool).await.unwrap();
    sqlx::query("INSERT INTO mini_raw_material_assignments(barcode,order_id,apparatus,canonical_apparatus_id,item_code,item_group,payload_json) VALUES($1,$2,$3,$3,'FILM','film',$4)")
        .bind(&barcode).bind(&f.a).bind(MACHINE)
        .bind(json!({"barcode":barcode,"order_id":f.a,"apparatus_id":MACHINE,"apparatus":MACHINE,"item_code":"FILM","item_name":"Film","item_group":"film","assigned_by_role":"aparatchi","assigned_by_ref":actor().ref_,"assigned_by_display_name":"Worker","assigned_at":"2026-10-26"}))
        .execute(&f.pool).await.unwrap();
    let mut session = f.current().await;
    session.payload_json["start_resources"] = json!({
        "session_id":session.session_id,
        "raw_materials":[{"barcode":barcode,"stock_id":stock_id,"source_qty":11.0,"uom":"kg","item_code":"FILM","item_name":"Film"}],
        "raw_materials_available":true,"qolip_codes":[],"qolip_available":false
    });
    f.store.put_order_run_session(session).await.unwrap();
    let stock_before: Value =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM mini_raw_material_stock s WHERE id=$1")
            .bind(&stock_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    f.stop(&f.control().await).await.unwrap();
    let paused = f.current().await;
    let states = f.store.apparatus_queue_states().await.unwrap();
    let down = SequenceMove {
        apparatus: MACHINE.into(),
        order_id: f.a.clone(),
        before_order_id: None,
        after_order_id: None,
        expected_version: f.service.live_snapshot_shared().await.unwrap()
            .sequence_versions[MACHINE].clone(),
        idempotency_key: "paused-stock-down".into(),
    };
    let saved = f.service.move_apparatus_sequence(down.clone(), actor()).await.unwrap();
    assert_eq!(saved.order_ids, vec![f.b.clone(), f.c.clone(), f.a.clone()]);
    assert!(!saved.adjusted);
    assert_eq!(f.current().await, paused);
    assert_eq!(f.store.apparatus_queue_states().await.unwrap(), states);
    let stale = SequenceMove {
        before_order_id: Some(f.b.clone()),
        expected_version: saved.version.clone(),
        idempotency_key: "paused-stock-before-resume".into(),
        ..down.clone()
    };
    let stock_after: Value =
        sqlx::query_scalar("SELECT to_jsonb(s) FROM mini_raw_material_stock s WHERE id=$1")
            .bind(&stock_id)
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(
        stock_before, stock_after,
        "temporary removal and reorder do not consume, release or reassign raw material"
    );
    f.run(&f.a, Action::Resume, Default::default())
        .await
        .unwrap();
    assert_eq!(f.current().await.session_id, paused.session_id);
    assert_eq!(
        f.service.move_apparatus_sequence(stale, actor()).await,
        Err(ProductionMapError::QueueReorderConflict),
    );
    // A durable retry acknowledges the pause-era move without changing the
    // resumed session or the material reservation.
    let resumed = f.current().await;
    assert_eq!(f.service.move_apparatus_sequence(down, actor()).await.unwrap(), saved);
    assert_eq!(f.current().await, resumed);
    assert_eq!(f.store.apparatus_sequences().await.unwrap()[MACHINE], saved.order_ids);
    f.stop(&f.control().await).await.unwrap();
    sqlx::query("UPDATE mini_raw_material_stock SET status='consumed' WHERE id=$1")
        .bind(&stock_id)
        .execute(&f.pool)
        .await
        .unwrap();
    assert!(
        f.run(&f.a, Action::Resume, Default::default())
            .await
            .is_err()
    );
    assert_eq!(f.current().await.status, OrderRunStatus::Paused);
    sqlx::query("UPDATE mini_raw_material_stock SET status='in_use' WHERE id=$1")
        .bind(&stock_id)
        .execute(&f.pool)
        .await
        .unwrap();
    f.run(&f.a, Action::Resume, Default::default())
        .await
        .unwrap();
    assert!(
        f.store
            .progress_batches_for_order(&f.a)
            .await
            .unwrap()
            .is_empty()
    );
}
