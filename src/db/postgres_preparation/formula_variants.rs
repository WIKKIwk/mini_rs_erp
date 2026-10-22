use super::*;

pub(super) async fn variant_scope(
    tx: &mut Transaction<'_, Postgres>,
    owner: &str,
    code: &str,
    material: &str,
    variant_id: &str,
    create_default: bool,
) -> Result<String, PreparationError> {
    let variant = variant_id.trim();
    if variant.is_empty() && create_default {
        return sqlx::query_scalar(
            "SELECT public.mini_preparation_default_formula_variant($1,$2,$3)",
        )
        .bind(owner)
        .bind(code)
        .bind(material)
        .fetch_one(&mut **tx)
        .await
        .map_err(PreparationError::from);
    }
    sqlx::query_scalar(
        "SELECT id FROM mini_preparation_formula_variants WHERE owner_ref=$1 AND product_code=$2
         AND material_id=$3 AND ((id=$4 AND $4<>'') OR ($4='' AND is_default))",
    )
    .bind(owner)
    .bind(code)
    .bind(material)
    .bind(variant)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(PreparationError::Invalid(
        "Shnek varianti topilmadi. Ro‘yxatni yangilang",
    ))
}

impl PostgresPreparationStore {
    pub async fn formula_variants(
        &self,
        owner: &str,
        product: &str,
        material: &str,
        saved_only: bool,
    ) -> Result<Value, PreparationError> {
        let code = product.trim();
        let material = material.trim();
        if code.is_empty() || code.chars().count() > 160 || material.chars().count() > 128 {
            return Err(PreparationError::Invalid(
                "Mahsulot yoki homashyo noto‘g‘ri",
            ));
        }
        if !saved_only {
            responsibility_material_name_tx(&self.pool, owner, material)
                .await?
                .ok_or(PreparationError::Forbidden)?;
        }
        let variants: Vec<Value> = sqlx::query_scalar(
            "SELECT jsonb_build_object('id',v.id,'name',v.name,'product_code',v.product_code,
                'material_id',v.material_id,'formula_count',count(f.name),
                'shares_complete',count(f.name)>0 AND count(f.name)=count(f.share_percent)
                    AND COALESCE(sum(f.share_percent),0)=100)
             FROM mini_preparation_formula_variants v LEFT JOIN mini_preparation_formulas f ON f.variant_id=v.id
             WHERE v.owner_ref=$1 AND v.product_code=$2 AND v.material_id=$3
             GROUP BY v.id ORDER BY v.created_at,v.id",
        ).bind(owner).bind(code).bind(material).fetch_all(&self.pool).await?;
        Ok(json!({"product_code":code,"material_id":material,"variants":variants}))
    }

    pub async fn create_formula_variant(
        &self,
        owner: &str,
        input: FormulaVariantCreate,
    ) -> Result<Value, PreparationError> {
        let name = input.normalized_name()?;
        let code = input.product_code.trim();
        let material = input.material_id.trim();
        let request = input.request_id.trim();
        let mut tx = self.formula_write_transaction(owner).await?;
        responsibility_material_name(&mut tx, owner, material).await?;
        let previous: Option<(String,String,String,String)> = sqlx::query_as(
            "SELECT id,product_code,material_id,name FROM mini_preparation_formula_variants WHERE owner_ref=$1 AND request_id=$2",
        ).bind(owner).bind(request).fetch_optional(&mut *tx).await?;
        let id = if let Some((id, previous_code, previous_material, previous_name)) = previous {
            if previous_code != code || previous_material != material || previous_name != name {
                return Err(PreparationError::Conflict(
                    "Variant yaratish so‘rovi boshqa ma’lumot bilan yuborilgan",
                ));
            }
            id
        } else {
            let duplicate: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mini_preparation_formula_variants
                WHERE owner_ref=$1 AND product_code=$2 AND material_id=$3 AND lower(name)=lower($4))")
                .bind(owner).bind(code).bind(material).bind(&name).fetch_one(&mut *tx).await?;
            if duplicate {
                return Err(PreparationError::Conflict(
                    "Bunday nomli shnek varianti mavjud",
                ));
            }
            sqlx::query_scalar("INSERT INTO mini_preparation_formula_variants(owner_ref,product_code,material_id,name,request_id,is_default)
                VALUES ($1,$2,$3,$4,$5,NOT EXISTS(SELECT 1 FROM mini_preparation_formula_variants
                    WHERE owner_ref=$1 AND product_code=$2 AND material_id=$3 AND is_default)) RETURNING id")
                .bind(owner).bind(code).bind(material).bind(&name).bind(request).fetch_one(&mut *tx).await?
        };
        tx.commit().await?;
        Ok(json!({"id":id,"name":name,"product_code":code,"material_id":material}))
    }
}
