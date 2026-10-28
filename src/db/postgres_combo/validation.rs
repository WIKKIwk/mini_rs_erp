use super::*;

pub(super) async fn lock_catalog_policy(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), ComboError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('mini_item_customer_policy', 0))")
        .execute(&mut **tx)
        .await
        .map_err(store_error)?;
    Ok(())
}

pub(super) async fn reject_combo_sku(
    tx: &mut Transaction<'_, Postgres>,
    item_code: &str,
) -> Result<(), ComboError> {
    let is_combo: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM mini_combo_revisions WHERE lower(item_code)=lower($1))",
    )
    .bind(item_code.trim())
    .fetch_one(&mut **tx)
    .await
    .map_err(store_error)?;
    if is_combo {
        return Err(ComboError::InvalidInput(
            "combo_reference_required_or_nested_combo".into(),
        ));
    }
    Ok(())
}

pub(super) async fn components(
    tx: &mut Transaction<'_, Postgres>,
    template: &CalculateOrderTemplate,
) -> Result<(), ComboError> {
    let customer_exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM mini_customers WHERE ref=$1)")
            .bind(template.customer_ref.trim())
            .fetch_one(&mut **tx)
            .await
            .map_err(store_error)?;
    if !customer_exists {
        return Err(ComboError::InvalidInput("combo_customer_not_found".into()));
    }
    let mut checked = std::collections::BTreeSet::new();
    for product in &template.products {
        let code = product.item_code.trim();
        if !checked.insert(code) {
            continue;
        }
        reject_combo_sku(tx, code).await?;
        let row = sqlx::query_as::<_, (String, serde_json::Value)>(
            "SELECT item_group,payload_json FROM mini_items WHERE code=$1 FOR SHARE",
        )
        .bind(code)
        .fetch_optional(&mut **tx)
        .await
        .map_err(store_error)?
        .ok_or_else(|| ComboError::InvalidInput("combo_component_not_found".into()))?;
        if row
            .0
            .eq_ignore_ascii_case(crate::core::combo::COMBO_ITEM_GROUP)
            || row.1.get("kind").and_then(serde_json::Value::as_str) == Some("combo")
        {
            return Err(ComboError::InvalidInput("nested_combo_not_allowed".into()));
        }
        let eligible: bool = sqlx::query_scalar(
            "WITH RECURSIVE path(name,parent_item_group) AS (
                 SELECT name,parent_item_group FROM mini_item_groups WHERE name=$1
                 UNION SELECT parent.name,parent.parent_item_group FROM path
                 JOIN mini_item_groups parent ON parent.name=path.parent_item_group
             ) SELECT EXISTS(SELECT 1 FROM path WHERE lower(btrim(name))='tayyor mahsulot')
                 AND EXISTS(SELECT 1 FROM mini_customer_items WHERE customer_ref=$2 AND item_code=$3)",
        ).bind(&row.0).bind(template.customer_ref.trim()).bind(code)
            .fetch_one(&mut **tx).await.map_err(store_error)?;
        if !eligible {
            return Err(ComboError::InvalidInput(
                "combo_component_customer_mismatch".into(),
            ));
        }
    }
    Ok(())
}
