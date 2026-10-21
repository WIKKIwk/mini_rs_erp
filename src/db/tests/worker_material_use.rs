use crate::core::apparatus_standard::service::{
    CanonicalApparatusPatch, CanonicalApparatusService, CanonicalCommandMetadata,
};
use crate::core::apparatus_standard::{ApparatusId, test_support::TestApparatusSpec};
use crate::core::production_map::{
    ProductionMapDefinition, ProductionMapError, QueueActionActor, RawMaterialAssignment,
};
use crate::db::postgres::{
    apply_foundation_migration, apply_postgres_migrations_through_version,
    postgres_test_database_options,
};
use crate::db::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository;
use crate::db::postgres_production_map::PostgresProductionMapStore;
use std::sync::Arc;

const APPARATUS: &str = "apparatus:test:worker-material";
const ORDER: &str = "zakaz-worker-material";

fn assignment(barcode: &str) -> RawMaterialAssignment {
    RawMaterialAssignment {
        order_id: ORDER.into(),
        apparatus_id: ApparatusId::new(APPARATUS).unwrap(),
        apparatus: APPARATUS.into(),
        barcode: barcode.into(),
        item_code: "FILM".into(),
        item_name: "PET 1000/12".into(),
        item_group: "Rulon".into(),
        assigned_by_role: "aparatchi".into(),
        assigned_by_ref: "worker".into(),
        assigned_by_display_name: "Worker".into(),
        assigned_at: "2026-10-21T00:00:00Z".into(),
    }
}

#[tokio::test]
async fn worker_material_use_rechecks_permission_placement_dimensions_and_commits_atomically() {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".into());
    let database = format!(
        "mini_rs_erp_test_worker_material_{:016x}",
        rand::random::<u64>()
    );
    let admin = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("test admin connection");
    sqlx::query(&format!("CREATE DATABASE \"{database}\""))
        .execute(&admin)
        .await
        .unwrap();
    let pool = sqlx::PgPool::connect_with(postgres_test_database_options(&admin_url, &database))
        .await
        .unwrap();
    let test_pool = pool.clone();
    let result = tokio::spawn(async move {
        exercise(test_pool).await;
    })
    .await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{database}\" WITH (FORCE)"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.expect("worker material use checks");
}

