use super::*;

impl PostgresPreparationStore {
    pub async fn formula_sets(&self, owner: &str) -> Result<Value, PreparationError> {
        let sets: Vec<Value> = sqlx::query_scalar(
            "WITH matched_orders AS (
                SELECT DISTINCT m.id, COALESCE(NULLIF(btrim(m.code), ''), m.order_number) AS code,
                       m.product_code
                FROM mini_production_maps m
                JOIN mini_preparation_formulas f ON f.product_code=m.product_code AND f.owner_ref=$1
                JOIN mini_preparation_material_responsibilities r
                  ON r.principal_role='tayyorlov_masteri' AND r.principal_ref=$1
                 AND lower(r.material_id)=lower(f.material_id)
                WHERE m.id NOT LIKE 'template-%' AND (EXISTS (
                    SELECT 1 FROM mini_order_products p
                    LEFT JOIN LATERAL jsonb_array_elements(COALESCE(p.layers_json,'[]'::jsonb)) l ON true
                    WHERE p.order_id=m.id AND (
                        lower(l->>'material_id')=lower(f.material_id)
                        OR (btrim(COALESCE(l->>'material_id',''))='' AND lower(l->>'material')=lower(r.material_name))
                        OR lower(p.first_layer_material)=lower(r.material_name)
                        OR lower(p.second_layer_material)=lower(r.material_name)
                        OR lower(p.third_layer_material)=lower(r.material_name)
                    )
                ) OR EXISTS (
                    SELECT 1 FROM mini_quick_order_templates t
                    LEFT JOIN LATERAL jsonb_array_elements(COALESCE(t.payload_json->'layers','[]'::jsonb)) l ON true
                    WHERE btrim(COALESCE(t.payload_json->>'source_map_id',''))=m.id
                      AND NOT EXISTS(SELECT 1 FROM mini_order_products p WHERE p.order_id=m.id)
                      AND (
                        lower(l->>'material_id')=lower(f.material_id)
                        OR (btrim(COALESCE(l->>'material_id',''))='' AND lower(l->>'material')=lower(r.material_name))
                        OR lower(t.payload_json->>'first_layer_material')=lower(r.material_name)
                        OR lower(t.payload_json->>'second_layer_material')=lower(r.material_name)
                        OR lower(t.payload_json->>'third_layer_material')=lower(r.material_name)
                      )
                ))
            )
            SELECT jsonb_build_object(
                'set_id',s.set_id,'name',s.name,'product_code',s.product_code,
                'product_title',public.mini_preparation_formula_set_default_name(s.product_code),
                'order_codes',COALESCE((SELECT jsonb_agg(code ORDER BY code) FROM (
                    SELECT DISTINCT m.code FROM matched_orders m
                    WHERE m.product_code=s.product_code AND btrim(m.code) <> ''
                ) codes),'[]'::jsonb),
                'formulas',jsonb_agg(jsonb_build_object(
                    'material_id',f.material_id,'material_name',COALESCE(r.material_name,f.material_name),
                    'name',f.name,'lines',f.lines,'share_percent',f.share_percent::text,
                    'variant_id',f.variant_id,'variant_name',v.name)
                    ORDER BY lower(COALESCE(r.material_name,f.material_name)),f.material_id,lower(f.name),f.name))
            FROM mini_preparation_formula_sets s
            JOIN mini_preparation_formulas f ON f.set_id=s.set_id
            JOIN mini_preparation_formula_variants v ON v.id=f.variant_id
            LEFT JOIN mini_preparation_material_responsibilities r
              ON r.principal_role='tayyorlov_masteri' AND r.principal_ref=s.owner_ref
             AND lower(r.material_id)=lower(f.material_id)
            WHERE s.owner_ref=$1
            GROUP BY s.set_id
            ORDER BY greatest(s.updated_at,max(f.updated_at)) DESC,s.set_id",
        )
        .bind(owner)
        .fetch_all(&self.pool)
        .await?;
        Ok(json!({"sets":sets}))
    }

    pub async fn rename_formula_set(
        &self,
        owner: &str,
        input: FormulaSetRename,
    ) -> Result<Value, PreparationError> {
        let (id, name) = input.normalized()?;
        sqlx::query_scalar(
            "UPDATE mini_preparation_formula_sets SET name=$3,updated_at=now()
             WHERE set_id=$1 AND owner_ref=$2
             RETURNING jsonb_build_object('set_id',set_id,'name',name)",
        )
        .bind(id)
        .bind(owner)
        .bind(name)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(PreparationError::Invalid(
            "Formula komplekti topilmadi. Ro‘yxatni yangilang",
        ))
    }
}
