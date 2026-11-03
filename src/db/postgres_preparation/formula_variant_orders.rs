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
        if product.trim().is_empty()
            || product.trim().chars().count() > 160
            || material.trim().chars().count() > 128
        {
            return Err(PreparationError::Invalid(
                "Mahsulot yoki homashyo noto‘g‘ri",
            ));
        }
        let mut tx = self.pool.begin().await?;
        order_scope(&mut tx, owner, order, product, material).await?;
        let variants: Vec<Value> = sqlx::query_scalar(
            "SELECT jsonb_build_object(
                'id',v.id,'name',COALESCE(s.display_name,v.name),'formula_code',v.formula_code,
                'product_code',v.product_code,'material_id',v.material_id,'revision_id',r.id,
                'formula_count',counts.total,
                'shares_complete',counts.total>0 AND counts.total=counts.assigned AND counts.share=100)
             FROM mini_preparation_formula_variants v
             LEFT JOIN mini_preparation_order_formula_selections s
               ON s.owner_ref=v.owner_ref AND s.order_id=$4 AND s.variant_id=v.id
             LEFT JOIN mini_preparation_formula_revisions r ON r.id=s.revision_id
             CROSS JOIN LATERAL (
                SELECT count(*) AS total,count(share) AS assigned,sum(share) AS share
                FROM (
                    SELECT share_percent AS share FROM mini_preparation_formulas
                    WHERE r.id IS NULL AND variant_id=v.id
                    UNION ALL
                    SELECT (f->>'share_percent')::numeric FROM jsonb_array_elements(r.formulas) f
                ) shares
             ) counts
             WHERE v.owner_ref=$1 AND v.product_code=$2 AND v.material_id=$3
               AND NOT EXISTS (
                 SELECT 1 FROM mini_preparation_order_formula_exclusions x
                 WHERE x.owner_ref=v.owner_ref AND x.order_id=$4 AND x.variant_id=v.id)
             ORDER BY v.created_at,v.id",
        )
        .bind(owner)
        .bind(product.trim())
        .bind(material.trim())
        .bind(order.trim())
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(json!({"product_code":product.trim(),"material_id":material.trim(),"variants":variants}))
    }

    pub async fn select_formula_variant(
        &self,
        owner: &str,
        target: FormulaVariantTarget,
    ) -> Result<Value, PreparationError> {
        let order = target.order_id.trim();
        let variant = target.variant_id.trim();
        let mut tx = self.formula_write_transaction(owner).await?;
        order_recipe_scope(
            &mut tx,
            owner,
            order,
            &target.product_code,
            &target.material_id,
            variant,
        )
        .await?;
        let recipe = formula_revisions::pin_order_recipe(&mut tx, owner, order, variant).await?;
        if !target.recipe_key.is_empty() {
            let key: String = sqlx::query_scalar(
                "SELECT encode(content_key,'hex') FROM mini_preparation_formula_revisions WHERE id=$1",
            )
            .bind(recipe.revision)
            .fetch_one(&mut *tx)
            .await?;
            if key != target.recipe_key {
                return Err(PreparationError::Conflict(
                    "Shnek tarkibi o‘zgargan. Ro‘yxatni yangilang",
                ));
            }
        }
        let code = variant_code(&mut tx, variant).await?;
        tx.commit().await?;
        Ok(recipe.summary(variant, &code))
    }

    pub async fn rename_formula_variant(
        &self,
        owner: &str,
        target: FormulaVariantTarget,
        name: &str,
    ) -> Result<Value, PreparationError> {
        let name = normalize_variant_name(name)?;
        let order = target.order_id.trim();
        let product = target.product_code.trim();
        let material = target.material_id.trim();
        let variant = target.variant_id.trim();
        validate_target(product, material, variant)?;
        let mut tx = self.formula_write_transaction(owner).await?;
        if order.is_empty() {
            formula_variants::variant_scope(&mut tx, owner, product, material, variant, false)
                .await?;
        } else {
            order_recipe_scope(&mut tx, owner, order, product, material, variant).await?;
        }

        let duplicate: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                 SELECT 1 FROM mini_preparation_formula_variants v
                 LEFT JOIN mini_preparation_order_formula_selections s
                   ON s.owner_ref=v.owner_ref AND s.order_id=$5 AND s.variant_id=v.id
                 WHERE v.owner_ref=$1 AND v.product_code=$2 AND v.material_id=$3
                   AND v.id<>$4 AND lower(COALESCE(s.display_name,v.name))=lower($6)
                   AND NOT EXISTS (
                       SELECT 1 FROM mini_preparation_order_formula_exclusions x
                       WHERE x.owner_ref=v.owner_ref AND x.order_id=$5 AND x.variant_id=v.id)
             )",
        )
        .bind(owner)
        .bind(product)
        .bind(material)
        .bind(variant)
        .bind(order)
        .bind(&name)
        .fetch_one(&mut *tx)
        .await?;
        if duplicate {
            return Err(PreparationError::Conflict(
                "Bunday nomli shnek varianti mavjud",
            ));
        }

        let code = variant_code(&mut tx, variant).await?;
        let result = if order.is_empty() {
            sqlx::query(
                "UPDATE mini_preparation_formula_variants SET name=$3
                 WHERE id=$1 AND owner_ref=$2 AND name<>$3",
            )
            .bind(variant)
            .bind(owner)
            .bind(&name)
            .execute(&mut *tx)
            .await?;
            json!({"id":variant,"name":name,"formula_code":code})
        } else {
            let mut recipe =
                formula_revisions::pin_order_recipe(&mut tx, owner, order, variant).await?;
            sqlx::query(
                "UPDATE mini_preparation_order_formula_selections SET display_name=$4
                 WHERE owner_ref=$1 AND order_id=$2 AND variant_id=$3 AND display_name<>$4",
            )
            .bind(owner)
            .bind(order)
            .bind(variant)
            .bind(&name)
            .execute(&mut *tx)
            .await?;
            recipe.name = name;
            recipe.summary(variant, &code)
        };
        tx.commit().await?;
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

