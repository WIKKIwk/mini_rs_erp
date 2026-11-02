use sqlx::PgPool;

use crate::core::production_map::{
    LaminationLabelSettingsPatch, LaminationSettings, ProductionMapError, QueueActionActor,
};

fn settings((wip, film): (bool, bool)) -> LaminationSettings {
    LaminationSettings {
        layer_remainders_required: wip && film,
        print_wip_remainder_labels: wip,
        print_film_remainder_labels: film,
    }
}

pub(super) async fn load(pool: &PgPool) -> Result<LaminationSettings, ProductionMapError> {
    sqlx::query_as::<_, (bool, bool)>(
        "SELECT print_wip_remainder_labels,print_film_remainder_labels
         FROM mini_lamination_settings WHERE singleton",
    )
    .fetch_one(pool)
    .await
    .map(settings)
    .map_err(|_| ProductionMapError::StoreFailed)
}

pub(super) async fn update(
    pool: &PgPool,
    patch: LaminationLabelSettingsPatch,
    actor: &QueueActionActor,
) -> Result<LaminationSettings, ProductionMapError> {
    // A partial update never overwrites the other switch from a stale screen.
    sqlx::query_as::<_, (bool, bool)>(
        "UPDATE mini_lamination_settings SET
         print_wip_remainder_labels=COALESCE($1,print_wip_remainder_labels),
         print_film_remainder_labels=COALESCE($2,print_film_remainder_labels),
         layer_remainders_required=COALESCE($1,print_wip_remainder_labels)
             AND COALESCE($2,print_film_remainder_labels),
         updated_at=now(),updated_by_ref=$3,updated_by_display_name=$4
         WHERE singleton RETURNING print_wip_remainder_labels,print_film_remainder_labels",
    )
    .bind(patch.print_wip_remainder_labels)
    .bind(patch.print_film_remainder_labels)
    .bind(actor.ref_.trim())
    .bind(actor.display_name.trim())
    .fetch_one(pool)
    .await
    .map(settings)
    .map_err(|_| ProductionMapError::StoreFailed)
}
