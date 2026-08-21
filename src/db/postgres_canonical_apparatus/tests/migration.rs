use sqlx::Row;

use crate::core::apparatus_standard::build_cutover_manifest;

use super::fixtures::TestDatabase;

#[tokio::test]
async fn migration_0068_to_0070_uses_exact_cutover_and_is_restart_stable() {
    let database = TestDatabase::create_through("upgrade", 68).await;
    let before = migration_history(&database).await;
    assert_eq!(before.len(), 68);
    assert_eq!(
        before.last().unwrap().0,
        "0068_canonical_apparatus_fk_indexes"
    );

    database.migrate_through(69).await;
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
    assert_eq!(after_upgrade.len(), 70);
    assert_eq!(&after_upgrade[..69], through_authority.as_slice());
    assert_eq!(
        after_upgrade.last().unwrap().0,
        "0070_canonical_apparatus_clean_cutover"
    );

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
