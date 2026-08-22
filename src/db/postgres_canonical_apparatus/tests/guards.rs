use sqlx::Error;

use super::fixtures::{TestDatabase, apparatus_state, draft, metadata};

#[tokio::test]
async fn append_only_authority_and_derived_projection_guards_reject_direct_writes() {
    let database = TestDatabase::create("guards").await;
    let service = database.service();
    let created = service
        .create(
            draft("physical-asset:guard-01", "Guard fixture"),
            metadata("command:guard-create-01"),
        )
        .await
        .unwrap();
    let apparatus_id = created.revision.apparatus_id;
    let baseline = apparatus_state(&database.pool, &apparatus_id).await;

    let direct_projection_writes = [
        "UPDATE mini_apparatus SET name = 'forbidden' WHERE id = $1",
        "UPDATE mini_apparatus_queue_policies SET updated_at = now() \
         WHERE canonical_apparatus_id = $1",
        "UPDATE mini_apparatus_material_rules SET updated_at = now() \
         WHERE canonical_apparatus_id = $1",
        "UPDATE mini_apparatus_capacity_profiles SET updated_at = now() \
         WHERE canonical_apparatus_id = $1",
    ];
    for statement in direct_projection_writes {
        let error = sqlx::query(statement)
            .bind(apparatus_id.as_str())
            .execute(&database.pool)
            .await
            .expect_err("canonical projection must reject independent writes");
        assert_database_code(&error, "42501");
    }

    for statement in [
        "UPDATE mini_canonical_apparatus_revisions SET actor_id = 'forbidden' \
         WHERE apparatus_id = $1",
        "DELETE FROM mini_canonical_apparatus_revisions WHERE apparatus_id = $1",
        "UPDATE mini_canonical_apparatus_identities SET physical_asset_id = 'forbidden' \
         WHERE apparatus_id = $1",
        "DELETE FROM mini_canonical_apparatus_identities WHERE apparatus_id = $1",
    ] {
        let error = sqlx::query(statement)
            .bind(apparatus_id.as_str())
            .execute(&database.pool)
            .await
            .expect_err("canonical identity and revisions must be append-only");
        assert_database_code(&error, "55000");
    }

    assert_eq!(
        apparatus_state(&database.pool, &apparatus_id).await,
        baseline
    );
    database.close().await;
}

fn assert_database_code(error: &Error, expected: &str) {
    let Error::Database(database) = error else {
        panic!("expected PostgreSQL database error, got {error:?}");
    };
    assert_eq!(database.code().as_deref(), Some(expected));
}
