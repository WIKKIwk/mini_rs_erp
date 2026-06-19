use sqlx::PgPool;

use crate::core::production_map::{
    ApparatusMaterialRule, ProductionMapError, RawMaterialAssignment,
};

pub(super) async fn load_apparatus_material_rules(
    pool: &PgPool,
) -> Result<Vec<ApparatusMaterialRule>, ProductionMapError> {
    let rows = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload_json
         FROM mini_apparatus_material_rules
         ORDER BY lower(apparatus) ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    rows.into_iter()
        .map(|payload| {
            serde_json::from_value::<ApparatusMaterialRule>(payload)
                .map_err(|_| ProductionMapError::StoreFailed)
        })
        .collect()
}

pub(super) async fn save_apparatus_material_rule(
    pool: &PgPool,
    rule: ApparatusMaterialRule,
) -> Result<(), ProductionMapError> {
    let item_groups =
        serde_json::to_value(&rule.item_groups).map_err(|_| ProductionMapError::StoreFailed)?;
    let payload = serde_json::to_value(&rule).map_err(|_| ProductionMapError::StoreFailed)?;
    sqlx::query(
        "INSERT INTO mini_apparatus_material_rules
            (apparatus, item_groups, requires_material, payload_json, updated_at)
         VALUES ($1, $2, $3, $4, now())
         ON CONFLICT (apparatus) DO UPDATE SET
           item_groups = excluded.item_groups,
           requires_material = excluded.requires_material,
           payload_json = excluded.payload_json,
           updated_at = excluded.updated_at",
    )
    .bind(rule.apparatus.trim())
    .bind(item_groups)
    .bind(rule.requires_material)
    .bind(payload)
    .execute(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    Ok(())
}

pub(super) async fn load_raw_material_assignments(
    pool: &PgPool,
) -> Result<Vec<RawMaterialAssignment>, ProductionMapError> {
    let rows = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT payload_json
         FROM mini_raw_material_assignments
         ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;

    rows.into_iter()
        .map(|payload| {
            serde_json::from_value::<RawMaterialAssignment>(payload)
                .map_err(|_| ProductionMapError::StoreFailed)
        })
        .collect()
}

pub(super) async fn save_raw_material_assignment(
    pool: &PgPool,
    assignment: RawMaterialAssignment,
) -> Result<(), ProductionMapError> {
    let payload = serde_json::to_value(&assignment).map_err(|_| ProductionMapError::StoreFailed)?;
    let result = sqlx::query(
        "INSERT INTO mini_raw_material_assignments
            (barcode, order_id, apparatus, item_code, item_group, payload_json, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, now())
         ON CONFLICT (barcode) DO NOTHING",
    )
    .bind(assignment.barcode.trim())
    .bind(assignment.order_id.trim())
    .bind(assignment.apparatus.trim())
    .bind(assignment.item_code.trim())
    .bind(assignment.item_group.trim())
    .bind(payload)
    .execute(pool)
    .await
    .map_err(|_| ProductionMapError::StoreFailed)?;
    if result.rows_affected() == 0 {
        return Err(ProductionMapError::RawMaterialAlreadyAssigned);
    }
    Ok(())
}
