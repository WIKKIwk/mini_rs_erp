const DEFAULT_MIN_CONNECTIONS: u32 = 2;
const DEFAULT_MAX_CONNECTIONS: u32 = 16;
const DEFAULT_ACQUIRE_TIMEOUT_MS: u64 = 500;
const MIGRATION_LOCK_KEY: i64 = 6_514_811_918_052_026_001;

const POSTGRES_MIGRATIONS: [(&str, &str); 87] = [
    (
        "0001_mini_erp_foundation",
        include_str!("../../../migrations/postgres/0001_mini_erp_foundation.sql"),
    ),
    (
        "0002_order_integrity",
        include_str!("../../../migrations/postgres/0002_order_integrity.sql"),
    ),
    (
        "0003_erp_data_integrity",
        include_str!("../../../migrations/postgres/0003_erp_data_integrity.sql"),
    ),
    (
        "0004_system_users",
        include_str!("../../../migrations/postgres/0004_system_users.sql"),
    ),
    (
        "0005_chat",
        include_str!("../../../migrations/postgres/0005_chat.sql"),
    ),
    (
        "0006_boyoqchi_returned_paint",
        include_str!("../../../migrations/postgres/0006_boyoqchi_returned_paint.sql"),
    ),
    (
        "0007_runtime_table_ownership",
        include_str!("../../../migrations/postgres/0007_runtime_table_ownership.sql"),
    ),
    (
        "0008_returned_paint_calculations",
        include_str!("../../../migrations/postgres/0008_returned_paint_calculations.sql"),
    ),
    (
        "0009_returned_paint_solvent_calculations",
        include_str!("../../../migrations/postgres/0009_returned_paint_solvent_calculations.sql"),
    ),
    (
        "0010_returned_paint_image_workflow",
        include_str!("../../../migrations/postgres/0010_returned_paint_image_workflow.sql"),
    ),
    (
        "0011_chat_media_foundation",
        include_str!("../../../migrations/postgres/0011_chat_media_foundation.sql"),
    ),
    (
        "0012_chat_media_v1",
        include_str!("../../../migrations/postgres/0012_chat_media_v1.sql"),
    ),
    (
        "0013_chat_media_incident_video",
        include_str!("../../../migrations/postgres/0013_chat_media_incident_video.sql"),
    ),
    (
        "0014_raw_material_stock_corrections",
        include_str!("../../../migrations/postgres/0014_raw_material_stock_corrections.sql"),
    ),
    (
        "0015_item_identity_updates",
        include_str!("../../../migrations/postgres/0015_item_identity_updates.sql"),
    ),
    (
        "0016_chat_delivery_reliability",
        include_str!("../../../migrations/postgres/0016_chat_delivery_reliability.sql"),
    ),
    (
        "0017_chat_delivery_reliability_followup",
        include_str!("../../../migrations/postgres/0017_chat_delivery_reliability_followup.sql"),
    ),
    (
        "0018_item_master_without_warehouse",
        include_str!("../../../migrations/postgres/0018_item_master_without_warehouse.sql"),
    ),
    (
        "0019_chat_voice_messages",
        include_str!("../../../migrations/postgres/0019_chat_voice_messages.sql"),
    ),
    (
        "0020_worker_identity_lifecycle",
        include_str!("../../../migrations/postgres/0020_worker_identity_lifecycle.sql"),
    ),
    (
        "0021_rps_batch_history",
        include_str!("../../../migrations/postgres/0021_rps_batch_history.sql"),
    ),
    (
        "0022_rps_batch_codes",
        include_str!("../../../migrations/postgres/0022_rps_batch_codes.sql"),
    ),
    (
        "0023_qolip_13_rows",
        include_str!("../../../migrations/postgres/0023_qolip_13_rows.sql"),
    ),
    (
        "0024_qolip_legacy_lookup_index",
        include_str!("../../../migrations/postgres/0024_qolip_legacy_lookup_index.sql"),
    ),
    (
        "0025_order_control_state",
        include_str!("../../../migrations/postgres/0025_order_control_state.sql"),
    ),
    (
        "0026_order_freeze_request_chat_cards",
        include_str!("../../../migrations/postgres/0026_order_freeze_request_chat_cards.sql"),
    ),
    (
        "0027_rps_runtime_privileges",
        include_str!("../../../migrations/postgres/0027_rps_runtime_privileges.sql"),
    ),
    (
        "0028_factory_locations",
        include_str!("../../../migrations/postgres/0028_factory_locations.sql"),
    ),
    (
        "0029_inventory_movements",
        include_str!("../../../migrations/postgres/0029_inventory_movements.sql"),
    ),
    (
        "0030_inventory_transfer_chat_cards",
        include_str!("../../../migrations/postgres/0030_inventory_transfer_chat_cards.sql"),
    ),
    (
        "0031_dynamic_order_layers",
        include_str!("../../../migrations/postgres/0031_dynamic_order_layers.sql"),
    ),
    (
        "0032_apparatus_order_transfers",
        include_str!("../../../migrations/postgres/0032_apparatus_order_transfers.sql"),
    ),
    (
        "0033_apparatus_master_metadata",
        include_str!("../../../migrations/postgres/0033_apparatus_master_metadata.sql"),
    ),
    (
        "0034_apparatus_capacity_scheduling",
        include_str!("../../../migrations/postgres/0034_apparatus_capacity_scheduling.sql"),
    ),
    (
        "0035_apparatus_schedule_paused_status",
        include_str!("../../../migrations/postgres/0035_apparatus_schedule_paused_status.sql"),
    ),
    (
        "0036_inventory_return_events",
        include_str!("../../../migrations/postgres/0036_inventory_return_events.sql"),
    ),
    (
        "0037_qolip_order_notes",
        include_str!("../../../migrations/postgres/0037_qolip_order_notes.sql"),
    ),
    (
        "0038_calculate_material_catalog",
        include_str!("../../../migrations/postgres/0038_calculate_material_catalog.sql"),
    ),
    (
        "0039_rezka_roll_fanout",
        include_str!("../../../migrations/postgres/0039_rezka_roll_fanout.sql"),
    ),
    (
        "0040_laminatsiya_astatka_reports",
        include_str!("../../../migrations/postgres/0040_laminatsiya_astatka_reports.sql"),
    ),
    (
        "0041_rezka_astatka_reports",
        include_str!("../../../migrations/postgres/0041_rezka_astatka_reports.sql"),
    ),
    (
        "0042_production_order_number_sequence",
        include_str!("../../../migrations/postgres/0042_production_order_number_sequence.sql"),
    ),
    (
        "0043_rezka_progress_diameter",
        include_str!("../../../migrations/postgres/0043_rezka_progress_diameter.sql"),
    ),
    (
        "0044_paddons",
        include_str!("../../../migrations/postgres/0044_paddons.sql"),
    ),
    (
        "0045_paddon_sequence_and_package_shape",
        include_str!("../../../migrations/postgres/0045_paddon_sequence_and_package_shape.sql"),
    ),
    (
        "0046_production_progress_bobina",
        include_str!("../../../migrations/postgres/0046_production_progress_bobina.sql"),
    ),
    (
        "0047_progress_batch_corrections",
        include_str!("../../../migrations/postgres/0047_progress_batch_corrections.sql"),
    ),
    (
        "0048_calculate_material_catalog_seed",
        include_str!("../../../migrations/postgres/0048_calculate_material_catalog_seed.sql"),
    ),
    (
        "0049_correct_calculate_material_defaults",
        include_str!("../../../migrations/postgres/0049_correct_calculate_material_defaults.sql"),
    ),
    (
        "0050_roll_detached_status",
        include_str!("../../../migrations/postgres/0050_roll_detached_status.sql"),
    ),
    (
        "0051_quantity_precision",
        include_str!("../../../migrations/postgres/0051_quantity_precision.sql"),
    ),
    (
        "0052_training_workspace",
        include_str!("../../../migrations/postgres/0052_training_workspace.sql"),
    ),
    (
        "0053_training_queue_states",
        include_str!("../../../migrations/postgres/0053_training_queue_states.sql"),
    ),
    (
        "0054_training_returned_paint",
        include_str!("../../../migrations/postgres/0054_training_returned_paint.sql"),
    ),
    (
        "0055_apparatus_capacity_identity",
        include_str!("../../../migrations/postgres/0055_apparatus_capacity_identity.sql"),
    ),
    (
        "0056_training_queue_events",
        include_str!("../../../migrations/postgres/0056_training_queue_events.sql"),
    ),
    (
        "0057_training_input_batches",
        include_str!("../../../migrations/postgres/0057_training_input_batches.sql"),
    ),
    (
        "0058_training_progress_batches",
        include_str!("../../../migrations/postgres/0058_training_progress_batches.sql"),
    ),
    (
        "0059_training_input_batch_sets",
        include_str!("../../../migrations/postgres/0059_training_input_batch_sets.sql"),
    ),
    (
        "0060_frozen_order_queue_state",
        include_str!("../../../migrations/postgres/0060_frozen_order_queue_state.sql"),
    ),
    (
        "0061_order_reset_append_only_override",
        include_str!("../../../migrations/postgres/0061_order_reset_append_only_override.sql"),
    ),
    (
        "0062_concurrency_idempotency_constraints",
        include_str!("../../../migrations/postgres/0062_concurrency_idempotency_constraints.sql"),
    ),
    (
        "0063_canonical_apparatus_reference_ids",
        include_str!("../../../migrations/postgres/0063_canonical_apparatus_reference_ids.sql"),
    ),
    (
        "0064_canonical_material_rule_apparatus_id",
        include_str!("../../../migrations/postgres/0064_canonical_material_rule_apparatus_id.sql"),
    ),
    (
        "0065_canonical_apparatus_cutover",
        include_str!("../../../migrations/postgres/0065_canonical_apparatus_cutover.sql"),
    ),
    (
        "0066_canonical_authority_remainder",
        include_str!("../../../migrations/postgres/0066_canonical_authority_remainder.sql"),
    ),
    (
        "0067_canonical_apparatus_payload_invariant",
        include_str!("../../../migrations/postgres/0067_canonical_apparatus_payload_invariant.sql"),
    ),
    (
        "0068_canonical_apparatus_fk_indexes",
        include_str!("../../../migrations/postgres/0068_canonical_apparatus_fk_indexes.sql"),
    ),
    (
        "0069_canonical_apparatus_revision_authority",
        include_str!(
            "../../../migrations/postgres/0069_canonical_apparatus_revision_authority.sql"
        ),
    ),
    (
        "0070_canonical_apparatus_clean_cutover",
        include_str!("../../../migrations/postgres/0070_canonical_apparatus_clean_cutover.sql"),
    ),
    (
        "0071_qolip_lock_ownership",
        include_str!("../../../migrations/postgres/0071_qolip_lock_ownership.sql"),
    ),
    (
        "0072_canonical_identity_indexes",
        include_str!("../../../migrations/postgres/0072_canonical_identity_indexes.sql"),
    ),
    (
        "0073_material_receipt_dimensions",
        include_str!("../../../migrations/postgres/0073_material_receipt_dimensions.sql"),
    ),
    (
        "0074_calculate_material_roll_catalog",
        include_str!("../../../migrations/postgres/0074_calculate_material_roll_catalog.sql"),
    ),
    (
        "0075_apparatus_collections",
        include_str!("../../../migrations/postgres/0075_apparatus_collections.sql"),
    ),
    (
        "0076_raw_material_soft_delete",
        include_str!("../../../migrations/postgres/0076_raw_material_soft_delete.sql"),
    ),
    (
        "0077_production_order_lifecycle",
        include_str!("../../../migrations/postgres/0077_production_order_lifecycle.sql"),
    ),
    (
        "0078_production_order_operational_status",
        include_str!("../../../migrations/postgres/0078_production_order_operational_status.sql"),
    ),
    (
        "0079_opening_wip",
        include_str!("../../../migrations/postgres/0079_opening_wip.sql"),
    ),
    (
        "0080_opening_wip_passport_metrics",
        include_str!("../../../migrations/postgres/0080_opening_wip_passport_metrics.sql"),
    ),
    (
        "0081_opening_wip_resume_stage",
        include_str!("../../../migrations/postgres/0081_opening_wip_resume_stage.sql"),
    ),
    (
        "0082_qolip_item_code_doctor",
        include_str!("../../../migrations/postgres/0082_qolip_item_code_doctor.sql"),
    ),
    (
        "0083_opening_wip_source_contract",
        include_str!("../../../migrations/postgres/0083_opening_wip_source_contract.sql"),
    ),
    (
        "0084_opening_wip_soft_delete",
        include_str!("../../../migrations/postgres/0084_opening_wip_soft_delete.sql"),
    ),
    (
        "0085_rezka_merge_lineage",
        include_str!("../../../migrations/postgres/0085_rezka_merge_lineage.sql"),
    ),
    (
        "0086_rezka_merge_action",
        include_str!("../../../migrations/postgres/0086_rezka_merge_action.sql"),
    ),
    (
        "0087_queue_event_stage_identity",
        include_str!("../../../migrations/postgres/0087_queue_event_stage_identity.sql"),
    ),
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
    Ok(pool)
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
