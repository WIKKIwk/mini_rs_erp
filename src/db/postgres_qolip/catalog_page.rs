use sqlx::PgPool;

use crate::core::qolip::{QolipCatalogEntry, QolipCatalogPosition, QolipError};

/// Paginate product identities, not molds. This deliberately omits dimensions,
/// colors, lock/session scans, customer arrays and order-image lookups.
pub(super) async fn load(
    pool: &PgPool,
    query: &str,
    limit: usize,
    after: Option<&QolipCatalogPosition>,
    allowed: Option<&[String]>,
) -> Result<Vec<QolipCatalogEntry>, QolipError> {
    let allowed = allowed.map(|names| {
        names
            .iter()
            .map(|name| name.trim().to_lowercase())
            .collect::<Vec<_>>()
    });
    if allowed.as_ref().is_some_and(Vec::is_empty) {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, (String, String)>(r#"
        WITH RECURSIVE group_path(group_name, node_name, parent_name) AS (
            SELECT lower(name), lower(name), lower(parent_item_group) FROM mini_item_groups
            UNION ALL
            SELECT path.group_name, lower(parent.name), lower(parent.parent_item_group)
            FROM group_path path
            JOIN mini_item_groups parent ON lower(parent.name) = path.parent_name
            WHERE path.parent_name <> ''
        ), eligible_items AS (
            SELECT item.code, item.name FROM mini_items item
            WHERE EXISTS (SELECT 1 FROM group_path path
                WHERE path.group_name = lower(item.item_group)
                  AND path.node_name LIKE '%tayyor%' AND path.node_name LIKE '%mahsulot%')
        ), legacy_locations AS (
            SELECT DISTINCT ON (lower(location.qolip_code))
                location.item_code, location.item_name, location.qolip_code,
                COALESCE(mini_qolip_assigned_warehouse(location.created_by_role, location.created_by_ref), '') AS warehouse
            FROM mini_qolip_locations location
            WHERE NOT EXISTS (SELECT 1 FROM mini_qolip_product_specs spec
                WHERE lower(spec.qolip_code) = lower(location.qolip_code))
            ORDER BY lower(location.qolip_code), location.updated_at DESC, location.created_at DESC
        ), legacy_checkouts AS (
            SELECT DISTINCT ON (lower(checkout.qolip_code))
                checkout.item_code, checkout.item_name, checkout.qolip_code,
                COALESCE(mini_qolip_assigned_warehouse(checkout.issued_by_role, checkout.issued_by_ref), '') AS warehouse
            FROM mini_qolip_checkouts checkout
            WHERE lower(checkout.status) = 'open'
              AND NOT EXISTS (SELECT 1 FROM mini_qolip_product_specs spec
                  WHERE lower(spec.qolip_code) = lower(checkout.qolip_code))
              AND NOT EXISTS (SELECT 1 FROM mini_qolip_locations location
                  WHERE lower(location.qolip_code) = lower(checkout.qolip_code))
            ORDER BY lower(checkout.qolip_code), checkout.updated_at DESC, checkout.issued_at DESC
        ), sources AS (
            SELECT item.code, item.name, spec.qolip_code,
                   COALESCE(spec.payload_json->>'warehouse', '') AS warehouse
            FROM mini_qolip_product_specs spec
            JOIN mini_qolip_effective_product_molds link ON link.qolip_code = spec.qolip_code
            JOIN eligible_items item ON item.code = link.item_code
            UNION ALL
            SELECT COALESCE(item.code, source.item_code),
                   COALESCE(NULLIF(btrim(item.name), ''), NULLIF(btrim(source.item_name), ''), source.item_code),
                   source.qolip_code, source.warehouse
            FROM legacy_locations source
            LEFT JOIN eligible_items item ON lower(item.code) = lower(source.item_code)
            UNION ALL
            SELECT COALESCE(item.code, source.item_code),
                   COALESCE(NULLIF(btrim(item.name), ''), NULLIF(btrim(source.item_name), ''), source.item_code),
                   source.qolip_code, source.warehouse
            FROM legacy_checkouts source
            LEFT JOIN eligible_items item ON lower(item.code) = lower(source.item_code)
        ), visible AS (
            SELECT * FROM sources
            WHERE btrim(qolip_code) <> ''
              AND public.mini_qolip_storage_state(qolip_code) <> 'archived'
              AND ($3::text[] IS NULL OR lower(btrim(warehouse)) = ANY($3))
        ), products AS (
            SELECT DISTINCT ON (lower(code)) code, name FROM visible
            ORDER BY lower(code), lower(name), name
        )
        SELECT product.code, product.name FROM products product
        WHERE ($1 = '' OR lower(product.name) LIKE $2 OR lower(product.code) LIKE $2
            OR EXISTS (SELECT 1 FROM visible source
                WHERE lower(source.code) = lower(product.code) AND lower(source.qolip_code) LIKE $2)
            OR EXISTS (SELECT 1 FROM mini_customer_items assignment
                JOIN mini_customers customer ON customer.ref = assignment.customer_ref
                WHERE lower(assignment.item_code) = lower(product.code)
                  AND (lower(customer.name) LIKE $2 OR lower(customer.ref) LIKE $2)))
          AND ($4::text IS NULL OR (lower(product.name) COLLATE "C", lower(product.code) COLLATE "C")
              > ($4 COLLATE "C", $5 COLLATE "C"))
        ORDER BY lower(product.name) COLLATE "C", lower(product.code) COLLATE "C"
        LIMIT $6
    "#)
        .bind(query.trim().to_lowercase())
        .bind(format!("%{}%", query.trim().to_lowercase()))
        .bind(allowed)
        .bind(after.map(|position| position.name.as_str()))
        .bind(after.map(|position| position.code.as_str()))
        .bind(limit.min(201) as i64)
        .fetch_all(pool).await.map_err(|_| QolipError::StoreFailed)?;
    Ok(rows
        .into_iter()
        .map(|(code, name)| QolipCatalogEntry { code, name })
        .collect())
}
