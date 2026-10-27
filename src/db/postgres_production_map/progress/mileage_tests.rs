use serde_json::{Value, json};
use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;

use super::put_order_progress_batch_tx;
use crate::core::production_map::{
    OrderProgressBatch, OrderProgressBatchStatus, OrderProgressBatchWipStatus, OrderRunSession,
    OrderRunStatus, queue_state::ApparatusQueueAction,
};
use crate::db::postgres_qolip::mileage::{
    begin_cycle_tx, capture_session_cycles_tx, correct_output_tx,
};

const PRINT: &str = "apparatus:mileage:print";
const LAMINATE: &str = "apparatus:mileage:laminate";

async fn setup(tx: &mut Transaction<'_, Postgres>) {
    let schema = format!(
        "qolip_mileage_{}_{:x}",
        std::process::id(),
        rand::random::<u32>()
    );
    sqlx::raw_sql(&format!(
        "CREATE SCHEMA {schema}; SET LOCAL search_path TO {schema}, public;
         CREATE TABLE {schema}.mini_qolip_product_specs(qolip_code text PRIMARY KEY,size integer DEFAULT 1)"
    ))
    .execute(&mut **tx)
    .await
    .unwrap();
    let ddl = include_str!("../../../../migrations/postgres/0166_qolip_mileage.sql")
        .replace("public.", &format!("{schema}."))
        .lines()
        .filter(|line| !line.starts_with("ALTER ") && !line.starts_with("GRANT "))
        .collect::<Vec<_>>()
        .join("\n");
    sqlx::raw_sql(&ddl).execute(&mut **tx).await.unwrap();
    sqlx::raw_sql(
        "CREATE TABLE mini_apparatus(id text PRIMARY KEY,name text,execution_profile_json jsonb);
         CREATE TABLE mini_order_run_sessions(session_id text PRIMARY KEY,canonical_apparatus_id text,
             order_id text,payload_json jsonb,created_at timestamptz NOT NULL DEFAULT now());
         CREATE TABLE mini_progress_batches(
             batch_id text PRIMARY KEY,session_id text,apparatus text,canonical_apparatus_id text,
             order_id text,action text,status text,produced_qty numeric(18,6),uom text,qr_payload text,
             label_item_code text,label_item_name text,executor_name text,worker_role text,worker_ref text,
             worker_display_name text,wip_status text,current_apparatus text,canonical_current_apparatus_id text,
             current_location text,next_apparatus text,canonical_next_apparatus_id text,parent_batch_id text,
             used_by_session_id text,used_by_apparatus text,canonical_used_by_apparatus_id text,
             processed_by_session_id text,processed_by_apparatus text,canonical_processed_by_apparatus_id text,
             return_ink_kg numeric,lamination_print_leftover_rolls numeric,lamination_film_leftover_rolls numeric,
             rezka_bosma_waste numeric,rezka_lamination_waste numeric,rezka_edge_waste numeric,total_waste numeric,
             finished_goods_kg numeric,bobina_kg numeric,finished_goods_meter numeric,diameter numeric,description text,
             payload_json jsonb,revision bigint,production_started_at timestamptz,production_completed_at timestamptz,
             created_at timestamptz,updated_at timestamptz);",
    ).execute(&mut **tx).await.unwrap();
    for (id, operation) in [(PRINT, "print"), (LAMINATE, "laminate")] {
        sqlx::query("INSERT INTO mini_apparatus VALUES($1,$1,$2)")
            .bind(id)
            .bind(json!({"operation":operation}))
            .execute(&mut **tx)
            .await
            .unwrap();
    }
}

