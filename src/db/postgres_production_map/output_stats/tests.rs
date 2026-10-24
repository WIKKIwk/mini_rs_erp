use super::*;
use crate::core::apparatus_standard::{
    ApparatusId, CanonicalApparatusService, test_support::standard_revisions,
};
use crate::core::production_map::{
    OrderProgressBatch, ProgressBatchCorrectionInput, QueueActionActor,
};
use crate::db::postgres::{apply_foundation_migration, apply_postgres_migrations_through_version};
use crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository;
use crate::db::postgres_production_map::PostgresProductionMapStore;
use std::sync::Arc;

const CUT: &str = "apparatus:default:asset-010";
const LAM: &str = "apparatus:default:asset-007";

async fn fixture() -> (PgPool, PgPool, String) {
    let url = std::env::var("OUTPUT_STATS_TEST_DATABASE_URL")
        .expect("explicit disposable test server required");
    let admin = PgPool::connect(&url).await.unwrap();
    let name = format!("output_stats_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPool::connect_with(
        url.parse::<sqlx::postgres::PgConnectOptions>()
            .unwrap()
            .database(&name),
    )
    .await
    .unwrap();
    apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    let canonical = CanonicalApparatusService::new(Arc::new(
        PostgresCanonicalApparatusRepository::new(pool.clone()),
    ));
    for revision in standard_revisions() {
        canonical
            .seed_for_test(revision.apparatus_id.clone(), revision.to_draft())
            .await
            .unwrap();
    }
    apply_foundation_migration(&pool).await.unwrap();
    let store = PostgresProductionMapStore::new(pool.clone());
    for id in ["stats-real", "training-stats", "demo-stats"] {
        let map = serde_json::from_value(json!({
            "id":id,"product_code":"STATS","title":"Stats fixture",
            "nodes":[{"id":"start","kind":"start","title":"Start"},
              {"id":"cut","kind":"apparatus","title":"Cut","apparatus_id":CUT,"rezka_kadr_count":1},
              {"id":"end","kind":"end","title":"End"}],
            "edges":[{"from":"start","to":"cut"},{"from":"cut","to":"end"}]
        }))
        .unwrap();
        store.put_map(map).await.unwrap();
    }
    (pool, admin, name)
}

fn request(from: &str, to: &str, bucket: &str, zone: &str) -> OutputStatsRequest {
    OutputStatsRequest::try_from(
        serde_json::from_value::<crate::core::production_map::output_stats::OutputStatsQuery>(
            json!({
                "from":from,"to":to,"bucket":bucket,"timezone":zone
            }),
        )
        .unwrap(),
    )
    .unwrap()
}

fn batch(id: &str, apparatus: &str, instant: &str, extra: Value) -> OrderProgressBatch {
    let mut value = json!({
        "batch_id":id,"session_id":"one-session-many-real-outputs","apparatus":apparatus,"order_id":"stats-real",
        "started_at_unix":0,"completed_at_unix":chrono::DateTime::parse_from_rfc3339(instant).unwrap().timestamp(),
        "action":"roll_complete","status":"completed","produced_qty":100.0,"uom":"m",
        "qr_payload":format!("QR-{id}"),"label_item_code":"STATS","label_item_name":"Stats",
        "executor_name":"Worker","worker_role":"aparatchi","worker_ref":"stats-worker","worker_display_name":"Worker",
        "wip_status":"waiting","current_apparatus":apparatus,"current_location":"Factory","next_apparatus":"",
        "finished_goods_meter":100.0,"finished_goods_kg":9.0,"bobina_kg":1.0,
        "payload_json":{"gross_qty":10.0,"production_timing_source":{"completion":"output_event"}}
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(value).unwrap()
}

async fn save(pool: &PgPool, batch: OrderProgressBatch) {
    PostgresProductionMapStore::new(pool.clone())
        .put_order_progress_batch(batch)
        .await
        .unwrap();
}

fn series<'a>(value: &'a Value, id: &str) -> &'a Value {
    value["series"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["apparatus_id"] == id)
        .unwrap()
}

