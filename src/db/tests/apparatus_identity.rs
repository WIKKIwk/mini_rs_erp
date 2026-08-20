use std::collections::BTreeMap;
use std::sync::Arc;

use sqlx::postgres::PgConnectOptions;
use sqlx::PgPool;

use crate::core::apparatus_groups::ApparatusGroupService;
use crate::core::apparatus_standard::ApparatusId;
use crate::core::production_map::{
    ApparatusCapacityProfile, ApparatusDowntime, ApparatusScheduleRequest, ApparatusScheduleStatus,
    ProductionMapError, ProductionMapService, ProductionMapStorePort, QueueActionActor,
};
use crate::db::postgres::{apply_foundation_migration, apply_postgres_migrations_through};
use crate::db::postgres_apparatus_group::PostgresApparatusGroupStore;
use crate::db::postgres_production_map::PostgresProductionMapStore;

const LAMINATION_1_ID: &str = "apparatus:default:asset-007";
const LAMINATION_1_NAME: &str = "Laminatsiya 1";
const LAMINATION_2_ID: &str = "apparatus:default:asset-008";
const LAMINATION_2_NAME: &str = "Laminatsiya 2";
const FLEXO_ID: &str = "apparatus:default:asset-005";
const FLEXO_NAME: &str = "Flexo pechat";
const LEGACY_LAMINATION_1_ID: &str = "apparatus:default:laminatsiya_1";
const LEGACY_LAMINATION_2_ID: &str = "apparatus:default:laminatsiya_2";

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
    let apparatus_groups =
        ApparatusGroupService::new(Arc::new(PostgresApparatusGroupStore::new(pool.clone())));

    let snapshot = service
        .apparatus_capacity_snapshot()
        .await
        .expect("load canonicalized legacy snapshot");
    assert_eq!(snapshot.profiles.len(), 2);
    let legacy_lamination_profile = snapshot
        .profiles
        .iter()
        .find(|profile| profile.apparatus_id.as_str() == LAMINATION_1_ID)
        .expect("canonicalized lamination profile");
    assert_eq!(legacy_lamination_profile.apparatus, LAMINATION_1_NAME);
    let legacy_flexo_profile = snapshot
        .profiles
        .iter()
        .find(|profile| profile.apparatus_id.as_str() == FLEXO_ID)
        .expect("canonicalized legacy flexo profile");
    assert_eq!(legacy_flexo_profile.apparatus, FLEXO_NAME);
    assert_eq!(snapshot.downtimes.len(), 1);
    assert_eq!(snapshot.downtimes[0].apparatus_id.as_str(), LAMINATION_1_ID);
    assert_eq!(snapshot.downtimes[0].apparatus, LAMINATION_1_NAME);
    assert_eq!(snapshot.reservations.len(), 1);
    assert_eq!(
        snapshot.reservations[0].apparatus_id.as_str(),
        LAMINATION_1_ID
    );
    assert_eq!(snapshot.reservations[0].apparatus, LAMINATION_1_NAME);

    store
        .update_apparatus_schedule_reservation_status(
            "legacy-identity-order",
            &ApparatusId::new(FLEXO_ID.to_string()).expect("flexo id"),
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
            &ApparatusId::new(LAMINATION_1_ID.to_string()).expect("lamination id"),
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
        .put_apparatus_capacity_profile(
            capacity_profile(LAMINATION_1_ID, LAMINATION_1_NAME, 2),
            &apparatus_groups,
        )
        .await
        .expect("save canonical profile");
    assert_eq!(saved.apparatus_id.as_str(), LAMINATION_1_ID);
    assert_eq!(saved.apparatus, LAMINATION_1_NAME);
    assert_eq!(saved.capacity_slots, 2);

    let mismatched_profile = service
        .put_apparatus_capacity_profile(
            capacity_profile(LAMINATION_1_ID, LAMINATION_2_NAME, 7),
            &apparatus_groups,
        )
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
        .put_apparatus_capacity_profile(
            capacity_profile(FLEXO_ID, FLEXO_NAME, 4),
            &apparatus_groups,
        )
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
            apparatus_id: ApparatusId::new(LAMINATION_1_ID).expect("canonical apparatus id"),
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
        .put_apparatus_capacity_profile(
            capacity_profile("apparatus:custom:stable", "Custom Original", 3),
            &apparatus_groups,
        )
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
        apparatus_id: ApparatusId::new(apparatus_id.to_string()).expect("canonical apparatus id"),
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

const MIGRATIONS_BEFORE_CANONICAL_CUTOVER: usize = 64;
const MIGRATIONS_THROUGH_CANONICAL_CHAIN: usize = 68;
const PRODUCTION_0062_INDEXES: [(&str, &str); 5] = [
    (
        "idx_mini_apparatus_factory_map_object_id_unique",
        "mini_apparatus",
    ),
    (
        "idx_mini_apparatus_material_rules_lower_apparatus",
        "mini_apparatus_material_rules",
    ),
    (
        "idx_mini_raw_material_stock_lower_barcode",
        "mini_raw_material_stock",
    ),
    (
        "idx_mini_raw_material_assignments_lower_barcode",
        "mini_raw_material_assignments",
    ),
    (
        "idx_mini_queue_action_events_pending_completion",
        "mini_queue_action_events",
    ),
];

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_canonical_cutover_acceptance"]
async fn postgres_canonical_migration_acceptance_fixture() {
    let db_name = "mini_rs_erp_test_canonical_cutover_acceptance";
    let (admin_url, pool) = create_isolated_test_database(db_name).await;

    apply_postgres_migrations_through(&pool, 61)
        .await
        .expect("apply production migrations through 0061");
    let history_through_0061 = canonical_migration_history(&pool).await;
    assert_eq!(history_through_0061.len(), 61);

    apply_postgres_migrations_through(&pool, 62)
        .await
        .expect("apply authoritative production 0062");
    let history_through_0062 = canonical_migration_history(&pool).await;
    assert_eq!(history_through_0062.len(), 62);
    assert_eq!(&history_through_0062[..61], history_through_0061.as_slice());
    assert_production_0062_indexes(&pool).await;

    apply_postgres_migrations_through(&pool, MIGRATIONS_BEFORE_CANONICAL_CUTOVER)
        .await
        .expect("apply canonical staging migrations 0063 and 0064");
    seed_valid_legacy_cutover_rows(&pool).await;

    apply_postgres_migrations_through(&pool, MIGRATIONS_THROUGH_CANONICAL_CHAIN)
        .await
        .expect("execute canonical migrations 0065 through 0068");
    assert_production_0062_indexes(&pool).await;

    let canonical_apparatus_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM mini_apparatus WHERE id LIKE 'apparatus:%:%'")
            .fetch_one(&pool)
            .await
            .expect("canonical apparatus count");
    assert_eq!(canonical_apparatus_count, 10);

    let worker_groups: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT apparatus, group_code, canonical_apparatus_id
         FROM mini_worker_groups ORDER BY group_code",
    )
    .fetch_all(&pool)
    .await
    .expect("canonical worker groups");
    assert_eq!(
        worker_groups,
        vec![
            (
                LEGACY_LAMINATION_1_ID.to_string(),
                "legacy-workers".to_string(),
                LAMINATION_1_ID.to_string(),
            ),
            (
                LEGACY_LAMINATION_2_ID.to_string(),
                "prepopulated-authority".to_string(),
                "apparatus:default:bosma_7".to_string(),
            ),
        ]
    );

    let profile: (String, String, String, i32) = sqlx::query_as(
        "SELECT apparatus_id, canonical_apparatus_id, apparatus, capacity_slots
         FROM mini_apparatus_capacity_profiles
         WHERE canonical_apparatus_id = $1",
    )
    .bind(LAMINATION_1_ID)
    .fetch_one(&pool)
    .await
    .expect("canonical capacity profile");
    assert_eq!(
        profile,
        (
            LAMINATION_1_ID.to_string(),
            LAMINATION_1_ID.to_string(),
            LAMINATION_1_NAME.to_string(),
            3,
        )
    );

    let downtime: (String, String, String) = sqlx::query_as(
        "SELECT apparatus_id, canonical_apparatus_id, apparatus
         FROM mini_apparatus_downtimes WHERE id = 'migration-legacy-downtime'",
    )
    .fetch_one(&pool)
    .await
    .expect("canonical downtime");
    assert_eq!(
        downtime,
        (
            LAMINATION_1_ID.to_string(),
            LAMINATION_1_ID.to_string(),
            LAMINATION_1_NAME.to_string(),
        )
    );

    let reservation: (String, String, String) = sqlx::query_as(
        "SELECT apparatus_id, canonical_apparatus_id, apparatus
         FROM mini_apparatus_schedule_reservations
         WHERE reservation_id = 'migration-legacy-reservation'",
    )
    .fetch_one(&pool)
    .await
    .expect("canonical schedule reservation");
    assert_eq!(
        reservation,
        (
            LAMINATION_1_ID.to_string(),
            LAMINATION_1_ID.to_string(),
            LAMINATION_1_NAME.to_string(),
        )
    );

    let transfer: (String, String, String, String) = sqlx::query_as(
        "SELECT from_apparatus, to_apparatus,
                canonical_from_apparatus_id, canonical_to_apparatus_id
         FROM mini_apparatus_order_transfers
         WHERE transfer_id = 'migration-legacy-transfer'",
    )
    .fetch_one(&pool)
    .await
    .expect("canonical apparatus transfer");
    assert_eq!(
        transfer,
        (
            LEGACY_LAMINATION_1_ID.to_string(),
            LEGACY_LAMINATION_2_ID.to_string(),
            LAMINATION_1_ID.to_string(),
            LAMINATION_2_ID.to_string(),
        )
    );

    let freeze_request: (String, String, String) = sqlx::query_as(
        "SELECT target_apparatus, canonical_target_apparatus_id, status
         FROM mini_order_freeze_requests
         WHERE request_id = 'migration-legacy-freeze'",
    )
    .fetch_one(&pool)
    .await
    .expect("canonical freeze request");
    assert_eq!(
        freeze_request,
        (
            LEGACY_LAMINATION_1_ID.to_string(),
            "apparatus:default:bosma_8".to_string(),
            "pending".to_string(),
        )
    );

    let training_queue_state: (String, String, String) = sqlx::query_as(
        "SELECT apparatus, canonical_apparatus_id, state
         FROM mini_training_queue_states
         WHERE order_id = 'training-migration-order'",
    )
    .fetch_one(&pool)
    .await
    .expect("canonical training queue state");
    assert_eq!(
        training_queue_state,
        (
            LEGACY_LAMINATION_1_ID.to_string(),
            LAMINATION_1_ID.to_string(),
            "paused".to_string(),
        )
    );

    let training_queue_event: (String, String, String) = sqlx::query_as(
        "SELECT apparatus, canonical_apparatus_id, action
         FROM mini_training_queue_events
         WHERE event_id = 'migration-training-event'",
    )
    .fetch_one(&pool)
    .await
    .expect("canonical training queue event");
    assert_eq!(
        training_queue_event,
        (
            LEGACY_LAMINATION_1_ID.to_string(),
            LAMINATION_1_ID.to_string(),
            "pause".to_string(),
        )
    );

    let training_progress: (String, String, String, String) = sqlx::query_as(
        "SELECT apparatus, canonical_apparatus_id,
                payload_json->>'apparatus', payload_json->>'preserved_marker'
         FROM mini_training_progress_batches
         WHERE batch_id = 'migration-training-progress'",
    )
    .fetch_one(&pool)
    .await
    .expect("canonical training progress batch");
    assert_eq!(
        training_progress,
        (
            LEGACY_LAMINATION_1_ID.to_string(),
            LAMINATION_1_ID.to_string(),
            LAMINATION_1_ID.to_string(),
            "training-progress-preserved".to_string(),
        )
    );

    let training_material: (String, String, String, String) = sqlx::query_as(
        "SELECT apparatus, canonical_apparatus_id,
                payload_json->>'apparatus', payload_json->>'preserved_marker'
         FROM mini_training_raw_material_assignments
         WHERE id = 'migration-training-material'",
    )
    .fetch_one(&pool)
    .await
    .expect("canonical training material assignment");
    assert_eq!(
        training_material,
        (
            LEGACY_LAMINATION_1_ID.to_string(),
            LAMINATION_1_ID.to_string(),
            LAMINATION_1_ID.to_string(),
            "training-material-preserved".to_string(),
        )
    );

    let material_rule: (String, String, bool, String) = sqlx::query_as(
        "SELECT apparatus, canonical_apparatus_id, requires_material,
                payload_json->>'preserved_marker'
         FROM mini_apparatus_material_rules
         WHERE apparatus = $1",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .fetch_one(&pool)
    .await
    .expect("canonical material rule");
    assert_eq!(
        material_rule,
        (
            LEGACY_LAMINATION_1_ID.to_string(),
            LAMINATION_1_ID.to_string(),
            true,
            "material-rule-preserved".to_string(),
        )
    );

    let completion: (String, String, String, String, i32, String) = sqlx::query_as(
        "SELECT apparatus, canonical_apparatus_id, action, status,
                produced_qty::integer, payload_json->>'preserved_marker'
         FROM mini_progress_batches
         WHERE batch_id = 'migration-completion-batch'",
    )
    .fetch_one(&pool)
    .await
    .expect("canonical completion batch");
    assert_eq!(
        completion,
        (
            LEGACY_LAMINATION_1_ID.to_string(),
            LAMINATION_1_ID.to_string(),
            "complete".to_string(),
            "completed".to_string(),
            1,
            "completion-preserved".to_string(),
        )
    );

    let future_json_apparatus: String = sqlx::query_scalar(
        "SELECT map_json #>> '{nodes,0,apparatus_id}'
         FROM mini_production_maps
         WHERE id = 'migration-future-json'",
    )
    .fetch_one(&pool)
    .await
    .expect("future canonical JSON identity");
    assert_eq!(future_json_apparatus, LAMINATION_1_ID);

    let warehouse_assignment: (String, String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT assignment_kind, warehouse, warehouse_name, apparatus_id
         FROM mini_warehouse_assignments
         WHERE principal_ref = 'legacy-warehouse-principal'",
    )
    .fetch_one(&pool)
    .await
    .expect("typed warehouse assignment");
    assert_eq!(
        warehouse_assignment,
        (
            "warehouse".to_string(),
            "Acceptance Warehouse".to_string(),
            Some("Acceptance Warehouse".to_string()),
            None,
        )
    );

    let typed_identity_count: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM mini_warehouse_assignments
         WHERE principal_ref = 'legacy-warehouse-principal'
           AND ((warehouse_name IS NOT NULL)::int + (apparatus_id IS NOT NULL)::int) = 1",
    )
    .fetch_one(&pool)
    .await
    .expect("exactly one typed warehouse identity");
    assert_eq!(typed_identity_count, 1);

    sqlx::query(
        "INSERT INTO mini_warehouse_assignments (
             warehouse, assignment_kind, warehouse_name, apparatus_id,
             principal_role, principal_ref, display_name
         ) VALUES (
             'Laminatsiya 1 apparatus snapshot', 'apparatus', NULL, $1,
             'admin', 'canonical-apparatus-principal', 'Canonical Apparatus'
         )",
    )
    .bind(LAMINATION_1_ID)
    .execute(&pool)
    .await
    .expect("insert typed apparatus assignment");

    let apparatus_assignment: (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT assignment_kind, warehouse_name, apparatus_id
         FROM mini_warehouse_assignments
         WHERE principal_ref = 'canonical-apparatus-principal'",
    )
    .fetch_one(&pool)
    .await
    .expect("typed apparatus assignment");
    assert_eq!(
        apparatus_assignment,
        (
            "apparatus".to_string(),
            None,
            Some(LAMINATION_1_ID.to_string()),
        )
    );

    let assignments_without_exactly_one_identity: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM mini_warehouse_assignments
         WHERE ((warehouse_name IS NOT NULL)::int + (apparatus_id IS NOT NULL)::int) <> 1",
    )
    .fetch_one(&pool)
    .await
    .expect("all warehouse assignments have one typed identity");
    assert_eq!(assignments_without_exactly_one_identity, 0);

    let duplicate_assignment = sqlx::query(
        "INSERT INTO mini_warehouse_assignments (
             warehouse, assignment_kind, warehouse_name, apparatus_id,
             principal_role, principal_ref, display_name
         ) VALUES (
             'Different legacy snapshot', 'apparatus', NULL, $1,
             'admin', 'canonical-apparatus-principal', 'Duplicate Apparatus'
         )",
    )
    .bind(LAMINATION_1_ID)
    .execute(&pool)
    .await
    .expect_err("canonical apparatus assignment uniqueness");
    assert!(duplicate_assignment
        .to_string()
        .contains("idx_mini_warehouse_assignments_apparatus_identity_unique"));

    let orphan_assignment = sqlx::query(
        "INSERT INTO mini_warehouse_assignments (
             warehouse, assignment_kind, warehouse_name, apparatus_id,
             principal_role, principal_ref, display_name
         ) VALUES (
             'Orphan apparatus snapshot', 'apparatus', NULL,
             'apparatus:default:orphan', 'admin', 'orphan-apparatus-principal',
             'Orphan Apparatus'
         )",
    )
    .execute(&pool)
    .await
    .expect_err("apparatus assignment must reject orphan apparatus_id");
    assert!(orphan_assignment
        .to_string()
        .contains("mini_warehouse_assignments_apparatus_id_fk"));

    let state_before_restart = capture_canonical_cutover_state(&pool).await;
    let history_before_restart = canonical_migration_history(&pool).await;
    assert_eq!(
        history_before_restart.len(),
        MIGRATIONS_THROUGH_CANONICAL_CHAIN
    );
    pool.close().await;

    let restart_options = admin_url
        .parse::<PgConnectOptions>()
        .expect("valid admin database url")
        .database(db_name);
    let restarted_pool = PgPool::connect_with(restart_options)
        .await
        .expect("reconnect canonical migration fixture");
    apply_foundation_migration(&restarted_pool)
        .await
        .expect("restart canonical migration chain");
    assert_eq!(
        canonical_migration_history(&restarted_pool).await,
        history_before_restart,
        "restart changed version/checksum/applied_at history"
    );
    assert_eq!(
        capture_canonical_cutover_state(&restarted_pool).await,
        state_before_restart,
        "restart changed canonical projection rows"
    );
    assert_production_0062_indexes(&restarted_pool).await;
    restarted_pool.close().await;
    drop_isolated_test_database(&admin_url, db_name).await;
}

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_canonical_assignment_malformed"]
async fn postgres_canonical_migration_rejects_malformed_typed_assignment() {
    let db_name = "mini_rs_erp_test_canonical_assignment_malformed";
    let (admin_url, pool) = create_isolated_test_database(db_name).await;
    apply_postgres_migrations_through(&pool, MIGRATIONS_BEFORE_CANONICAL_CUTOVER)
        .await
        .expect("apply migrations immediately before 0065");
    stage_warehouse_assignment_columns(&pool).await;
    seed_legacy_assignment_snapshot(&pool).await;
    sqlx::query(
        "INSERT INTO mini_warehouse_assignments (
             warehouse, assignment_kind, warehouse_name, apparatus_id,
             principal_role, principal_ref, display_name
         ) VALUES (
             'Legacy Assignment Warehouse', 'apparatus', NULL,
             'not-an-apparatus-id', 'admin', 'malformed-assignment', 'Malformed'
         )",
    )
    .execute(&pool)
    .await
    .expect("seed malformed typed assignment");

    assert_cutover_rejected(&pool, "mini_warehouse_assignments_assignment_kind_check").await;
    pool.close().await;
    drop_isolated_test_database(&admin_url, db_name).await;
}

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_canonical_assignment_mixed"]
async fn postgres_canonical_migration_rejects_mixed_typed_assignment() {
    let db_name = "mini_rs_erp_test_canonical_assignment_mixed";
    let (admin_url, pool) = create_isolated_test_database(db_name).await;
    apply_postgres_migrations_through(&pool, MIGRATIONS_BEFORE_CANONICAL_CUTOVER)
        .await
        .expect("apply migrations immediately before 0065");
    stage_warehouse_assignment_columns(&pool).await;
    seed_legacy_assignment_snapshot(&pool).await;
    sqlx::query(
        "INSERT INTO mini_warehouse_assignments (
             warehouse, assignment_kind, warehouse_name, apparatus_id,
             principal_role, principal_ref, display_name
         ) VALUES (
             'Legacy Assignment Warehouse', NULL, 'Legacy Assignment Warehouse',
             $1, 'admin', 'mixed-assignment', 'Mixed'
         )",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .execute(&pool)
    .await
    .expect("seed mixed typed assignment");

    assert_cutover_rejected(
        &pool,
        "0066 warehouse assignment has both canonical identity columns populated before backfill",
    )
    .await;
    pool.close().await;
    drop_isolated_test_database(&admin_url, db_name).await;
}

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_canonical_assignment_incomplete"]
async fn postgres_canonical_migration_rejects_incomplete_typed_assignment() {
    let db_name = "mini_rs_erp_test_canonical_assignment_incomplete";
    let (admin_url, pool) = create_isolated_test_database(db_name).await;
    apply_postgres_migrations_through(&pool, MIGRATIONS_BEFORE_CANONICAL_CUTOVER)
        .await
        .expect("apply migrations immediately before 0065");
    stage_warehouse_assignment_columns(&pool).await;
    seed_legacy_assignment_snapshot(&pool).await;
    sqlx::query(
        "INSERT INTO mini_warehouse_assignments (
             warehouse, assignment_kind, warehouse_name, apparatus_id,
             principal_role, principal_ref, display_name
         ) VALUES (
             'Legacy Assignment Warehouse', 'apparatus', NULL, NULL,
             'admin', 'incomplete-assignment', 'Incomplete'
         )",
    )
    .execute(&pool)
    .await
    .expect("seed incomplete typed assignment");

    assert_cutover_rejected(
        &pool,
        "0066 warehouse assignment does not have exactly one typed canonical identity",
    )
    .await;
    pool.close().await;
    drop_isolated_test_database(&admin_url, db_name).await;
}

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_canonical_apparatus_unresolved"]
async fn postgres_canonical_migration_rejects_unresolved_legacy_apparatus_mapping() {
    let db_name = "mini_rs_erp_test_canonical_apparatus_unresolved";
    let (admin_url, pool) = create_isolated_test_database(db_name).await;
    apply_postgres_migrations_through(&pool, MIGRATIONS_BEFORE_CANONICAL_CUTOVER)
        .await
        .expect("apply migrations immediately before 0065");
    sqlx::query(
        "INSERT INTO mini_worker_groups (apparatus, group_code, shift)
         VALUES ('legacy-apparatus-never-mapped', 'unresolved-group', 'day')",
    )
    .execute(&pool)
    .await
    .expect("seed unresolved legacy apparatus reference");

    assert_cutover_rejected(
        &pool,
        "0065 unresolved legacy apparatus reference in mini_worker_groups.apparatus",
    )
    .await;
    pool.close().await;
    drop_isolated_test_database(&admin_url, db_name).await;
}

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_canonical_virtual_training_queue"]
async fn postgres_canonical_migration_rejects_virtual_training_queue_identity() {
    let db_name = "mini_rs_erp_test_canonical_virtual_training_queue";
    let (admin_url, pool) = create_isolated_test_database(db_name).await;
    apply_postgres_migrations_through(&pool, MIGRATIONS_BEFORE_CANONICAL_CUTOVER)
        .await
        .expect("apply migrations immediately before 0065");
    sqlx::query(
        "INSERT INTO mini_training_queue_states (apparatus, order_id, state)
         VALUES ('training-input:bosma', 'training-virtual-order', 'pending')",
    )
    .execute(&pool)
    .await
    .expect("seed virtual training queue identity");

    assert_cutover_rejected(
        &pool,
        "0065 unresolved legacy apparatus reference in mini_training_queue_states.apparatus",
    )
    .await;
    pool.close().await;
    drop_isolated_test_database(&admin_url, db_name).await;
}

