use super::*;

// Historical formulas can outlive orders and material responsibilities. Their
// owner may manage the exact saved identity, but cannot create or rebind one
// through these endpoints. In particular, an empty legacy scope is not a wildcard.
fn saved_scope<'a>(
    product: &'a str,
    material: &'a str,
) -> Result<(&'a str, &'a str), PreparationError> {
    let code = product.trim();
    let material = material.trim();
    if code.is_empty() || code.chars().count() > 160 {
        return Err(PreparationError::Invalid("Mahsulot kodi noto‘g‘ri"));
    }
    if material.chars().count() > 128 {
        return Err(PreparationError::Invalid("Homashyo turi noto‘g‘ri"));
    }
    Ok((code, material))
}

impl PostgresPreparationStore {
    pub async fn list_saved_formulas(
        &self,
        owner: &str,
        product_code: &str,
        material_id: &str,
    ) -> Result<Value, PreparationError> {
        let (code, material) = saved_scope(product_code, material_id)?;
        let formulas: Vec<Value> = sqlx::query_scalar(
            "SELECT jsonb_build_object('name',name,'lines',lines)
             FROM mini_preparation_formulas
             WHERE owner_ref=$1 AND product_code=$2 AND material_id=$3
             ORDER BY lower(name),name",
        )
        .bind(owner)
        .bind(code)
        .bind(material)
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({"product_code":code,"material_id":material,"formulas":formulas}))
    }

    pub async fn update_saved_formula(
        &self,
        actor: &Principal,
        input: FormulaUpsert,
    ) -> Result<Value, PreparationError> {
        let (code, material) = saved_scope(&input.product_code, &input.material_id)?;
        let name = input.formula_name()?;
        let normalized = input.normalized_lines()?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '5s'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("preparation:formula:{}", actor.ref_))
            .execute(&mut *tx)
            .await?;
        let material_name_saved: String = sqlx::query_scalar(
            "SELECT material_name FROM mini_preparation_formulas
             WHERE owner_ref=$1 AND product_code=$2 AND material_id=$3 AND name=$4",
        )
        .bind(&actor.ref_)
        .bind(code)
        .bind(material)
        .bind(&name)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(PreparationError::Invalid(
            "Formula topilmadi. Ro‘yxatni yangilang",
        ))?;
        let mut lines = Vec::new();
        for (item_code, percent) in normalized {
            let item_name = material_name(&mut tx, &actor.ref_, &item_code).await?;
            lines.push((item_code, item_name, percent));
        }
        lines.sort_by(|a, b| {
            a.1.to_lowercase()
                .cmp(&b.1.to_lowercase())
                .then_with(|| a.0.cmp(&b.0))
        });
        let payload: Vec<Value> = lines.iter().map(|(item_code, item_name, percent)|
            json!({"item_code":item_code,"name":item_name,"percent":decimal_text(*percent)})
        ).collect();
        // Lock ingredient rows before the formula, like catalog rename/delete.
        // UPDATE-only plus the affected-row check prevents resurrecting a formula
        // that was deleted while its ingredients were being resolved.
        let updated = sqlx::query(
            "UPDATE mini_preparation_formulas SET lines=$5,updated_at=now()
            WHERE owner_ref=$1 AND product_code=$2 AND material_id=$3 AND name=$4",
        )
        .bind(&actor.ref_)
        .bind(code)
        .bind(material)
        .bind(&name)
        .bind(json!(payload))
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(PreparationError::Invalid(
                "Formula topilmadi. Ro‘yxatni yangilang",
            ));
        }
        tx.commit().await?;
        Ok(
            json!({"product_code":code,"material_id":material,"material_name":material_name_saved,
            "name":name,"lines":payload}),
        )
    }

    pub async fn delete_saved_formula(
        &self,
        owner: &str,
        product_code: &str,
        name: &str,
        material_id: &str,
    ) -> Result<Value, PreparationError> {
        let (code, material) = saved_scope(product_code, material_id)?;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(PreparationError::Invalid(
                "Formula nomi 1–80 ta belgidan iborat bo‘lishi kerak",
            ));
        }
        let deleted = sqlx::query(
            "DELETE FROM mini_preparation_formulas
            WHERE owner_ref=$1 AND product_code=$2 AND material_id=$3 AND name=$4",
        )
        .bind(owner)
        .bind(code)
        .bind(material)
        .bind(name)
        .execute(&self.pool)
        .await?;
        if deleted.rows_affected() != 1 {
            return Err(PreparationError::Invalid(
                "Formula topilmadi. Ro‘yxatni yangilang",
            ));
        }
        Ok(json!({"product_code":code,"material_id":material,"name":name,"deleted":true}))
    }
}
