//! The historical repair runs only in a newly created, disposable database.
use serde_json::{Value, json};

#[tokio::test]
#[ignore = "requires WIP_TIME_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn restores_each_roll_from_events_without_rewriting_history() {
    let url = std::env::var("WIP_TIME_TEST_DATABASE_URL").expect("explicit test server required");
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let name = format!("wip_times_{:032x}", rand::random::<u128>());
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
    sqlx::raw_sql(r#"
        CREATE TABLE mini_order_run_sessions (
            session_id text PRIMARY KEY, started_at timestamptz, updated_at timestamptz,
            status text, payload_json jsonb DEFAULT '{}');
        CREATE TABLE mini_order_progress_events (
            id bigserial PRIMARY KEY, event_id text, session_id text, batch_id text,
            action text, created_at timestamptz, payload_json jsonb DEFAULT '{}', produced_qty numeric DEFAULT 0);
        CREATE TABLE mini_progress_batches (
            batch_id text PRIMARY KEY, session_id text, parent_batch_id text DEFAULT '',
            created_at timestamptz, updated_at timestamptz, payload_json jsonb DEFAULT '{}');
        INSERT INTO mini_order_run_sessions VALUES
            ('s', to_timestamp(100), to_timestamp(99999), 'active', '{}'),
            ('r', to_timestamp(1000), to_timestamp(99999), 'completed', '{}'),
            ('d', to_timestamp(500), to_timestamp(99999), 'paused', '{}'),
            ('checkpoint', to_timestamp(100), to_timestamp(200), 'active',
                '{"_resume_pause":{"resumed_fingerprint":"must remain valid"}}');
        INSERT INTO mini_progress_batches (batch_id, session_id, created_at, updated_at) VALUES
            ('a', 's', to_timestamp(110), to_timestamp(99999)),
            ('b', 's', to_timestamp(230), to_timestamp(99999)),
            ('c', 's', to_timestamp(330), to_timestamp(99999)),
            ('after-missing-output', 'd', to_timestamp(650), to_timestamp(99999)),
            ('after-issue-output', 'd', to_timestamp(810), to_timestamp(99999)),
            ('frame-1', 'r', to_timestamp(1010), to_timestamp(99999)),
            ('frame-2', 'r', to_timestamp(1020), to_timestamp(99999)),
            ('frame-3', 'r', to_timestamp(1040), to_timestamp(99999)),
            ('frame-4', 'r', to_timestamp(1040), to_timestamp(99999)),
            ('missing-event', 's', to_timestamp(444), to_timestamp(99999)),
            ('missing-session', 'absent', to_timestamp(555), to_timestamp(99999));
        INSERT INTO mini_progress_batches VALUES
            ('remainder', 's', 'a', to_timestamp(777), to_timestamp(99999), '{"origin":"layer_remainder"}'),
            ('nested-remainder', 's', 'remainder', to_timestamp(888), to_timestamp(99999), '{"origin":"layer_remainder"}');
    "#).execute(&pool).await.unwrap();
    for (session, action, at, batch, payload) in [
        ("s", "start", 100, "", json!({})),
        ("s", "detach_roll", 110, "a", json!({})),
        ("s", "resume", 200, "a", json!({})),
        ("s", "freeze", 210, "", json!({})),
        ("s", "resume", 220, "", json!({})),
        ("s", "detach_roll", 230, "b", json!({})),
        ("s", "resume", 300, "", json!({})),
        ("s", "pause", 310, "", json!({"event":"re_pause"})),
        ("s", "resume", 320, "", json!({})),
        ("s", "roll_complete", 330, "c", json!({})),
        ("s", "merge", 340, "input", json!({})),
        ("s", "freeze", 350, "", json!({})),
        ("s", "resume", 360, "", json!({})),
        ("d", "start", 500, "", json!({})),
        ("d", "detach_roll", 520, "missing-output-row", json!({})),
        ("d", "resume", 600, "", json!({})),
        ("d", "detach_roll", 650, "after-missing-output", json!({})),
        ("d", "resume", 700, "", json!({})),
        (
            "d",
            "pause",
            710,
            "",
            json!({"rezka_frame_issues":[{"frame_index":1,"issue_note":"damaged"}]}),
        ),
        ("d", "resume", 800, "", json!({})),
        ("d", "detach_roll", 810, "after-issue-output", json!({})),
        ("r", "start", 1000, "input", json!({})),
        (
            "r",
            "roll_complete",
            1010,
            "frame-1",
            json!({"rezka_record_frame_index":1}),
        ),
        (
            "r",
            "roll_complete",
            1020,
            "frame-2",
            json!({"rezka_record_frame_index":2}),
        ),
        (
            "r",
            "roll_complete",
            1030,
            "frame-1",
            json!({"rezka_previously_recorded_batches":["frame-1","frame-2"]}),
        ),
        (
            "r",
            "complete",
            1040,
            "frame-3",
            json!({"rezka_output_batches":[{"batch_id":"frame-3"},{"batch_id":"frame-4"}]}),
        ),
        (
            "r",
            "complete",
            9999,
            "",
            json!({"complete_without_output":true}),
        ),
    ] {
        sqlx::query("INSERT INTO mini_order_progress_events (event_id,session_id,batch_id,action,created_at,payload_json,produced_qty) VALUES ($1,$2,$3,$4,to_timestamp($5),$6,$7)")
            .bind(format!("{session}-{at}")) .bind(session).bind(batch).bind(action)
            .bind(at as f64).bind(payload)
            .bind(if !batch.is_empty() && matches!(action, "pause" | "detach_roll" | "roll_complete" | "complete") { 10_i32 } else { 0 })
            .execute(&pool).await.unwrap();
    }
    let before: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(e) ORDER BY id) FROM mini_order_progress_events e",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../migrations/postgres/0151_immutable_wip_production_times.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    for (batch, start, end) in [
        ("a", 100, 110),
        ("b", 200, 230),
        ("c", 320, 330),
        ("after-missing-output", 600, 650),
        ("after-issue-output", 800, 810),
        ("frame-1", 1000, 1010),
        ("frame-2", 1000, 1020),
        ("frame-3", 1030, 1040),
        ("frame-4", 1030, 1040),
        ("remainder", 100, 110),
        ("nested-remainder", 100, 110),
        ("missing-event", 0, 444),
        ("missing-session", 0, 555),
    ] {
        let times: (i64, i64, i64) = sqlx::query_as("SELECT COALESCE(extract(epoch FROM production_started_at)::bigint,0), extract(epoch FROM production_completed_at)::bigint, extract(epoch FROM updated_at)::bigint FROM mini_progress_batches WHERE batch_id=$1")
            .bind(batch).fetch_one(&pool).await.unwrap();
        assert_eq!(times, (start, end, 99999), "{batch}");
    }
    let session: Value =
        sqlx::query_scalar("SELECT payload_json FROM mini_order_run_sessions WHERE session_id='s'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(session["wip_started_at_unix"], 330);
    assert_eq!(session["wip_waiting_for_resume"], false);
    let checkpoint: Value = sqlx::query_scalar(
        "SELECT payload_json FROM mini_order_run_sessions WHERE session_id='checkpoint'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        checkpoint,
        json!({"_resume_pause":{"resumed_fingerprint":"must remain valid"}})
    );
    let after: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(e) ORDER BY id) FROM mini_order_progress_events e",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before, after, "original event evidence is unchanged");
    let error = sqlx::query("UPDATE mini_progress_batches SET production_started_at=to_timestamp(101) WHERE batch_id='a'")
        .execute(&pool).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