#[tokio::test]
#[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_canonical_apparatus_ambiguous"]
async fn postgres_canonical_migration_rejects_ambiguous_legacy_apparatus_mapping() {
    let db_name = "mini_rs_erp_test_canonical_apparatus_ambiguous";
    let (admin_url, pool) = create_isolated_test_database(db_name).await;
    apply_postgres_migrations_through(&pool, MIGRATIONS_BEFORE_CANONICAL_CUTOVER)
        .await
        .expect("apply migrations immediately before 0065");
    sqlx::query(
        "INSERT INTO mini_apparatus (id, name, base_name, kind, payload_json)
         VALUES
             ('apparatus:custom:asset-a', 'Ambiguous A', 'Shared Legacy Apparatus',
              'custom', '{}'::jsonb),
             ('apparatus:custom:asset-b', 'Ambiguous B', 'Shared Legacy Apparatus',
              'custom', '{}'::jsonb)",
    )
    .execute(&pool)
    .await
    .expect("seed ambiguous legacy apparatus candidates");

    assert_cutover_rejected(
        &pool,
        "0065 ambiguous legacy apparatus identity shared legacy apparatus",
    )
    .await;
    pool.close().await;
    drop_isolated_test_database(&admin_url, db_name).await;
}

async fn create_isolated_test_database(db_name: &str) -> (String, PgPool) {
    assert!(db_name.starts_with("mini_rs_erp_test_"));
    assert_ne!(db_name, "mini_rs_erp");
    let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
    let admin_pool = PgPool::connect(&admin_url).await.expect("admin db");
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
    let pool = PgPool::connect_with(test_options)
        .await
        .expect("test db");
    (admin_url, pool)
}

