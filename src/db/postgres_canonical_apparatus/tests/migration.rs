use sqlx::Row;

use crate::core::apparatus_standard::build_cutover_manifest;
use crate::db::postgres::{
    apply_postgres_migrations_through, apply_postgres_migrations_through_version,
};

use super::fixtures::TestDatabase;

#[tokio::test]
async fn migration_0068_to_0071_uses_exact_cutover_and_is_restart_stable() {
    let database = TestDatabase::create_through("upgrade", 68).await;
    let before = migration_history(&database).await;
    assert_eq!(before.len(), 68);
    assert_eq!(
        before.last().unwrap().0,
        "0068_canonical_apparatus_fk_indexes"
    );

    apply_postgres_migrations_through_version(&database.pool, "0069")
        .await
        .expect("operator migration gate through 0069");
    let through_authority = migration_history(&database).await;
    assert_eq!(through_authority.len(), 69);
    assert_eq!(&through_authority[..68], before.as_slice());
    assert_eq!(
        through_authority.last().unwrap().0,
        "0069_canonical_apparatus_revision_authority"
    );

    let service = database.service();
    let report = service.cutover_preflight().await.unwrap();
    let draft = super::cutover::draft_manifest(&report, false);
    let manifest = build_cutover_manifest(&report, draft).unwrap();
    service.apply_legacy_cutover(manifest).await.unwrap();
    database.migrate_current().await;
    let after_upgrade = migration_history(&database).await;
    assert_eq!(after_upgrade.len(), 71);
    assert_eq!(&after_upgrade[..69], through_authority.as_slice());
    assert_eq!(after_upgrade.last().unwrap().0, "0071_qolip_lock_ownership");

    database.migrate_current().await;
    assert_eq!(migration_history(&database).await, after_upgrade);
    for obsolete_index in [
        "idx_mini_apparatus_lower_name",
        "idx_mini_apparatus_material_rules_lower_apparatus",
    ] {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_indexes WHERE indexname = $1)")
                .bind(obsolete_index)
                .fetch_one(&database.pool)
                .await
                .unwrap();
        assert!(
            !exists,
            "obsolete display identity index survived: {obsolete_index}"
        );
    }
    assert_clean_cutover_schema(&database).await;
    database.close().await;
}

#[tokio::test]
async fn migration_0065_backfills_append_only_raw_material_events_and_restores_guard() {
    let database = TestDatabase::create_through("0065_raw_event", 64).await;
    seed_raw_material_event(&database, "7 ta rangli bosma aparat").await;
    assert_raw_material_event_guard_enabled(&database).await;

    database.migrate_through(65).await;

    let canonical_apparatus_id: Option<String> = sqlx::query_scalar(
        "SELECT canonical_apparatus_id FROM mini_raw_material_events
         WHERE event_id = 'canonical-cutover-raw-event'",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!(
        canonical_apparatus_id.as_deref(),
        Some("apparatus:default:bosma_7")
    );
    assert_raw_material_event_guard_enabled(&database).await;
    let mutation = sqlx::query(
        "UPDATE mini_raw_material_events SET payload_json = '{\"mutated\":true}'::jsonb
         WHERE event_id = 'canonical-cutover-raw-event'",
    )
    .execute(&database.pool)
    .await
    .expect_err("append-only guard must be restored after 0065");
    assert!(mutation.to_string().contains("append-only"));

    let history = migration_history(&database).await;
    database.migrate_through(65).await;
    assert_eq!(migration_history(&database).await, history);
    database.close().await;
}

#[tokio::test]
async fn failed_migration_0065_rolls_back_raw_material_guard_and_schema_changes() {
    let database = TestDatabase::create_through("0065_raw_event_failure", 64).await;
    seed_raw_material_event(&database, "unmapped apparatus fixture").await;

    let error = apply_postgres_migrations_through(&database.pool, 65)
        .await
        .expect_err("0065 must reject an unresolved apparatus identity");
    assert!(
        error
            .to_string()
            .contains("unresolved legacy apparatus reference")
    );
    assert_raw_material_event_guard_enabled(&database).await;
    let canonical_column_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM information_schema.columns
             WHERE table_schema = 'public'
               AND table_name = 'mini_raw_material_events'
               AND column_name = 'canonical_apparatus_id'
         )",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert!(!canonical_column_exists);
    assert_eq!(migration_history(&database).await.len(), 64);
    database.close().await;
}

