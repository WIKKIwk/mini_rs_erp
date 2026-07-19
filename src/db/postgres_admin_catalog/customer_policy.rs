use sqlx::{Postgres, Transaction};

use crate::core::admin::item_customer_policy::FINISHED_GOODS_GROUP;
use crate::core::admin::ports::AdminPortError;

pub(crate) async fn lock_item_customer_policy(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<(), AdminPortError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('mini_item_customer_policy', 0))")
        .execute(&mut **transaction)
        .await
        .map_err(|_| AdminPortError::LookupFailed)?;
    Ok(())
}

pub(crate) async fn group_requires_customer(
    transaction: &mut Transaction<'_, Postgres>,
    item_group: &str,
) -> Result<bool, AdminPortError> {
    sqlx::query_scalar(
        "WITH RECURSIVE group_path(name, parent_item_group) AS (
             SELECT name, COALESCE(parent_item_group, '')
             FROM mini_item_groups
             WHERE lower(name) = lower($1)
             UNION
             SELECT parent.name, COALESCE(parent.parent_item_group, '')
             FROM group_path path
             JOIN mini_item_groups parent
               ON lower(parent.name) = lower(path.parent_item_group)
             WHERE path.parent_item_group <> ''
         )
         SELECT EXISTS (
             SELECT 1 FROM group_path WHERE lower(btrim(name)) = lower($2)
         )",
    )
    .bind(item_group.trim())
    .bind(FINISHED_GOODS_GROUP)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| AdminPortError::LookupFailed)
}

pub(crate) async fn item_requires_customer(
    transaction: &mut Transaction<'_, Postgres>,
    item_code: &str,
) -> Result<bool, AdminPortError> {
    sqlx::query_scalar(
        "WITH RECURSIVE group_path(name, parent_item_group) AS (
             SELECT groups.name, COALESCE(groups.parent_item_group, '')
             FROM mini_items items
             JOIN mini_item_groups groups ON lower(groups.name) = lower(items.item_group)
             WHERE items.code = $1
             UNION
             SELECT parent.name, COALESCE(parent.parent_item_group, '')
             FROM group_path path
             JOIN mini_item_groups parent
               ON lower(parent.name) = lower(path.parent_item_group)
             WHERE path.parent_item_group <> ''
         )
         SELECT EXISTS (
             SELECT 1 FROM group_path WHERE lower(btrim(name)) = lower($2)
         )",
    )
    .bind(item_code.trim())
    .bind(FINISHED_GOODS_GROUP)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| AdminPortError::LookupFailed)
}

pub(crate) async fn customerless_item_codes(
    transaction: &mut Transaction<'_, Postgres>,
    item_codes: &[String],
) -> Result<Vec<String>, AdminPortError> {
    if item_codes.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_scalar(
        "SELECT items.code
         FROM mini_items items
         WHERE items.code = ANY($1)
           AND NOT EXISTS (
               SELECT 1
               FROM mini_customer_items assignments
               WHERE assignments.item_code = items.code
           )
         ORDER BY lower(items.code)",
    )
    .bind(item_codes)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|_| AdminPortError::LookupFailed)
}

pub(crate) async fn customerless_items_in_subtree(
    transaction: &mut Transaction<'_, Postgres>,
    root_group: &str,
) -> Result<Vec<String>, AdminPortError> {
    sqlx::query_scalar(
        "WITH RECURSIVE subtree(name) AS (
             SELECT name
             FROM mini_item_groups
             WHERE lower(name) = lower($1)
             UNION
             SELECT child.name
             FROM subtree parent_path
             JOIN mini_item_groups child
               ON lower(child.parent_item_group) = lower(parent_path.name)
         ),
         item_path(item_code, name, parent_item_group) AS (
             SELECT items.code, groups.name, COALESCE(groups.parent_item_group, '')
             FROM mini_items items
             JOIN subtree ON lower(subtree.name) = lower(items.item_group)
             JOIN mini_item_groups groups ON lower(groups.name) = lower(items.item_group)
             UNION
             SELECT path.item_code, parent.name, COALESCE(parent.parent_item_group, '')
             FROM item_path path
             JOIN mini_item_groups parent
               ON lower(parent.name) = lower(path.parent_item_group)
             WHERE path.parent_item_group <> ''
         )
         SELECT DISTINCT path.item_code
         FROM item_path path
         WHERE lower(btrim(path.name)) = lower($2)
           AND NOT EXISTS (
               SELECT 1
               FROM mini_customer_items assignments
               WHERE assignments.item_code = path.item_code
           )
         ORDER BY path.item_code",
    )
    .bind(root_group.trim())
    .bind(FINISHED_GOODS_GROUP)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|_| AdminPortError::LookupFailed)
}
