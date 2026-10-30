use super::*;
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    canonical_apparatus_service, postgres_test_database_options,
};
use crate::db::postgres_production_map::PostgresProductionMapStore;
use serde_json::json;

#[tokio::test]
async fn recorded_rezka_correction_updates_batch_and_reference_in_isolated_postgres() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin = match sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(3))
        .max_connections(1)
        .connect(&admin_url)
        .await
    {
        Ok(pool) => pool,
        Err(_) => {
            eprintln!("PostgreSQL unavailable; isolated recorded Rezka correction test skipped");
            return;
        }
    };
    let database = format!("rezka_correction_{:032x}", rand::random::<u128>());
    sqlx::query(&format!(r#"CREATE DATABASE "{database}""#))
        .execute(&admin)
        .await
        .expect("create isolated test database");
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &database))
        .await
        .expect("connect isolated test database");
    let result = exercise_correction(&pool).await;
    pool.close().await;
    let dropped = sqlx::query(&format!(r#"DROP DATABASE "{database}" WITH (FORCE)"#))
        .execute(&admin)
        .await;
    admin.close().await;
    dropped.expect("drop isolated test database");
    assert!(result.is_ok(), "{}", result.unwrap_err());
}

async fn exercise_correction(pool: &PgPool) -> Result<(), String> {
    apply_postgres_migrations_through_version(pool, "0121")
        .await
        .map_err(|e| e.to_string())?;
    canonical_apparatus_service(pool.clone())
        .bootstrap_factory_defaults()
        .await
        .map_err(|e| e.to_string())?;
    apply_foundation_migration(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO mini_production_maps(id,product_code,title,map_json)
        VALUES('rezka-correction-order','ITEM','Correction test','{}'::jsonb)",
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    let store = PostgresProductionMapStore::new(pool.clone());
    let actor = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "worker-correction".into(),
        display_name: "Worker".into(),
    };
    let session: OrderRunSession = serde_json::from_value(json!({
        "session_id":"rezka-correction-session", "order_id":"rezka-correction-order",
        "apparatus":"apparatus:default:asset-010", "stage_node_id":"cut", "status":"active",
        "worker_role":actor.role,"worker_ref":actor.ref_,"worker_display_name":actor.display_name,
        "started_at_unix":1,"updated_at_unix":2,"payload_json":{
            "rezka_output_cycle":"cycle-1","rezka_output_revision":8,
            "rezka_reference_frame_index":3,"rezka_recorded_kadr_counts":[1,1,1],
            "rezka_output_report":[
                {"frame_index":1,"batch_id":"other-roll","qr_payload":"other-qr",
                 "input":{"produced_qty":100.0,"gross_qty":13.0}},
                {"frame_index":3,"batch_id":"corrected-roll","qr_payload":"corrected-qr",
                 "input":{"produced_qty":100.0,"gross_qty":126.0,"finished_goods_kg":126.0,
                    "finished_goods_meter":100.0,"bobina_kg":0.5,"diameter":45.0}}
            ]}
    }))
    .map_err(|e| e.to_string())?;
    store
        .put_order_run_session(session)
        .await
        .map_err(|e| format!("seed session: {e}"))?;
    let batch: OrderProgressBatch = serde_json::from_value(json!({
        "batch_id":"corrected-roll","session_id":"rezka-correction-session",
        "started_at_unix":1,"completed_at_unix":2,"apparatus":"apparatus:default:asset-010",
        "order_id":"rezka-correction-order","action":"roll_complete","status":"completed",
        "produced_qty":100.0,"uom":"m","qr_payload":"corrected-qr",
        "label_item_code":"ITEM","label_item_name":"Product","executor_name":"Worker",
        "worker_role":actor.role,"worker_ref":actor.ref_,"worker_display_name":actor.display_name,
        "wip_status":"waiting","finished_goods_kg":126.0,"finished_goods_meter":100.0,
        "bobina_kg":0.5,"diameter":45.0,"payload_json":{"gross_qty":126.0}
    }))
    .map_err(|e| e.to_string())?;
    let mut seed = pool.begin().await.map_err(|e| e.to_string())?;
    put_order_progress_batch_tx(&mut seed, &batch)
        .await
        .map_err(|e| format!("seed batch: {e}"))?;
    seed.commit().await.map_err(|e| e.to_string())?;
    let mut input: ProgressBatchCorrectionInput = serde_json::from_value(json!({
        "batch_id":batch.batch_id,"expected_revision":1,"produced_qty":100.0,"uom":"m",
        "finished_goods_kg":12.6,"finished_goods_meter":100.0,"bobina_kg":0.5,
        "diameter":45.0,"reason":"Corrected decimal point"
    }))
    .map_err(|e| e.to_string())?;
    let corrected = store
        .correct_progress_batch(batch.clone(), input.clone(), actor.clone())
        .await
        .map_err(|e| format!("correct batch: {e}"))?;
    let payload = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload_json FROM mini_order_run_sessions WHERE session_id='rezka-correction-session'"
    ).fetch_one(pool).await.map_err(|e| e.to_string())?;
    if corrected.finished_goods_kg != Some(12.6)
        || corrected.revision != 2
        || payload["rezka_output_revision"] != 9
        || payload["rezka_reference_frame_index"] != 3
        || payload["rezka_output_report"][1]["input"]["gross_qty"] != 12.6
        || payload["rezka_output_report"][1]["batch_id"] != "corrected-roll"
        || payload["rezka_output_report"][1]["qr_payload"] != "corrected-qr"
        || payload["rezka_output_report"][0]["input"]["gross_qty"] != 13.0
    {
        return Err(format!("correction or reference mismatch: {payload}"));
    }
    input.finished_goods_kg = Some(14.0);
    if !matches!(
        store.correct_progress_batch(batch, input, actor).await,
        Err(ProductionMapError::ProgressBatchCorrectionConflict)
    ) {
        return Err("stale correction must be rejected".into());
    }
    let after: serde_json::Value = sqlx::query_scalar(
        "SELECT payload_json FROM mini_order_run_sessions WHERE session_id='rezka-correction-session'"
    ).fetch_one(pool).await.map_err(|e| e.to_string())?;
    if after != payload {
        return Err("rejected correction changed recorded measurements".into());
    }
    Ok(())
}