async fn exercise(pool: sqlx::PgPool) {
    // The map backfill expects the factory's canonical laminate pair to exist.
    apply_postgres_migrations_through_version(&pool, "0121")
        .await
        .unwrap();
    super::seed_standard_canonical_apparatus(&pool).await;
    apply_foundation_migration(&pool).await.unwrap();
    super::seed_canonical_apparatus(&pool, TestApparatusSpec::laminate(APPARATUS, "Machine")).await;
    let map: ProductionMapDefinition = serde_json::from_value(serde_json::json!({
        "id":ORDER,"product_code":"FILM","title":"Order","width_mm":1000,
    }))
    .unwrap();
    sqlx::query("INSERT INTO mini_production_maps(id,product_code,title,map_json,lifecycle_status) VALUES($1,'FILM','Order',$2,'released')")
        .bind(ORDER).bind(serde_json::to_value(&map).unwrap()).execute(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO mini_item_groups(name,parent_item_group) VALUES('All Item Groups',NULL) ON CONFLICT DO NOTHING;
        INSERT INTO mini_item_groups(name,parent_item_group) VALUES('Rulon','All Item Groups') ON CONFLICT DO NOTHING;
        INSERT INTO mini_items(code,name,item_group) VALUES('FILM','PET 1000/12','Rulon');
        INSERT INTO mini_warehouses(id,name) VALUES('wh','Warehouse');
        INSERT INTO mini_factory_locations(id,name) VALUES('floor','Floor');")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO mini_factory_location_apparatus_links(location_id,apparatus_id) VALUES('floor',$1)")
        .bind(APPARATUS).execute(&pool).await.unwrap();
    sqlx::raw_sql("INSERT INTO mini_raw_material_stock(id,warehouse,item_code,item_name,barcode,qty,uom,status,payload_json)
        SELECT 'stock-'||i,'Warehouse','FILM','PET 1000/12','R'||i,10,'kg','available','{}' FROM generate_series(1,3)i;
        INSERT INTO mini_inventory_placements(asset_kind,asset_ref,physical_location_id,updated_by_role,updated_by_ref,updated_by_name)
        SELECT 'raw_material','stock-'||i,location.id,'aparatchi','mover','Mover' FROM generate_series(1,3)i,
        mini_inventory_locations location WHERE location.factory_location_id='floor';")
        .execute(&pool).await.unwrap();
    let store = PostgresProductionMapStore::new(pool.clone());
    let actor = QueueActionActor {
        role: "aparatchi".into(),
        ref_: "worker".into(),
        display_name: "Worker".into(),
    };
    let revision: i64 =
        sqlx::query_scalar("SELECT source_revision FROM mini_apparatus WHERE id=$1")
            .bind(APPARATUS)
            .fetch_one(&pool)
            .await
            .unwrap();
    let snapshot = store
        .worker_material_validation_snapshot("R1")
        .await
        .unwrap();
    assert_eq!(
        store
            .worker_material_candidates(APPARATUS, ORDER)
            .await
            .unwrap()
            .len(),
        3
    );
    assert!(matches!(
        store
            .use_worker_material(
                assignment("R1"),
                revision as u64,
                &map,
                1,
                &snapshot,
                &actor
            )
            .await,
        Err(ProductionMapError::ApparatusNotAssigned)
    ));
    let canonical = CanonicalApparatusService::new(Arc::new(
        PostgresCanonicalApparatusRepository::new(pool.clone()),
    ));
    canonical
        .patch(
            ApparatusId::new(APPARATUS).unwrap(),
            revision as u64,
            CanonicalApparatusPatch {
                worker_material_assignment_allowed: Some(true),
                ..Default::default()
            },
            CanonicalCommandMetadata::new("admin", "worker-material-test"),
        )
        .await
        .unwrap();
    let revision = canonical
        .current_projection(&ApparatusId::new(APPARATUS).unwrap())
        .await
        .unwrap()
        .unwrap()
        .source_revision;
    // A roll moved after the precomputed list loaded cannot be used by that scan.
    sqlx::query("UPDATE mini_inventory_placements SET version=version+1 WHERE asset_ref='stock-1'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        store
            .use_worker_material(
                assignment("R1"),
                revision as u64,
                &map,
                1,
                &snapshot,
                &actor
            )
            .await,
        Err(ProductionMapError::RawMaterialStockUnavailable)
    ));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_raw_material_assignments")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let result = store
        .use_worker_material(
            assignment("R1"),
            revision as u64,
            &map,
            2,
            &snapshot,
            &actor,
        )
        .await
        .unwrap();
    assert!(!result.in_use); // A waiting order retains the normal start and tooling gates.
    let replay = store
        .use_worker_material(
            assignment("R1"),
            revision as u64,
            &map,
            2,
            &snapshot,
            &actor,
        )
        .await
        .unwrap();
    assert!(!replay.in_use);
    assert_eq!(
        store
            .worker_material_candidates(APPARATUS, ORDER)
            .await
            .unwrap()
            .len(),
        2
    );
    // Stock dimensions or catalog metadata changing during validation invalidates confirmation.
    let snapshot = store
        .worker_material_validation_snapshot("R2")
        .await
        .unwrap();
    sqlx::query(
        r#"UPDATE mini_raw_material_stock SET payload_json='{"width_mm":1500}' WHERE barcode='R2'"#,
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(matches!(
        store
            .use_worker_material(
                assignment("R2"),
                revision as u64,
                &map,
                1,
                &snapshot,
                &actor
            )
            .await,
        Err(ProductionMapError::RawMaterialStockUnavailable)
    ));
    // An active order atomically receives the assignment, stock reservation and usage event.
    sqlx::query("INSERT INTO mini_queue_states(apparatus,canonical_apparatus_id,order_id,state) VALUES($1,$1,$2,'in_progress')")
        .bind(APPARATUS).bind(ORDER).execute(&pool).await.unwrap();
    let snapshot = store
        .worker_material_validation_snapshot("R3")
        .await
        .unwrap();
    sqlx::raw_sql("CREATE FUNCTION reject_worker_use() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN IF NEW.barcode='R3' AND NEW.event_type='usage_started' THEN RAISE EXCEPTION 'injected usage failure'; END IF;
        RETURN NEW; END $$; CREATE TRIGGER reject_worker_use BEFORE INSERT ON mini_raw_material_events
        FOR EACH ROW EXECUTE FUNCTION reject_worker_use();").execute(&pool).await.unwrap();
    assert!(
        store
            .use_worker_material(
                assignment("R3"),
                revision as u64,
                &map,
                1,
                &snapshot,
                &actor
            )
            .await
            .is_err()
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mini_raw_material_assignments WHERE barcode='R3'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    let status: String =
        sqlx::query_scalar("SELECT status FROM mini_raw_material_stock WHERE barcode='R3'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "available");
    sqlx::query("DROP TRIGGER reject_worker_use ON mini_raw_material_events")
        .execute(&pool)
        .await
        .unwrap();
    let result = store
        .use_worker_material(
            assignment("R3"),
            revision as u64,
            &map,
            1,
            &snapshot,
            &actor,
        )
        .await
        .unwrap();
    assert!(result.in_use);
    let stock: (String, String) = sqlx::query_as(
        "SELECT status,reserved_order_id FROM mini_raw_material_stock WHERE barcode='R3'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stock, ("in_use".into(), ORDER.into()));
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_raw_material_events WHERE barcode='R3' AND event_type='usage_started'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(events, 1);
    assert!(
        store
            .use_worker_material(
                assignment("R3"),
                revision as u64,
                &map,
                1,
                &snapshot,
                &actor
            )
            .await
            .unwrap()
            .in_use
    );
    let events: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_raw_material_events WHERE barcode='R3' AND event_type='usage_started'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(events, 1);
    sqlx::query("UPDATE mini_queue_states SET state='completed' WHERE order_id=$1")
        .bind(ORDER)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        store
            .worker_material_candidates(APPARATUS, ORDER)
            .await
            .unwrap()
            .is_empty()
    );
}
