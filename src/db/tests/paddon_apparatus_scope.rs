use super::*;
use crate::core::production_map::{ProductionMapError, ProductionMapService, ProductionMapStorePort,
    QueueActionActor, QueueProgressInput, queue_state::ApparatusQueueAction as Action};
use crate::core::production_map::CanonicalServiceApparatusResolver;
use crate::db::postgres::{apply_foundation_migration, apply_postgres_migrations_through_version,
    postgres_test_database_options};
use crate::db::postgres_production_map::PostgresProductionMapStore;

#[tokio::test]
async fn paddon_apparatus_scope_backfill_preserves_unresolved_and_conflicting_history() {
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin = PgPool::connect(&url).await.unwrap();
    let schema = format!("paddon_scope_migration_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
    let search_path = format!("SET search_path TO {schema}");
    let pool = sqlx::postgres::PgPoolOptions::new().after_connect(move |conn, _| {
        let sql = search_path.clone();
        Box::pin(async move { sqlx::query(&sql).execute(conn).await?; Ok(()) })
    }).connect(&url).await.unwrap();
    sqlx::raw_sql("CREATE TABLE mini_paddons(id TEXT PRIMARY KEY, code TEXT UNIQUE, location TEXT DEFAULT '', updated_at TIMESTAMPTZ DEFAULT now());
        CREATE TABLE mini_apparatus(id TEXT PRIMARY KEY, execution_profile_json JSONB);
        INSERT INTO mini_apparatus VALUES ('cut-a','{\"operation\":\"cut\"}'),('cut-b','{\"operation\":\"cut\"}'),('lam','{\"operation\":\"laminate\"}');
        CREATE TABLE mini_active_rezka_paddons(paddon_code TEXT, apparatus_id TEXT);
        CREATE TABLE mini_paddon_print_successors(source_code TEXT, paddon_code TEXT, apparatus_id TEXT);
        CREATE TABLE mini_progress_batches(batch_id TEXT, canonical_apparatus_id TEXT);
        CREATE TABLE mini_paddon_items(paddon_id TEXT, progress_batch_id TEXT);
        INSERT INTO mini_paddons(id,code,location) VALUES ('location','location','cut-a'),('active','active',''),('mixed','mixed',''),
          ('wip','wip','Warehouse'),('mixed-wip','mixed-wip',''),('source','source',''),('next','next',''),('empty','empty',''),
          ('non-cut','non-cut','lam'),('conflict','conflict','cut-b');
        INSERT INTO mini_active_rezka_paddons VALUES ('active','cut-a'),('mixed','cut-a'),('mixed','cut-b'),('conflict','cut-a');
        INSERT INTO mini_progress_batches VALUES ('w1','cut-a'),('w2','cut-b');
        INSERT INTO mini_paddon_items VALUES ('wip','w1'),('mixed-wip','w1'),('mixed-wip','w2');
        INSERT INTO mini_paddon_print_successors VALUES ('source','next','cut-a');")
        .execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!("../../../migrations/postgres/0161_paddon_apparatus_scope.sql"))
        .execute(&pool).await.unwrap();
    let rows = sqlx::query_as::<_, (String,String)>("SELECT code, apparatus_id FROM mini_paddons ORDER BY code")
        .fetch_all(&pool).await.unwrap().into_iter().collect::<std::collections::BTreeMap<_,_>>();
    assert_eq!(rows.len(), 10, "all historical packages must remain");
    for code in ["location", "active", "wip", "source", "next"] { assert_eq!(rows[code], "cut-a", "{code}"); }
    for code in ["mixed", "mixed-wip", "empty", "non-cut", "conflict"] { assert_eq!(rows[code], "", "{code}"); }
    assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mini_paddon_items").fetch_one(&pool).await.unwrap(), 3);
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE")).execute(&admin).await.unwrap();
    admin.close().await;
}

#[tokio::test]
async fn paddon_apparatus_scope_routes_outputs_to_each_machines_active_package() {
    const FIRST: &str = "apparatus:default:asset-010";
    const SECOND: &str = "apparatus:default:asset-011";
    let url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let admin = PgPool::connect(&url).await.unwrap();
    let db = format!("mini_rs_erp_test_paddon_scope_{:016x}", rand::random::<u64>());
    sqlx::query(&format!("CREATE DATABASE {db}")).execute(&admin).await.unwrap();
    let pool = PgPool::connect_with(postgres_test_database_options(&url, &db)).await.unwrap();
    apply_postgres_migrations_through_version(&pool, "0121").await.unwrap();
    seed_standard_canonical_apparatus(&pool).await;
    seed_canonical_apparatus(&pool, TestApparatusSpec::cut(SECOND, "Rezka 2")).await;
    apply_foundation_migration(&pool).await.unwrap();
    let canonical = CanonicalApparatusService::new(Arc::new(PostgresCanonicalApparatusRepository::new(pool.clone())));
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new(store.clone(), Arc::new(CanonicalServiceApparatusResolver::new(canonical)));
    let actor = QueueActionActor { role:"aparatchi".into(), ref_:"multi-apparatus-worker".into(), display_name:"Worker".into() };
    let first = service.create_paddon_for_apparatus(Some(FIRST), "", "", &actor).await.unwrap();
    let second = service.create_paddon_for_apparatus(Some(SECOND), "", "", &actor).await.unwrap();
    let unresolved = service.create_paddon("", "legacy", &actor).await.unwrap();
    assert_eq!(service.set_active_rezka_paddon(FIRST, &actor, &unresolved.code).await, Err(ProductionMapError::PaddonInvalidInput));
    service.set_active_rezka_paddon(FIRST, &actor, &first.code).await.unwrap();
    service.set_active_rezka_paddon(SECOND, &actor, &second.code).await.unwrap();
    assert_eq!(service.set_active_rezka_paddon(SECOND, &actor, &first.code).await, Err(ProductionMapError::PaddonInvalidInput));
    for (apparatus, own, wrong, number) in [(FIRST, &first, &second, "9701"),(SECOND, &second, &first, "9702")] {
        let listed = service.visible_paddons_for_apparatus(1, true, Some(apparatus), &actor).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].code, own.code);
        let order = format!("zakaz-scope-order-{number}");
        service.upsert_map(serde_json::from_value(serde_json::json!({
            "id":order,"product_code":order,"title":order,"order_number":number,
            "nodes":[{"id":"start","kind":"start","title":"Start"},
                {"id":"cut","kind":"apparatus","title":"Cut","apparatus_id":apparatus,"rezka_kadr_count":1},
                {"id":"end","kind":"end","title":"End"}],
            "edges":[{"from":"start","to":"cut"},{"from":"cut","to":"end"}]
        })).unwrap()).await.unwrap();
        let start = service.apply_apparatus_queue_action_with_progress(apparatus, &order, Action::Start,
            &[apparatus.into()], actor.clone(), QueueProgressInput::default()).await.unwrap();
        let input = QueueProgressInput { rezka_record_frame_index:Some(1), rezka_output_cycle:start.session.unwrap().session_id,
            rezka_frames:vec![serde_json::from_value(serde_json::json!({"produced_qty":120.0,"gross_qty":12.0,"bobina_kg":0.5,"diameter":45.0})).unwrap()],
            ..Default::default() };
        let mut rejected = service.prepare_apparatus_queue_action_with_progress(apparatus, &order, Action::RollComplete,
            &[apparatus.into()], actor.clone(), input.clone()).await.unwrap();
        rejected.attach_output_paddon(&wrong.code);
        assert!(matches!(service.commit_prepared_queue_action(rejected).await, Err(ProductionMapError::PaddonInvalidInput)));
        assert!(store.progress_batches_for_order(&order).await.unwrap().is_empty());
        let mut output = service.prepare_apparatus_queue_action_with_progress(apparatus, &order, Action::RollComplete,
            &[apparatus.into()], actor.clone(), input).await.unwrap();
        output.attach_active_paddon();
        let batch = service.commit_prepared_queue_action(output).await.unwrap().progress_batch.unwrap();
        let assigned = sqlx::query_scalar::<_, String>("SELECT paddon_id FROM mini_paddon_items WHERE progress_batch_id=$1 AND removed_at IS NULL")
            .bind(batch.batch_id).fetch_one(&pool).await.unwrap();
        assert_eq!(assigned, own.id);
        assert_eq!(service.active_rezka_paddon(apparatus, &actor).await.unwrap().as_deref(), Some(own.code.as_str()));
    }
    // Corrupt legacy selections cannot cross apparatus boundaries on GET or output.
    sqlx::query("UPDATE mini_active_rezka_paddons SET paddon_code=$1 WHERE apparatus_id=$2")
        .bind(&first.code).bind(SECOND).execute(&pool).await.unwrap();
    assert_eq!(service.active_rezka_paddon(SECOND, &actor).await.unwrap(), None);
    service.confirm_paddon_print(&first.code, &actor).await.unwrap();
    assert_eq!(service.create_active_paddon_successor(&first.code, SECOND, &actor).await, Err(ProductionMapError::PaddonInvalidInput));
    let next = service.create_active_paddon_successor(&first.code, FIRST, &actor).await.unwrap();
    assert_eq!(next.apparatus_id, FIRST);
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {db}")).execute(&admin).await.unwrap();
    admin.close().await;
}
