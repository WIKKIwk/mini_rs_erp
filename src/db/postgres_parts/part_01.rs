const DEFAULT_MIN_CONNECTIONS: u32 = 2;
const DEFAULT_MAX_CONNECTIONS: u32 = 16;
const DEFAULT_ACQUIRE_TIMEOUT_MS: u64 = 500;
const MIGRATION_LOCK_KEY: i64 = 6_514_811_918_052_026_001;

macro_rules! migration {
    ($name:literal) => {
        ($name, include_str!(concat!("../../../migrations/postgres/", $name, ".sql")))
    };
}

const POSTGRES_MIGRATIONS: [(&str, &str); 214] = [
    migration!("0001_mini_erp_foundation"),
    migration!("0002_order_integrity"),
    migration!("0003_erp_data_integrity"),
    migration!("0004_system_users"),
    migration!("0005_chat"),
    migration!("0006_boyoqchi_returned_paint"),
    migration!("0007_runtime_table_ownership"),
    migration!("0008_returned_paint_calculations"),
    migration!("0009_returned_paint_solvent_calculations"),
    migration!("0010_returned_paint_image_workflow"),
    migration!("0011_chat_media_foundation"),
    migration!("0012_chat_media_v1"),
    migration!("0013_chat_media_incident_video"),
    migration!("0014_raw_material_stock_corrections"),
    migration!("0015_item_identity_updates"),
    migration!("0016_chat_delivery_reliability"),
    migration!("0017_chat_delivery_reliability_followup"),
    migration!("0018_item_master_without_warehouse"),
    migration!("0019_chat_voice_messages"),
    migration!("0020_worker_identity_lifecycle"),
    migration!("0021_rps_batch_history"),
    migration!("0022_rps_batch_codes"),
    migration!("0023_qolip_13_rows"),
    migration!("0024_qolip_legacy_lookup_index"),
    migration!("0025_order_control_state"),
    migration!("0026_order_freeze_request_chat_cards"),
    migration!("0027_rps_runtime_privileges"),
    migration!("0028_factory_locations"),
    migration!("0029_inventory_movements"),
    migration!("0030_inventory_transfer_chat_cards"),
    migration!("0031_dynamic_order_layers"),
    migration!("0032_apparatus_order_transfers"),
    migration!("0033_apparatus_master_metadata"),
    migration!("0034_apparatus_capacity_scheduling"),
    migration!("0035_apparatus_schedule_paused_status"),
    migration!("0036_inventory_return_events"),
    migration!("0037_qolip_order_notes"),
    migration!("0038_calculate_material_catalog"),
    migration!("0039_rezka_roll_fanout"),
    migration!("0040_laminatsiya_astatka_reports"),
    migration!("0041_rezka_astatka_reports"),
    migration!("0042_production_order_number_sequence"),
    migration!("0043_rezka_progress_diameter"),
    migration!("0044_paddons"),
    migration!("0045_paddon_sequence_and_package_shape"),
    migration!("0046_production_progress_bobina"),
    migration!("0047_progress_batch_corrections"),
    migration!("0048_calculate_material_catalog_seed"),
    migration!("0049_correct_calculate_material_defaults"),
    migration!("0050_roll_detached_status"),
    migration!("0051_quantity_precision"),
    migration!("0052_training_workspace"),
    migration!("0053_training_queue_states"),
    migration!("0054_training_returned_paint"),
    migration!("0055_apparatus_capacity_identity"),
    migration!("0056_training_queue_events"),
    migration!("0057_training_input_batches"),
    migration!("0058_training_progress_batches"),
    migration!("0059_training_input_batch_sets"),
    migration!("0060_frozen_order_queue_state"),
    migration!("0061_order_reset_append_only_override"),
    migration!("0062_concurrency_idempotency_constraints"),
    migration!("0063_canonical_apparatus_reference_ids"),
    migration!("0064_canonical_material_rule_apparatus_id"),
    migration!("0065_canonical_apparatus_cutover"),
    migration!("0066_canonical_authority_remainder"),
    migration!("0067_canonical_apparatus_payload_invariant"),
    migration!("0068_canonical_apparatus_fk_indexes"),
    migration!("0069_canonical_apparatus_revision_authority"),
    migration!("0070_canonical_apparatus_clean_cutover"),
    migration!("0071_qolip_lock_ownership"),
    migration!("0072_canonical_identity_indexes"),
    migration!("0073_material_receipt_dimensions"),
    migration!("0074_calculate_material_roll_catalog"),
    migration!("0075_apparatus_collections"),
    migration!("0076_raw_material_soft_delete"),
    migration!("0077_production_order_lifecycle"),
    migration!("0078_production_order_operational_status"),
    migration!("0079_opening_wip"),
    migration!("0080_opening_wip_passport_metrics"),
    migration!("0081_opening_wip_resume_stage"),
    migration!("0082_qolip_item_code_doctor"),
    migration!("0083_opening_wip_source_contract"),
    migration!("0084_opening_wip_soft_delete"),
    migration!("0085_rezka_merge_lineage"),
    migration!("0086_rezka_merge_action"),
    migration!("0087_queue_event_stage_identity"),
    migration!("0088_order_run_session_stage_identity"),
    migration!("0089_progress_batch_typed_payload_mirrors"),
    migration!("0090_drop_progress_batch_current_apparatus_key"),
    migration!("0091_production_order_canonical_flow_stock_status"),
    migration!("0092_rezka_lineage_payload_cutover"),
    migration!("0093_preparation_master"),
    migration!("0094_qolip_warehouse_ownership"),
    migration!("0095_bosma_astatka_reports"),
    migration!("0096_paddon_receipts"),
    migration!("0097_raw_material_split"),
    migration!("0098_raw_material_split_weights"),
    migration!("0099_raw_material_split_issues"),
    migration!("0100_raw_material_split_issue_completion"),
    migration!("0101_raw_material_split_output_length"),
    migration!("0102_runtime_privileges"),
    migration!("0103_runtime_foreign_key_locks"),
    migration!("0104_runtime_security_boundaries"),
    migration!("0105_chat_delivery_recovery"),
    migration!("0106_preparation_formulas"),
    migration!("0107_preparation_formula_names"),
    migration!("0108_preparation_material_responsibilities"),
    migration!("0109_preparation_formula_material_scope"),
    migration!("0110_material_receipt_length"),
    migration!("0111_telegram_pending_orders"),
    migration!("0112_opened_order_edit"),
    migration!("0113_preparation_receipts_catalog_items"),
    migration!("0114_material_link_requests"),
    migration!("0115_preparation_material_warehouse_scope"),
    migration!("0116_paddon_inventory_receipt_events"),
    migration!("0117_preparation_material_warehouse_scopes"),
    migration!("0118_paddon_receipt_line_ledger"),
    migration!("0119_preparation_receipt_reversals"),
    migration!("0120_print_preflight_holds"),
    migration!("0121_print_preflight_order_status"),
    migration!("0122_production_map_alternative_backfill"),
    migration!("0123_repair_duplicate_laminate_alternatives"),
    migration!("0124_allow_sessionless_preflight_freeze_targets"),
    migration!("0125_active_rezka_paddon"),
    migration!("0126_auth_credentials"),
    migration!("0127_preparation_warehouse_management"),
    migration!("0128_preparation_material_management"),
    migration!("0129_admin_access_code_visibility"),
    migration!("0130_order_early_close"),
    migration!("0131_order_edit_history_locks"),
    migration!("0132_queue_reorder_commands"),
    migration!("0133_queue_events_outbox"),
    migration!("0134_preserve_template_maps_on_order_reset"),
    migration!("0135_material_delivery_receipts"),
    migration!("0136_qolip_alternative_sets"),
    migration!("0137_qolip_set_integrity"),
    migration!("0138_incremental_wip_lifecycle"),
    migration!("0139_paddon_print_lock"),
    migration!("0140_paddon_management_settings"),
    migration!("0141_paddon_worker_visibility"),
    migration!("0142_lamination_layer_remainder"),
    migration!("0143_lamination_astatka_transaction"),
    migration!("0144_raw_material_consumption_reversal"),
    migration!("0145_order_creation_receipts"),
    migration!("0146_resume_pause_checkpoint"),
    migration!("0147_qolip_shared_sets"),
    migration!("0148_qolip_product_overrides"),
    migration!("0149_rezka_astatka_idempotency"),
    migration!("0150_qolip_exact_product_lookup"),
    migration!("0151_immutable_wip_production_times"),
    migration!("0152_qolip_set_management"),
    migration!("0153_qolip_set_codes"),
    migration!("0154_qolip_numeric_set_ids"),
    migration!("0155_qolip_standalone_sets"),
    migration!("0156_production_live_invalidation"),
    migration!("0157_print_preflight_revision"),
    migration!("0158_preparation_formula_sets"),
    migration!("0159_preparation_screw_shares"),
    migration!("0160_preparation_formula_variants"),
    migration!("0161_paddon_apparatus_scope"),
    migration!("0162_stage_order_controls"),
    migration!("0163_lamination_settings"),
    migration!("0164_production_output_stats_indexes"),
    migration!("0165_qolip_reworks"),
    migration!("0166_qolip_mileage"),
    migration!("0167_qolip_set_names"),
    migration!("0168_qolip_start_settings"),
    migration!("0169_combo_catalog"),
    migration!("0170_inventory_live"),
    migration!("0171_requeued_apparatus_transfers"),
    migration!("0172_sales_manager"),
    migration!("0173_qolip_paddons"),
    migration!("0174_qolip_storage_guards"),
    migration!("0175_preparation_formula_codes"),
    migration!("0176_preparation_recipe_codes"),
    migration!("0177_preparation_production_journal_index"),
    migration!("0178_brigader"),
    migration!("0179_cancel_start_material_usage"),
    migration!("0180_qolip_paddon_delete"),
    migration!("0181_preparation_order_formula_exclusions"),
    migration!("0182_production_order_creators"),
    migration!("0183_legacy_order_creator_lookup"),
    migration!("0184_scanned_wip_inputs"),
    migration!("0185_lamination_remainder_label_settings"),
    migration!("0186_opening_wip_layer_remainders"),
    migration!("0187_lamination_input_roll_binding"),
    migration!("0188_preparation_formula_revisions"),
    migration!("0189_preparation_empty_recipe_drafts"),
    migration!("0190_preparation_shared_order_recipes"),
    migration!("0191_preparation_reuse_copy_chains"),
    migration!("0192_rezka_master"),
    migration!("0193_rezka_peremotka"),
    migration!("0194_order_print_side_preferences"),
    migration!("0195_backfill_order_print_side"),
    migration!("0196_metalayzer_role"),
    migration!("0197_raw_material_metalayzer"),
    migration!("0198_preparation_production_order_notes"),
    migration!("0199_preparation_output_notes"),
    migration!("0200_raw_material_paddons"),
    migration!("0201_raw_material_paddon_names"),
    migration!("0202_raw_material_paddon_issues"),
    migration!("0203_preparation_output_note_audience"),
    migration!("0204_raw_material_paddon_custody"),
    migration!("0205_gscale_receipt_barcode_lookup"),
    migration!("0206_preparation_roll_paddons"),
    migration!("0207_qolip_product_information"),
    migration!("0208_waste_bins"),
    migration!("0209_waste_receiver_role"),
    migration!("0210_waste_warehouses"),
    migration!("0211_waste_bin_access"),
    migration!("0212_material_paddon_roll_attachments"),
    migration!("0213_waste_bin_generated_codes"),
    migration!("0214_preparation_formula_live"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresConfig {
    pub database_url: String,
    pub migration_database_url: String,
    pub min_connections: u32,
    pub max_connections: u32,
    pub acquire_timeout: Duration,
}

impl PostgresConfig {
    #[allow(dead_code)]
    pub fn from_env() -> Result<Self, PostgresConfigError> {
        Self::from_env_with(|key| std::env::var(key).ok())
    }

    pub fn from_env_with(
        get_env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, PostgresConfigError> {
        let database_url = get_env("MINI_ERP_DATABASE_URL")
            .unwrap_or_default()
            .trim()
            .to_string();
        if database_url.is_empty() {
            return Err(PostgresConfigError::MissingDatabaseUrl);
        }
        let migration_database_url = get_env("MINI_ERP_MIGRATION_DATABASE_URL")
            .unwrap_or_else(|| database_url.clone())
            .trim()
            .to_string();
        let migration_database_url = if migration_database_url.is_empty() {
            database_url.clone()
        } else {
            migration_database_url
        };

        let max_connections = env_u32(&get_env, "MINI_ERP_PG_MAX_CONNECTIONS")
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_MAX_CONNECTIONS);
        let min_connections = env_u32(&get_env, "MINI_ERP_PG_MIN_CONNECTIONS")
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_MIN_CONNECTIONS)
            .min(max_connections);
        let acquire_timeout = Duration::from_millis(
            env_u64(&get_env, "MINI_ERP_PG_ACQUIRE_TIMEOUT_MS")
                .filter(|value| *value > 0)
                .unwrap_or(DEFAULT_ACQUIRE_TIMEOUT_MS),
        );

        Ok(Self {
            database_url,
            migration_database_url,
            min_connections,
            max_connections,
            acquire_timeout,
        })
    }

    #[allow(dead_code)]
    pub fn pool_options(&self) -> PgPoolOptions {
        PgPoolOptions::new()
            .min_connections(self.min_connections)
            .max_connections(self.max_connections)
            .acquire_timeout(self.acquire_timeout)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PostgresConfigError {
    MissingDatabaseUrl,
}

#[derive(Debug, thiserror::Error)]
pub enum PostgresBootstrapError {
    #[error("MINI_ERP_DATABASE_URL is required")]
    MissingDatabaseUrl,
    #[error("postgres connection failed: {0}")]
    Connect(#[source] sqlx::Error),
    #[error("postgres migration failed: {0}")]
    Migrate(#[source] sqlx::Error),
}

pub async fn connect_and_migrate_required() -> Result<PgPool, PostgresBootstrapError> {
    let config =
        PostgresConfig::from_env().map_err(|_| PostgresBootstrapError::MissingDatabaseUrl)?;
    let pool = config
        .pool_options()
        .connect(&config.migration_database_url)
        .await
        .map_err(PostgresBootstrapError::Connect)?;
    apply_foundation_migration(&pool)
        .await
        .map_err(PostgresBootstrapError::Migrate)?;
    // Never leak migration/owner credentials into HTTP stores or workers.
    if config.migration_database_url == config.database_url {
        Ok(pool)
    } else {
        pool.close().await;
        config
            .pool_options()
            .connect(&config.database_url)
            .await
            .map_err(PostgresBootstrapError::Connect)
    }
}

/// Connect with the migration credential and stop at an explicit migration
/// gate. This exists for operator-reviewed clean cutovers such as the 0069
/// canonical-apparatus authority boundary; normal application startup must
/// continue to use [`connect_and_migrate_required`].
pub async fn connect_and_migrate_required_through(
    target_version: &str,
) -> Result<PgPool, PostgresBootstrapError> {
    let config =
        PostgresConfig::from_env().map_err(|_| PostgresBootstrapError::MissingDatabaseUrl)?;
    let pool = config
        .pool_options()
        .connect(&config.migration_database_url)
        .await
        .map_err(PostgresBootstrapError::Connect)?;
    apply_postgres_migrations_through_version(&pool, target_version)
        .await
        .map_err(PostgresBootstrapError::Migrate)?;
    Ok(pool)
}

pub fn canonical_apparatus_service(
    pool: PgPool,
) -> crate::core::apparatus_standard::CanonicalApparatusService {
    crate::core::apparatus_standard::CanonicalApparatusService::new(std::sync::Arc::new(
        super::postgres_canonical_apparatus::PostgresCanonicalApparatusRepository::new(pool),
    ))
}

/// Apply the complete versioned migration set after an external database
/// restore. The restore flow uses this entry point so it does not depend on a
/// separate `mini_rs_migrate` process being available in the runtime image.
pub async fn migrate_database(
    database_url: &str,
    migration_database_url: Option<&str>,
) -> Result<(), PostgresBootstrapError> {
    let database_url = database_url.trim();
    if database_url.is_empty() {
        return Err(PostgresBootstrapError::MissingDatabaseUrl);
    }
    let migration_database_url = migration_database_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(database_url);
    let pool = PgPoolOptions::new()
        .connect(migration_database_url)
        .await
        .map_err(PostgresBootstrapError::Connect)?;
    let result = apply_foundation_migration(&pool)
        .await
        .map_err(PostgresBootstrapError::Migrate);
    pool.close().await;
    result
}

#[allow(dead_code)]
pub fn foundation_migration_sql() -> &'static str {
    POSTGRES_MIGRATIONS[0].1
}

#[allow(dead_code)]
pub async fn apply_foundation_migration(pool: &PgPool) -> Result<(), sqlx::Error> {
    apply_postgres_migrations(pool, &POSTGRES_MIGRATIONS).await
}