async fn session(
    tx: &mut Transaction<'_, Postgres>,
    id: &str,
    apparatus: &str,
    started: i64,
    owner: bool,
    inherited: Option<Value>,
) -> OrderRunSession {
    let mut payload = inherited.unwrap_or_else(|| json!({"qolip_codes":["A","B"]}));
    payload["qolip_lock_owner"] = json!(owner);
    sqlx::query("INSERT INTO mini_order_run_sessions(session_id,canonical_apparatus_id,order_id,payload_json,created_at)
        VALUES($1,$2,'order-mileage',$3,to_timestamp($4::float8))")
        .bind(id).bind(apparatus).bind(&payload).bind(started as f64).execute(&mut **tx).await.unwrap();
    let mut session = OrderRunSession {
        session_id: id.into(),
        apparatus: apparatus.into(),
        order_id: "order-mileage".into(),
        stage_node_id: "stage-print".into(),
        status: OrderRunStatus::Active,
        worker_role: "aparatchi".into(),
        worker_ref: "worker".into(),
        worker_display_name: "Worker".into(),
        started_at_unix: started,
        updated_at_unix: started,
        payload_json: payload,
    };
    capture_session_cycles_tx(tx, &mut session).await.unwrap();
    sqlx::query("UPDATE mini_order_run_sessions SET payload_json=$2 WHERE session_id=$1")
        .bind(id)
        .bind(&session.payload_json)
        .execute(&mut **tx)
        .await
        .unwrap();
    session
}

fn batch(id: &str, session: &OrderRunSession, meter: f64) -> OrderProgressBatch {
    OrderProgressBatch {
        batch_id: id.into(),
        revision: 1,
        session_id: session.session_id.clone(),
        started_at_unix: session.started_at_unix,
        completed_at_unix: session.started_at_unix + 10,
        apparatus: session.apparatus.clone(),
        order_id: session.order_id.clone(),
        action: ApparatusQueueAction::RollComplete,
        status: OrderProgressBatchStatus::Completed,
        produced_qty: meter,
        uom: "m".into(),
        qr_payload: format!("qr-{id}"),
        label_item_code: "item".into(),
        label_item_name: "Label".into(),
        executor_name: "Worker".into(),
        worker_role: "aparatchi".into(),
        worker_ref: "worker".into(),
        worker_display_name: "Worker".into(),
        wip_status: OrderProgressBatchWipStatus::Waiting,
        status_detail: Default::default(),
        current_apparatus: session.apparatus.clone(),
        current_location: String::new(),
        next_apparatus: String::new(),
        parent_batch_id: String::new(),
        used_by_session_id: String::new(),
        used_by_apparatus: String::new(),
        processed_by_session_id: String::new(),
        processed_by_apparatus: String::new(),
        return_ink_kg: None,
        lamination_print_leftover_rolls: None,
        lamination_film_leftover_rolls: None,
        rezka_bosma_waste: None,
        rezka_lamination_waste: None,
        rezka_edge_waste: None,
        total_waste: None,
        finished_goods_kg: Some(100.0),
        bobina_kg: Some(1.0),
        finished_goods_meter: Some(meter),
        diameter: None,
        description: String::new(),
        payload_json: session.payload_json.clone(),
    }
}

async fn totals(tx: &mut Transaction<'_, Postgres>, code: &str) -> Vec<(String, f64, bool)> {
    sqlx::query_as(
        "SELECT rework_id,meter::float8,ended_at IS NULL FROM mini_qolip_mileage_cycles
        WHERE qolip_code_key=lower($1) ORDER BY started_at,id",
    )
    .bind(code)
    .fetch_all(&mut **tx)
    .await
    .unwrap()
}

#[tokio::test]
async fn qolip_mileage_wip_pipeline_counts_actual_prints_once_and_keeps_return_cycles() {
    let Ok(url) = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL") else {
        eprintln!("MINI_ERP_TEST_ADMIN_DATABASE_URL absent; isolated PostgreSQL test skipped");
        return;
    };
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    setup(&mut tx).await;
    let base = OffsetDateTime::now_utc().unix_timestamp() - 1000;
    sqlx::query("INSERT INTO mini_qolip_product_specs(qolip_code) VALUES('A'),('B'),('untracked')")
        .execute(&mut *tx)
        .await
        .unwrap();
    let before = session(&mut tx, "before", PRINT, base + 50, true, None).await;
    let old_batch = batch("before-output", &before, 50.0);
    put_order_progress_batch_tx(&mut tx, &old_batch)
        .await
        .unwrap();
    assert!(totals(&mut tx, "A").await.is_empty());
    for code in ["A", "B"] {
        begin_cycle_tx(
            &mut tx,
            "return-1",
            code,
            OffsetDateTime::from_unix_timestamp(base + 100).unwrap(),
        )
        .await
        .unwrap();
    }
    sqlx::query("UPDATE mini_qolip_product_specs SET qolip_code='untracked-renamed' WHERE qolip_code='untracked'")
        .execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE mini_qolip_product_specs SET size=2 WHERE qolip_code='A'")
        .execute(&mut *tx)
        .await
        .unwrap();
    for statement in [
        "UPDATE mini_qolip_product_specs SET qolip_code='A-new' WHERE qolip_code='A'",
        "DELETE FROM mini_qolip_product_specs WHERE qolip_code='A'",
    ] {
        sqlx::query("SAVEPOINT identity_guard")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query(statement).execute(&mut *tx).await.unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.constraint()),
            Some("mini_qolip_mileage_identity_locked")
        );
        sqlx::query("ROLLBACK TO SAVEPOINT identity_guard")
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    let printing = session(&mut tx, "print-1", PRINT, base + 200, true, None).await;
    let mut output = batch("printed-output", &printing, 1000.0);
    put_order_progress_batch_tx(&mut tx, &output).await.unwrap();
    put_order_progress_batch_tx(&mut tx, &output).await.unwrap();
    assert_eq!(
        totals(&mut tx, "A").await,
        vec![("return-1".into(), 1000.0, true)]
    );
    assert_eq!(
        totals(&mut tx, "B").await,
        vec![("return-1".into(), 1000.0, true)]
    );
    // A downstream stage carries print lineage, but did not use the plates.
    let laminating = session(
        &mut tx,
        "laminate",
        LAMINATE,
        base + 220,
        false,
        Some(printing.payload_json.clone()),
    )
    .await;
    put_order_progress_batch_tx(&mut tx, &batch("laminated-output", &laminating, 1000.0))
        .await
        .unwrap();
    let mut opening = batch("imported-output", &printing, 700.0);
    opening.payload_json["origin"] = json!("opening_wip");
    put_order_progress_batch_tx(&mut tx, &opening)
        .await
        .unwrap();
    let sources: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mini_qolip_mileage_contributions WHERE cycle_id IS NOT NULL",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(sources, 2, "only A and B at the actual print output count");
    // Only the returned A resets; B continues its previous mileage.
    let return_at = OffsetDateTime::from_unix_timestamp(base + 300).unwrap();
    begin_cycle_tx(&mut tx, "return-2", "A", return_at)
        .await
        .unwrap();
    begin_cycle_tx(&mut tx, "return-2", "a", return_at)
        .await
        .unwrap();
    assert_eq!(
        totals(&mut tx, "A").await,
        vec![
            ("return-1".into(), 1000.0, false),
            ("return-2".into(), 0.0, true)
        ]
    );
    assert_eq!(totals(&mut tx, "B").await[0].1, 1000.0);
    // Ordinary stock upserts/reprints never add mileage or enroll a new cycle.
    output.finished_goods_meter = Some(1200.0);
    output.started_at_unix += 1;
    output.completed_at_unix += 1;
    put_order_progress_batch_tx(&mut tx, &output).await.unwrap();
    assert_eq!(totals(&mut tx, "A").await[1].1, 0.0);
    assert_eq!(totals(&mut tx, "B").await[0].1, 1000.0);
    // Explicit meter correction adjusts the original source's old cycle.
    correct_output_tx(&mut tx, &output).await.unwrap();
    correct_output_tx(&mut tx, &output).await.unwrap();
    assert_eq!(
        totals(&mut tx, "A").await,
        vec![
            ("return-1".into(), 1200.0, false),
            ("return-2".into(), 0.0, true)
        ]
    );
    assert_eq!(totals(&mut tx, "B").await[0].1, 1200.0);
    let mut without_meter = output.clone();
    without_meter.finished_goods_meter = None;
    without_meter.uom = "kg".into();
    assert_eq!(
        correct_output_tx(&mut tx, &without_meter).await,
        Err(crate::core::production_map::ProductionMapError::ProgressInputInvalid)
    );
    correct_output_tx(&mut tx, &old_batch).await.unwrap();
    // An old pre-tracking session still carries off/null at a late output.
    put_order_progress_batch_tx(&mut tx, &batch("late-old-output", &before, 300.0))
        .await
        .unwrap();
    assert_eq!(totals(&mut tx, "A").await[1].1, 0.0);
    let next = session(&mut tx, "print-2", PRINT, base + 400, true, None).await;
    put_order_progress_batch_tx(&mut tx, &batch("new-output", &next, 250.0))
        .await
        .unwrap();
    assert_eq!(totals(&mut tx, "A").await[1].1, 250.0);
    assert_eq!(totals(&mut tx, "B").await[0].1, 1450.0);
    let source_time: (i64, i64) = sqlx::query_as(
        "SELECT EXTRACT(EPOCH FROM production_started_at)::bigint,
        EXTRACT(EPOCH FROM production_completed_at)::bigint FROM mini_qolip_mileage_contributions
        WHERE batch_id='printed-output' AND qolip_code_key='a'",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(source_time, (base + 200, base + 210));
    tx.rollback().await.unwrap();
    pool.close().await;
}