async fn drop_isolated_test_database(admin_url: &str, db_name: &str) {
    let admin_pool = PgPool::connect(admin_url).await.expect("admin cleanup");
    sqlx::query(&format!(
        r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
    ))
    .execute(&admin_pool)
    .await
    .expect("cleanup test db");
    admin_pool.close().await;
}

async fn seed_valid_legacy_cutover_rows(pool: &PgPool) {
    sqlx::query(
        "ALTER TABLE mini_order_freeze_requests
             ADD COLUMN IF NOT EXISTS canonical_target_apparatus_id TEXT",
    )
    .execute(pool)
    .await
    .expect("stage freeze canonical column");

    sqlx::query(
        "INSERT INTO mini_warehouses (id, name)
         VALUES ('warehouse:canonical-acceptance', 'Acceptance Warehouse')",
    )
    .execute(pool)
    .await
    .expect("seed acceptance warehouse");
    sqlx::query(
        "INSERT INTO mini_warehouse_assignments (
             warehouse, principal_role, principal_ref, display_name, payload_json
         ) VALUES (
             'Acceptance Warehouse', 'admin', 'legacy-warehouse-principal',
             'Legacy Warehouse Principal', '{}'::jsonb
         )",
    )
    .execute(pool)
    .await
    .expect("seed legacy warehouse assignment");

    sqlx::query(
        "INSERT INTO mini_worker_groups (
             apparatus, group_code, shift, canonical_apparatus_id
         ) VALUES
             ($1, 'legacy-workers', 'day', NULL),
             ($2, 'prepopulated-authority', 'day', 'apparatus:default:bosma_7')",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .bind(LEGACY_LAMINATION_2_ID)
    .execute(pool)
    .await
    .expect("seed legacy worker groups");

    sqlx::query(
        "INSERT INTO mini_apparatus_capacity_profiles (
             apparatus_id, apparatus, capacity_slots, setup_minutes,
             cleanup_minutes, efficiency_percent, finite_capacity,
             working_windows, capabilities, capability_levels, notes,
             canonical_apparatus_id
         ) VALUES (
             $1, $2, 3, 5, 6, 95, TRUE, '[]'::jsonb, '[]'::jsonb,
             '{}'::jsonb, 'legacy profile', NULL
         )",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .bind(LAMINATION_1_NAME)
    .execute(pool)
    .await
    .expect("seed legacy capacity profile");

    sqlx::query(
        "INSERT INTO mini_apparatus_downtimes (
             id, apparatus_id, apparatus, starts_at, ends_at, reason,
             canonical_apparatus_id
         ) VALUES (
             'migration-legacy-downtime', $1, $2,
             to_timestamp(1700000000), to_timestamp(1700003600),
             'legacy maintenance', NULL
         )",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .bind(LAMINATION_1_NAME)
    .execute(pool)
    .await
    .expect("seed legacy downtime");

    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES ('migration-legacy-order', 'MIGRATION-ITEM', 'Migration order', '{}'::jsonb)",
    )
    .execute(pool)
    .await
    .expect("seed legacy migration order");
    sqlx::query(
        "INSERT INTO mini_production_maps (id, product_code, title, map_json)
         VALUES (
             'migration-future-json', 'MIGRATION-FUTURE', 'Future canonical JSON',
             '{\"nodes\":[{\"kind\":\"apparatus\",\"apparatus_id\":\"apparatus:default:asset-007\"}]}'::jsonb
         )",
    )
    .execute(pool)
    .await
    .expect("seed future canonical JSON identity");
    sqlx::query(
        "INSERT INTO mini_apparatus_schedule_reservations (
             reservation_id, idempotency_key, order_id, apparatus_id, apparatus,
             starts_at, ends_at, requested_duration_minutes,
             reserved_duration_minutes, status, capability_requirements,
             canonical_apparatus_id
         ) VALUES (
             'migration-legacy-reservation', 'migration-legacy-reservation',
             'migration-legacy-order', $1, $2,
             to_timestamp(1700000000), to_timestamp(1700003600), 60, 60,
             'planned', '[]'::jsonb, NULL
         )",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .bind(LAMINATION_1_NAME)
    .execute(pool)
    .await
    .expect("seed legacy schedule reservation");

    sqlx::query(
        "INSERT INTO mini_apparatus_order_transfers (
             transfer_id, idempotency_key, order_id, from_apparatus, to_apparatus,
             reason, actor_role, actor_ref, actor_display_name, session_id,
             progress_batch_id, material_barcodes, payload_json,
             canonical_from_apparatus_id, canonical_to_apparatus_id
         ) VALUES (
             'migration-legacy-transfer', 'migration-legacy-transfer',
             'migration-legacy-order', $1, $2, 'migration preservation',
             'admin', 'migration-test', 'Migration Test', 'migration-session',
             'migration-progress', '[]'::jsonb, '{\"preserved_marker\":\"transfer-preserved\"}'::jsonb,
             NULL, NULL
         )",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .bind(LEGACY_LAMINATION_2_ID)
    .execute(pool)
    .await
    .expect("seed transfer preservation row");

    sqlx::query(
        "INSERT INTO mini_order_freeze_requests (
             request_id, order_id, status, requester_role, requester_ref,
             requester_display_name, target_session_id, target_apparatus,
             target_worker_role, target_worker_ref, target_worker_display_name,
             requested_at_unix, transitioned_at_unix, canonical_target_apparatus_id
         ) VALUES (
             'migration-legacy-freeze', 'migration-legacy-order', 'pending',
             'admin', 'migration-requester', 'Migration Requester',
             'migration-freeze-session', $1, 'operator', 'migration-worker',
             'Migration Worker', 1700000000, 1700000000,
             'apparatus:default:bosma_8'
         )",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .execute(pool)
    .await
    .expect("seed freeze preservation row");

    sqlx::query(
        "INSERT INTO mini_training_queue_states (
             apparatus, order_id, state, canonical_apparatus_id
         ) VALUES ($1, 'training-migration-order', 'paused', NULL)",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .execute(pool)
    .await
    .expect("seed training queue state");
    sqlx::query(
        "INSERT INTO mini_training_queue_events (
             event_id, apparatus, order_id, action, from_state, to_state,
             actor_ref, actor_display_name
         ) VALUES (
             'migration-training-event', $1, 'training-migration-order',
             'pause', 'in_progress', 'paused',
             'migration-worker', 'Migration Worker'
         )",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .execute(pool)
    .await
    .expect("seed training queue event");
    sqlx::query(
        "INSERT INTO mini_training_progress_batches (
             batch_id, order_id, apparatus, qr_payload, payload_json,
             canonical_apparatus_id
         ) VALUES (
             'migration-training-progress', 'training-migration-order', $1,
             'migration-training-progress-qr',
             jsonb_build_object(
                 'apparatus', $1, 'preserved_marker', 'training-progress-preserved'
             ), NULL
         )",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .execute(pool)
    .await
    .expect("seed training progress batch");
    sqlx::query(
        "INSERT INTO mini_training_raw_material_assignments (
             id, order_id, apparatus, barcode, payload_json
         ) VALUES (
             'migration-training-material', 'training-migration-order', $1,
             'migration-training-barcode',
             jsonb_build_object(
                 'apparatus', $1, 'preserved_marker', 'training-material-preserved'
             )
         )",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .execute(pool)
    .await
    .expect("seed training material assignment");
    sqlx::query(
        "INSERT INTO mini_training_apparatus_modes (apparatus, enabled)
         VALUES ($1, TRUE)",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .execute(pool)
    .await
    .expect("seed training apparatus mode");
    sqlx::query(
        "INSERT INTO mini_training_input_batches (
             order_id, apparatus, batch_id, session_id, qr_payload
         ) VALUES (
             'training-input-preservation-order', $1,
             'migration-training-input-batch', 'migration-training-input-session',
             'migration-training-input-qr'
         )",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .execute(pool)
    .await
    .expect("seed training input batch");

    sqlx::query(
        "INSERT INTO mini_apparatus_material_rules (
             apparatus, item_groups, requirement_groups, requires_material, payload_json
         ) VALUES (
             $1, '[\"film\"]'::jsonb, '[\"film\"]'::jsonb, TRUE,
             '{\"preserved_marker\":\"material-rule-preserved\"}'::jsonb
         )",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .execute(pool)
    .await
    .expect("seed material rule");

    sqlx::query(
        "INSERT INTO mini_progress_batches (
             batch_id, session_id, apparatus, order_id, action, status,
             produced_qty, uom, qr_payload, label_item_code, label_item_name,
             payload_json
         ) VALUES (
             'migration-completion-batch', 'migration-completion-session', $1,
             'migration-legacy-order', 'complete', 'completed', 1, 'kg',
             'migration-completion-qr', 'MIGRATION-ITEM', 'Migration Item',
             '{\"preserved_marker\":\"completion-preserved\"}'::jsonb
         )",
    )
    .bind(LEGACY_LAMINATION_1_ID)
    .execute(pool)
    .await
    .expect("seed completion batch");
}

