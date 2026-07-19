use sqlx::{Postgres, Transaction};

use crate::core::admin::ports::AdminPortError;

pub(super) async fn item_delete_blocker(
    transaction: &mut Transaction<'_, Postgres>,
    item_code: &str,
    item_name: &str,
) -> Result<String, AdminPortError> {
    if active_order_uses_item(transaction, item_code, item_name).await? {
        return Ok("item is used by active order".to_string());
    }
    if active_stock_exists(transaction, item_code).await? {
        return Ok("item has active stock".to_string());
    }
    if pending_receipt_exists(transaction, item_code).await? {
        return Ok("item has pending receipt".to_string());
    }
    if active_rps_batch_exists(transaction, item_code).await? {
        return Ok("item is used by active rps batch".to_string());
    }
    if active_qolip_reference_exists(transaction, item_code).await? {
        return Ok("item is used by active qolip operation".to_string());
    }
    if quick_order_template_exists(transaction, item_code, item_name).await? {
        return Ok("item is used by quick order template".to_string());
    }
    if unresolved_material_assignment_exists(transaction, item_code).await? {
        return Ok("item is used by unresolved material assignment".to_string());
    }
    Ok(String::new())
}

async fn active_order_uses_item(
    transaction: &mut Transaction<'_, Postgres>,
    item_code: &str,
    item_name: &str,
) -> Result<bool, AdminPortError> {
    sqlx::query_scalar::<_, bool>(
        r#"
        WITH matching_orders AS (
            SELECT DISTINCT orders.id, orders.status
            FROM mini_orders orders
            WHERE lower(orders.product_code) = lower($1)
               OR (
                    btrim(orders.product_code) = ''
                    AND lower(orders.product_name) = lower($2)
               )
               OR EXISTS (
                    SELECT 1
                    FROM mini_order_products products
                    WHERE products.order_id = orders.id
                      AND (
                            lower(products.item_code) = lower($1)
                            OR (
                                btrim(products.item_code) = ''
                                AND lower(products.product_name) = lower($2)
                            )
                      )
               )
        ),
        matching_maps AS (
            SELECT maps.id, maps.map_json
            FROM mini_production_maps maps
            WHERE lower(maps.product_code) = lower($1)
               OR EXISTS (
                    SELECT 1
                    FROM matching_orders orders
                    WHERE orders.id = maps.id OR orders.id = maps.order_id
               )
               OR EXISTS (
                    SELECT 1
                    FROM mini_raw_material_assignments assignments
                    WHERE assignments.order_id = maps.id
                      AND lower(assignments.item_code) = lower($1)
               )
        ),
        active_maps AS (
            SELECT maps.id
            FROM matching_maps maps
            WHERE EXISTS (
                    SELECT 1
                    FROM mini_order_run_sessions sessions
                    WHERE sessions.order_id = maps.id
                      AND sessions.status IN ('active', 'paused')
                  )
               OR EXISTS (
                    SELECT 1
                    FROM mini_progress_batches batches
                    WHERE batches.order_id = maps.id
                      AND batches.wip_status <> 'processed'
                  )
               OR NOT EXISTS (
                    SELECT 1
                    FROM mini_queue_states states
                    WHERE states.order_id = maps.id
                  )
               OR EXISTS (
                    SELECT 1
                    FROM mini_queue_states states
                    WHERE states.order_id = maps.id
                      AND states.state <> 'completed'
                  )
               OR EXISTS (
                    SELECT 1
                    FROM jsonb_array_elements(
                        CASE
                            WHEN jsonb_typeof(maps.map_json->'nodes') = 'array'
                                THEN maps.map_json->'nodes'
                            ELSE '[]'::jsonb
                        END
                    ) AS node
                    WHERE node->>'kind' = 'apparatus'
                      AND NOT EXISTS (
                            SELECT 1
                            FROM mini_queue_states states
                            WHERE states.order_id = maps.id
                              AND states.state = 'completed'
                              AND lower(btrim(states.apparatus)) = lower(btrim(
                                    COALESCE(
                                        NULLIF(node->>'alternative_assigned_title', ''),
                                        node->>'title'
                                    )
                              ))
                      )
                  )
        )
        SELECT EXISTS (
            SELECT 1 FROM active_maps
            UNION ALL
            SELECT 1
            FROM matching_orders orders
            WHERE orders.status NOT IN ('completed', 'cancelled')
              AND NOT EXISTS (
                    SELECT 1
                    FROM mini_production_maps maps
                    WHERE maps.id = orders.id OR maps.order_id = orders.id
              )
        )
        "#,
    )
    .bind(item_code)
    .bind(item_name)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| AdminPortError::LookupFailed)
}

