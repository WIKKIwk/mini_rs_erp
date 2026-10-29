use super::*;

pub(super) async fn order_scope(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    order: &str,
    product: &str,
    material: &str,
) -> Result<(), PreparationError> {
    if order.trim().is_empty() || order.chars().count() > 512 {
        return Err(PreparationError::Invalid("Buyurtma tanlanmagan"));
    }
    let matches: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM mini_production_maps WHERE id=$1
         AND COALESCE(NULLIF(map_json->>'product_code',''),product_code)=$2)",
    )
    .bind(order.trim())
    .bind(product.trim())
    .fetch_one(&mut **tx)
    .await?;
    if !matches
        || !order_matches_material_responsibility(tx, owner, order.trim(), material.trim()).await?
    {
        return Err(PreparationError::Forbidden);
    }
    Ok(())
}

pub(super) async fn restore(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    order: &str,
    variant: &str,
) -> Result<(), PreparationError> {
    if !order.trim().is_empty() {
        sqlx::query(
            "DELETE FROM mini_preparation_order_formula_exclusions
                     WHERE owner_ref=$1 AND order_id=$2 AND variant_id=$3",
        )
        .bind(owner)
        .bind(order.trim())
        .bind(variant)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

impl PostgresPreparationStore {
    pub async fn formula_variants_in_order(
        &self,
        owner: &str,
        product: &str,
        material: &str,
        saved_only: bool,
        order: &str,
    ) -> Result<Value, PreparationError> {
        if order.trim().is_empty() {
            return self
                .formula_variants(owner, product, material, saved_only)
                .await;
        }
        let mut tx = self.pool.begin().await?;
        order_scope(&mut tx, owner, order, product, material).await?;
        let excluded: Vec<String> = sqlx::query_scalar(
            "SELECT variant_id FROM mini_preparation_order_formula_exclusions
             WHERE owner_ref=$1 AND order_id=$2",
        )
        .bind(owner)
        .bind(order.trim())
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        let mut result = self
            .formula_variants(owner, product, material, saved_only)
            .await?;
        if let Some(variants) = result["variants"].as_array_mut() {
            variants.retain(|variant| {
                !excluded
                    .iter()
                    .any(|id| variant["id"].as_str() == Some(id.as_str()))
            });
        }
        Ok(result)
    }

    pub async fn remove_formula_variant_from_order(
        &self,
        owner: &str,
        input: FormulaVariantOrderRemove,
    ) -> Result<Value, PreparationError> {
        FormulaVariantReuse {
            product_code: input.product_code.clone(),
            material_id: input.material_id.clone(),
            source_variant_id: input.variant_id.clone(),
        }
        .validate()?;
        let mut tx = self.formula_write_transaction(owner).await?;
        order_scope(
            &mut tx,
            owner,
            &input.order_id,
            &input.product_code,
            &input.material_id,
        )
        .await?;
        let variant = formula_variants::variant_scope(
            &mut tx,
            owner,
            input.product_code.trim(),
            input.material_id.trim(),
            &input.variant_id,
            false,
        )
        .await?;
        sqlx::query(
            "INSERT INTO mini_preparation_order_formula_exclusions
            (owner_ref,order_id,variant_id,product_code,material_id) VALUES($1,$2,$3,$4,$5)
            ON CONFLICT(owner_ref,order_id,variant_id) DO NOTHING",
        )
        .bind(owner)
        .bind(input.order_id.trim())
        .bind(&variant)
        .bind(input.product_code.trim())
        .bind(input.material_id.trim())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(json!({"removed":true,"order_id":input.order_id.trim(),"variant_id":variant}))
    }
}