async fn stage_warehouse_assignment_columns(pool: &PgPool) {
    sqlx::query(
        "ALTER TABLE mini_warehouse_assignments
             ADD COLUMN assignment_kind TEXT,
             ADD COLUMN warehouse_name TEXT,
             ADD COLUMN apparatus_id TEXT",
    )
    .execute(pool)
    .await
    .expect("stage warehouse assignment columns");
}

async fn seed_legacy_assignment_snapshot(pool: &PgPool) {
    sqlx::query(
        "INSERT INTO mini_warehouses (id, name)
         VALUES ('warehouse:legacy-assignment', 'Legacy Assignment Warehouse')",
    )
    .execute(pool)
    .await
    .expect("seed legacy assignment warehouse");
}

async fn canonical_migration_history(pool: &PgPool) -> Vec<(String, String, String)> {
    sqlx::query_as(
        "SELECT version, checksum, applied_at::text
         FROM mini_schema_migrations
         ORDER BY version",
    )
    .fetch_all(pool)
    .await
    .expect("canonical migration history")
}

async fn assert_production_0062_indexes(pool: &PgPool) {
    for (index_name, expected_table) in PRODUCTION_0062_INDEXES {
        let (table_name, is_unique, is_valid, is_ready): (String, bool, bool, bool) =
            sqlx::query_as(
                "SELECT table_class.relname,
                        index_meta.indisunique,
                        index_meta.indisvalid,
                        index_meta.indisready
                 FROM pg_index index_meta
                 JOIN pg_class index_class ON index_class.oid = index_meta.indexrelid
                 JOIN pg_class table_class ON table_class.oid = index_meta.indrelid
                 JOIN pg_namespace namespace ON namespace.oid = index_class.relnamespace
                 WHERE namespace.nspname = 'public'
                   AND index_class.relname = $1",
            )
            .bind(index_name)
            .fetch_one(pool)
            .await
            .unwrap_or_else(|error| panic!("catalog entry for {index_name}: {error}"));
        assert_eq!(table_name, expected_table);
        assert!(is_unique, "{index_name} is not unique");
        assert!(is_valid, "{index_name} is not valid");
        assert!(is_ready, "{index_name} is not ready");
    }
}

