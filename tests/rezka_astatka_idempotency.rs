#![cfg(feature = "verification")]
//! Real transaction checks run only against an explicitly supplied disposable server.
use mini_rs_erp::{
    core::production_map::*,
    db::{postgres, postgres_production_map::PostgresProductionMapStore},
};
use serde_json::json;
use std::sync::Arc;
const CUT: &str = "apparatus:default:asset-010";
const LAM: &str = "apparatus:default:asset-007";
const ORDER: &str = "zakaz-rezka-receipts";
fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "aparatchi".into(),
        ref_: "receipt-worker".into(),
        display_name: "Worker".into(),
    }
}
fn service(pool: &sqlx::PgPool) -> ProductionMapService {
    ProductionMapService::new(
        Arc::new(PostgresProductionMapStore::new(pool.clone())),
        Arc::new(CanonicalServiceApparatusResolver::new(
            postgres::canonical_apparatus_service(pool.clone()),
        )),
    )
}
async fn submit(
    service: &ProductionMapService,
    session: &str,
    request: &str,
    waste: f64,
) -> Result<RezkaAstatkaReport, ProductionMapError> {
    service
        .record_rezka_astatka(
            CUT,
            ORDER,
            actor(),
            Some(waste),
            None,
            None,
            None,
            None,
            None,
            None,
            "",
            "cut",
            session,
            request,
        )
        .await
}

#[tokio::test]
#[ignore = "requires ASTATKA_TEST_DATABASE_URL pointing at a disposable PostgreSQL server"]
async fn postgres_rezka_astatka_atomic_retry_session_guard_and_restart() {
    let url = std::env::var("ASTATKA_TEST_DATABASE_URL").expect("disposable database URL required");
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let name = format!("rezka_astatka_test_{:032x}", rand::random::<u128>());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    let options = url
        .parse::<sqlx::postgres::PgConnectOptions>()
        .unwrap()
        .database(&name);
    let pool = sqlx::PgPool::connect_with(options.clone()).await.unwrap();
    postgres::apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    postgres::canonical_apparatus_service(pool.clone())
        .bootstrap_factory_defaults()
        .await
        .unwrap();
    postgres::apply_foundation_migration(&pool).await.unwrap();
    let s = service(&pool);
    s.upsert_map(serde_json::from_value(json!({
        "id":ORDER,"product_code":"RECEIPTS","title":"Receipt fixture",
        "nodes":[{"id":"start","kind":"start","title":"Start"},
            {"id":"cut","kind":"apparatus","title":"Cut","apparatus_id":CUT},
            {"id":"lam","kind":"apparatus","title":"Lamination","apparatus_id":LAM},
            {"id":"end","kind":"end","title":"End"}],
        "edges":[{"from":"start","to":"cut"},{"from":"cut","to":"lam"},{"from":"lam","to":"end"}]
    })).unwrap()).await.unwrap();
    let store = PostgresProductionMapStore::new(pool.clone());
    let original = OrderRunSession {
        session_id: "original".into(),
        order_id: ORDER.into(),
        apparatus: CUT.into(),
        stage_node_id: "cut".into(),
        status: OrderRunStatus::Completed,
        worker_role: actor().role,
        worker_ref: actor().ref_,
        worker_display_name: "Worker".into(),
        started_at_unix: 100,
        updated_at_unix: 101,
        payload_json: json!({"stage_work_protocol":1}),
    };
    store.put_order_run_session(original.clone()).await.unwrap();
    // Independent service guards cannot mask the database retry race.
    let second = service(&pool);
    let (a, b) = tokio::join!(
        submit(&s, "original", "first-request", 2.5),
        submit(&second, "original", "first-request", 2.5)
    );
    let first = a.unwrap();
    assert_eq!(first, b.unwrap());
    assert_eq!(
        store
            .rezka_astatka_reports_for_order(ORDER)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        submit(&s, "original", "first-request", 9.0).await,
        Err(ProductionMapError::ProgressInputInvalid)
    );
    submit(&s, "original", "additional-request", 0.5)
        .await
        .unwrap();
    let reports = store.rezka_astatka_reports_for_order(ORDER).await.unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports.iter().map(|r| r.total_waste).sum::<f64>(), 3.0);

    let mut replacement = original.clone();
    replacement.session_id = "reactivated".into();
    replacement.started_at_unix = 102;
    replacement.updated_at_unix = 103;
    store
        .put_order_run_session(replacement.clone())
        .await
        .unwrap();
    assert_eq!(
        submit(&s, "original", "old-dialog-request", 7.0).await,
        Err(ProductionMapError::QueueActionNotAllowed)
    );
    // Also reject a prepared report if a different process replaces its anchor
    // after the service read and before the transaction acquires its locks.
    let mut stale = first.clone();
    stale.report_id = "stale-prepared".into();
    assert_eq!(
        store
            .commit_rezka_astatka_report(
                "stale-prepared-request".into(),
                json!({"session_id":"original"}),
                stale,
                original,
                actor()
            )
            .await,
        Err(ProductionMapError::QueueActionNotAllowed)
    );
    let current = store
        .order_run_sessions_for_order(ORDER)
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.session_id == "reactivated")
        .unwrap();
    assert!(current.payload_json.get("stage_work_report").is_none());
    assert_eq!(
        store
            .rezka_astatka_reports_for_order(ORDER)
            .await
            .unwrap()
            .len(),
        2
    );

    // A failed receipt write rolls the report and completion stamp back together.
    sqlx::query("ALTER TABLE mini_rezka_astatka_operations ADD CONSTRAINT injected_failure CHECK (request_id <> 'rollback-request')")
        .execute(&pool).await.unwrap();
    assert_eq!(
        submit(&s, "reactivated", "rollback-request", 1.0).await,
        Err(ProductionMapError::StoreFailed)
    );
    assert_eq!(
        store
            .rezka_astatka_reports_for_order(ORDER)
            .await
            .unwrap()
            .len(),
        2
    );
    let current = store
        .order_run_sessions_for_order(ORDER)
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.session_id == "reactivated")
        .unwrap();
    assert!(current.payload_json.get("stage_work_report").is_none());
    sqlx::query("ALTER TABLE mini_rezka_astatka_operations DROP CONSTRAINT injected_failure")
        .execute(&pool)
        .await
        .unwrap();
    submit(&s, "reactivated", "rollback-request", 0.0)
        .await
        .unwrap();
    pool.close().await;
    let restarted_pool = sqlx::PgPool::connect_with(options).await.unwrap();
    let restarted = service(&restarted_pool);
    assert_eq!(
        submit(&restarted, "original", "first-request", 2.5)
            .await
            .unwrap(),
        first
    );
    let store = PostgresProductionMapStore::new(restarted_pool.clone());
    assert_eq!(
        store
            .rezka_astatka_reports_for_order(ORDER)
            .await
            .unwrap()
            .len(),
        3
    );
    restarted_pool.close().await;
    sqlx::query(&format!("DROP DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