async fn report(pool: &PgPool, req: &OutputStatsRequest, scope: Option<&[String]>) -> Value {
    serde_json::from_str(&load(pool, req, scope).await.unwrap().0).unwrap()
}

async fn fingerprint(pool: &PgPool) -> Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(
      'batches',(SELECT jsonb_agg(to_jsonb(b) ORDER BY batch_id) FROM mini_progress_batches b),
      'events',(SELECT jsonb_agg(to_jsonb(e) ORDER BY id) FROM mini_order_progress_events e),
      'sessions',(SELECT jsonb_agg(to_jsonb(s) ORDER BY session_id) FROM mini_order_run_sessions s),
      'opening',(SELECT jsonb_agg(to_jsonb(o) ORDER BY batch_id) FROM mini_opening_wip_batches o),
      'stock',(SELECT jsonb_agg(to_jsonb(s)) FROM mini_finished_goods_stock s))",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires OUTPUT_STATS_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn output_stats_postgres_contracts() {
    let (pool, admin, name) = fixture().await;
    let day = request(
        "2026-10-18T19:00:00Z",
        "2026-10-20T19:00:00Z",
        "day",
        "Asia/Tashkent",
    );
    for (id, at, extra) in [
        (
            "start",
            "2026-10-18T19:00:00Z",
            json!({"action":"pause","status":"resumed"}),
        ),
        (
            "detach",
            "2026-10-19T10:00:00Z",
            json!({"action":"detach_roll","status":"roll_detached"}),
        ),
        ("frame-a", "2026-10-19T11:00:00Z", json!({})),
        ("frame-b", "2026-10-19T11:00:00Z", json!({})),
        ("at-to", "2026-10-20T19:00:00Z", json!({})),
        (
            "remainder",
            "2026-10-19T11:00:00Z",
            json!({"parent_batch_id":"start","payload_json":{"origin":"layer_remainder"}}),
        ),
        (
            "training",
            "2026-10-19T11:00:00Z",
            json!({"order_id":"training-stats"}),
        ),
        (
            "demo",
            "2026-10-19T11:00:00Z",
            json!({"order_id":"demo-stats"}),
        ),
    ] {
        save(&pool, batch(id, CUT, at, extra)).await;
    }
    save(
        &pool,
        batch(
            "lam",
            LAM,
            "2026-10-19T12:00:00Z",
            json!({"produced_qty":40.0,"finished_goods_meter":null,"uom":"meter"}),
        ),
    )
    .await;
    sqlx::query("INSERT INTO mini_opening_wip_intakes(intake_id,idempotency_key,request_fingerprint,order_id,
      entry_apparatus,source_operation,current_location,resume_apparatus,resume_stage_node_id)
      VALUES ('opening-stats','opening-stats','fixture','stats-real',$1,'opening','Factory',$1,'cut')")
      .bind(CUT).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO mini_opening_wip_batches(batch_id,intake_id,order_id,sequence_no,qr_payload,
      quantity,uom,quantity_basis,label_item_code,label_item_name,finished_goods_meter,finished_goods_kg,bobina_kg)
      VALUES ('opening-stats','opening-stats','stats-real',1,'opening-qr',99999,'m','measured','STATS','Opening',99999,999,1)")
      .execute(&pool).await.unwrap();
    let initial = report(&pool, &day, None).await;
    assert_eq!(series(&initial, CUT)["totals"]["batch_count"], 4);
    assert_eq!(
        series(&initial, CUT)["totals"]["meters"]["value"].as_f64(),
        Some(400.0)
    );
    assert_eq!(
        series(&initial, CUT)["totals"]["net_kg"]["value"].as_f64(),
        Some(36.0)
    );
    assert_eq!(
        series(&initial, LAM)["totals"]["meters"]["value"].as_f64(),
        Some(40.0)
    );
    assert_eq!(series(&initial, CUT)["points"][1]["batch_count"], 0);
    assert_eq!(series(&initial, CUT)["points"][1]["net_kg"]["value"], 0);
    assert_eq!(initial["coverage"]["complete"], true);

    // A resume/reprint event is not another output. Moving/accepting/archiving
    // affects neither producer attribution nor the original production clock.
    sqlx::query("INSERT INTO mini_order_progress_events(event_id,session_id,batch_id,apparatus,canonical_apparatus_id,order_id,action) VALUES
      ('resume-1','one-session-many-real-outputs','start','Cut',$1,'stats-real','resume'),
      ('resume-2','one-session-many-real-outputs','start','Cut',$1,'stats-real','resume')")
      .bind(CUT).execute(&pool).await.unwrap();
    sqlx::query("UPDATE mini_progress_batches SET wip_status='processed',canonical_current_apparatus_id=$1,
      current_location='Warehouse',processed_by_apparatus='warehouse:Finished',updated_at=now() WHERE batch_id='start'")
      .bind(LAM).execute(&pool).await.unwrap();
    let moved = report(&pool, &day, None).await;
    assert_eq!(moved["series"], initial["series"]);

    // Correction audit overrides a legacy independent gross; a subsequent
    // tare/meter-only correction preserves the last actual kg correction.
    let store = PostgresProductionMapStore::new(pool.clone());
    let actor = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "stats-worker".into(),
        display_name: "Worker".into(),
    };
    let mut correction: ProgressBatchCorrectionInput = serde_json::from_value(json!({
      "batch_id":"detach","expected_revision":1,"produced_qty":120.123456,"uom":"m", "finished_goods_meter":120.123456,
      "finished_goods_kg":12.123456,"bobina_kg":1.0,"reason":"Measured correction"
    })).unwrap();
    let current = store.progress_batch("detach").await.unwrap().unwrap();
    let current = store
        .correct_progress_batch(current, correction.clone(), actor.clone())
        .await
        .unwrap();
    correction.expected_revision = 2;
    correction.bobina_kg = Some(2.0);
    correction.reason = "Tare correction".into();
    store
        .correct_progress_batch(current, correction, actor)
        .await
        .unwrap();
    // Simulate pre-sync legacy payload; the kg-only audit remains authoritative.
    sqlx::query("UPDATE mini_progress_batches SET payload_json=payload_json || '{\"gross_qty\":10}' WHERE batch_id='detach'")
      .execute(&pool).await.unwrap();
    let corrected = report(&pool, &day, None).await;
    assert_eq!(
        series(&corrected, CUT)["totals"]["meters"]["value"],
        json!(420.123456)
    );
    assert_eq!(
        series(&corrected, CUT)["totals"]["net_kg"]["value"],
        json!(37.123456)
    );
    assert_eq!(series(&corrected, CUT)["points"][1]["batch_count"], 0);
    assert_eq!(series(&corrected, CUT)["totals"]["batch_count"], 4);

    // Missing metrics remain independent from coverage and known zero.
    let metric_req = request("2026-09-01T00:00:00Z", "2026-09-02T00:00:00Z", "day", "UTC");
    for (id, extra) in [
        ("no-tare", json!({"bobina_kg":null})),
        (
            "no-gross",
            json!({"finished_goods_kg":null,"payload_json":{}}),
        ),
        ("invalid-tare", json!({"bobina_kg":11.0})),
        ("zero", json!({"finished_goods_meter":0.0,"bobina_kg":10.0})),
        (
            "not-meters",
            json!({"finished_goods_meter":null,"uom":"kg"}),
        ),
        ("pieces", json!({"finished_goods_meter":null,"uom":"dona"})),
        ("gross-zero", json!({"payload_json":{"gross_qty":0.0}})),
        ("bad-gross", json!({"payload_json":{"gross_qty":"invalid"}})),
        (
            "invalid-meter",
            json!({"finished_goods_meter":null,"uom":"kg","payload_json":{"gross_qty":-1.0}}),
        ),
    ] {
        save(&pool, batch(id, CUT, "2026-09-01T12:00:00Z", extra)).await;
    }
    let metrics = report(&pool, &metric_req, Some(&[CUT.into()])).await;
    let totals = &series(&metrics, CUT)["totals"];
    assert_eq!(totals["batch_count"], 9);
    assert_eq!(
        totals["meters"],
        json!({"value":null,"known_value":500.0,"missing_count":3})
    );
    assert_eq!(
        totals["net_kg"],
        json!({"value":null,"known_value":18.0,"missing_count":6})
    );
    assert_eq!(metrics["coverage"]["complete"], true);

    // More than the WIP list's 500 limit; tiny fractions sum exactly once.
    sqlx::query("INSERT INTO mini_progress_batches(batch_id,session_id,apparatus,canonical_apparatus_id,order_id,action,status,
      produced_qty,uom,qr_payload,label_item_code,label_item_name,finished_goods_meter,finished_goods_kg,bobina_kg,production_completed_at)
      SELECT 'bulk-'||n,'bulk-session','Cut',$1,'stats-real','roll_complete','completed',1,'m','bulk-qr-'||n,'STATS','Stats',
        0.000001,0.000002,0.000001,'2026-08-01T12:00:00Z'::timestamptz FROM generate_series(1,601) n")
      .bind(CUT).execute(&pool).await.unwrap();
    let bulk_req = request(
        "2026-08-01T00:00:00Z",
        "2026-08-02T00:00:00Z",
        "hour",
        "UTC",
    );
    let bulk = report(&pool, &bulk_req, Some(&[CUT.into()])).await;
    assert_eq!(series(&bulk, CUT)["totals"]["batch_count"], 601);
    assert_eq!(
        series(&bulk, CUT)["totals"]["meters"]["value"],
        json!(0.000601)
    );
    assert_eq!(
        series(&bulk, CUT)["totals"]["net_kg"]["value"],
        json!(0.000601)
    );
    assert_eq!(
        series(&bulk, CUT)["points"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["batch_count"].as_i64().unwrap())
            .sum::<i64>(),
        601
    );
    // The HTTP JSON must retain exact micro-units even beyond f64's range.
    sqlx::query("INSERT INTO mini_progress_batches(batch_id,session_id,apparatus,canonical_apparatus_id,order_id,action,status,
      produced_qty,uom,qr_payload,label_item_code,label_item_name,finished_goods_meter,production_completed_at)
      SELECT 'large-'||n,'large-session','Cut',$1,'stats-real','complete','completed',1,'m','large-qr-'||n,'STATS','Stats',
        999999999999.000001,'2026-07-01T12:00:00Z'::timestamptz FROM generate_series(1,2) n")
      .bind(CUT).execute(&pool).await.unwrap();
    let large_req = request("2026-07-01T00:00:00Z", "2026-07-02T00:00:00Z", "day", "UTC");
    let large = load(&pool, &large_req, Some(&[CUT.into()])).await.unwrap();
    assert!(large.0.contains("1999999999998.000002"));

    // Both occurrences of a repeated local hour have their own aggregate.
    for (id, at) in [
        ("dst-first", "2025-11-02T01:10:00-04:00"),
        ("dst-second", "2025-11-02T01:10:00-05:00"),
    ] {
        save(&pool, batch(id, CUT, at, json!({}))).await;
    }
    let dst_req = request(
        "2025-11-02T00:00:00-04:00",
        "2025-11-02T03:00:00-05:00",
        "hour",
        "America/New_York",
    );
    let dst = report(&pool, &dst_req, Some(&[CUT.into()])).await;
    let p = &series(&dst, CUT)["points"];
    assert_eq!(p.as_array().unwrap().len(), 4);
    assert_eq!(p[1]["start"], "2025-11-02T01:00:00-04:00");
    assert_eq!(p[2]["start"], "2025-11-02T01:00:00-05:00");
    assert_eq!(p[1]["batch_count"], 1);
    assert_eq!(p[2]["batch_count"], 1);

    // A legacy catalog gap never drops the ID; an undated row remains undated.
    sqlx::raw_sql("CREATE SCHEMA stats_legacy;
      CREATE TABLE stats_legacy.mini_apparatus(id text PRIMARY KEY,name text);
      CREATE TABLE stats_legacy.mini_progress_batches(LIKE public.mini_progress_batches);
      ALTER TABLE stats_legacy.mini_progress_batches ALTER COLUMN production_completed_at DROP NOT NULL;
      CREATE TABLE stats_legacy.mini_progress_batch_corrections(LIKE public.mini_progress_batch_corrections);
      INSERT INTO stats_legacy.mini_progress_batches SELECT * FROM public.mini_progress_batches WHERE batch_id='frame-a';
      INSERT INTO stats_legacy.mini_progress_batches SELECT * FROM public.mini_progress_batches WHERE batch_id='frame-b';
      UPDATE stats_legacy.mini_progress_batches SET production_completed_at=NULL WHERE batch_id='frame-b';")
      .execute(&pool).await.unwrap();
    let legacy = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET search_path TO stats_legacy, public")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let historical = report(&legacy, &day, None).await;
    assert_eq!(series(&historical, CUT)["apparatus_name"], CUT);
    assert_eq!(series(&historical, CUT)["totals"]["batch_count"], 1);
    assert_eq!(
        historical["coverage"]["excluded_unknown_timestamp_count"],
        1
    );
    assert_eq!(historical["coverage"]["complete"], false);
    sqlx::query("INSERT INTO stats_legacy.mini_apparatus SELECT 'apparatus:stats:asset-'||n, 'Test '||n FROM generate_series(1,50) n")
      .execute(&pool).await.unwrap();
    let excessive = request(
        "2024-01-01T00:00:00Z",
        "2024-03-01T00:00:00Z",
        "hour",
        "UTC",
    );
    assert_eq!(
        load(&legacy, &excessive, None).await.unwrap_err(),
        OutputStatsError::Invalid("output_stats_too_many_points")
    );
    legacy.close().await;

    // Provenance warnings are scoped. Unknown time is excluded across the
    // selected producer's entire history, because its interval is unknowable.
    save(
        &pool,
        batch(
            "approx",
            CUT,
            "2026-10-19T12:00:00Z",
            json!({"payload_json":{"production_timing_source":{"completion":"batch_created_at"}}}),
        ),
    )
    .await;
    save(
        &pool,
        batch("unknown", LAM, "1970-01-01T00:00:00Z", json!({})),
    )
    .await;
    let scoped = report(&pool, &day, Some(&[CUT.into()])).await;
    assert_eq!(scoped["coverage"]["complete"], true);
    assert_eq!(scoped["coverage"]["approximate_timestamp_count"], 1);
    assert_eq!(scoped["series"].as_array().unwrap().len(), 1);
    let all = report(&pool, &day, None).await;
    assert_eq!(all["coverage"]["complete"], false);
    assert_eq!(all["coverage"]["excluded_unknown_timestamp_count"], 1);
    assert_eq!(series(&all, LAM)["totals"]["batch_count"], 1);
    let mut single = day.clone();
    single.apparatus = Some(CUT.into());
    assert_eq!(
        report(&pool, &single, None).await["series"],
        scoped["series"]
    );
    single.apparatus = Some(
        ApparatusId::new("apparatus:test:unknown")
            .unwrap()
            .to_string(),
    );
    assert_eq!(
        load(&pool, &single, None).await.unwrap_err(),
        OutputStatsError::Invalid("output_stats_unknown_apparatus")
    );

    // A report transaction is READ ONLY and keeps all business data unchanged.
    let before = fingerprint(&pool).await;
    report(&pool, &day, None).await;
    assert_eq!(before, fingerprint(&pool).await);
    // Actual database failures are errors, never successful zero production.
    let broken = PgPool::connect_lazy("postgres://nobody@127.0.0.1:1/missing").unwrap();
    broken.close().await;
    assert_eq!(
        load(&broken, &day, None).await.unwrap_err(),
        OutputStatsError::StoreFailed
    );
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {name} WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
