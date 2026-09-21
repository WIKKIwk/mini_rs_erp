use super::*;

impl PostgresPreparationStore {
    /// Formulas retain their existing owner/product/material identity. An order
    /// is shown only when its current layers match a currently assigned scope.
    /// Saved formulas without such an order remain manageable under their product.
    pub async fn formula_orders(&self, owner: &str) -> Result<Value, PreparationError> {
        let orders: Vec<Value> = sqlx::query_scalar(
            "WITH matched AS (
                SELECT m.id, m.code, m.product_code, m.title, m.created_at,
                       f.material_id, r.material_name, f.name, f.lines
                FROM mini_production_maps m
                JOIN mini_preparation_formulas f ON f.product_code=m.product_code AND f.owner_ref=$1
                JOIN mini_preparation_material_responsibilities r
                  ON r.principal_role='tayyorlov_masteri' AND r.principal_ref=$1
                 AND lower(r.material_id)=lower(f.material_id)
                WHERE EXISTS (
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
                )
            ), entries AS (
                SELECT *, true AS has_order FROM matched
                UNION ALL
                SELECT 'saved:' || f.product_code, '', f.product_code,
                    COALESCE(
                        (SELECT NULLIF(btrim(i.name),'') FROM mini_items i WHERE i.code=f.product_code),
                        (SELECT NULLIF(btrim(t.product_name),'') FROM mini_quick_order_templates t
                         WHERE t.item_code=f.product_code ORDER BY t.saved_at DESC,t.id LIMIT 1),
                        f.product_code),
                    f.updated_at, f.material_id, f.material_name, f.name, f.lines, false
                FROM mini_preparation_formulas f
                WHERE f.owner_ref=$1 AND NOT EXISTS (
                    SELECT 1 FROM matched m WHERE m.product_code=f.product_code
                      AND m.material_id=f.material_id AND m.name=f.name
                )
            )
            SELECT jsonb_build_object('order_id',id,'order_code',code,'product_code',product_code,
                'has_order',has_order,
                'title',title,'formulas',jsonb_agg(jsonb_build_object(
                    'material_id',material_id,'material_name',material_name,'name',name,'lines',lines)
                    ORDER BY lower(material_name),material_id,lower(name),name))
            FROM entries GROUP BY id,code,product_code,title,has_order
            ORDER BY max(created_at) DESC,id",
        ).bind(owner).fetch_all(&self.pool).await?;
        Ok(json!({"orders":orders}))
    }
}
