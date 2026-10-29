use super::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(serde::Deserialize)]
struct StoredFormulaLine {
    item_code: String,
    percent: String,
}

impl PostgresPreparationStore {
    pub async fn available_formula_variants(
        &self,
        owner: &str,
        product: &str,
        material: &str,
        order_id: &str,
    ) -> Result<Value, PreparationError> {
        if order_id.trim().is_empty() {
            return Err(PreparationError::Invalid("Buyurtma tanlanmagan"));
        }
        let linked = self.formula_variants_in_order(owner, product, material, false, order_id).await?;
        let linked: BTreeSet<String> = linked["variants"].as_array().into_iter().flatten()
            .filter_map(|entry| entry["id"].as_str().map(str::to_owned)).collect();
        let copies: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, request_id FROM mini_preparation_formula_variants
             WHERE owner_ref=$1 AND product_code=$2 AND material_id=$3",
        ).bind(owner).bind(product.trim()).bind(material.trim()).fetch_all(&self.pool).await?;
        let copies: BTreeMap<String, String> = copies.into_iter()
            .filter_map(|(id, request)| request.map(|request| (request, id))).collect();
        let mut catalog = self.formula_catalog(owner).await?;
        if let Some(entries) = catalog["formulas"].as_array_mut() {
            entries.retain(|entry| {
                let id = entry["id"].as_str().unwrap_or_default();
                if entry["product_code"].as_str() == Some(product.trim())
                    && entry["material_id"].as_str() == Some(material.trim()) {
                    return !linked.contains(id);
                }
                let key = reuse_request_id(product.trim(), material.trim(), id);
                !copies.get(&key).is_some_and(|target| linked.contains(target))
            });
        }
        Ok(catalog)
    }

    pub async fn reuse_formula_variant(
        &self,
        owner: &str,
        input: FormulaVariantReuse,
    ) -> Result<Value, PreparationError> {
        self.reuse_formula_variant_in_order(owner, input, "").await
    }

    pub async fn reuse_formula_variant_in_order(
        &self,
        owner: &str,
        input: FormulaVariantReuse,
        order_id: &str,
    ) -> Result<Value, PreparationError> {
        input.validate()?;
        let product = input.product_code.trim();
        let material = input.material_id.trim();
        let source = input.source_variant_id.trim();
        let mut tx = self.formula_write_transaction(owner).await?;
        let target_material = responsibility_material_name(&mut tx, owner, material).await?;
        if !order_id.trim().is_empty() {
            formula_variant_orders::order_scope(&mut tx, owner, order_id, product, material).await?;
        }
        let (source_product, source_material, source_name): (String, String, String) =
            sqlx::query_as(
                "SELECT product_code, material_id, name FROM mini_preparation_formula_variants
                 WHERE id=$1 AND owner_ref=$2",
            )
            .bind(source)
            .bind(owner)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(PreparationError::Forbidden)?;
        if source_product == product && source_material == material {
            formula_variant_orders::restore(&mut tx, owner, order_id, source).await?;
            let result = variant_summary(&mut tx, owner, source).await?;
            tx.commit().await?;
            return Ok(result);
        }

        // Re-selecting a recipe, concurrent taps and a lost response all refer
        // to the same copy. Keep this identity in the existing request registry.
        let request_id = reuse_request_id(product, material, source);
        let previous: Option<String> = sqlx::query_scalar(
            "SELECT id FROM mini_preparation_formula_variants
             WHERE owner_ref=$1 AND request_id=$2 AND product_code=$3 AND material_id=$4",
        )
        .bind(owner)
        .bind(&request_id)
        .bind(product)
        .bind(material)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(id) = previous {
            formula_variant_orders::restore(&mut tx, owner, order_id, &id).await?;
            let result = variant_summary(&mut tx, owner, &id).await?;
            tx.commit().await?;
            return Ok(result);
        }

        let source_formulas: Vec<(String, Value, Option<String>)> = sqlx::query_as(
            "SELECT name, lines, share_percent::text FROM mini_preparation_formulas
             WHERE owner_ref=$1 AND variant_id=$2 ORDER BY name",
        )
        .bind(owner)
        .bind(source)
        .fetch_all(&mut *tx)
        .await?;
        let mut formulas = Vec::new();
        let mut ingredients = BTreeSet::new();
        for (name, lines, share) in source_formulas {
            let lines = serde_json::from_value::<Vec<StoredFormulaLine>>(lines)
                .map_err(|_| PreparationError::Invalid("Formula tarkibi noto‘g‘ri"))?
                .into_iter()
                .map(|line| FormulaLine {
                    item_code: line.item_code,
                    percent: line.percent,
                })
                .collect();
            let lines = FormulaUpsert {
                product_code: product.into(),
                material_id: material.into(),
                name: name.clone(),
                lines,
            }
            .normalized_lines()?;
            ingredients.extend(lines.iter().map(|(code, _)| code.clone()));
            formulas.push((name, lines, share));
        }
        let mut names = BTreeMap::new();
        for code in ingredients {
            names.insert(code.clone(), material_name(&mut tx, owner, &code).await?);
        }

        let mut name = source_name.clone();
        let mut suffix_number = 2;
        while sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM mini_preparation_formula_variants
             WHERE owner_ref=$1 AND product_code=$2 AND material_id=$3 AND lower(name)=lower($4))",
        )
        .bind(owner)
        .bind(product)
        .bind(material)
        .bind(&name)
        .fetch_one(&mut *tx)
        .await?
        {
            let suffix = format!(" ({suffix_number})");
            name = format!(
                "{}{}",
                source_name
                    .chars()
                    .take(160 - suffix.len())
                    .collect::<String>(),
                suffix
            );
            suffix_number += 1;
        }
        let id: String = sqlx::query_scalar(
            "INSERT INTO mini_preparation_formula_variants(owner_ref,product_code,material_id,name,request_id,is_default)
             VALUES ($1,$2,$3,$4,$5,NOT EXISTS(SELECT 1 FROM mini_preparation_formula_variants
                 WHERE owner_ref=$1 AND product_code=$2 AND material_id=$3 AND is_default)) RETURNING id",
        ).bind(owner).bind(product).bind(material).bind(name).bind(request_id)
            .fetch_one(&mut *tx).await?;
        for (name, lines, share) in formulas {
            let mut lines: Vec<_> = lines
                .into_iter()
                .map(|(code, percent)| {
                    let name = names[&code].clone();
                    (code, name, percent)
                })
                .collect();
            lines.sort_by(|a, b| {
                a.1.to_lowercase()
                    .cmp(&b.1.to_lowercase())
                    .then_with(|| a.0.cmp(&b.0))
            });
            let lines: Vec<Value> = lines.into_iter().map(|(code, name, percent)|
                json!({"item_code":code,"name":name,"percent":decimal_text(percent)})
            ).collect();
            sqlx::query(
                "INSERT INTO mini_preparation_formulas(owner_ref,product_code,material_id,material_name,variant_id,name,lines,share_percent)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8::text::numeric)",
            ).bind(owner).bind(product).bind(material).bind(&target_material).bind(&id)
                .bind(name).bind(json!(lines)).bind(share).execute(&mut *tx).await?;
        }
        let result = variant_summary(&mut tx, owner, &id).await?;
        tx.commit().await?;
        Ok(result)
    }
}

fn reuse_request_id(product: &str, material: &str, source: &str) -> String {
    format!("reuse:{:x}", Sha256::digest(json!([product, material, source]).to_string()))
}

async fn variant_summary(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    id: &str,
) -> Result<Value, PreparationError> {
    Ok(sqlx::query_scalar(
        "SELECT jsonb_build_object('id',v.id,'name',v.name,'formula_code',v.formula_code,
             'product_code',v.product_code,'material_id',v.material_id,'formula_count',count(f.name),
             'shares_complete',count(f.name)>0 AND count(f.name)=count(f.share_percent)
                 AND COALESCE(sum(f.share_percent),0)=100)
         FROM mini_preparation_formula_variants v LEFT JOIN mini_preparation_formulas f ON f.variant_id=v.id
         WHERE v.id=$1 AND v.owner_ref=$2 GROUP BY v.id",
    ).bind(id).bind(owner).fetch_one(&mut **tx).await?)
}
