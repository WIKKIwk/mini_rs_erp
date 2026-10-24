use sqlx::PgPool;

use crate::core::qolip::{QolipError, QolipProduct};

/// The task list needs only warehouse-scoped readiness, not physical molds.
/// Match compatible_specs_for_product, including shared sets and legacy molds.
pub(super) async fn readiness(
    pool: &PgPool,
    codes: &[String],
    warehouses: &[String],
) -> Result<std::collections::BTreeMap<String, bool>, QolipError> {
    let normalized = |values: &[String]| {
        values
            .iter()
            .map(|value| value.trim().to_lowercase())
            .collect::<Vec<_>>()
    };
    let rows = sqlx::query_as::<_, (String, bool)>(r#"
        SELECT lower(btrim(item.code)), EXISTS (
            SELECT 1 FROM mini_qolip_product_specs spec
            JOIN mini_qolip_effective_product_molds link ON link.qolip_code = spec.qolip_code
            WHERE link.item_code_key = lower(btrim(item.code))
              AND btrim(spec.qolip_code) <> ''
              AND lower(btrim(COALESCE(spec.payload_json->>'warehouse', ''))) = ANY($2)
              AND (lower(btrim(spec.item_code)) <> link.item_code_key
                   OR lower(btrim(spec.item_group)) = lower(btrim(item.item_group)))
        ) OR EXISTS (
            SELECT 1 FROM mini_qolip_locations location
            WHERE lower(btrim(location.item_code)) = lower(btrim(item.code))
              AND btrim(item.item_group) <> '' AND btrim(location.qolip_code) <> ''
              AND lower(btrim(COALESCE(mini_qolip_assigned_warehouse(location.created_by_role, location.created_by_ref), ''))) = ANY($2)
              AND NOT EXISTS (SELECT 1 FROM mini_qolip_product_specs spec
                              WHERE lower(spec.qolip_code) = lower(location.qolip_code))
        ) OR EXISTS (
            SELECT 1 FROM mini_qolip_checkouts checkout
            WHERE lower(btrim(checkout.item_code)) = lower(btrim(item.code))
              AND btrim(item.item_group) <> '' AND btrim(checkout.qolip_code) <> ''
              AND lower(checkout.status) = 'open'
              AND lower(btrim(COALESCE(mini_qolip_assigned_warehouse(checkout.issued_by_role, checkout.issued_by_ref), ''))) = ANY($2)
              AND NOT EXISTS (SELECT 1 FROM mini_qolip_product_specs spec
                              WHERE lower(spec.qolip_code) = lower(checkout.qolip_code))
              AND NOT EXISTS (SELECT 1 FROM mini_qolip_locations location
                              WHERE lower(location.qolip_code) = lower(checkout.qolip_code))
        ) AS ready
        FROM mini_items item WHERE lower(btrim(item.code)) = ANY($1)
        "#)
        .bind(normalized(codes)).bind(normalized(warehouses))
        .fetch_all(pool).await.map_err(|_| QolipError::StoreFailed)?;
    Ok(rows.into_iter().collect())
}

/// One bounded query for the products in the queue window. Mirrors the start
/// validator's code/group matching and its legacy location/checkout fallback.
pub(super) async fn load(pool: &PgPool, codes: &[String]) -> Result<Vec<QolipProduct>, QolipError> {
    let codes: Vec<_> = codes
        .iter()
        .map(|code| code.trim().to_lowercase())
        .collect();
    let rows = sqlx::query_as::<_, (String, String, String, bool)>(
        r#"
        SELECT item.code, item.name, item.item_group,
            btrim(item.item_group) <> '' AND (
                EXISTS (
                    SELECT 1 FROM mini_qolip_effective_product_molds mold
                    WHERE mold.item_code_key = lower(btrim(item.code)) AND btrim(mold.qolip_code) <> ''
                ) OR EXISTS (
                    SELECT 1 FROM mini_qolip_locations location
                    WHERE lower(btrim(location.item_code)) = lower(btrim(item.code))
                      AND btrim(location.qolip_code) <> ''
                      AND NOT EXISTS (
                          SELECT 1 FROM mini_qolip_product_specs spec
                          WHERE lower(spec.qolip_code) = lower(location.qolip_code)
                      )
                ) OR EXISTS (
                    SELECT 1 FROM mini_qolip_checkouts checkout
                    WHERE lower(btrim(checkout.item_code)) = lower(btrim(item.code))
                      AND lower(checkout.status) = 'open'
                      AND btrim(checkout.qolip_code) <> ''
                      AND NOT EXISTS (
                          SELECT 1 FROM mini_qolip_product_specs spec
                          WHERE lower(spec.qolip_code) = lower(checkout.qolip_code)
                      )
                      AND NOT EXISTS (
                          SELECT 1 FROM mini_qolip_locations location
                          WHERE lower(location.qolip_code) = lower(checkout.qolip_code)
                      )
                )
            ) AS has_qolip_spec
        FROM mini_items item
        WHERE lower(btrim(item.code)) = ANY($1)
        "#,
    )
    .bind(codes)
    .fetch_all(pool)
    .await
    .map_err(|_| QolipError::StoreFailed)?;
    Ok(rows
        .into_iter()
        .map(|(code, name, item_group, has_qolip_spec)| QolipProduct {
            qolip_set_id: String::new(),
            code,
            name,
            item_group,
            has_qolip_spec,
            ..Default::default()
        })
        .collect())
}