fn validate_target(product: &str, material: &str, variant: &str) -> Result<(), PreparationError> {
    if product.is_empty()
        || product.chars().count() > 160
        || material.chars().count() > 128
        || variant.is_empty()
        || variant.chars().count() > 160
    {
        return Err(PreparationError::Invalid("Variant ma’lumotlari noto‘g‘ri"));
    }
    Ok(())
}

pub(super) async fn order_recipe_scope(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    order: &str,
    product: &str,
    material: &str,
    variant: &str,
) -> Result<(), PreparationError> {
    validate_target(product.trim(), material.trim(), variant.trim())?;
    if material.trim().is_empty() {
        return Err(PreparationError::Invalid("Homashyo tanlanmadi"));
    }
    order_scope(tx, owner, order, product, material).await?;
    formula_variants::variant_scope(tx, owner, product.trim(), material.trim(), variant, false)
        .await?;
    require_visible_variant(tx, owner, order.trim(), variant.trim()).await
}

pub(super) async fn require_visible_variant(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    order: &str,
    variant: &str,
) -> Result<(), PreparationError> {
    let excluded: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM mini_preparation_order_formula_exclusions
         WHERE owner_ref=$1 AND order_id=$2 AND variant_id=$3)",
    )
    .bind(owner)
    .bind(order)
    .bind(variant)
    .fetch_one(&mut **tx)
    .await?;
    if excluded {
        return Err(PreparationError::Invalid(
            "Bu shnek formulasi buyurtmadan olib tashlangan",
        ));
    }
    Ok(())
}

async fn variant_code(
    tx: &mut Transaction<'_, Postgres>,
    variant: &str,
) -> Result<String, PreparationError> {
    Ok(
        sqlx::query_scalar(
            "SELECT formula_code FROM mini_preparation_formula_variants WHERE id=$1",
        )
        .bind(variant)
        .fetch_one(&mut **tx)
        .await?,
    )
}
