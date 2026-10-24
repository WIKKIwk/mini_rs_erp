use sqlx::{PgPool, Postgres, Transaction};

use crate::core::production_map::{LaminationSettings, ProductionMapError, QueueActionActor};

pub(super) async fn load(pool: &PgPool) -> Result<LaminationSettings, ProductionMapError> {
    let layer_remainders_required = sqlx::query_scalar(
        "SELECT layer_remainders_required FROM mini_lamination_settings WHERE singleton",
    )
    .fetch_one(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(LaminationSettings {
        layer_remainders_required,
    })
}

pub(super) async fn update(
    pool: &PgPool,
    required: bool,
    actor: &QueueActionActor,
) -> Result<LaminationSettings, ProductionMapError> {
    let layer_remainders_required = sqlx::query_scalar(
        "UPDATE mini_lamination_settings SET layer_remainders_required=$1,
         updated_at=now(), updated_by_ref=$2, updated_by_display_name=$3
         WHERE singleton RETURNING layer_remainders_required",
    )
    .bind(required)
    .bind(actor.ref_.trim())
    .bind(actor.display_name.trim())
    .fetch_one(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(LaminationSettings {
        layer_remainders_required,
    })
}

pub(super) async fn require_enabled_tx(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), ProductionMapError> {
    // A setting change waits for previously admitted returns to finish.
    let required: bool = sqlx::query_scalar(
        "SELECT layer_remainders_required FROM mini_lamination_settings WHERE singleton FOR SHARE",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if !required {
        return Err(ProductionMapError::QueueActionNotAllowed);
    }
    Ok(())
}