async fn seed_raw_material_event(database: &TestDatabase, apparatus: &str) {
    sqlx::query(
        "INSERT INTO mini_raw_material_events (
             event_id, idempotency_key, event_type, warehouse, barcode,
             item_code, item_name, qty_delta, uom, stock_status_after, apparatus,
             actor_role, actor_ref, source_type, source_id, payload_json
         ) VALUES (
             'canonical-cutover-raw-event', 'canonical-cutover-raw-event',
             'receipt_posted', 'Fixture Warehouse', 'CUTOVER-RAW-EVENT',
             'fixture-item', 'Fixture Item', 1, 'kg', 'available', $1,
             'system', 'migration-fixture', 'system', 'migration-fixture', '{}'::jsonb
         )",
    )
    .bind(apparatus)
    .execute(&database.pool)
    .await
    .unwrap();
}

async fn assert_raw_material_event_guard_enabled(database: &TestDatabase) {
    let enabled: String = sqlx::query_scalar(
        "SELECT tgenabled::text FROM pg_trigger
         WHERE tgrelid = 'mini_raw_material_events'::regclass
           AND tgname = 'mini_rme_no_update_delete_trg'",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!(enabled, "O");
}

async fn assert_clean_cutover_schema(database: &TestDatabase) {
    let legacy_group_table: bool =
        sqlx::query_scalar("SELECT to_regclass('public.mini_apparatus_groups') IS NOT NULL")
            .fetch_one(&database.pool)
            .await
            .unwrap();
    assert!(!legacy_group_table);

    for (table, forbidden_columns) in [
        ("mini_apparatus", &["group_id", "base_name", "kind"][..]),
        (
            "mini_apparatus_queue_policies",
            &["apparatus", "policy", "actor_role", "actor_ref"][..],
        ),
        (
            "mini_apparatus_material_rules",
            &[
                "apparatus",
                "item_groups",
                "requirement_groups",
                "requires_material",
            ][..],
        ),
        (
            "mini_apparatus_capacity_profiles",
            &[
                "apparatus_id",
                "apparatus",
                "capacity_slots",
                "capabilities",
                "notes",
            ][..],
        ),
    ] {
        let remaining: Vec<String> = sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns
             WHERE table_schema = 'public' AND table_name = $1
               AND column_name = ANY($2::text[])
             ORDER BY column_name",
        )
        .bind(table)
        .bind(forbidden_columns)
        .fetch_all(&database.pool)
        .await
        .unwrap();
        assert!(
            remaining.is_empty(),
            "legacy columns remain: {table} {remaining:?}"
        );
    }

    let incomplete_projection_rows: i64 = sqlx::query_scalar(
        "SELECT
             (SELECT count(*) FROM mini_apparatus
               WHERE source_revision IS NULL OR source_aasx_sha256 IS NULL)
           + (SELECT count(*) FROM mini_apparatus_queue_policies
               WHERE source_revision IS NULL OR source_aasx_sha256 IS NULL)
           + (SELECT count(*) FROM mini_apparatus_material_rules
               WHERE source_revision IS NULL OR source_aasx_sha256 IS NULL)
           + (SELECT count(*) FROM mini_apparatus_capacity_profiles
               WHERE source_revision IS NULL OR source_aasx_sha256 IS NULL)",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!(incomplete_projection_rows, 0);
}

async fn migration_history(database: &TestDatabase) -> Vec<(String, String, String)> {
    sqlx::query(
        "SELECT version, checksum, applied_at::text AS applied_at
         FROM mini_schema_migrations ORDER BY version",
    )
    .fetch_all(&database.pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| {
        (
            row.get("version"),
            row.get("checksum"),
            row.get("applied_at"),
        )
    })
    .collect()
}
