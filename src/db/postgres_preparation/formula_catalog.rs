use super::*;

impl PostgresPreparationStore {
    /// List recipes, including empty and historical variants, once per owner.
    pub async fn formula_catalog(&self, owner: &str) -> Result<Value, PreparationError> {
        let recipes: Vec<Value> = sqlx::query_scalar(
            "WITH recipe_scopes AS (
                SELECT product_code,material_id FROM mini_preparation_formula_variants
                WHERE owner_ref=$1 AND catalog_source_id IS NULL
                UNION
                SELECT product_code,material_id FROM mini_preparation_order_formula_selections
                WHERE owner_ref=$1
            ), matched_orders AS (
                SELECT DISTINCT m.id AS order_id, v.product_code, v.material_id,
                    COALESCE(NULLIF(btrim(m.code), ''), m.order_number) AS code
                FROM recipe_scopes v
                JOIN mini_production_maps m
                  ON COALESCE(NULLIF(m.map_json->>'product_code',''),m.product_code)=v.product_code
                JOIN mini_preparation_material_responsibilities r
                  ON r.principal_role='tayyorlov_masteri' AND r.principal_ref=$1
                 AND lower(r.material_id)=lower(v.material_id)
                WHERE m.id NOT LIKE 'template-%' AND (EXISTS (
                    SELECT 1 FROM mini_order_products p
                    LEFT JOIN LATERAL jsonb_array_elements(COALESCE(p.layers_json,'[]'::jsonb)) l ON true
                    WHERE p.order_id=m.id AND (
                        lower(l->>'material_id')=lower(v.material_id)
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
                        lower(l->>'material_id')=lower(v.material_id)
                        OR (btrim(COALESCE(l->>'material_id',''))='' AND lower(l->>'material')=lower(r.material_name))
                        OR lower(t.payload_json->>'first_layer_material')=lower(r.material_name)
                        OR lower(t.payload_json->>'second_layer_material')=lower(r.material_name)
                        OR lower(t.payload_json->>'third_layer_material')=lower(r.material_name)
                      )
                ))
            )
            SELECT jsonb_build_object(
                'id',v.id,'formula_code',v.formula_code,'name',v.name,
                'product_code',v.product_code,
                'product_title',public.mini_preparation_formula_set_default_name(v.product_code),
                'set_name',COALESCE(s.name,public.mini_preparation_formula_set_default_name(v.product_code)),
                'material_id',v.material_id,
                'can_create',EXISTS(SELECT 1 FROM mini_preparation_material_responsibilities r
                    WHERE r.principal_role='tayyorlov_masteri' AND r.principal_ref=v.owner_ref
                      AND lower(r.material_id)=lower(v.material_id)),
                'material_name',COALESCE(
                    (SELECT r.material_name FROM mini_preparation_material_responsibilities r
                     WHERE r.principal_role='tayyorlov_masteri' AND r.principal_ref=v.owner_ref
                       AND lower(r.material_id)=lower(v.material_id) ORDER BY r.material_id LIMIT 1),
                    (SELECT NULLIF(btrim(f.material_name),'') FROM mini_preparation_formulas f
                     WHERE f.variant_id=v.id AND btrim(f.material_name)<>'' ORDER BY f.updated_at DESC LIMIT 1),
                    (SELECT NULLIF(btrim(m.payload_json->>'name'),'') FROM mini_calculate_materials m
                     WHERE lower(m.id)=lower(v.material_id) LIMIT 1), v.material_id),
                'order_codes',COALESCE((SELECT jsonb_agg(code ORDER BY code) FROM (
                    SELECT m.code FROM matched_orders m
                    WHERE m.product_code=v.product_code AND lower(m.material_id)=lower(v.material_id)
                      AND btrim(m.code)<>''
                      AND NOT EXISTS(SELECT 1 FROM mini_preparation_order_formula_exclusions removed
                          WHERE removed.owner_ref=v.owner_ref AND removed.order_id=m.order_id
                            AND removed.variant_id=v.id AND removed.product_code=m.product_code
                            AND lower(removed.material_id)=lower(m.material_id))
                    UNION
                    SELECT m.code FROM matched_orders m
                    JOIN mini_preparation_order_formula_selections chosen
                      ON chosen.owner_ref=v.owner_ref AND chosen.order_id=m.order_id
                      AND chosen.product_code=m.product_code AND lower(chosen.material_id)=lower(m.material_id)
                    JOIN mini_preparation_formula_variants selected_variant ON selected_variant.id=chosen.variant_id
                    WHERE COALESCE(selected_variant.catalog_source_id, selected_variant.id)=v.id
                      AND btrim(m.code)<>''
                      AND NOT EXISTS(SELECT 1 FROM mini_preparation_order_formula_exclusions removed
                          WHERE removed.owner_ref=chosen.owner_ref AND removed.order_id=chosen.order_id
                            AND removed.variant_id=chosen.variant_id AND removed.product_code=chosen.product_code
                            AND removed.material_id=chosen.material_id)
                ) codes),'[]'::jsonb),
                'formula_count',f.formula_count,
                'shares_complete',f.formula_count>0 AND f.formula_count=f.share_count AND f.share_total=100
            )
            FROM mini_preparation_formula_variants v
            LEFT JOIN mini_preparation_formula_sets s ON s.owner_ref=v.owner_ref AND s.product_code=v.product_code
            LEFT JOIN LATERAL (
                SELECT count(*) AS formula_count, count(share_percent) AS share_count,
                       COALESCE(sum(share_percent),0) AS share_total
                FROM mini_preparation_formulas WHERE variant_id=v.id
            ) f ON true
            WHERE v.owner_ref=$1 AND v.catalog_source_id IS NULL
            ORDER BY v.created_at DESC,v.id",
        ).bind(owner).fetch_all(&self.pool).await?;
        Ok(json!({"formulas": recipes}))
    }
}
