use super::*;

impl PostgresPreparationStore {
    pub(super) async fn formula_write_transaction(
        &self,
        owner: &str,
    ) -> Result<Transaction<'_, Postgres>, PreparationError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '5s'")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("preparation:formula:{owner}"))
            .execute(&mut *tx)
            .await?;
        Ok(tx)
    }

    pub async fn update_formula_shares(
        &self,
        owner: &str,
        input: FormulaSharesUpdate,
        saved_only: bool,
    ) -> Result<Value, PreparationError> {
        self.update_formula_variant_shares(owner, input, saved_only, "").await
    }

    pub async fn update_formula_variant_shares(&self, owner: &str, input: FormulaSharesUpdate,
        saved_only: bool, variant_id: &str) -> Result<Value, PreparationError> {
        let shares = input.normalized()?;
        let code = input.product_code.trim();
        let material = input.material_id.trim();
        if !saved_only && material.is_empty() {
            return Err(PreparationError::Invalid("Homashyo tanlanmadi"));
        }
        let mut tx = self.formula_write_transaction(owner).await?;
        if !saved_only {
            responsibility_material_name(&mut tx, owner, material).await?;
        }
        let variant = super::formula_variants::variant_scope(&mut tx, owner, code, material, variant_id, false).await?;
        let current: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM mini_preparation_formulas
             WHERE owner_ref=$1 AND product_code=$2 AND material_id=$3 AND variant_id=$4 ORDER BY name FOR UPDATE",
        )
        .bind(owner)
        .bind(code)
        .bind(material)
        .bind(&variant)
        .fetch_all(&mut *tx)
        .await?;
        if current.is_empty()
            || current.iter().collect::<std::collections::BTreeSet<_>>()
                != shares
                    .iter()
                    .map(|(name, _)| name)
                    .collect::<std::collections::BTreeSet<_>>()
        {
            return Err(PreparationError::Conflict(
                "Shnek formulalari o‘zgargan. Ro‘yxatni yangilang",
            ));
        }
        for (name, percent) in shares {
            sqlx::query("UPDATE mini_preparation_formulas SET share_percent=$5::text::numeric,updated_at=now()
                         WHERE owner_ref=$1 AND product_code=$2 AND material_id=$3 AND name=$4 AND variant_id=$6")
                .bind(owner).bind(code).bind(material).bind(name).bind(decimal_text(percent)).bind(&variant)
                .execute(&mut *tx).await?;
        }
        let formulas: Vec<Value> = sqlx::query_scalar(
            "SELECT jsonb_build_object('formula_code',formula_code,'name',name,'lines',lines,'share_percent',share_percent::text,'variant_id',variant_id)
             FROM mini_preparation_formulas WHERE owner_ref=$1 AND product_code=$2 AND material_id=$3 AND variant_id=$4
             ORDER BY lower(name),name",
        ).bind(owner).bind(code).bind(material).bind(&variant).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(json!({"product_code":code,"material_id":material,"formulas":formulas}))
    }
}
