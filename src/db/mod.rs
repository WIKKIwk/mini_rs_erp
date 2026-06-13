pub mod postgres;
pub mod postgres_apparatus_group;
pub mod postgres_calculate_order;
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

#[cfg(test)]
mod postgres_calculate_order_tests {
    use crate::core::calculate_orders::{CalculateOrderStorePort, CalculateOrderTemplate};
    use crate::db::postgres::apply_foundation_migration;
    use crate::db::postgres_calculate_order::PostgresCalculateOrderStore;

    #[tokio::test]
    #[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_calculate_orders"]
    async fn postgres_calculate_order_store_round_trips_and_dedupes_quick_templates() {
        let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
        let db_name = "mini_rs_erp_test_calculate_orders";
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
        let store = PostgresCalculateOrderStore::new(pool.clone());

        let first = store
            .upsert("admin:admin", test_template("1111", 530.0, 500.0))
            .await
            .expect("save first");
        let second = store
            .upsert("admin:admin", test_template("2222", 530.0, 900.0))
            .await
            .expect("save duplicate quick template");
        assert_ne!(first.code, second.code);

        let rows = store.list("admin:admin").await.expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].code, second.code);

        let updated = store
            .upsert(
                "admin:admin",
                CalculateOrderTemplate {
                    width_mm: 640.0,
                    ..second.clone()
                },
            )
            .await
            .expect("update second");
        assert_eq!(updated.id, second.id);

        let rows = store.list("admin:admin").await.expect("list after update");
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row.code == first.code));
        assert!(rows.iter().any(|row| row.code == updated.code));

        store
            .delete("admin:admin", &updated.id)
            .await
            .expect("delete updated");
        let rows = store.list("admin:admin").await.expect("list after delete");
        assert_eq!(rows, vec![first]);

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

    fn test_template(code: &str, width_mm: f64, kg: f64) -> CalculateOrderTemplate {
        CalculateOrderTemplate {
            id: String::new(),
            code: code.to_string(),
            name: "Qurt".to_string(),
            saved_at: String::new(),
            order_number: code.to_string(),
            customer_ref: String::new(),
            customer: String::new(),
            item_code: "QURT-001".to_string(),
            product: "Qurt".to_string(),
            status: String::new(),
            material_display: String::new(),
            color: String::new(),
            image_id: String::new(),
            image_name: String::new(),
            image_mime: String::new(),
            image_size_bytes: 0,
            image_url: String::new(),
            width_mm,
            waste_percent: 5.0,
            roll_count: Some(7.0),
            first_layer_material: "pet".to_string(),
            first_layer_micron: "12".to_string(),
            second_layer_material: "pe oq".to_string(),
            second_layer_micron: "30".to_string(),
            third_layer_material: String::new(),
            third_layer_micron: String::new(),
            note: String::new(),
            kg,
            source_map_id: format!("zakaz-{code}"),
        }
    }
}