async fn assert_cutover_rejected(pool: &PgPool, expected_message: &str) {
    let error = apply_postgres_migrations_through(pool, MIGRATIONS_THROUGH_CANONICAL_CHAIN)
        .await
        .expect_err("invalid legacy state must fail closed");
    assert!(
        error.to_string().contains(expected_message),
        "migration error did not contain {expected_message:?}: {error}"
    );

    let applied_migrations: i64 = sqlx::query_scalar("SELECT count(*) FROM mini_schema_migrations")
        .fetch_one(pool)
        .await
        .expect("migration history after rollback");
    assert_eq!(applied_migrations, MIGRATIONS_BEFORE_CANONICAL_CUTOVER as i64);
}

async fn capture_canonical_cutover_state(
    pool: &PgPool,
) -> (
    Vec<(String, String)>,
    Vec<(String, String, String)>,
    Vec<(String, String, String, i32)>,
    Vec<(String, String, String)>,
    Vec<(String, String, String)>,
    Vec<(String, String, Option<String>, Option<String>)>,
) {
    let apparatus: Vec<(String, String)> =
        sqlx::query_as("SELECT id, name FROM mini_apparatus ORDER BY id")
            .fetch_all(pool)
            .await
            .expect("apparatus state");
    let worker_groups: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT apparatus, group_code, canonical_apparatus_id
         FROM mini_worker_groups ORDER BY group_code",
    )
    .fetch_all(pool)
    .await
    .expect("worker group state");
    let profiles: Vec<(String, String, String, i32)> = sqlx::query_as(
        "SELECT apparatus_id, canonical_apparatus_id, apparatus, capacity_slots
         FROM mini_apparatus_capacity_profiles ORDER BY canonical_apparatus_id",
    )
    .fetch_all(pool)
    .await
    .expect("capacity profile state");
    let downtimes: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT id, apparatus_id, canonical_apparatus_id
         FROM mini_apparatus_downtimes ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .expect("downtime state");
    let reservations: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT reservation_id, apparatus_id, canonical_apparatus_id
         FROM mini_apparatus_schedule_reservations ORDER BY reservation_id",
    )
    .fetch_all(pool)
    .await
    .expect("reservation state");
    let assignments: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT assignment_kind, warehouse, warehouse_name, apparatus_id
         FROM mini_warehouse_assignments ORDER BY principal_ref",
    )
    .fetch_all(pool)
    .await
    .expect("warehouse assignment state");
    (
        apparatus,
        worker_groups,
        profiles,
        downtimes,
        reservations,
        assignments,
    )
}
