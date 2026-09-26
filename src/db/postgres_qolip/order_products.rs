use sqlx::PgPool;

use crate::core::qolip::{QolipError, QolipProduct};

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
                    SELECT 1 FROM mini_qolip_product_specs spec
                    WHERE lower(btrim(spec.item_code)) = lower(btrim(item.code))
                      AND lower(btrim(spec.item_group)) = lower(btrim(item.item_group))
                      AND btrim(spec.qolip_code) <> ''
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
            code,
            name,
            item_group,
            has_qolip_spec,
            ..Default::default()
        })
        .collect())
}
