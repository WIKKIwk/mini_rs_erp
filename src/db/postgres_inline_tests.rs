#[cfg(test)]
mod tests {
    use super::*;

    const CONCURRENCY_IDEMPOTENCY_INDEXES: [(&str, &str); 5] = [
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
        assert_eq!(config.migration_database_url, config.database_url);
        assert_eq!(config.max_connections, 16);
        assert_eq!(config.min_connections, 2);
    }

    #[test]
    fn postgres_config_supports_a_separate_migration_role() {
        let config = PostgresConfig::from_env_with(|key| match key {
            "MINI_ERP_DATABASE_URL" => Some("postgres://runtime/db".to_string()),
            "MINI_ERP_MIGRATION_DATABASE_URL" => Some("postgres://owner/db".to_string()),
            _ => None,
        })
        .expect("config");

        assert_eq!(config.database_url, "postgres://runtime/db");
        assert_eq!(config.migration_database_url, "postgres://owner/db");
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
    fn postgres_migration_runner_ignores_semicolons_in_sql_comments() {
        let statements = split_sql_statements(
            "-- line comment; the rest is still a comment\nSELECT 1;\n/* block comment; with nesting /* inner; */ */ SELECT 2;",
        );

        assert_eq!(statements.len(), 2);
        assert!(statements[0].contains("SELECT 1"));
        assert!(statements[1].contains("SELECT 2"));
    }

    #[test]
    fn postgres_migrations_are_versioned_and_checksummed() {
        let versions = POSTGRES_MIGRATIONS
            .iter()
            .map(|(version, _)| *version)
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(versions.len(), POSTGRES_MIGRATIONS.len());
        assert!(versions.contains("0066_canonical_authority_remainder"));
        assert!(versions.contains("0067_canonical_apparatus_payload_invariant"));
        assert!(versions.contains("0068_canonical_apparatus_fk_indexes"));
        assert!(versions.contains("0069_canonical_apparatus_revision_authority"));
        assert!(versions.contains("0070_canonical_apparatus_clean_cutover"));
        assert!(versions.contains("0071_qolip_lock_ownership"));
        assert!(versions.contains("0072_canonical_identity_indexes"));
        assert!(versions.contains("0087_queue_event_stage_identity"));
        assert!(versions.contains("0088_order_run_session_stage_identity"));
        assert!(versions.contains("0089_progress_batch_typed_payload_mirrors"));
        assert!(versions.contains("0090_drop_progress_batch_current_apparatus_key"));
        assert!(POSTGRES_MIGRATIONS.iter().all(|(version, sql)| {
            !version.trim().is_empty() && migration_checksum(sql).len() == 64
        }));
    }

    #[test]
    fn rezka_merge_lineage_migration_is_registered_and_keeps_three_audit_layers() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0085_rezka_merge_lineage")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("Rezka merge lineage migration");

        for invariant in [
            "create table if not exists mini_order_run_input_links",
            "unique (session_id, sequence_no)",
            "create unique index if not exists idx_mini_order_run_input_links_one_in_use",
            "where status = 'in_use'",
            "create table if not exists mini_rezka_active_partial_rolls",
            "source_input_batch_ids text[] not null",
            "check (cardinality(source_input_batch_ids) > 0)",
            "create table if not exists mini_progress_batch_input_links",
            "unique (output_batch_id, sequence_no)",
            "references mini_order_run_sessions(session_id) on delete cascade",
            "references mini_progress_batches(batch_id) on delete cascade",
            "insert into mini_order_run_input_links",
            "insert into mini_progress_batch_input_links",
            "from mini_opening_wip_batches",
            "from mini_progress_batches input_progress_batch",
        ] {
            assert!(
                migration.contains(invariant),
                "missing Rezka merge lineage invariant: {invariant}"
            );
        }
        assert!(migration.matches("<>").count() >= 2);
        assert!(!migration.contains("contribution_qty"));
    }

    #[test]
    fn rezka_merge_action_migration_updates_event_constraints_without_output_batches() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0086_rezka_merge_action")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("Rezka merge action migration");

