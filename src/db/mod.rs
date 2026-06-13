pub mod postgres;
pub mod postgres_apparatus_group;
pub mod postgres_engine;

#[cfg(test)]
mod postgres_apparatus_group_tests {
    use std::sync::Arc;

    use crate::core::apparatus_groups::{ApparatusGroupService, ApparatusGroupUpsert};
    use crate::db::postgres::apply_foundation_migration;
    use crate::db::postgres_apparatus_group::PostgresApparatusGroupStore;

    #[tokio::test]
    #[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_apparatus_groups"]
    async fn postgres_apparatus_group_store_round_trips_groups() {
        let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
        let db_name = "mini_rs_erp_test_apparatus_groups";
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

        let test_url = format!("postgres://wikki@127.0.0.1:5432/{db_name}");
        let pool = sqlx::PgPool::connect(&test_url).await.expect("test db");
        apply_foundation_migration(&pool)
            .await
            .expect("apply migration");
        let service =
            ApparatusGroupService::new(Arc::new(PostgresApparatusGroupStore::new(pool.clone())));

        let saved = service
            .upsert_group(ApparatusGroupUpsert {
                name: " pechat ".to_string(),
                apparatus: vec![
                    "7 ta rangli pechat".to_string(),
                    "8 ta rangli pechat".to_string(),
                    "7 TA RANGLI PECHAT".to_string(),
                ],
            })
            .await
            .expect("save group");
        assert_eq!(saved.name, "pechat");
        assert_eq!(
            saved.apparatus,
            vec![
                "7 ta rangli pechat".to_string(),
                "8 ta rangli pechat".to_string(),
            ]
        );

        let reloaded = service.groups().await.expect("load groups");
        assert_eq!(reloaded, vec![saved]);

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
}
