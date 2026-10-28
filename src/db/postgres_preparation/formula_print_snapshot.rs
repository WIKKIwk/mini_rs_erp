use super::*;

impl PostgresPreparationStore {
    /// Freeze the complete screw recipe once per batch. Receipts retain this
    /// value even if the master's saved formulas are subsequently edited.
    pub async fn formula_print_snapshot(
        &self,
        owner: &str,
        order_id: &str,
        item_code: &str,
        reference: &PreparationFormulaReference,
    ) -> Result<Value, PreparationError> {
        if order_id.trim().is_empty() || reference.material_id.trim().is_empty() {
            return Err(PreparationError::Invalid(
                "Buyurtma yoki shnek formulasi tanlanmagan",
            ));
        }
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let material_name =
            responsibility_material_name(&mut tx, owner, &reference.material_id).await?;
        if !order_matches_material_responsibility(&mut tx, owner, order_id, &reference.material_id)
            .await?
        {
            return Err(PreparationError::Forbidden);
        }
        let product: String = sqlx::query_scalar(
            "SELECT COALESCE(NULLIF(map_json->>'product_code',''),product_code)
             FROM mini_production_maps WHERE id=$1",
        )
        .bind(order_id.trim())
        .fetch_optional(&mut *tx)
        .await?
        .filter(|code: &String| !code.trim().is_empty())
        .ok_or(PreparationError::Forbidden)?;
        let item_matches: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM mini_items WHERE lower(code)=lower($1)
             AND (lower(btrim(code)) IN (lower(btrim($2)),lower(btrim($3)))
               OR lower(btrim(name)) IN (lower(btrim($2)),lower(btrim($3)))))",
        )
        .bind(item_code.trim())
        .bind(reference.material_id.trim())
        .bind(&material_name)
        .fetch_one(&mut *tx)
        .await?;
        if !item_matches {
            return Err(PreparationError::Forbidden);
        }
        let variant = formula_variants::variant_scope(
            &mut tx,
            owner,
            &product,
            reference.material_id.trim(),
            &reference.variant_id,
            false,
        )
        .await?;
        let snapshot: Value = sqlx::query_scalar(
            "SELECT jsonb_build_object('schema_version',1,'owner_ref',v.owner_ref,
                'product_code',v.product_code,'material_id',v.material_id,'material_name',$2::text,
                'variant_id',v.id,'variant_name',v.name,'captured_at',transaction_timestamp(),
                'formulas',COALESCE((SELECT jsonb_agg(jsonb_build_object(
                    'set_id',f.set_id,'name',f.name,'share_percent',f.share_percent::text,
                    'lines',f.lines,'updated_at',f.updated_at) ORDER BY lower(f.name),f.name)
                    FROM mini_preparation_formulas f WHERE f.variant_id=v.id),'[]'::jsonb))
             FROM mini_preparation_formula_variants v WHERE v.id=$1",
        )
        .bind(&variant)
        .bind(&material_name)
        .fetch_one(&mut *tx)
        .await?;
        validate_print_recipe(&snapshot)?;
        tx.commit().await?;
        Ok(snapshot)
    }
}

fn validate_print_recipe(snapshot: &Value) -> Result<(), PreparationError> {
    let formulas = snapshot["formulas"]
        .as_array()
        .ok_or(PreparationError::Invalid("Shnek formulasi topilmadi"))?;
    let mut shares = Vec::new();
    for formula in formulas {
        let name = formula["name"].as_str().unwrap_or_default().to_owned();
        let lines = formula["lines"]
            .as_array()
            .ok_or(PreparationError::Invalid("Shnek tarkibi kiritilmagan"))?
            .iter()
            .map(|line| FormulaLine {
                item_code: line["item_code"].as_str().unwrap_or_default().to_owned(),
                percent: line["percent"].as_str().unwrap_or_default().to_owned(),
            })
            .collect();
        FormulaUpsert {
            product_code: snapshot["product_code"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            material_id: snapshot["material_id"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            name: name.clone(),
            lines,
        }
        .normalized_lines()?;
        shares.push(FormulaShare {
            name,
            percent: formula["share_percent"]
                .as_str()
                .ok_or(PreparationError::Invalid(
                    "Avval shnek ulushlarini belgilang",
                ))?
                .to_owned(),
        });
    }
    FormulaSharesUpdate {
        product_code: snapshot["product_code"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        material_id: snapshot["material_id"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        shares,
    }
    .normalized()?;
    Ok(())
}
