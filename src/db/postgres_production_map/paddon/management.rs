use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{PaddonManagementSettings, ProductionMapError, QueueActionActor};

pub(super) async fn load(pool: &PgPool) -> Result<PaddonManagementSettings, ProductionMapError> {
    let (free_movement_enabled, worker_visibility_enabled) = sqlx::query_as(
        "SELECT free_movement_enabled, worker_visibility_enabled FROM mini_paddon_management_settings WHERE singleton",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(PaddonManagementSettings {
        free_movement_enabled,
        worker_visibility_enabled,
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
    free_movement_enabled: Option<bool>,
    worker_visibility_enabled: Option<bool>,
    actor: &QueueActionActor,
) -> Result<PaddonManagementSettings, ProductionMapError> {
    let (free_movement_enabled, worker_visibility_enabled) = sqlx::query_as("UPDATE mini_paddon_management_settings SET free_movement_enabled=COALESCE($1,free_movement_enabled), worker_visibility_enabled=COALESCE($2,worker_visibility_enabled), updated_at=now(), updated_by_ref=$3, updated_by_display_name=$4 WHERE singleton RETURNING free_movement_enabled, worker_visibility_enabled")
        .bind(free_movement_enabled).bind(worker_visibility_enabled).bind(actor.ref_.trim()).bind(actor.display_name.trim())
        .fetch_one(pool).await.map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(PaddonManagementSettings {
        free_movement_enabled,
        worker_visibility_enabled,
    })
}