        assert!(migration.contains("mini_queue_action_events_action_allowed"));
        assert!(migration.contains("mini_order_progress_events_action_allowed"));
        assert!(migration.contains("'merge'"));
        assert!(!migration.contains("mini_progress_batches_action_allowed"));
    }

    #[test]
    fn queue_event_stage_identity_migration_cuts_over_json_authority() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0087_queue_event_stage_identity")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("queue event stage identity migration");
        let compact = migration.split_whitespace().collect::<String>();

        for invariant in [
            "addcolumnifnotexistsstage_node_idtextnotnulldefault''",
            "setstage_node_id=btrim(coalesce(payload_json->>'stage_node_id',''))",
            "setpayload_json=payload_json-'stage_node_id'",
            "mini_queue_action_events_stage_payload_forbidden",
            "check(not(payload_json?'stage_node_id'))",
            "idx_mini_queue_action_events_order_stage_created",
        ] {
            assert!(compact.contains(invariant), "missing invariant: {invariant}");
        }
    }

    #[test]
    fn order_run_session_stage_identity_migration_is_registered() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0088_order_run_session_stage_identity")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("order run session stage identity migration");
        let compact = migration.split_whitespace().collect::<String>();

        for invariant in [
            "addcolumnifnotexistsstage_node_idtextnotnulldefault''",
            "setstage_node_id=btrim(coalesce(payload_json->>'stage_node_id',''))",
            "setpayload_json=payload_json-'stage_node_id'",
            "mini_order_run_sessions_stage_node_id_trimmed",
            "check(stage_node_id=btrim(stage_node_id))",
            "mini_order_run_sessions_stage_payload_forbidden",
            "check(not(payload_json?'stage_node_id'))",
        ] {
            assert!(compact.contains(invariant), "missing invariant: {invariant}");
        }
    }

    #[test]
    fn progress_batch_typed_payload_mirrors_migration_is_registered() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0089_progress_batch_typed_payload_mirrors")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("progress batch typed payload mirrors migration");
        let compact = migration.split_whitespace().collect::<String>();

        for invariant in [
            "mini_progress_batches_wip_typed_payload_forbidden",
            "check(not(payload_json?|array[",
            "'status_detail'",
            "'wip_status'",
            "'current_apparatus'",
            "'current_apparatus_key'",
            "'current_location'",
            "'next_apparatus'",
            "'parent_batch_id'",
            "'used_by_session_id'",
            "'used_by_apparatus'",
            "'used_by_order_id'",
            "'processed_by_session_id'",
            "'processed_by_apparatus'",
            "'from_apparatus'",
        ] {
            assert!(compact.contains(invariant), "missing invariant: {invariant}");
        }
    }

    #[test]
    fn drop_progress_batch_current_apparatus_key_migration_invariants() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0090_drop_progress_batch_current_apparatus_key")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("drop progress batch current apparatus key migration");
        let compact = migration.split_whitespace().collect::<String>();

        for invariant in [
            "idx_mini_progress_batches_wip_status_apparatus_key",
            "idx_mini_progress_batches_wip_status_canonical_current_apparatus",
            "dropcolumnifexistscurrent_apparatus_key",
        ] {
            assert!(compact.contains(invariant), "missing invariant: {invariant}");
        }
    }

    #[test]
    fn apparatus_collections_migration_is_registered_and_canonical_id_scoped() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0075_apparatus_collections")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("apparatus collections migration");

        for invariant in [
            "create table mini_apparatus_collections",
            "create table mini_apparatus_collection_members",
            "references mini_canonical_apparatus_identities (apparatus_id)",
            "on update restrict on delete restrict",
            "unique (collection_id, position)",
        ] {
            assert!(
                migration.contains(invariant),
                "missing invariant: {invariant}"
            );
        }
        assert!(!migration.contains("mini_production_maps"));
        assert!(!migration.contains("mini_production_orders"));
        assert!(!migration.contains("mini_apparatus_queue"));
    }

    #[test]
    fn canonical_revision_authority_migration_is_registered_and_guarded() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0069_canonical_apparatus_revision_authority")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("canonical revision authority migration");

        for invariant in [
            "create table mini_canonical_apparatus_identities",
            "create table mini_canonical_apparatus_revisions",
            "create table mini_canonical_apparatus_heads",
            "create table mini_canonical_apparatus_change_outbox",
            "mini_reject_canonical_identity_or_revision_mutation",
            "mini_require_canonical_apparatus_writer",
            "mini_validate_canonical_apparatus_alignment",
            "drop constraint if exists mini_apparatus_name_unique",
            "drop constraint if exists mini_apparatus_canonical_payload_contract_check",
            "drop index if exists idx_mini_apparatus_lower_name",
            "drop index if exists idx_mini_apparatus_material_rules_lower_apparatus",
            "create view mini_canonical_apparatus_projection_drift",
        ] {
            assert!(
                migration.contains(invariant),
                "missing 0069 invariant: {invariant}"
            );
        }
    }

    #[test]
    fn canonical_identity_index_migration_replaces_display_keys() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0072_canonical_identity_indexes")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("canonical identity index migration");
        let compact = migration.split_whitespace().collect::<String>();

        for invariant in [
            "dropindexifexistsidx_mini_apparatus_factory_map_object_id_unique",
            "payload_json#>>'{placement,factory_map_object_id}'",
            "dropindexifexistsidx_mini_queue_action_events_pending_completion",
            "onmini_queue_action_events(canonical_apparatus_id,order_id)",
        ] {
            assert!(compact.contains(invariant), "missing 0072 invariant: {invariant}");
        }
        assert!(!compact.contains("lower(apparatus)"));
    }

    #[test]
    fn canonical_payload_invariant_migration_is_registered_with_the_runner() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0067_canonical_apparatus_payload_invariant")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("canonical payload invariant migration");

        assert!(
            migration.contains("add constraint mini_apparatus_canonical_payload_contract_check")
        );
        assert!(migration.contains("0067 canonical apparatus payload invariant preflight failed"));
        assert!(migration.contains(") is true"));
    }

    #[test]
    fn canonical_authority_remainder_migration_is_registered_and_types_warehouse_assignments() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0066_canonical_authority_remainder")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("canonical authority remainder migration");
        let compact = migration.split_whitespace().collect::<String>();

        for expected in [
            "altertablemini_warehouse_assignmentsaddcolumnifnotexistsassignment_kindtext",
            "addcolumnifnotexistswarehouse_nametext",
            "addcolumnifnotexistsapparatus_idtext",
            "altertablemini_warehouse_assignmentsaltercolumnassignment_kindsetnotnull",
            "assignment_kind='warehouse'",
            "assignment_kind='apparatus'",
            "foreignkey(warehouse_name)referencesmini_warehouses(name)",
            "foreignkey(apparatus_id)referencesmini_apparatus(id)",
            "createindexifnotexistsidx_mini_warehouse_assignments_warehouse_name",
            "createindexifnotexistsidx_mini_warehouse_assignments_apparatus_id",
            "createuniqueindexifnotexistsidx_mini_warehouse_assignments_warehouse_identity_unique",
            "createuniqueindexifnotexistsidx_mini_warehouse_assignments_apparatus_identity_unique",
            "raiseexception'0066warehouseassignmentlegacyvaluematchesbothwarehouseandapparatusidentities'",
            "raiseexception'0066warehouseassignmentlegacyvaluematchesneitherwarehousenorcanonicalapparatusidentity'",
            "raiseexception'0066virtualtraininginputnodecannotcarryproductionapparatusidentity'",
        ] {
            assert!(compact.contains(expected), "missing {expected}");
        }
        assert!(migration.contains("apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'"));
    }

    #[test]
    fn postgres_migration_registry_matches_every_file_in_contiguous_order() {
        let migration_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/migrations/postgres");
        let mut files = std::fs::read_dir(migration_dir)
            .expect("postgres migrations directory")
            .map(|entry| {
                entry
                    .expect("postgres migration directory entry")
                    .file_name()
                    .into_string()
                    .expect("postgres migration filename")
            })
            .filter(|name| name.ends_with(".sql"))
            .collect::<Vec<_>>();
        files.sort();

        let registered_files = POSTGRES_MIGRATIONS
            .iter()
            .map(|(version, _)| format!("{version}.sql"))
            .collect::<Vec<_>>();

        assert_eq!(registered_files, files);
        validate_migration_registry(&POSTGRES_MIGRATIONS)
            .expect("production migration registry must be contiguous");
    }

    #[test]
    fn postgres_migration_validation_rejects_duplicate_skip_empty_and_unknown_history() {
        let duplicate_registry = [("0001_first", "SELECT 1"), ("0001_duplicate", "SELECT 1")];
        assert!(validate_migration_registry(&duplicate_registry).is_err());

        let skipped_registry = [("0001_first", "SELECT 1"), ("0003_third", "SELECT 1")];
        assert!(validate_migration_registry(&skipped_registry).is_err());

        let empty_registry = [("0001_first", "   ")];
        assert!(validate_migration_registry(&empty_registry).is_err());

        let registry = [
            ("0001_first", "SELECT 1"),
            ("0002_second", "SELECT 1"),
            ("0003_third", "SELECT 1"),
        ];
        let unknown_history = vec!["0001_first".to_string(), "0004_future".to_string()];
        assert!(validate_applied_migration_versions(&unknown_history, &registry).is_err());

        let out_of_order_history = vec!["0001_first".to_string(), "0003_third".to_string()];
        assert!(validate_applied_migration_versions(&out_of_order_history, &registry).is_err());
    }

    #[test]
    fn concurrency_idempotency_migration_registers_exactly_five_non_destructive_invariants() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0062_concurrency_idempotency_constraints")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("concurrency idempotency migration");

        for (index_name, _) in CONCURRENCY_IDEMPOTENCY_INDEXES {
            assert!(migration.contains(index_name), "missing {index_name}");
        }
        assert_eq!(
            migration
                .matches("create unique index if not exists")
                .count(),
            5
        );
        for destructive in ["delete from", "drop table", "truncate"] {
            assert!(
                !migration.contains(destructive),
                "0062 must not contain {destructive}"
            );
        }
    }

    #[test]
    fn training_progress_batch_migration_persists_each_printable_qr() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0058_training_progress_batches")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("training progress batch migration");

        assert!(migration.contains("create table if not exists mini_training_progress_batches"));
        assert!(migration.contains("batch_id text primary key"));
        assert!(migration.contains("qr_payload text not null unique"));
        assert!(migration.contains("payload_json jsonb not null"));
    }

    #[test]
    fn training_input_batch_set_migration_is_registered_with_the_runner() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0059_training_input_batch_sets")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("training input batch set migration");

        assert!(migration.contains("drop constraint if exists mini_training_input_batches_pkey"));
        assert!(migration.contains("add primary key using index"));
        assert!(migration.contains("idx_mini_training_input_batches_order_apparatus"));
    }

    #[test]
    fn frozen_order_queue_state_migration_is_registered_with_the_runner() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0060_frozen_order_queue_state")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("frozen order queue state migration");

        assert!(migration.contains("'frozen'"));
        assert!(migration.contains("'freeze'"));
        assert!(migration.contains("idx_mini_order_run_sessions_one_open"));
    }

    #[test]
    fn roll_detached_status_migration_is_registered_with_the_runner() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0050_roll_detached_status")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("roll detached status migration");

        assert!(migration.contains("'detach_roll'"));
        assert!(migration.contains("'roll_detached'"));
        assert!(migration.contains("idx_mini_order_run_sessions_one_open"));
    }

    #[test]
    fn apparatus_capacity_identity_migration_is_registered_with_the_runner() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0055_apparatus_capacity_identity")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("apparatus capacity identity migration");

        assert!(migration.contains("mini_apparatus_id_name_unique"));
        assert!(migration.contains("foreign key (apparatus_id, apparatus)"));
        assert_eq!(migration.matches("not valid;").count(), 3);
    }

    #[test]
    fn quantity_precision_migration_enforces_one_operational_scale() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0051_quantity_precision")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("quantity precision migration");
        let compact = migration.replace(char::is_whitespace, "");

        for table in [
            "mini_gscale_receipts",
            "mini_raw_material_stock",
            "mini_finished_goods_stock",
            "mini_raw_material_events",
            "mini_orders",
            "mini_production_maps",
            "mini_order_progress_events",
            "mini_progress_batches",
            "mini_inventory_transfer_lines",
            "mini_inventory_movement_events",
            "mini_laminatsiya_astatka_reports",
            "mini_rezka_astatka_reports",
        ] {
            assert!(
                compact.contains(&format!("altertable{table}")),
                "quantity migration does not cover {table}"
            );
        }
        assert!(compact.contains("altercolumnqtytypenumeric(18,6)"));
        assert!(compact.contains("altercolumnroll_counttypeinteger"));
        assert!(compact.contains("mini_inventory_transfer_lines_dona_integer"));
        assert!(compact.contains("mini_inventory_movement_events_dona_integer"));
        assert!(!compact.contains("numeric(18,3)"));
        assert!(!compact.contains("numeric(24,9)"));
    }

    #[test]
    fn inventory_transfer_flow_keeps_quantities_as_exact_micro_units() {
        let source = [
            include_str!("postgres_inventory_movements.rs"),
            include_str!("postgres_inventory_movements_parts/part_01.rs"),
            include_str!("postgres_inventory_movements_parts/part_02.rs"),
            include_str!("postgres_inventory_movements_parts/part_03.rs"),
            include_str!("postgres_inventory_movements_parts/part_04.rs"),
        ]
        .join("\n");

        assert!(source.contains("(stock.qty * 1000000)::bigint AS qty_units"));
        assert!(source.contains("asset.qty_units != line.qty_units"));
        assert!(source.contains("($11::bigint::numeric / 1000000)::numeric(18,6)"));
        assert!(!source.contains("qty::float8 AS qty"));
        assert!(!source.contains("numeric(18,3)"));
    }

    #[test]
    fn apparatus_capacity_migration_is_registered_with_the_runner() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0034_apparatus_capacity_scheduling")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("apparatus capacity migration");

        for expected in [
            "mini_apparatus_capacity_profiles",
            "mini_apparatus_downtimes",
            "mini_apparatus_schedule_reservations",
            "status in ('planned', 'active', 'completed', 'cancelled')",
        ] {
            assert!(migration.contains(expected), "missing {expected}");
        }
    }

    #[test]
    fn apparatus_schedule_paused_status_migration_is_registered_with_the_runner() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0035_apparatus_schedule_paused_status")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("apparatus schedule paused status migration");

        assert!(migration.contains("drop constraint if exists"));
        assert!(
            migration
                .contains("status in ('planned', 'active', 'paused', 'completed', 'cancelled')")
        );
    }

    #[test]
    fn rezka_roll_fanout_migration_is_registered_with_the_runner() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0039_rezka_roll_fanout")
            .map(|(_, sql)| *sql)
            .expect("rezka roll fanout migration");
        let migration_lower = migration.to_lowercase();

        assert!(migration_lower.contains("roll_complete"));
        assert!(migration_lower.contains("mini_progress_batches_action_allowed"));

        let statements = split_sql_statements(migration);
        assert_eq!(statements.len(), 6);
    }

    #[test]
    fn rezka_astatka_migration_is_registered_with_the_runner() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0041_rezka_astatka_reports")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("rezka astatka migration");

        for expected in [
            "mini_rezka_astatka_reports",
            "rezka_bosma_waste",
            "rezka_lamination_waste",
            "rezka_edge_waste",
            "check (to_at >= from_at)",
        ] {
            assert!(migration.contains(expected), "missing {expected}");
        }
    }

    #[test]
    fn production_progress_bobina_migration_is_registered_with_the_runner() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0046_production_progress_bobina")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("production progress bobina migration");

        for expected in [
            "mini_order_progress_events",
            "mini_progress_batches",
            "mini_laminatsiya_astatka_reports",
            "mini_rezka_astatka_reports",
            "bobina_kg",
        ] {
            assert!(migration.contains(expected), "missing {expected}");
        }
    }

    #[test]
    fn progress_batch_correction_migration_is_append_only_and_revisioned() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0047_progress_batch_corrections")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("progress batch correction migration");

        for expected in [
            "add column if not exists revision bigint not null default 1",
            "create table if not exists mini_progress_batch_corrections",
            "old_values jsonb not null",
            "new_values jsonb not null",
            "unique (batch_id, new_revision)",
            "on table mini_progress_batch_corrections to mini_rs_erp",
            "on sequence mini_progress_batch_corrections_id_seq to mini_rs_erp",
        ] {
            assert!(migration.contains(expected), "missing {expected}");
        }
    }

    #[test]
    fn rps_runtime_privilege_migration_is_fail_closed() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0027_rps_runtime_privileges")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("RPS runtime privilege migration");

        for table in [
            "mini_rps_batches",
            "mini_rps_batch_history",
            "mini_rps_batch_identities",
        ] {
            assert!(migration.contains(table));
        }
        assert!(migration.contains("grant select, insert, update, delete"));
        assert!(migration.contains("raise exception"));
    }

    #[test]
    fn factory_location_migration_has_stable_apparatus_ids_and_runtime_grants() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0028_factory_locations")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("factory location migration");

        for expected in [
            "mini_factory_locations",
            "mini_factory_location_apparatus_links",
            "apparatus:default:bosma_7",
            "apparatus:default:rezka",
            "on delete cascade",
            "on delete restrict",
        ] {
            assert!(migration.contains(expected), "missing {expected}");
        }
        assert!(migration.contains("grant select, insert, update, delete"));
        assert!(migration.contains("raise exception"));

        let cutover = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0065_canonical_apparatus_cutover")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("canonical apparatus cutover migration");
        assert!(cutover.contains("'apparatus:default:rezka', 'apparatus:default:asset-010'"));
    }

    #[test]
    fn inventory_movement_migration_preserves_stock_quantity_sources() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0029_inventory_movements")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("inventory movement migration");

        for expected in [
            "mini_inventory_locations",
            "mini_inventory_placements",
            "mini_inventory_transfers",
            "mini_inventory_transfer_actions",
            "mini_inventory_transfer_lines",
            "mini_inventory_movement_events",
            "transfer_reserved",
            "in_transit",
            "append-only",
            "mini_qolip_transfer_lock_guard",
        ] {
            assert!(migration.contains(expected), "missing {expected}");
        }
        let placement_definition = migration
            .split("create table if not exists mini_inventory_placements")
            .nth(1)
            .and_then(|tail| tail.split(");").next())
            .expect("placement table definition");
        assert!(
            !placement_definition.contains("qty"),
            "physical placement must never become a quantity source"
        );
        assert!(!migration.contains("drop table"));
        assert!(!migration.contains("delete from mini_raw_material_stock"));
        assert!(!migration.contains("delete from mini_finished_goods_stock"));
        assert!(!migration.contains("delete from mini_qolip_locations"));
        assert!(split_sql_statements(&migration).len() > 20);
    }

    #[test]
    fn inventory_transfer_chat_card_migration_is_recipient_scoped_and_additive() {
        let migration = POSTGRES_MIGRATIONS
            .iter()
            .find(|(version, _)| *version == "0030_inventory_transfer_chat_cards")
            .map(|(_, sql)| sql.to_lowercase())
            .expect("inventory transfer chat card migration");

        for expected in [
            "mini_inventory_transfer_chat_outbox",
            "target_role",
            "target_ref",
            "inventory_transfer_request",
            "event_sequence bigserial primary key",
        ] {
            assert!(migration.contains(expected), "missing {expected}");
        }
        assert!(!migration.contains("drop table"));
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
        let migration_history = postgres_0062_migration_history(&pool).await;
        assert_eq!(migration_history.len(), POSTGRES_MIGRATIONS.len());
        assert_eq!(migration_history.len(), 92);
        let obsolete_material_index_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM pg_indexes
                 WHERE schemaname = 'public'
                   AND indexname = 'idx_mini_apparatus_material_rules_lower_apparatus'
             )",
        )
        .fetch_one(&pool)
        .await
        .expect("query obsolete material-rule index");
        assert!(!obsolete_material_index_exists);
        let canonical_material_identity_index: (bool, bool, bool) = sqlx::query_as(
            "SELECT index_meta.indisunique, index_meta.indisvalid, index_meta.indisready
             FROM pg_index index_meta
             JOIN pg_class index_class ON index_class.oid = index_meta.indexrelid
             WHERE index_class.relname = 'mini_apparatus_material_rules_pkey'",
        )
        .fetch_one(&pool)
        .await
        .expect("canonical material-rule identity index");
        assert_eq!(canonical_material_identity_index, (true, true, true));

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
        assert_eq!(table_count, 22);

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

        apply_postgres_migrations_through(&pool, 17)
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

    #[derive(Clone, Copy)]
    struct Postgres0062NegativeCase {
        suffix: &'static str,
        expected_index: &'static str,
        seed_sql: &'static str,
    }

    #[tokio::test]
    async fn postgres_live_0062_concurrency_idempotency_constraints() {
        let admin_url = std::env::var("MINI_ERP_TEST_ADMIN_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://wikki@127.0.0.1:5432/postgres".to_string());
        let runtime_role_created = ensure_postgres_0062_runtime_role(&admin_url).await;
        let database_prefix = format!("mini_rs_erp_test_0062_{}", std::process::id());
        let positive_database = format!("{database_prefix}_positive");
        let positive_url = recreate_postgres_0062_database(&admin_url, &positive_database).await;
        let pool = connect_postgres_0062_database(&positive_url).await;

        apply_postgres_migrations_through(&pool, 61)
            .await
            .expect("apply production migrations through 0061");
        let history_through_0061 = postgres_0062_migration_history(&pool).await;
        assert_eq!(history_through_0061.len(), 61);

        seed_postgres_0062_valid_rows(&pool).await;
        let rows_before = postgres_0062_target_rows_snapshot(&pool).await;
        apply_postgres_migrations_through(&pool, 62)
            .await
            .expect("apply 0062 on valid production rows");
        let rows_after = postgres_0062_target_rows_snapshot(&pool).await;
        assert_eq!(rows_after, rows_before, "0062 changed existing valid rows");

        assert_postgres_0062_indexes(&pool).await;
        let history_after_0062 = postgres_0062_migration_history(&pool).await;
        assert_eq!(history_after_0062.len(), 62);
        assert_eq!(&history_after_0062[..61], history_through_0061.as_slice());
        for ((version, checksum, _), (expected_version, sql)) in
            history_after_0062.iter().zip(POSTGRES_MIGRATIONS.iter())
        {
            assert_eq!(version, expected_version);
            assert_eq!(checksum, &migration_checksum(sql));
        }

        pool.close().await;
        let restarted_pool = connect_postgres_0062_database(&positive_url).await;
        apply_postgres_migrations_through(&restarted_pool, 62)
            .await
            .expect("0062 restart must keep migration history stable");
        let history_after_restart = postgres_0062_migration_history(&restarted_pool).await;
        assert_eq!(history_after_restart, history_after_0062);
        assert_eq!(
            postgres_0062_target_rows_snapshot(&restarted_pool).await,
            rows_after,
            "restart changed rows protected by 0062"
        );
        restarted_pool.close().await;
        drop_postgres_0062_database(&admin_url, &positive_database).await;
        eprintln!("0062 positive fixture: PASS");

        let source_database = format!("{database_prefix}_source");
        let source_url = recreate_postgres_0062_database(&admin_url, &source_database).await;
        let source_pool = connect_postgres_0062_database(&source_url).await;
        apply_postgres_migrations_through(&source_pool, 62)
            .await
            .expect("apply through 0062 for source behavior fixture");
        assert_postgres_0062_source_behaviors(&source_url, &source_pool).await;
        source_pool.close().await;
        drop_postgres_0062_database(&admin_url, &source_database).await;
        eprintln!("0062 source behavior fixture: PASS");

        let negative_cases = [
            Postgres0062NegativeCase {
                suffix: "apparatus",
                expected_index: "idx_mini_apparatus_factory_map_object_id_unique",
                seed_sql: r#"
                    INSERT INTO mini_apparatus
                        (id, name, base_name, kind, payload_json)
                    VALUES
                        ('fixture:0062:negative:apparatus:1', '0062 Negative Apparatus 1', '', 'fixture',
                         '{"fixture":"0062-negative","factory_map_object_id":"duplicate-map-object"}'::jsonb),
                        ('fixture:0062:negative:apparatus:2', '0062 Negative Apparatus 2', '', 'fixture',
                         '{"fixture":"0062-negative","factory_map_object_id":" duplicate-map-object "}'::jsonb)
                "#,
            },
            Postgres0062NegativeCase {
                suffix: "material_rule",
                expected_index: "idx_mini_apparatus_material_rules_lower_apparatus",
                seed_sql: r#"
                    INSERT INTO mini_apparatus_material_rules
                        (apparatus, payload_json)
                    VALUES
                        ('0062 Negative Material Rule', '{"fixture":"0062-negative"}'::jsonb),
                        ('0062 negative material rule', '{"fixture":"0062-negative"}'::jsonb)
                "#,
            },
            Postgres0062NegativeCase {
                suffix: "stock",
                expected_index: "idx_mini_raw_material_stock_lower_barcode",
                seed_sql: r#"
                    INSERT INTO mini_raw_material_stock
                        (id, warehouse, item_code, item_name, barcode, qty, payload_json)
                    VALUES
                        ('fixture:0062:negative:stock:1', 'Fixture Warehouse', 'fixture-item',
                         'Fixture Item', '0062-NEGATIVE-STOCK', 1,
                         '{"fixture":"0062-negative"}'::jsonb),
                        ('fixture:0062:negative:stock:2', 'Fixture Warehouse', 'fixture-item',
                         'Fixture Item', '0062-negative-stock', 1,
                         '{"fixture":"0062-negative"}'::jsonb)
                "#,
            },
            Postgres0062NegativeCase {
                suffix: "assignment",
                expected_index: "idx_mini_raw_material_assignments_lower_barcode",
                seed_sql: r#"
                    INSERT INTO mini_items
                        (code, name, item_group, payload_json)
                    VALUES
                        ('fixture:0062:negative:item', '0062 Negative Item', 'All Item Groups',
                         '{"fixture":"0062-negative"}'::jsonb);
                    INSERT INTO mini_production_maps
                        (id, product_code, title, map_json)
                    VALUES
                        ('fixture:0062:negative:order', 'fixture-product', '0062 Negative Order',
                         '{"fixture":"0062-negative"}'::jsonb);
                    ALTER TABLE mini_raw_material_assignments
                        DROP CONSTRAINT mini_raw_material_assignments_stock_fkey;
                    INSERT INTO mini_raw_material_assignments
                        (barcode, order_id, apparatus, item_code, item_group, payload_json)
                    VALUES
                        ('0062-NEGATIVE-ASSIGNMENT', 'fixture:0062:negative:order',
                         '0062 Negative Apparatus', 'fixture:0062:negative:item', 'All Item Groups',
                         '{"fixture":"0062-negative"}'::jsonb),
                        ('0062-negative-assignment', 'fixture:0062:negative:order',
                         '0062 Negative Apparatus', 'fixture:0062:negative:item', 'All Item Groups',
                         '{"fixture":"0062-negative"}'::jsonb)
                "#,
            },
            Postgres0062NegativeCase {
                suffix: "pending_completion",
                expected_index: "idx_mini_queue_action_events_pending_completion",
                seed_sql: r#"
                    INSERT INTO mini_queue_action_events
                        (event_id, apparatus, order_id, action, from_state, to_state, policy,
                         assigned_apparatus, payload_json)
                    VALUES
                        ('fixture:0062:negative:completion:1', '0062 Negative Queue',
                         'fixture:0062:negative:queue-order', 'complete', 'in_progress', 'completed',
                         'free_pick', '[]'::jsonb,
                         '{"fixture":"0062-negative","completion_request":true}'::jsonb),
                        ('fixture:0062:negative:completion:2', '0062 negative queue',
                         'fixture:0062:negative:queue-order', 'complete', 'in_progress', 'completed',
                         'free_pick', '[]'::jsonb,
                         '{"fixture":"0062-negative","completion_request":true,"completion_request_status":"pending"}'::jsonb)
                "#,
            },
        ];

        for case in negative_cases {
            let database_name = format!("{database_prefix}_{}", case.suffix);
            let database_url = recreate_postgres_0062_database(&admin_url, &database_name).await;
            let pool = connect_postgres_0062_database(&database_url).await;
            apply_postgres_migrations_through(&pool, 61)
                .await
                .unwrap_or_else(|error| panic!("{}: apply through 0061: {error}", case.suffix));
            sqlx::raw_sql(case.seed_sql)
                .execute(&pool)
                .await
                .unwrap_or_else(|error| panic!("{}: seed duplicates: {error}", case.suffix));
            let rows_before = postgres_0062_target_rows_snapshot(&pool).await;
            let history_before = postgres_0062_migration_history(&pool).await;

            let error = apply_postgres_migrations_through(&pool, 62)
                .await
                .expect_err("0062 must reject duplicate data");
            match &error {
                sqlx::Error::Database(database) => {
                    assert_eq!(
                        database.code().as_deref(),
                        Some("23505"),
                        "{}: unexpected SQLSTATE for {error}",
                        case.suffix
                    );
                    assert!(
                        database.constraint() == Some(case.expected_index)
                            || database.message().contains(case.expected_index),
                        "{}: failure did not identify {}: {error}",
                        case.suffix,
                        case.expected_index
                    );
                }
                _ => panic!(
                    "{}: expected PostgreSQL duplicate error: {error}",
                    case.suffix
                ),
            }

            assert_eq!(
                postgres_0062_target_rows_snapshot(&pool).await,
                rows_before,
                "{}: failed migration changed existing rows",
                case.suffix
            );
            assert_eq!(
                postgres_0062_migration_history(&pool).await,
                history_before,
                "{}: failed migration changed history",
                case.suffix
            );
            assert_eq!(
                postgres_0062_index_count(&pool).await,
                0,
                "{}: failed migration left partial indexes",
                case.suffix
            );
            pool.close().await;
            drop_postgres_0062_database(&admin_url, &database_name).await;
            eprintln!("0062 negative fixture {}: PASS", case.suffix);
        }
        if runtime_role_created {
            drop_postgres_0062_runtime_role(&admin_url).await;
        }
    }

    async fn ensure_postgres_0062_runtime_role(admin_url: &str) -> bool {
        let admin_pool = sqlx::PgPool::connect(admin_url)
            .await
            .expect("connect to PostgreSQL admin database for runtime role");
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp')",
        )
        .fetch_one(&admin_pool)
        .await
        .expect("check runtime role");
        if !exists {
            sqlx::query("CREATE ROLE mini_rs_erp NOLOGIN")
                .execute(&admin_pool)
                .await
                .expect("create fixture runtime role");
        }
        admin_pool.close().await;
        !exists
    }

    async fn drop_postgres_0062_runtime_role(admin_url: &str) {
        let admin_pool = sqlx::PgPool::connect(admin_url)
            .await
            .expect("connect to PostgreSQL admin database for runtime role cleanup");
        sqlx::query("DROP ROLE mini_rs_erp")
            .execute(&admin_pool)
            .await
            .expect("drop fixture runtime role");
        admin_pool.close().await;
    }

    async fn recreate_postgres_0062_database(admin_url: &str, database_name: &str) -> String {
        assert!(database_name.starts_with("mini_rs_erp_test_0062_"));
        assert!(
            database_name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        );
        let admin_pool = sqlx::PgPool::connect(admin_url)
            .await
            .expect("connect to PostgreSQL admin database");
        sqlx::query(&format!(
            r#"DROP DATABASE IF EXISTS "{database_name}" WITH (FORCE)"#
        ))
        .execute(&admin_pool)
        .await
        .expect("drop stale 0062 test database");
        sqlx::query(&format!(r#"CREATE DATABASE "{database_name}""#))
            .execute(&admin_pool)
            .await
            .expect("create 0062 test database");
        admin_pool.close().await;

        let (admin_base_url, _) = admin_url
            .rsplit_once('/')
            .expect("admin database URL must include a database name");
        format!("{admin_base_url}/{database_name}")
    }

    async fn drop_postgres_0062_database(admin_url: &str, database_name: &str) {
        let admin_pool = sqlx::PgPool::connect(admin_url)
            .await
            .expect("connect to PostgreSQL admin database for cleanup");
        sqlx::query(&format!(
            r#"DROP DATABASE IF EXISTS "{database_name}" WITH (FORCE)"#
        ))
        .execute(&admin_pool)
        .await
        .expect("drop 0062 test database");
        admin_pool.close().await;
    }

    async fn connect_postgres_0062_database(database_url: &str) -> PgPool {
        PgPoolOptions::new()
            .max_connections(2)
            .connect(database_url)
            .await
            .expect("connect to 0062 test database")
    }

    async fn postgres_0062_migration_history(pool: &PgPool) -> Vec<(String, String, String)> {
        sqlx::query_as(
            "SELECT version, checksum, applied_at::text
             FROM mini_schema_migrations
             ORDER BY version",
        )
        .fetch_all(pool)
        .await
        .expect("load migration history")
    }

    async fn seed_postgres_0062_valid_rows(pool: &PgPool) {
        sqlx::raw_sql(
            r#"
                INSERT INTO mini_items
                    (code, name, item_group, payload_json)
                VALUES
                    ('fixture:0062:item:1', '0062 Fixture Item', 'All Item Groups',
                     '{"fixture":"0062-positive"}'::jsonb);
                INSERT INTO mini_production_maps
                    (id, product_code, title, map_json)
                VALUES
                    ('fixture:0062:order:1', 'fixture-product', '0062 Fixture Order',
                     '{"fixture":"0062-positive"}'::jsonb);
                INSERT INTO mini_apparatus
                    (id, name, base_name, kind, payload_json)
                VALUES
                    ('fixture:0062:apparatus:1', '0062 Fixture Apparatus 1', '', 'fixture',
                     '{"fixture":"0062-positive","factory_map_object_id":"fixture-map-object-1"}'::jsonb),
                    ('fixture:0062:apparatus:2', '0062 Fixture Apparatus 2', '', 'fixture',
                     '{"fixture":"0062-positive","factory_map_object_id":"fixture-map-object-2"}'::jsonb);
                INSERT INTO mini_apparatus_material_rules
                    (apparatus, payload_json)
                VALUES
                    ('0062 Fixture Material Rule 1', '{"fixture":"0062-positive"}'::jsonb),
                    ('0062 Fixture Material Rule 2', '{"fixture":"0062-positive"}'::jsonb);
                INSERT INTO mini_raw_material_stock
                    (id, warehouse, item_code, item_name, barcode, qty, payload_json)
                VALUES
                    ('fixture:0062:stock:1', 'Fixture Warehouse', 'fixture:0062:item:1',
                     '0062 Fixture Item', '0062-FIXTURE-STOCK-1', 2,
                     '{"fixture":"0062-positive"}'::jsonb),
                    ('fixture:0062:stock:2', 'Fixture Warehouse', 'fixture:0062:item:1',
                     '0062 Fixture Item', '0062-FIXTURE-STOCK-2', 3,
                     '{"fixture":"0062-positive"}'::jsonb);
                INSERT INTO mini_raw_material_assignments
                    (barcode, order_id, apparatus, item_code, item_group, payload_json)
                VALUES
                    ('0062-FIXTURE-STOCK-1', 'fixture:0062:order:1', '0062 Fixture Apparatus 1',
                     'fixture:0062:item:1', 'All Item Groups',
                     '{"fixture":"0062-positive"}'::jsonb),
                    ('0062-FIXTURE-STOCK-2', 'fixture:0062:order:1', '0062 Fixture Apparatus 2',
                     'fixture:0062:item:1', 'All Item Groups',
                     '{"fixture":"0062-positive"}'::jsonb);
                INSERT INTO mini_queue_action_events
                    (event_id, apparatus, order_id, action, from_state, to_state, policy,
                     assigned_apparatus, payload_json)
                VALUES
                    ('fixture:0062:completion:pending', '0062 Fixture Queue',
                     'fixture:0062:queue-order', 'complete', 'in_progress', 'completed',
                     'free_pick', '[]'::jsonb,
                     '{"fixture":"0062-positive","completion_request":true}'::jsonb),
                    ('fixture:0062:completion:approved', '0062 fixture queue',
                     'fixture:0062:queue-order', 'complete', 'in_progress', 'completed',
                     'free_pick', '[]'::jsonb,
                     '{"fixture":"0062-positive","completion_request":true,"completion_request_status":"approved"}'::jsonb)
            "#,
        )
        .execute(pool)
        .await
        .expect("seed valid 0062 rows");
    }

    async fn postgres_0062_target_rows_snapshot(pool: &PgPool) -> serde_json::Value {
        sqlx::query_scalar(
            r#"
                SELECT jsonb_build_object(
                    'apparatus', COALESCE((
                        SELECT jsonb_agg(to_jsonb(rows) ORDER BY rows.id)
                        FROM (SELECT * FROM mini_apparatus) rows
                    ), '[]'::jsonb),
                    'material_rules', COALESCE((
                        SELECT jsonb_agg(to_jsonb(rows) ORDER BY rows.apparatus)
                        FROM (SELECT * FROM mini_apparatus_material_rules) rows
                    ), '[]'::jsonb),
                    'raw_material_stock', COALESCE((
                        SELECT jsonb_agg(to_jsonb(rows) ORDER BY rows.id)
                        FROM (SELECT * FROM mini_raw_material_stock) rows
                    ), '[]'::jsonb),
                    'raw_material_assignments', COALESCE((
                        SELECT jsonb_agg(to_jsonb(rows) ORDER BY rows.barcode)
                        FROM (SELECT * FROM mini_raw_material_assignments) rows
                    ), '[]'::jsonb),
                    'queue_action_events', COALESCE((
                        SELECT jsonb_agg(to_jsonb(rows) ORDER BY rows.id)
                        FROM (SELECT * FROM mini_queue_action_events) rows
                    ), '[]'::jsonb)
                )
            "#,
        )
        .fetch_one(pool)
        .await
        .expect("snapshot rows protected by 0062")
    }

    async fn assert_postgres_0062_indexes(pool: &PgPool) {
        for (index_name, expected_table) in CONCURRENCY_IDEMPOTENCY_INDEXES {
            let (table_name, is_unique, is_valid, is_ready, definition): (
                String,
                bool,
                bool,
                bool,
                String,
            ) = sqlx::query_as(
                "SELECT table_class.relname,
                        index_meta.indisunique,
                        index_meta.indisvalid,
                        index_meta.indisready,
                        pg_get_indexdef(index_meta.indexrelid)
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
            assert!(definition.contains("CREATE UNIQUE INDEX"));
            assert!(definition.contains(index_name));
        }
        assert_eq!(postgres_0062_index_count(pool).await, 5);
    }

    async fn postgres_0062_index_count(pool: &PgPool) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM pg_class index_class
             JOIN pg_namespace namespace ON namespace.oid = index_class.relnamespace
             WHERE namespace.nspname = 'public'
               AND index_class.relname IN (
                   'idx_mini_apparatus_factory_map_object_id_unique',
                   'idx_mini_apparatus_material_rules_lower_apparatus',
                   'idx_mini_raw_material_stock_lower_barcode',
                   'idx_mini_raw_material_assignments_lower_barcode',
                   'idx_mini_queue_action_events_pending_completion'
               )",
        )
        .fetch_one(pool)
        .await
        .expect("count 0062 indexes")
    }

    async fn assert_postgres_0062_source_behaviors(database_url: &str, pool: &PgPool) {
        use crate::core::apparatus_standard::ApparatusId;
        use crate::core::production_map::{
            ApparatusQueueActionEvent, ApparatusQueuePolicy, ProductionMapError,
            ProductionMapStorePort, QueueActionActor, RawMaterialAssignment,
            queue_state::{ApparatusQueueAction, ApparatusQueueOrderState},
        };
        use crate::db::postgres_production_map::PostgresProductionMapStore;

        sqlx::query(
            "INSERT INTO mini_apparatus (id, name, base_name, kind, payload_json)
             VALUES ($1, $2, '', 'fixture', $3)",
        )
        .bind("apparatus:fixture:source-apparatus-1")
        .bind("0062 Source Apparatus 1")
        .bind(serde_json::json!({"factory_map_object_id":"fixture-source-map-object"}))
        .execute(pool)
        .await
        .expect("insert apparatus with unique factory map object");
        let duplicate_factory_object = sqlx::query(
            "INSERT INTO mini_apparatus (id, name, base_name, kind, payload_json)
             VALUES ($1, $2, '', 'fixture', $3)",
        )
        .bind("apparatus:fixture:source-apparatus-2")
        .bind("0062 Source Apparatus 2")
        .bind(serde_json::json!({"factory_map_object_id":"fixture-source-map-object"}))
        .execute(pool)
        .await
        .expect_err("duplicate factory map object must fail");
        assert_eq!(
            duplicate_factory_object
                .as_database_error()
                .and_then(|error| error.code().map(|code| code.into_owned())),
            Some("23505".to_string())
        );

        sqlx::query(
            "DELETE FROM mini_apparatus WHERE id LIKE 'apparatus:fixture:source-apparatus-%'",
        )
        .execute(pool)
        .await
        .expect("remove pre-canonical source apparatus fixture");
        apply_postgres_migrations_through(pool, 68)
            .await
            .expect("upgrade source behavior fixture through canonical 0068");
        assert_postgres_0062_indexes(pool).await;

        let apparatus_id = ApparatusId::new("apparatus:default:asset-007".to_string())
            .expect("canonical source apparatus id");
        let apparatus_display: String =
            sqlx::query_scalar("SELECT name FROM mini_apparatus WHERE id = $1")
                .bind(apparatus_id.as_str())
                .fetch_one(pool)
                .await
                .expect("canonical source apparatus display");

        let production_store = PostgresProductionMapStore::new(pool.clone());
        sqlx::raw_sql(
            "INSERT INTO mini_items (code, name, item_group, payload_json)
             VALUES ('fixture:0062:item:1', '0062 Fixture Item', 'All Item Groups',
                     '{\"fixture\":\"0062-source\"}'::jsonb);
             INSERT INTO mini_production_maps (id, product_code, title, map_json)
             VALUES ('fixture:0062:order:1', 'fixture-product', '0062 Source Order',
                     '{\"fixture\":\"0062-source\"}'::jsonb);
             INSERT INTO mini_raw_material_stock
                (id, warehouse, item_code, item_name, barcode, qty, payload_json)
             VALUES
                ('fixture:0062:source-stock', 'Fixture Warehouse', 'fixture:0062:item:1',
                 '0062 Fixture Item', '0062-SOURCE-BARCODE', 1,
                 '{\"fixture\":\"0062-source\"}'::jsonb)",
        )
        .execute(pool)
        .await
        .expect("insert source assignment stock");
        let assignment = RawMaterialAssignment {
            order_id: "fixture:0062:order:1".to_string(),
            apparatus_id: apparatus_id.clone(),
            apparatus: apparatus_display.clone(),
            barcode: "0062-SOURCE-BARCODE".to_string(),
            item_code: "fixture:0062:item:1".to_string(),
            item_name: "0062 Fixture Item".to_string(),
            item_group: "All Item Groups".to_string(),
            assigned_by_role: "fixture".to_string(),
            assigned_by_ref: "fixture".to_string(),
            assigned_by_display_name: "0062 Fixture".to_string(),
            assigned_at: "2026-01-01T00:00:00Z".to_string(),
        };
        production_store
            .put_raw_material_assignment(assignment.clone())
            .await
            .expect("insert source raw material assignment");
        let mut duplicate_assignment = assignment;
        duplicate_assignment.barcode = "0062-source-barcode".to_string();
        assert_eq!(
            production_store
                .put_raw_material_assignment(duplicate_assignment)
                .await,
            Err(ProductionMapError::RawMaterialAlreadyAssigned)
        );

        let blocker_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(database_url)
            .await
            .expect("connect queue blocker pool");
        let mut blocker = blocker_pool.begin().await.expect("begin queue blocker");
        sqlx::query(
            "INSERT INTO mini_queue_action_events
                (event_id, apparatus, canonical_apparatus_id, order_id, action, from_state, to_state, policy,
                 assigned_apparatus, payload_json)
             VALUES
                ('fixture:0062:source-race:blocker', $1, $2,
                 'fixture:0062:source-race:order', 'complete', 'in_progress', 'completed',
                 'free_pick', '[]'::jsonb,
                 '{\"completion_request\":true}'::jsonb)",
        )
        .bind(&apparatus_display)
        .bind(apparatus_id.as_str())
        .execute(&mut *blocker)
        .await
        .expect("insert uncommitted pending completion");

        let race_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(database_url)
            .await
            .expect("connect queue race pool");
        let race_store = PostgresProductionMapStore::new(race_pool.clone());
        let race_event = ApparatusQueueActionEvent {
            event_id: "fixture:0062:source-race:contender".to_string(),
            apparatus: apparatus_id.to_string(),
            order_id: "fixture:0062:source-race:order".to_string(),
            stage_node_id: String::new(),
            action: ApparatusQueueAction::Complete,
            from_state: ApparatusQueueOrderState::InProgress,
            to_state: ApparatusQueueOrderState::Completed,
            policy: ApparatusQueuePolicy::FreePick,
            actor: QueueActionActor::default(),
            assigned_apparatus: Vec::new(),
            payload_json: serde_json::json!({"completion_request": true}),
        };
        let contender = tokio::spawn(async move {
            race_store
                .append_apparatus_queue_action_event(race_event)
                .await
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(
            !contender.is_finished(),
            "queue contender must wait on the uncommitted unique key"
        );
        blocker.commit().await.expect("commit queue blocker");
        let contender_result = tokio::time::timeout(std::time::Duration::from_secs(5), contender)
            .await
            .expect("queue contender did not unblock")
            .expect("queue contender task failed");
        assert_eq!(
            contender_result,
            Err(ProductionMapError::QueueActionNotAllowed),
            "queue 23505 must map to a domain error"
        );
        race_pool.close().await;
        blocker_pool.close().await;
    }
}
