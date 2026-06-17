pub mod postgres;
pub mod postgres_apparatus_group;
pub mod postgres_calculate_order;
pub mod postgres_engine;
pub mod postgres_mini_order;
pub mod postgres_production_map;
pub mod postgres_push_token;
pub mod postgres_rps_batch;
pub mod postgres_warehouse;
pub mod postgres_worker;
pub mod postgres_worker_group;

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

        let created = service
            .upsert_apparatus(crate::core::apparatus_groups::ApparatusUpsert {
                warehouse: " Bobst 1 ".to_string(),
            })
            .await
            .expect("save apparatus");
        assert_eq!(created, "Bobst 1");
        assert_eq!(
            service.apparatus("bob", 20).await.expect("list apparatus"),
            vec!["Bobst 1".to_string()]
        );

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
    use crate::core::calculate_orders::{
        CalculateOrderImage, CalculateOrderStorePort, CalculateOrderTemplate,
    };
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

        let saved_image = store
            .save_image(
                "admin:admin",
                CalculateOrderImage {
                    image_id: "img-1".to_string(),
                    image_name: "rang.jpg".to_string(),
                    image_mime: "image/jpeg".to_string(),
                    image_size_bytes: 0,
                    body: b"fake-jpeg".to_vec(),
                },
            )
            .await
            .expect("save image");
        let loaded_image = store
            .get_image("admin:admin", "img-1")
            .await
            .expect("get image")
            .expect("image exists");
        assert_eq!(loaded_image, saved_image);
        assert!(
            store
                .get_image("werka:werka", "img-1")
                .await
                .expect("other owner")
                .is_none()
        );

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

#[cfg(test)]
mod postgres_production_map_tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use crate::core::production_map::{
        ProductionMapDefinition, ProductionMapEdge, ProductionMapError, ProductionMapNode,
        ProductionMapNodeKind, ProductionMapService, ProductionMapStorePort,
    };
    use crate::db::postgres::apply_foundation_migration;
    use crate::db::postgres_production_map::PostgresProductionMapStore;

    #[tokio::test]
    #[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_production_maps"]
    async fn postgres_production_map_store_persists_maps_sequences_and_queue_states() {
        let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
        let db_name = "mini_rs_erp_test_production_maps";
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
        let store = Arc::new(PostgresProductionMapStore::new(pool.clone()));
        let service = ProductionMapService::new(store.clone());

        let saved = service
            .upsert_map(test_map("zakaz-1001", "1001", "HOT"))
            .await
            .expect("save map");
        assert_eq!(saved.map.id, "zakaz-1001");
        assert_eq!(saved.map.order_number, "1001");
        let node_rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT node_id, kind, title
             FROM mini_production_map_nodes
             WHERE map_id = $1
             ORDER BY node_id",
        )
        .bind("zakaz-1001")
        .fetch_all(&pool)
        .await
        .expect("read mirrored nodes");
        assert_eq!(
            node_rows,
            vec![
                (
                    "apparatus".to_string(),
                    "apparatus".to_string(),
                    "7 ta rangli pechat".to_string(),
                ),
                ("end".to_string(), "end".to_string(), "End".to_string()),
                (
                    "start".to_string(),
                    "start".to_string(),
                    "Start".to_string()
                ),
            ]
        );
        let edge_rows: Vec<(i32, String, String)> = sqlx::query_as(
            "SELECT edge_index, from_node_id, to_node_id
             FROM mini_production_map_edges
             WHERE map_id = $1
             ORDER BY edge_index",
        )
        .bind("zakaz-1001")
        .fetch_all(&pool)
        .await
        .expect("read mirrored edges");
        assert_eq!(
            edge_rows,
            vec![
                (0, "start".to_string(), "apparatus".to_string()),
                (1, "apparatus".to_string(), "end".to_string()),
            ]
        );

        let duplicate = service
            .upsert_map(test_map("zakaz-1002", "1001", "OTHER"))
            .await;
        assert_eq!(duplicate, Err(ProductionMapError::DuplicateOrderNumber));

        service
            .set_apparatus_sequence(
                "7 ta rangli pechat",
                vec!["zakaz-1001".to_string(), " ".to_string()],
            )
            .await
            .expect("save sequence");
        let mut states = BTreeMap::new();
        states.insert("zakaz-1001".to_string(), "in_progress".to_string());
        service
            .apply_apparatus_queue_action(
                "7 ta rangli pechat",
                "zakaz-1001",
                crate::core::production_map::queue_state::ApparatusQueueAction::Complete,
                &["7 ta rangli pechat".to_string()],
                crate::core::production_map::QueueActionActor {
                    role: "admin".to_string(),
                    ref_: "test".to_string(),
                    display_name: "Test Admin".to_string(),
                },
            )
            .await
            .expect_err("cannot complete before state exists through service");

        store
            .put_apparatus_queue_states("7 ta rangli pechat", states)
            .await
            .expect("save queue states");
        let snapshot = service.live_snapshot().await.expect("snapshot");
        assert_eq!(
            snapshot
                .sequences
                .get("7 ta rangli pechat")
                .expect("sequence"),
            &vec!["zakaz-1001".to_string()]
        );
        assert_eq!(
            snapshot
                .queue_states
                .get("7 ta rangli pechat")
                .and_then(|items| items.get("zakaz-1001")),
            Some(&"in_progress".to_string())
        );

        service
            .restore_map(None, "zakaz-1001")
            .await
            .expect("delete map");
        assert!(service.maps().await.expect("maps").is_empty());

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

    fn test_map(id: &str, order_number: &str, product_code: &str) -> ProductionMapDefinition {
        ProductionMapDefinition {
            id: id.to_string(),
            product_code: product_code.to_string(),
            title: "Test map".to_string(),
            code: order_number.to_string(),
            order_number: order_number.to_string(),
            roll_count: Some(7.0),
            width_mm: Some(650.0),
            nodes: vec![
                test_node("start", ProductionMapNodeKind::Start, "Start", 0.0),
                test_node(
                    "apparatus",
                    ProductionMapNodeKind::Apparatus,
                    "7 ta rangli pechat",
                    120.0,
                ),
                test_node("end", ProductionMapNodeKind::End, "End", 240.0),
            ],
            edges: vec![
                ProductionMapEdge {
                    from: "start".to_string(),
                    to: "apparatus".to_string(),
                    branch: String::new(),
                },
                ProductionMapEdge {
                    from: "apparatus".to_string(),
                    to: "end".to_string(),
                    branch: String::new(),
                },
            ],
        }
    }

    fn test_node(id: &str, kind: ProductionMapNodeKind, title: &str, y: f64) -> ProductionMapNode {
        ProductionMapNode {
            id: id.to_string(),
            kind,
            title: title.to_string(),
            formula: None,
            role_code: String::new(),
            item_code: String::new(),
            qty_formula: String::new(),
            from_location: String::new(),
            to_location: String::new(),
            alternative_group_id: String::new(),
            alternative_group_label: String::new(),
            alternative_assigned_title: String::new(),
            x: 0.0,
            y,
        }
    }
}

