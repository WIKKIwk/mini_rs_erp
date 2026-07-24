#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postgres_config_uses_mini_erp_database_url() {
        let config = PostgresConfig::from_env_with(|key| match key {
            "MINI_ERP_DATABASE_URL" => {
                Some("postgres://mini:secret@127.0.0.1:5432/mini_rs_erp".to_string())
            }
            _ => None,
        })
        .expect("config");

        assert_eq!(
            config.database_url,
            "postgres://mini:secret@127.0.0.1:5432/mini_rs_erp"
        );
        assert_eq!(config.max_connections, 16);
        assert_eq!(config.min_connections, 2);
    }

    #[test]
    fn postgres_config_rejects_blank_database_url() {
        let error = PostgresConfig::from_env_with(|_| Some(" ".to_string()))
            .expect_err("blank url rejected");

        assert_eq!(error, PostgresConfigError::MissingDatabaseUrl);
    }

    #[tokio::test]
    async fn postgres_bootstrap_requires_database_url() {
        let previous = std::env::var("MINI_ERP_DATABASE_URL").ok();
        unsafe {
            std::env::remove_var("MINI_ERP_DATABASE_URL");
        }

        let error = connect_and_migrate_required()
            .await
            .expect_err("missing database url must fail");

        assert!(matches!(error, PostgresBootstrapError::MissingDatabaseUrl));
        unsafe {
            if let Some(value) = previous {
                std::env::set_var("MINI_ERP_DATABASE_URL", value);
            }
        }
    }

    #[test]
    fn postgres_foundation_migration_defines_core_tables() {
        let migration = foundation_migration_sql();

        for table in [
            "mini_orders",
            "mini_order_products",
            "mini_quick_order_templates",
            "mini_quick_order_images",
            "mini_push_tokens",
            "mini_items",
            "mini_customers",
            "mini_customer_items",
            "mini_item_groups",
            "mini_production_maps",
            "mini_production_map_nodes",
            "mini_production_map_edges",
            "mini_apparatus",
            "mini_apparatus_groups",
            "mini_workers",
            "mini_worker_groups",
            "mini_queue_sequences",
            "mini_queue_states",
            "mini_warehouses",
            "mini_qolip_locations",
            "mini_gscale_receipts",
            "mini_raw_material_stock",
            "mini_raw_material_events",
            "mini_finished_goods_stock",
            "mini_rps_batches",
            "mini_engine_events",
            "mini_idempotency_keys",
        ] {
            assert!(
                migration.contains(&format!("CREATE TABLE IF NOT EXISTS {table}")),
                "missing table {table}"
            );
        }

        for forbidden in ["tabWork Order", "tabBOM", "tabStock Entry", "doctype"] {
            assert!(
                !migration.to_lowercase().contains(&forbidden.to_lowercase()),
                "migration must not contain legacy term {forbidden}"
            );
        }
    }

    #[test]
    fn item_identity_migration_cascades_customer_assignments() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(name, _)| *name == "0015_item_identity_updates")
            .map(|(_, sql)| *sql)
            .expect("item identity migration");

        assert!(migration.contains("mini_customer_items_item_code_fkey"));
        assert!(migration.contains("ON UPDATE CASCADE"));
        assert!(migration.contains("ON DELETE CASCADE"));
    }

    #[test]
    fn postgres_migration_runner_splits_foundation_sql() {
        let statements = split_sql_statements(foundation_migration_sql());

        assert!(statements.len() > 12);
        assert!(
            statements
                .iter()
                .any(|statement| statement.starts_with("CREATE TABLE IF NOT EXISTS mini_orders"))
        );
        assert!(statements.iter().all(|statement| !statement.ends_with(';')));
    }

    #[test]
    fn postgres_migration_runner_keeps_dollar_quoted_functions_together() {
        let statements = split_sql_statements(
            "SELECT 1;\nCREATE FUNCTION demo() RETURNS void LANGUAGE plpgsql AS $$\nBEGIN\n  PERFORM 1;\nEND;\n$$;\nSELECT 2;",
        );

        assert_eq!(statements.len(), 3);
        assert!(statements[1].contains("PERFORM 1;"));
        assert!(statements[1].contains("END;"));
    }

    #[test]
    fn postgres_migrations_are_versioned_and_checksummed() {
        let versions = POSTGRES_MIGRATIONS
            .iter()
            .map(|(version, _)| *version)
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(versions.len(), POSTGRES_MIGRATIONS.len());
        assert!(POSTGRES_MIGRATIONS.iter().all(|(version, sql)| {
            !version.trim().is_empty() && migration_checksum(sql).len() == 64
        }));
    }

    #[test]
    fn qolip_legacy_lookup_migration_indexes_locations_and_checkouts() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0024_qolip_legacy_lookup_index")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("qolip legacy lookup migration");

        assert!(migration.contains("idx_mini_qolip_locations_qolip_code"));
        assert!(migration.contains("idx_mini_qolip_checkouts_qolip_code_status"));
        assert!(migration.contains("lower(qolip_code)"));
    }

    #[test]
    fn order_control_migration_persists_strict_freeze_states() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0025_order_control_state")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("order control migration");

        assert!(migration.contains("create table if not exists mini_order_control_states"));
        assert!(migration.contains("freeze_requested"));
        assert!(migration.contains("frozen_at_unix"));
        assert!(migration.contains("references mini_production_maps(id) on delete cascade"));
    }

    #[test]
    fn order_freeze_chat_card_migration_is_request_scoped_and_ordered() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0026_order_freeze_request_chat_cards")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("order freeze chat card migration");

        assert!(migration.contains("create table if not exists mini_order_freeze_requests"));
        assert!(migration.contains("target_session_id"));
        assert!(migration.contains("target_worker_ref"));
        assert!(migration.contains("freeze_request_id"));
        assert!(migration.contains("create table if not exists mini_order_freeze_chat_outbox"));
        assert!(migration.contains("event_sequence bigserial primary key"));
        assert!(migration.contains("order_freeze_request"));
    }

    #[test]
    fn rps_batch_history_migration_is_additive_and_owner_scoped() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0021_rps_batch_history")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("RPS batch history migration");

        assert!(migration.contains("create table if not exists mini_rps_batch_history"));
        assert!(migration.contains("primary key (owner_key, batch_id)"));
        assert!(migration.contains("owner_key text not null"));
        assert!(migration.contains("payload_json jsonb not null"));
        assert!(!migration.contains("delete from"));
        assert!(!migration.contains("drop table"));
    }

    #[test]
    fn rps_batch_code_migration_is_unique_additive_and_backfills_payloads() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0022_rps_batch_codes")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("RPS batch code migration");

        assert!(migration.contains("create table if not exists mini_rps_batch_identities"));
        assert!(migration.contains("batch_code char(24) primary key"));
        assert!(migration.contains("unique (owner_key, batch_id)"));
        assert!(migration.contains("jsonb_set"));
        assert!(migration.contains("on conflict (owner_key, batch_id) do nothing"));
        assert!(!migration.contains("delete from"));
        assert!(!migration.contains("drop table"));
    }

    #[test]
    fn applied_chat_delivery_migration_checksum_is_immutable() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0016_chat_delivery_reliability")
            .expect("chat delivery migration");

        assert_eq!(
            migration_checksum(migration.1),
            "89a259d3c0a55e2ab8a0baea80b2c75edc2d43d4457a294c86b9a0e5a43d5e59"
        );
    }

    #[test]
    fn chat_delivery_followup_reconciles_applied_recipient_rows() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0017_chat_delivery_reliability_followup")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("chat delivery followup migration");

        assert!(migration.contains("mini_chat_assign_event_cursor"));
        assert!(migration.contains("delete from mini_chat_push_deliveries"));
        assert!(migration.contains("event.push_recipient_keys"));
        assert!(migration.contains("insert into mini_chat_push_deliveries"));
    }

    #[test]
    fn item_master_migration_removes_warehouse_ownership() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0018_item_master_without_warehouse")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("item master warehouse migration");

        assert!(migration.contains("payload_json - 'warehouse'"));
        assert!(migration.contains("drop column if exists warehouse"));
    }

    #[test]
    fn chat_voice_migration_extends_media_and_message_contracts() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0019_chat_voice_messages")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("chat voice migration");

        assert!(migration.contains("media_kind in ('image', 'video', 'audio')"));
        assert!(migration.contains("declared_size_bytes <= 67108864"));
        assert!(migration.contains("'canonicalize_audio'"));
        assert!(migration.contains("'text', 'image', 'video', 'audio'"));
        assert!(migration.contains("message_type in ('image', 'video', 'audio')"));
    }

    #[test]
    fn worker_identity_migration_preserves_history_without_name_identity() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0020_worker_identity_lifecycle")
            .map(|(_, sql)| *sql)
            .expect("worker identity migration");

        assert!(migration.contains("ADD COLUMN IF NOT EXISTS active BOOLEAN"));
        assert!(migration.contains("DROP CONSTRAINT IF EXISTS mini_workers_name_unique"));
        assert!(migration.contains("mini_worker_identity_aliases"));
        assert!(migration.contains("valid_from TIMESTAMPTZ NOT NULL"));
        assert!(migration.contains("WHERE active AND phone_key <> ''"));
        assert!(migration.contains("ON DELETE RESTRICT"));
    }

    #[test]
    fn postgres_chat_migration_defines_durable_message_flow() {
        let migration = POSTGRES_MIGRATIONS[4].1.to_lowercase();

        for table in [
            "mini_chat_principals",
            "mini_chat_conversations",
            "mini_chat_conversation_members",
            "mini_chat_messages",
            "mini_chat_device_cursors",
            "mini_chat_outbox_events",
        ] {
            assert!(
                migration.contains(&format!("create table if not exists {table}")),
                "missing chat table {table}"
            );
        }
        assert!(migration.contains("mini_chat_messages_client_id_unique"));
        assert!(migration.contains("last_read_sequence"));
        assert!(migration.contains("published_at is null"));
        assert!(!migration.contains("partition by"));
    }

    #[test]
    fn postgres_chat_media_migration_defines_private_upload_foundation() {
        let migration = POSTGRES_MIGRATIONS[10].1.to_lowercase();

        for table in [
            "mini_chat_media",
            "mini_chat_message_attachments",
            "mini_chat_media_jobs",
        ] {
            assert!(
                migration.contains(&format!("create table if not exists {table}")),
                "missing chat media table {table}"
            );
        }
        assert!(migration.contains("mini_chat_media_client_upload_unique"));
        assert!(migration.contains("declared_size_bytes > 0"));
        assert!(migration.contains("media_kind in ('image', 'video')"));
        assert!(migration.contains("message_id text not null unique"));
        assert!(migration.contains("media_id text not null unique"));
        assert!(migration.contains("job_status = 'pending'"));
        assert!(!migration.contains("public_url"));
    }

    #[test]
    fn postgres_chat_media_v1_migration_enables_processed_attachments() {
        let migration = POSTGRES_MIGRATIONS[11].1.to_lowercase();

        assert!(migration.contains("processed_content_type"));
        assert!(migration.contains("processed_size_bytes"));
        assert!(migration.contains("'image', 'video'"));
        assert!(migration.contains("char_length(body) between 0 and 4000"));
        assert!(migration.contains("idx_mini_chat_media_jobs_claim"));
        assert!(!migration.contains("public_url"));
    }

    #[test]
    fn postgres_chat_media_incident_video_migration_enables_resumable_limits() {
        let migration = POSTGRES_MIGRATIONS[12].1.to_lowercase();

        assert!(migration.contains("declared_duration_ms between 1 and 600000"));
        assert!(migration.contains("declared_size_bytes <= 2147483648"));
        assert!(migration.contains("processed_size_bytes <= 1073741824"));
        assert!(migration.contains("upload_mode in ('single', 'chunked')"));
        assert!(migration.contains("create table if not exists mini_chat_media_upload_chunks"));
        assert!(migration.contains("primary key (media_id, chunk_index)"));
        assert!(migration.contains("frame_rate_milli between 1 and 60000"));
        assert!(!migration.contains("public_url"));
    }

    #[test]
    fn postgres_raw_material_correction_migration_extends_audit_constraints_safely() {
        let migration = POSTGRES_MIGRATIONS[13].1.to_lowercase();

        assert!(migration.contains("'stock_corrected'"));
        assert!(migration.contains("'stock_correction'"));
        assert!(migration.contains("mini_rme_stock_correction_consistent"));
        assert!(migration.contains("set local lock_timeout = '5s'"));
        assert!(migration.contains("set local statement_timeout = '60s'"));
        assert!(migration.contains("not valid"));
        assert!(migration.contains("validate constraint mini_rme_event_type_allowed"));
        assert!(migration.contains("validate constraint mini_rme_source_type_allowed"));
        assert!(migration.contains("validate constraint mini_rme_qty_sign_allowed"));
        assert!(!migration.contains("delete from mini_raw_material_events"));
        assert!(!migration.contains("update mini_raw_material_events"));
    }

    #[test]
    fn postgres_boyoqchi_migration_defines_role_inbox() {
        let migration = POSTGRES_MIGRATIONS[5].1.to_lowercase();

        assert!(migration.contains("'qolipchi', 'boyoqchi'"));
        assert!(migration.contains("create table if not exists mini_returned_paint_requests"));
        assert!(migration.contains("target_role = 'boyoqchi'"));
        assert!(migration.contains("jsonb_array_length(items_json) > 0"));
    }

    #[test]
    fn postgres_runtime_ownership_migration_repairs_service_tables() {
        let migration = POSTGRES_MIGRATIONS[6].1.to_lowercase();

        assert!(migration.contains("rolname = 'mini_rs_erp'"));
        for table in [
            "mini_system_users",
            "mini_chat_principals",
            "mini_chat_conversations",
            "mini_chat_conversation_members",
            "mini_chat_messages",
            "mini_chat_device_cursors",
            "mini_chat_outbox_events",
            "mini_returned_paint_requests",
        ] {
            assert!(migration.contains(&format!("'{table}'")));
        }
        assert!(migration.contains("owner to mini_rs_erp"));
    }

    #[test]
    fn postgres_returned_paint_calculation_migration_uses_exact_numeric_columns() {
        let migration = POSTGRES_MIGRATIONS[7].1.to_lowercase();

        assert!(migration.contains("numeric(30, 12)"));
        assert!(migration.contains("rasxot_mix_total"));
        assert!(migration.contains("final_used_alcohol"));
        assert!(migration.contains("final_used_paint"));
        assert!(migration.contains("jsonb_each"));
        assert!(migration.contains("round(rasxot_mix_total, 12)"));
        assert!(migration.contains("999999999999999999"));
    }

    #[test]
    fn postgres_returned_paint_solvent_migration_adds_all_solvent_values_to_alcohol() {
        let migration = POSTGRES_MIGRATIONS[8].1.to_lowercase();

        assert!(migration.contains("category = 'solvents'"));
        assert!(migration.contains("jsonb_each"));
        assert!(migration.contains("rasxot_direct_alcohol"));
        assert!(migration.contains("astatka_direct_alcohol"));
        assert!(migration.contains("rasxot_mix_total * 0.30::numeric"));
        assert!(migration.contains("astatka_mix_total * 0.30::numeric"));
        assert!(migration.contains("final_used_alcohol"));
    }

    #[test]
    fn postgres_returned_paint_image_migration_supports_pending_and_idempotent_completion() {
        let migration = POSTGRES_MIGRATIONS[9].1.to_lowercase();

        assert!(migration.contains("create table if not exists mini_returned_paint_images"));
        assert!(migration.contains("waiting_for_boyoqchi_input"));
        assert!(migration.contains("mini_returned_paint_requests_workflow_consistent"));
        assert!(migration.contains("jsonb_array_length(items_json) = 0"));
        assert!(migration.contains("image_size_bytes = octet_length(body)"));
        assert!(migration.contains("create unique index"));
    }

    #[test]
    fn postgres_order_integrity_migration_links_orders_and_indexes_foreign_keys() {
        let migration = POSTGRES_MIGRATIONS[1].1.to_lowercase();

        assert!(migration.contains("idx_mini_order_products_order_id"));
        assert!(migration.contains("idx_mini_customer_items_item_code"));
        assert!(migration.contains("set order_id = orders.id"));
        assert!(migration.contains("maps.id = orders.id"));
    }

    #[test]
    fn postgres_erp_integrity_migration_uses_exact_quantities_and_constraints() {
        let migration = POSTGRES_MIGRATIONS[2].1.to_lowercase();

        assert!(migration.contains("numeric(24, 9)"));
        assert!(migration.contains("mini_production_maps_width_positive"));
        assert!(migration.contains("mini_gscale_receipts_qty_positive"));
        assert!(migration.contains("mini_raw_material_events_qty_finite"));
        assert!(migration.contains("idx_mini_customers_phone_key_unique"));
        assert!(migration.contains("mini_raw_material_assignments_order_fkey"));
        assert!(migration.contains("product_form"));
    }

    #[test]
    fn postgres_foundation_migration_indexes_apparatus_case_insensitively() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(
            migration.contains("idx_mini_apparatus_groups_lower_name")
                && migration.contains("lower(name)")
        );
        assert!(migration.contains("idx_mini_apparatus_lower_name"));
    }

    #[test]
    fn postgres_foundation_migration_keeps_quick_template_codes_unique() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains("idx_mini_quick_order_templates_owner_lower_code"));
        assert!(migration.contains("idx_mini_quick_order_templates_owner_quick_key"));
        assert!(!migration.contains("owner_key_unique unique"));
    }

    #[test]
    fn postgres_foundation_migration_keeps_qolip_codes_unique_not_item_codes() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains(
            "alter table mini_qolip_product_specs drop constraint if exists mini_qolip_product_specs_pkey"
        ));
        assert!(migration.contains("idx_mini_qolip_product_specs_qolip_code_unique"));
        assert!(migration.contains("lower(qolip_code)"));
        assert!(!migration.contains("item_code text primary key"));
    }

    #[test]
    fn postgres_foundation_migration_backfills_quick_template_frame_fields() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains("quick_template_dimensions"));
        assert!(migration.contains("frame_product_size_mm"));
        assert!(migration.contains("frame_count"));
        assert!(migration.contains("jsonb_set"));
    }

    #[test]
    fn postgres_foundation_migration_leaves_production_order_number_to_store_logic() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains("idx_mini_production_maps_order_number"));
        assert!(!migration.contains("mini_production_maps_order_number_unique"));
    }

    #[test]
    fn postgres_foundation_migration_guards_one_open_order_run_session() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains("idx_mini_order_run_sessions_one_open"));
        assert!(migration.contains("where status in ('active', 'paused')"));
    }

    #[test]
    fn postgres_foundation_migration_persists_bosma_progress_metrics() {
        let migration = foundation_migration_sql().to_lowercase();

        for column in [
            "return_ink_kg",
            "lamination_print_leftover_rolls",
            "lamination_film_leftover_rolls",
            "rezka_bosma_waste",
            "rezka_lamination_waste",
            "rezka_edge_waste",
            "total_waste",
            "finished_goods_kg",
            "finished_goods_meter",
            "description",
        ] {
            assert!(
                migration.contains(column),
                "missing progress metric column {column}"
            );
        }
    }

    #[test]
    fn postgres_foundation_migration_indexes_wip_apparatus_key() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains("current_apparatus_key"));
        assert!(migration.contains("idx_mini_progress_batches_wip_status_apparatus_key"));
        assert!(migration.contains("wip_status, current_apparatus_key, updated_at desc"));
    }

    #[test]
    fn postgres_foundation_migration_persists_material_requirement_groups() {
        let migration = foundation_migration_sql().to_lowercase();

        assert!(migration.contains("mini_apparatus_material_rules"));
        assert!(migration.contains("requirement_groups jsonb not null default '[]'::jsonb"));
        assert!(
            migration.contains("mini_apparatus_material_rules_requirement_groups_array"),
            "missing requirement_groups array constraint"
        );
    }

    #[tokio::test]
    #[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test"]
    async fn postgres_live_foundation_migration_applies_to_clean_database() {
        let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
        let db_name = std::env::var("MINI_ERP_TEST_DATABASE_NAME")
            .unwrap_or_else(|_| "mini_rs_erp_test".to_string());
        assert!(
            db_name.starts_with("mini_rs_erp_test"),
            "test database name must start with mini_rs_erp_test"
        );

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

        let (admin_base_url, _) = admin_url
            .rsplit_once('/')
            .expect("admin database URL must include a database name");
        let test_url = format!("{admin_base_url}/{db_name}");
        let pool = sqlx::PgPool::connect(&test_url).await.expect("test db");
        apply_foundation_migration(&pool)
            .await
            .expect("apply foundation migration");

        let table_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM information_schema.tables
             WHERE table_schema = 'public'
               AND table_name IN (
                 'mini_orders',
                 'mini_order_products',
                 'mini_quick_order_templates',
                 'mini_quick_order_images',
                 'mini_items',
                 'mini_item_groups',
                 'mini_production_maps',
                 'mini_production_map_nodes',
                 'mini_production_map_edges',
                 'mini_apparatus',
                 'mini_apparatus_groups',
                 'mini_workers',
                 'mini_worker_groups',
                 'mini_qolip_locations',
                 'mini_queue_sequences',
                 'mini_queue_states',
                 'mini_apparatus_queue_policies',
                 'mini_queue_action_events',
                 'mini_engine_events',
                 'mini_idempotency_keys',
                 'mini_chat_media',
                 'mini_chat_message_attachments',
                 'mini_chat_media_jobs'
               )",
        )
        .fetch_one(&pool)
        .await
        .expect("count tables");
        assert_eq!(table_count, 23);

        sqlx::query(
            "INSERT INTO mini_idempotency_keys (key, domain, action, entity_id)
             VALUES ('test-key-1', 'production_maps', 'batch_move', 'zakaz-1')",
        )
        .execute(&pool)
        .await
        .expect("insert idempotency key");

        let duplicate = sqlx::query(
            "INSERT INTO mini_idempotency_keys (key, domain, action)
             VALUES ('test-key-1', 'production_maps', 'batch_move')",
        )
        .execute(&pool)
        .await;
        assert!(duplicate.is_err(), "idempotency key must be unique");

        pool.close().await;

        let admin_pool = sqlx::PgPool::connect(&admin_url)
            .await
            .expect("admin db cleanup");
        sqlx::query(&format!(
            r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
        ))
        .execute(&admin_pool)
        .await
        .expect("cleanup test db");
        admin_pool.close().await;
    }

    #[tokio::test]
    #[ignore = "requires local PostgreSQL and creates/drops mini_rs_erp_test_chat_followup"]
    async fn postgres_live_chat_delivery_followup_upgrades_applied_0016() {
        let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
        let db_name = std::env::var("MINI_ERP_TEST_CHAT_DATABASE_NAME")
            .unwrap_or_else(|_| "mini_rs_erp_test_chat_followup".to_string());
        assert!(
            db_name.starts_with("mini_rs_erp_test"),
            "test database name must start with mini_rs_erp_test"
        );

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

        let (admin_base_url, _) = admin_url
            .rsplit_once('/')
            .expect("admin database URL must include a database name");
        let test_url = format!("{admin_base_url}/{db_name}");
        let pool = sqlx::PgPool::connect(&test_url).await.expect("test db");
        apply_postgres_migrations_through(&pool, 15)
            .await
            .expect("apply through 0015");
        sqlx::raw_sql(
            r#"INSERT INTO mini_chat_principals
                   (principal_id, principal_role, principal_ref, display_name)
               VALUES
                   ('sender', 'admin', 'SENDER', 'Sender'),
                   ('receiver', 'customer', 'RECEIVER', 'Receiver');
               INSERT INTO mini_chat_conversations
                   (conversation_id, kind, title, dm_key, created_by_principal_id)
               VALUES ('conversation', 'dm', '', 'sender:receiver', 'sender');
               INSERT INTO mini_chat_outbox_events
                   (event_id, topic, conversation_id, message_sequence,
                    recipient_keys, payload_json)
               VALUES (
                   'event-1',
                   'chat.message.created',
                   'conversation',
                   1,
                   '["admin:SENDER","customer:RECEIVER"]'::jsonb,
                   '{"message":{"sender_role":"admin","sender_ref":"SENDER"}}'::jsonb
               )"#,
        )
        .execute(&pool)
        .await
        .expect("seed pre-0016 outbox event");

        apply_postgres_migrations_through(&pool, 16)
            .await
            .expect("apply original 0016");
        let original_checksum: String = sqlx::query_scalar(
            "SELECT checksum FROM mini_schema_migrations
             WHERE version = '0016_chat_delivery_reliability'",
        )
        .fetch_one(&pool)
        .await
        .expect("0016 checksum");
        assert_eq!(
            original_checksum,
            "89a259d3c0a55e2ab8a0baea80b2c75edc2d43d4457a294c86b9a0e5a43d5e59"
        );
        let delivery_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM mini_chat_push_deliveries WHERE event_id = 'event-1'",
        )
        .fetch_one(&pool)
        .await
        .expect("old delivery count");
        assert_eq!(delivery_count, 2);

        apply_foundation_migration(&pool)
            .await
            .expect("apply 0017 followup");
        let recipient_keys: serde_json::Value = sqlx::query_scalar(
            "SELECT push_recipient_keys FROM mini_chat_outbox_events
             WHERE event_id = 'event-1'",
        )
        .fetch_one(&pool)
        .await
        .expect("reconciled recipient keys");
        assert_eq!(recipient_keys, serde_json::json!(["customer:RECEIVER"]));
        let recipients: Vec<String> = sqlx::query_scalar(
            "SELECT recipient_key FROM mini_chat_push_deliveries
             WHERE event_id = 'event-1' ORDER BY recipient_key",
        )
        .fetch_all(&pool)
        .await
        .expect("reconciled deliveries");
        assert_eq!(recipients, vec!["customer:RECEIVER"]);

        sqlx::query(
            r#"INSERT INTO mini_chat_outbox_events
                   (event_id, topic, conversation_id, message_sequence,
                    recipient_keys, push_recipient_keys, payload_json)
               VALUES (
                   'event-2',
                   'chat.message.created',
                   'conversation',
                   2,
                   '["customer:RECEIVER"]'::jsonb,
                   '["customer:RECEIVER"]'::jsonb,
                   '{"message":{"sender_role":"admin","sender_ref":"SENDER"}}'::jsonb
               )"#,
        )
        .execute(&pool)
        .await
        .expect("trigger assigns cursor");
        let second_cursor: i64 = sqlx::query_scalar(
            "SELECT event_cursor FROM mini_chat_outbox_events WHERE event_id = 'event-2'",
        )
        .fetch_one(&pool)
        .await
        .expect("second cursor");
        assert!(second_cursor > 0);

        pool.close().await;
        let admin_pool = sqlx::PgPool::connect(&admin_url)
            .await
            .expect("admin db cleanup");
        sqlx::query(&format!(
            r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#
        ))
        .execute(&admin_pool)
        .await
        .expect("cleanup test db");
        admin_pool.close().await;
    }
}