async fn active_stock_exists(
    transaction: &mut Transaction<'_, Postgres>,
    item_code: &str,
) -> Result<bool, AdminPortError> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM mini_raw_material_stock
             WHERE lower(item_code) = lower($1)
               AND qty > 0
               AND status IN ('available', 'reserved', 'in_use')
             UNION ALL
             SELECT 1
             FROM mini_finished_goods_stock
             WHERE lower(item_code) = lower($1)
               AND qty > 0
               AND status = 'available'
         )",
    )
    .bind(item_code)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| AdminPortError::LookupFailed)
}

async fn pending_receipt_exists(
    transaction: &mut Transaction<'_, Postgres>,
    item_code: &str,
) -> Result<bool, AdminPortError> {
    exists_by_code(
        transaction,
        "SELECT EXISTS (
             SELECT 1 FROM mini_gscale_receipts
             WHERE lower(item_code) = lower($1) AND status = 'draft'
         )",
        item_code,
    )
    .await
}

async fn active_rps_batch_exists(
    transaction: &mut Transaction<'_, Postgres>,
    item_code: &str,
) -> Result<bool, AdminPortError> {
    exists_by_code(
        transaction,
        "SELECT EXISTS (
             SELECT 1 FROM mini_rps_batches
             WHERE lower(item_code) = lower($1) AND active = true
         )",
        item_code,
    )
    .await
}

async fn active_qolip_reference_exists(
    transaction: &mut Transaction<'_, Postgres>,
    item_code: &str,
) -> Result<bool, AdminPortError> {
    exists_by_code(
        transaction,
        "SELECT EXISTS (
             SELECT 1 FROM mini_qolip_checkouts
             WHERE lower(item_code) = lower($1) AND status = 'open'
             UNION ALL
             SELECT 1 FROM mini_qolip_locations
             WHERE lower(item_code) = lower($1) AND quantity > 0
             UNION ALL
             SELECT 1 FROM mini_qolip_product_specs
             WHERE lower(item_code) = lower($1)
         )",
        item_code,
    )
    .await
}

async fn quick_order_template_exists(
    transaction: &mut Transaction<'_, Postgres>,
    item_code: &str,
    item_name: &str,
) -> Result<bool, AdminPortError> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1
             FROM mini_quick_order_templates
             WHERE lower(item_code) = lower($1)
                OR (btrim(item_code) = '' AND lower(product_name) = lower($2))
         )",
    )
    .bind(item_code)
    .bind(item_name)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| AdminPortError::LookupFailed)
}

async fn unresolved_material_assignment_exists(
    transaction: &mut Transaction<'_, Postgres>,
    item_code: &str,
) -> Result<bool, AdminPortError> {
    exists_by_code(
        transaction,
        "SELECT EXISTS (
             SELECT 1
             FROM mini_raw_material_assignments assignments
             WHERE lower(assignments.item_code) = lower($1)
               AND NOT EXISTS (
                    SELECT 1
                    FROM mini_production_maps maps
                    WHERE maps.id = assignments.order_id
               )
         )",
        item_code,
    )
    .await
}

async fn exists_by_code(
    transaction: &mut Transaction<'_, Postgres>,
    query: &'static str,
    item_code: &str,
) -> Result<bool, AdminPortError> {
    sqlx::query_scalar::<_, bool>(query)
        .bind(item_code)
        .fetch_one(&mut **transaction)
        .await
        .map_err(|_| AdminPortError::LookupFailed)
}

pub(super) fn map_item_delete_write_error(error: sqlx::Error) -> AdminPortError {
    if error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| code == "23503")
    {
        AdminPortError::InvalidInput("item is still referenced".to_string())
    } else {
        AdminPortError::LookupFailed
    }
}
