use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{PaddonManagementSettings, ProductionMapError, QueueActionActor};

pub(super) async fn load(pool: &PgPool) -> Result<PaddonManagementSettings, ProductionMapError> {
    let enabled = sqlx::query_scalar(
        "SELECT free_movement_enabled FROM mini_paddon_management_settings WHERE singleton",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(PaddonManagementSettings {
        free_movement_enabled: enabled,
    })
}

pub(super) async fn lock_enabled(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<bool, ProductionMapError> {
    // Disabling the mode waits for already-started membership edits to finish.
    sqlx::query_scalar("SELECT free_movement_enabled FROM mini_paddon_management_settings WHERE singleton FOR SHARE")
        .fetch_one(&mut **tx).await.map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn update(
    pool: &PgPool,
    enabled: bool,
    actor: &QueueActionActor,
) -> Result<PaddonManagementSettings, ProductionMapError> {
    let enabled = sqlx::query_scalar("UPDATE mini_paddon_management_settings SET free_movement_enabled=$1, updated_at=now(), updated_by_ref=$2, updated_by_display_name=$3 WHERE singleton RETURNING free_movement_enabled")
        .bind(enabled).bind(actor.ref_.trim()).bind(actor.display_name.trim())
        .fetch_one(pool).await.map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(PaddonManagementSettings {
        free_movement_enabled: enabled,
    })
}