#[cfg(test)]
mod postgres_mini_order_tests {
    use crate::core::calculate_orders::CalculateOrderTemplate;
    use crate::core::mini_orders::MiniOrderSink;
    use crate::core::production_map::{
        ProductionMapDefinition, ProductionMapEdge, ProductionMapNode, ProductionMapNodeKind,
    };
    use crate::db::postgres::apply_foundation_migration;
    use crate::db::postgres_mini_order::PostgresMiniOrderSink;

    #[tokio::test]
    #[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_mini_orders"]
    async fn postgres_mini_order_sink_saves_order_and_product_rows() {
        let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
        let db_name = "mini_rs_erp_test_mini_orders";
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
        let sink = PostgresMiniOrderSink::new(pool.clone());

        sink.save_order(&test_map(), &test_template())
            .await
            .expect("save mini order");

        let order_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mini_orders")
            .fetch_one(&pool)
            .await
            .expect("count orders");
        let product_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mini_order_products")
            .fetch_one(&pool)
            .await
            .expect("count products");
        assert_eq!(order_count, 1);
        assert_eq!(product_count, 1);

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

    fn test_map() -> ProductionMapDefinition {
        ProductionMapDefinition {
            id: "zakaz-9001".to_string(),
            product_code: "ITEM-9001".to_string(),
            title: "Mini order map".to_string(),
            code: "9001".to_string(),
            order_number: "9001".to_string(),
            roll_count: Some(7.0),
            width_mm: Some(650.0),
            nodes: vec![
                test_node("start", ProductionMapNodeKind::Start, "Start", 0.0),
                test_node("end", ProductionMapNodeKind::End, "End", 120.0),
            ],
            edges: vec![ProductionMapEdge {
                from: "start".to_string(),
                to: "end".to_string(),
                branch: String::new(),
            }],
        }
    }

    fn test_node(id: &str, kind: ProductionMapNodeKind, title: &str, y: f64) -> ProductionMapNode {
        ProductionMapNode {
            id: id.to_string(),
            kind,
            title: title.to_string(),
            formula: None,
            role_code: String::new(),
            item_code: String::new(),
            qty_formula: String::new(),
            from_location: String::new(),
            to_location: String::new(),
            alternative_group_id: String::new(),
            alternative_group_label: String::new(),
            alternative_assigned_title: String::new(),
            x: 0.0,
            y,
        }
    }

    fn test_template() -> CalculateOrderTemplate {
        CalculateOrderTemplate {
            id: String::new(),
            code: "9001".to_string(),
            name: "Mini mahsulot".to_string(),
            saved_at: String::new(),
            order_number: "9001".to_string(),
            customer_ref: "CUST-9001".to_string(),
            customer: "Mijoz".to_string(),
            item_code: "ITEM-9001".to_string(),
            product: "Mini mahsulot".to_string(),
            status: String::new(),
            material_display: "PET / PE".to_string(),
            color: "oq".to_string(),
            image_id: String::new(),
            image_name: String::new(),
            image_mime: String::new(),
            image_size_bytes: 0,
            image_url: String::new(),
            width_mm: 650.0,
            waste_percent: 5.0,
            roll_count: Some(7.0),
            first_layer_material: "pet".to_string(),
            first_layer_micron: "12".to_string(),
            second_layer_material: "pe oq".to_string(),
            second_layer_micron: "30".to_string(),
            third_layer_material: String::new(),
            third_layer_micron: String::new(),
            note: "test".to_string(),
            kg: 500.0,
            source_map_id: "zakaz-9001".to_string(),
        }
    }
}
