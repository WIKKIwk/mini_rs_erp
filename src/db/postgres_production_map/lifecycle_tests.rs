use super::*;

#[tokio::test]
async fn postgres_print_preflight_persists_common_status_and_restores_without_expiry() {
    use crate::core::production_map::{PrintPreflightHold, PrintPreflightStatus};
    use super::super::print_preflight;
    let pool = isolated_pool().await;
    sqlx::raw_sql("
        CREATE TEMP TABLE mini_apparatus (id TEXT PRIMARY KEY, name TEXT);
        ALTER TABLE mini_queue_states ADD COLUMN apparatus TEXT, ADD COLUMN updated_at TIMESTAMPTZ;
        ALTER TABLE mini_queue_states ADD PRIMARY KEY (canonical_apparatus_id, order_id);
        ALTER TABLE mini_queue_states ADD CONSTRAINT mini_queue_states_state_allowed
            CHECK (state IN ('pending', 'in_progress', 'paused', 'frozen', 'completed'));
        ALTER TABLE mini_queue_action_events ADD COLUMN from_state TEXT;
        ALTER TABLE mini_queue_action_events ADD CONSTRAINT mini_queue_action_events_from_state_allowed
            CHECK (from_state IN ('pending', 'in_progress', 'paused', 'frozen', 'completed'));
        ALTER TABLE mini_queue_action_events ADD CONSTRAINT mini_queue_action_events_to_state_allowed
            CHECK (to_state IN ('pending', 'in_progress', 'paused', 'frozen', 'completed'));
        ALTER TABLE mini_production_maps ADD CONSTRAINT mini_production_maps_operational_status_allowed
            CHECK (operational_status IN ('not_started', 'ready', 'in_progress', 'paused', 'frozen',
                'waiting_next_stage', 'partially_completed', 'completed', 'completed_with_issue'));
        ALTER TABLE mini_production_maps ADD CONSTRAINT mini_production_maps_flow_status_allowed
            CHECK (flow_status IN ('not_started', 'ready', 'in_progress', 'paused', 'frozen',
                'waiting_next_stage', 'partially_completed', 'completed', 'completed_with_issue',
                'free_wip', 'accepted_to_stock'));
    ").execute(&pool).await.unwrap();
    sqlx::raw_sql(&include_str!("../../../migrations/postgres/0120_print_preflight_holds.sql")
        .replace("CREATE TABLE IF NOT EXISTS", "CREATE TEMP TABLE"))
        .execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!("../../../migrations/postgres/0121_print_preflight_order_status.sql"))
        .execute(&pool).await.expect("migration accepts the new common status");
    let map = serde_json::json!({
        "id":"colour", "product_code":"COLOUR", "title":"Colour",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"print","kind":"apparatus","title":"Print","apparatus_id":"apparatus:test:print"},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"print"},{"from":"print","to":"end"}]
    });
    sqlx::query("INSERT INTO mini_production_maps (id, map_json, lifecycle_status, operational_status, flow_status)
        VALUES ('colour', $1, 'released', 'not_started', 'not_started')")
        .bind(map).execute(&pool).await.unwrap();
    let original = load_production_order_lifecycles(&pool, &["colour".into()]).await.unwrap();
    let mut hold = PrintPreflightHold {
        hold_id: "colour-trial".into(), idempotency_key: "colour-trial".into(),
        order_id: "colour".into(), apparatus: "apparatus:test:print".into(), stage_node_id: "print".into(),
        status: PrintPreflightStatus::Running,
        actor: QueueActionActor { role: "aparatchi".into(), ref_: "worker".into(), display_name: "Worker".into() },
        created_at_unix: 1, updated_at_unix: 1, expires_at_unix: 2, previous_queue_state: None,
    };
    print_preflight::put(&pool, &hold).await.unwrap();
    let restored = load_production_order_lifecycles(&pool, &["colour".into()]).await.unwrap();
    assert_eq!(restored["colour"].operational_status, ProductionOrderOperationalStatus::PrintPreflight);
    assert_eq!(restored["colour"].status, ProductionOrderLifecycleStatus::Released);
    assert_eq!(restored["colour"].flow_status, "print_preflight");
    assert_eq!(print_preflight::load_active(&pool).await.unwrap(), vec![hold.clone()],
        "the old expiry timestamp must not end the status");
    let queue: String = sqlx::query_scalar("SELECT state FROM mini_queue_states WHERE order_id = 'colour'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(queue, "print_preflight");
    hold.status = PrintPreflightStatus::Failed;
    print_preflight::update(&pool, &hold).await.unwrap();
    let reset = load_production_order_lifecycles(&pool, &["colour".into()]).await.unwrap();
    assert_eq!(reset["colour"].status, original["colour"].status);
    assert_eq!(reset["colour"].operational_status, original["colour"].operational_status);
    assert_eq!(reset["colour"].flow_status, original["colour"].flow_status);
    assert!(print_preflight::load_active(&pool).await.unwrap().is_empty());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_queue_states WHERE order_id = 'colour'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(count, 0, "missing queue row is restored exactly");
    pool.close().await;
}

// Connection-local tables exercise the real SQL/transaction path without
// creating, resetting, or modifying any application database or shared tables.
async fn isolated_pool() -> PgPool {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    sqlx::raw_sql("
        CREATE TEMP TABLE mini_production_maps (
            id TEXT PRIMARY KEY, map_json JSONB NOT NULL,
            lifecycle_status TEXT NOT NULL DEFAULT 'in_progress', lifecycle_version BIGINT NOT NULL DEFAULT 1,
            operational_status TEXT NOT NULL DEFAULT 'partially_completed',
            completed_with_issue_count BIGINT NOT NULL DEFAULT 0,
            completion_outcome TEXT NOT NULL DEFAULT '', flow_status TEXT NOT NULL DEFAULT '',
            stock_status TEXT NOT NULL DEFAULT '', lifecycle_changed_at TIMESTAMPTZ DEFAULT now(),
            operational_status_changed_at TIMESTAMPTZ DEFAULT now(), production_completed_at TIMESTAMPTZ,
            closed_at TIMESTAMPTZ
        );
        CREATE TEMP TABLE mini_queue_states (order_id TEXT, canonical_apparatus_id TEXT, state TEXT);
        CREATE TEMP TABLE mini_queue_action_events (
            id BIGSERIAL PRIMARY KEY, order_id TEXT, stage_node_id TEXT, action TEXT, to_state TEXT,
            payload_json JSONB DEFAULT '{}'::jsonb
        );
        CREATE TEMP TABLE mini_order_run_sessions (order_id TEXT, status TEXT,
            session_id TEXT, canonical_apparatus_id TEXT, stage_node_id TEXT,
            worker_role TEXT, worker_ref TEXT, worker_display_name TEXT,
            started_at TIMESTAMPTZ, updated_at TIMESTAMPTZ, payload_json JSONB);
        CREATE TEMP TABLE mini_progress_batches (
            order_id TEXT, wip_status TEXT, canonical_next_apparatus_id TEXT, processed_by_apparatus TEXT,
            canonical_apparatus_id TEXT, payload_json JSONB
        );
        CREATE TEMP TABLE mini_opening_wip_intakes (intake_id TEXT, order_id TEXT, status TEXT,
            source_apparatus TEXT, resume_apparatus TEXT, resume_stage_node_id TEXT);
        CREATE TEMP TABLE mini_opening_wip_batches (intake_id TEXT, wip_status TEXT);
        CREATE TEMP TABLE mini_production_order_lifecycle_events (
            event_id TEXT UNIQUE, order_id TEXT, from_status TEXT, to_status TEXT,
            completion_outcome TEXT, actor_role TEXT, actor_ref TEXT, actor_display_name TEXT,
            source_event_id TEXT, reason TEXT, lifecycle_version BIGINT, created_at TIMESTAMPTZ
        );
    ").execute(&pool).await.unwrap();
    pool
}

async fn seed(pool: &PgPool) {
    let map = serde_json::json!({
        "id": "split", "product_code": "TEST", "title": "Split work",
        "nodes": [
            {"id": "start", "kind": "start", "title": "Start"},
            {"id": "before", "kind": "apparatus", "title": "Rezka", "apparatus_id": "apparatus:test:rezka"},
            {"id": "lam1", "kind": "apparatus", "title": "Lam1", "apparatus_id": "apparatus:test:lam1",
             "alternative_group_id": "lamination", "alternative_assigned_apparatus_id": "apparatus:test:lam2"},
            {"id": "lam2", "kind": "apparatus", "title": "Lam2", "apparatus_id": "apparatus:test:lam2",
             "alternative_group_id": "lamination", "alternative_assigned_apparatus_id": "apparatus:test:lam2"},
            {"id": "after", "kind": "apparatus", "title": "Rezka", "apparatus_id": "apparatus:test:rezka"},
            {"id": "end", "kind": "end", "title": "End"}
        ],
        "edges": [
            {"from": "start", "to": "before"}, {"from": "before", "to": "lam1"},
            {"from": "before", "to": "lam2"}, {"from": "lam1", "to": "after"},
            {"from": "lam2", "to": "after"}, {"from": "after", "to": "end"}
        ]
    });
    sqlx::query("INSERT INTO mini_production_maps (id, map_json) VALUES ('split', $1)")
        .bind(map)
        .execute(pool)
        .await
        .unwrap();
    sqlx::raw_sql(
        "
        INSERT INTO mini_queue_states VALUES
            ('split', 'apparatus:test:rezka', 'completed'),
            ('split', 'apparatus:test:lam1', 'pending'),
            ('split', 'apparatus:test:lam2', 'completed');
        INSERT INTO mini_queue_action_events (order_id, stage_node_id, action, to_state) VALUES
            ('split', 'before', 'complete', 'completed'),
            ('split', 'lam1', 'complete', 'pending'),
            ('split', 'lam2', 'start', 'in_progress'),
            ('split', 'lam2', 'complete', 'pending'),
            ('split', 'after', 'complete', 'completed');
    ",
    )
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn postgres_stage_lifecycle_split_work_and_reconciliation_are_idempotent() {
    let pool = isolated_pool().await;
    seed(&pool).await;
    reconcile_alternative_order_lifecycles(&pool).await.unwrap();
    let status: String =
        sqlx::query_scalar("SELECT lifecycle_status FROM mini_production_maps WHERE id='split'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        status, "in_progress",
        "individual completed rolls are not the operation's end"
    );
    sqlx::raw_sql("
        INSERT INTO mini_queue_action_events (order_id, stage_node_id, action, to_state) VALUES
            ('split', 'lam2', 'complete', 'completed');
        INSERT INTO mini_queue_action_events (order_id, stage_node_id, action, to_state, payload_json) VALUES
            ('split', 'lam1', 'complete', 'in_progress', '{\"completion_request\":true}');
    ").execute(&pool).await.unwrap();
    reconcile_alternative_order_lifecycles(&pool).await.unwrap();
    let after: (String, String, i64, bool) = sqlx::query_as(
        "SELECT lifecycle_status, operational_status, lifecycle_version, production_completed_at IS NOT NULL
         FROM mini_production_maps WHERE id='split'").fetch_one(&pool).await.unwrap();
    assert_eq!(
        after,
        ("production_completed".into(), "completed".into(), 2, true)
    );
    assert_eq!(
        reconcile_alternative_order_lifecycles(&pool).await.unwrap(),
        0
    );
    let event_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mini_production_order_lifecycle_events")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(event_count, 1);
    let old_state: String = sqlx::query_scalar(
        "SELECT state FROM mini_queue_states WHERE canonical_apparatus_id='apparatus:test:lam1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        old_state, "pending",
        "repair must not rewrite individual apparatus history"
    );
    pool.close().await;
}

#[tokio::test]
async fn postgres_stage_lifecycle_latest_partial_event_blocks_old_completion() {
    let pool = isolated_pool().await;
    seed(&pool).await;
    sqlx::raw_sql(
        "
        INSERT INTO mini_queue_action_events (order_id, stage_node_id, action, to_state) VALUES
            ('split', 'lam1', 'complete', 'completed'),
            ('split', 'lam2', 'complete', 'pending');
    ",
    )
    .execute(&pool)
    .await
    .unwrap();
    reconcile_alternative_order_lifecycles(&pool).await.unwrap();
    let status: String =
        sqlx::query_scalar("SELECT lifecycle_status FROM mini_production_maps WHERE id='split'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "in_progress");
    pool.close().await;
}
