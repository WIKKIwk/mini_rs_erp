use sqlx::Row;

use super::fixtures::TestDatabase;

#[tokio::test]
async fn migration_0068_to_0069_is_append_only_and_restart_stable() {
    let database = TestDatabase::create_through("upgrade", 68).await;
    let before = migration_history(&database).await;
    assert_eq!(before.len(), 68);
    assert_eq!(
        before.last().unwrap().0,
        "0068_canonical_apparatus_fk_indexes"
    );

    database.migrate_current().await;
    let after_upgrade = migration_history(&database).await;
    assert_eq!(after_upgrade.len(), 69);
    assert_eq!(&after_upgrade[..68], before.as_slice());
    assert_eq!(
        after_upgrade.last().unwrap().0,
        "0069_canonical_apparatus_revision_authority"
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
    database.close().await;
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
