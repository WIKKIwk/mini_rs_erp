use std::collections::BTreeMap;
use std::sync::Arc;

use sqlx::postgres::PgConnectOptions;

use crate::core::production_map::{
    ApparatusCapacityProfile, ApparatusDowntime, ApparatusScheduleRequest, ApparatusScheduleStatus,
    ProductionMapError, ProductionMapService, ProductionMapStorePort, QueueActionActor,
};
use crate::db::postgres::{apply_foundation_migration, apply_postgres_migrations_through};
use crate::db::postgres_production_map::PostgresProductionMapStore;

const LAMINATION_1_ID: &str = "apparatus:default:laminatsiya_1";
const LAMINATION_1_NAME: &str = "Laminatsiya 1";
const LAMINATION_2_NAME: &str = "Laminatsiya 2";
const FLEXO_ID: &str = "apparatus:default:flexo_pechat";
const FLEXO_NAME: &str = "Flexo pechat";

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_apparatus_identity"]
async fn postgres_capacity_writes_enforce_canonical_apparatus_identity_without_breaking_legacy_rows()
 {
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres:///postgres".to_string());
    let db_name = "mini_rs_erp_test_apparatus_identity";
    assert!(db_name.starts_with("mini_rs_erp_test_"));

    let admin_pool = sqlx::PgPool::connect(&admin_url).await.expect("admin db");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("drop test db");
    sqlx::query(&format!(r#"CREATE DATABASE "{db_name}""#))
        .execute(&admin_pool)
        .await
        .expect("create test db");
    admin_pool.close().await;

    let test_options = admin_url
        .parse::<PgConnectOptions>()
        .expect("valid admin database url")
        .database(db_name);
    let pool = sqlx::PgPool::connect_with(test_options)
        .await
        .expect("test db");

    apply_postgres_migrations_through(&pool, 54)
        .await
        .expect("apply migrations before identity enforcement");
    sqlx::query(
        "INSERT INTO mini_apparatus_capacity_profiles (apparatus_id, apparatus)
         VALUES
             ($1, $2),
             ('apparatus:flexo', 'Flexo pechat')",
    )
    .bind(LAMINATION_1_ID)
    .bind(LAMINATION_2_NAME)
    .execute(&pool)
    .await
    .expect("seed legacy mismatched profile");
    sqlx::query(
        "INSERT INTO mini_apparatus_downtimes (
             id, apparatus_id, apparatus, starts_at, ends_at, reason, actor_json
         ) VALUES (
             'legacy-mismatched-downtime', $1, $2,
             to_timestamp(1700000000), to_timestamp(1700003600), 'legacy',
             '{\"role\":\"admin\"}'::jsonb
         )",
    )
    .bind(LAMINATION_1_ID)
    .bind(LAMINATION_2_NAME)
    .execute(&pool)
    .await
    .expect("seed legacy mismatched downtime");
    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES ('legacy-identity-order', 'ITEM-IDENTITY', 'Identity order', '{}'::jsonb)",
    )
    .execute(&pool)
    .await
    .expect("seed legacy reservation order");
    sqlx::query(
        "INSERT INTO mini_apparatus_schedule_reservations (
             reservation_id, idempotency_key, order_id, apparatus_id, apparatus,
             starts_at, ends_at, requested_duration_minutes, reserved_duration_minutes,
             status, capability_requirements, actor_json
         ) VALUES (
             'legacy-mismatched-reservation', 'legacy-mismatched-reservation',
             'legacy-identity-order', $1, $2,
             to_timestamp(1700000000), to_timestamp(1700003600), 60, 60,
             'planned', '[]'::jsonb, '{\"role\":\"admin\"}'::jsonb
         )",
    )
    .bind(LAMINATION_1_ID)
    .bind(LAMINATION_2_NAME)
    .execute(&pool)
    .await
    .expect("seed legacy mismatched reservation");

    apply_foundation_migration(&pool)
        .await
        .expect("identity migration preserves legacy rows");
    let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
    let service = ProductionMapService::new(store.clone());

    let snapshot = service
        .apparatus_capacity_snapshot()
        .await
        .expect("load canonicalized legacy snapshot");
    assert_eq!(snapshot.profiles.len(), 2);
    let legacy_lamination_profile = snapshot
        .profiles
        .iter()
        .find(|profile| profile.apparatus_id == LAMINATION_1_ID)
        .expect("canonicalized lamination profile");
    assert_eq!(legacy_lamination_profile.apparatus, LAMINATION_1_NAME);
    let legacy_flexo_profile = snapshot
        .profiles
        .iter()
        .find(|profile| profile.apparatus_id == FLEXO_ID)
        .expect("canonicalized legacy flexo profile");
    assert_eq!(legacy_flexo_profile.apparatus, FLEXO_NAME);
    assert_eq!(snapshot.downtimes.len(), 1);
    assert_eq!(snapshot.downtimes[0].apparatus_id, LAMINATION_1_ID);
    assert_eq!(snapshot.downtimes[0].apparatus, LAMINATION_1_NAME);
    assert_eq!(snapshot.reservations.len(), 1);
    assert_eq!(snapshot.reservations[0].apparatus_id, LAMINATION_1_ID);
    assert_eq!(snapshot.reservations[0].apparatus, LAMINATION_1_NAME);

    store
        .update_apparatus_schedule_reservation_status(
            "legacy-identity-order",
            LAMINATION_2_NAME,
            ApparatusScheduleStatus::Active,
            &actor(),
        )
        .await
        .expect("wrong apparatus does not update legacy reservation");
    let status_after_wrong_apparatus: String = sqlx::query_scalar(
        "SELECT status FROM mini_apparatus_schedule_reservations
         WHERE reservation_id = 'legacy-mismatched-reservation'",
    )
    .fetch_one(&pool)
    .await
    .expect("legacy reservation status after wrong apparatus");
    assert_eq!(status_after_wrong_apparatus, "planned");
    store
        .update_apparatus_schedule_reservation_status(
            "legacy-identity-order",
            LAMINATION_1_NAME,
            ApparatusScheduleStatus::Active,
            &actor(),
        )
        .await
        .expect("canonical apparatus updates legacy reservation");
    let status_after_canonical_apparatus: String = sqlx::query_scalar(
        "SELECT status FROM mini_apparatus_schedule_reservations
         WHERE reservation_id = 'legacy-mismatched-reservation'",
    )
    .fetch_one(&pool)
    .await
    .expect("legacy reservation status after canonical apparatus");
    assert_eq!(status_after_canonical_apparatus, "active");

    let saved = service
        .put_apparatus_capacity_profile(capacity_profile(LAMINATION_1_ID, LAMINATION_1_NAME, 2))
        .await
        .expect("save canonical profile");
    assert_eq!(saved.apparatus_id, LAMINATION_1_ID);
    assert_eq!(saved.apparatus, LAMINATION_1_NAME);
    assert_eq!(saved.capacity_slots, 2);

    let mismatched_profile = service
        .put_apparatus_capacity_profile(capacity_profile(LAMINATION_1_ID, LAMINATION_2_NAME, 7))
        .await;
    assert_eq!(
        mismatched_profile,
        Err(ProductionMapError::CapacityProfileInvalid)
    );
    let persisted_profile: (String, i32) = sqlx::query_as(
        "SELECT apparatus, capacity_slots
         FROM mini_apparatus_capacity_profiles WHERE apparatus_id = $1",
    )
    .bind(LAMINATION_1_ID)
    .fetch_one(&pool)
    .await
    .expect("canonical profile remains unchanged");
    assert_eq!(persisted_profile, (LAMINATION_1_NAME.to_string(), 2));

    service
        .put_apparatus_capacity_profile(capacity_profile(FLEXO_ID, FLEXO_NAME, 4))
        .await
        .expect("replace legacy alias with canonical flexo profile");
    let legacy_flexo_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mini_apparatus_capacity_profiles
         WHERE apparatus_id = 'apparatus:flexo'",
    )
    .fetch_one(&pool)
    .await
    .expect("legacy flexo alias count");
    assert_eq!(legacy_flexo_count, 0);

    let mismatched_downtime = service
        .put_apparatus_downtime(ApparatusDowntime {
            id: "new-mismatched-downtime".to_string(),
            apparatus_id: LAMINATION_1_ID.to_string(),
            apparatus: LAMINATION_2_NAME.to_string(),
            starts_at_unix: 1_800_000_000,
            ends_at_unix: 1_800_003_600,
            reason: "invalid pair".to_string(),
            active: true,
            actor: actor(),
            created_at_unix: 1_800_000_000,
        })
        .await;
    assert_eq!(
        mismatched_downtime,
        Err(ProductionMapError::CapacityProfileInvalid)
    );

    let direct_mismatch = sqlx::query(
        "INSERT INTO mini_apparatus_downtimes (
             id, apparatus_id, apparatus, starts_at, ends_at, reason
         ) VALUES (
             'direct-mismatched-downtime', $1, $2,
             to_timestamp(1800000000), to_timestamp(1800003600), 'invalid pair'
         )",
    )
    .bind(LAMINATION_1_ID)
    .bind(LAMINATION_2_NAME)
    .execute(&pool)
    .await;
    let direct_mismatch = direct_mismatch.expect_err("database must reject mismatched identity");
    assert!(
        direct_mismatch
            .to_string()
            .contains("mini_apparatus_downtimes_identity_fk")
    );

    let mismatched_schedule = service
        .schedule_apparatus_order(ApparatusScheduleRequest {
            order_id: "missing-order".to_string(),
            apparatus_id: LAMINATION_1_ID.to_string(),
            apparatus: LAMINATION_2_NAME.to_string(),
            earliest_start_unix: 1_800_000_000,
            latest_end_unix: None,
            duration_minutes: 10,
            priority: 0,
            source: "identity-test".to_string(),
            reason: String::new(),
            idempotency_key: "identity-test-mismatch".to_string(),
            capability_requirements: Vec::new(),
            candidate_apparatuses: Vec::new(),
            actor: actor(),
        })
        .await;
    assert_eq!(
        mismatched_schedule,
        Err(ProductionMapError::ScheduleInputInvalid)
    );

    sqlx::query(
        "INSERT INTO mini_apparatus (id, name, base_name, kind, payload_json)
         VALUES ('apparatus:custom:stable', 'Custom Original', 'Custom Original',
                 'custom', '{}'::jsonb)",
    )
    .execute(&pool)
    .await
    .expect("seed custom apparatus");
    service
        .put_apparatus_capacity_profile(capacity_profile(
            "apparatus:custom:stable",
            "Custom Original",
            3,
        ))
        .await
        .expect("save custom stable identity");
    sqlx::query(
        "UPDATE mini_apparatus SET name = 'Custom Renamed'
         WHERE id = 'apparatus:custom:stable'",
    )
    .execute(&pool)
    .await
    .expect("rename custom apparatus with cascade");
    let renamed_profile: String = sqlx::query_scalar(
        "SELECT apparatus FROM mini_apparatus_capacity_profiles
         WHERE apparatus_id = 'apparatus:custom:stable'",
    )
    .fetch_one(&pool)
    .await
    .expect("renamed profile");
    assert_eq!(renamed_profile, "Custom Renamed");

    pool.close().await;
    let admin_pool = sqlx::PgPool::connect(&admin_url)
        .await
        .expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("cleanup test db");
    admin_pool.close().await;
}

fn capacity_profile(
    apparatus_id: &str,
    apparatus: &str,
    capacity_slots: u16,
) -> ApparatusCapacityProfile {
    ApparatusCapacityProfile {
        apparatus_id: apparatus_id.to_string(),
        apparatus: apparatus.to_string(),
        capacity_slots,
        setup_minutes: 0,
        cleanup_minutes: 0,
        efficiency_percent: 100,
        finite_capacity: true,
        working_windows: Vec::new(),
        capabilities: Vec::new(),
        capability_levels: BTreeMap::new(),
        notes: String::new(),
        updated_at_unix: 0,
    }
}

fn actor() -> QueueActionActor {
    QueueActionActor {
        role: "admin".to_string(),
        ref_: "apparatus-identity-test".to_string(),
        display_name: "Identity Test".to_string(),
    }
}
