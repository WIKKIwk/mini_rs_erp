//! Small read catalogue for the preparation order list. No inventory, operation
//! history, compiled programs or full map payloads are read on this path.
use super::*;
use crate::core::production_map::ProductionMapDefinition;

pub(crate) struct PreparationOrderCandidate {
    pub map: ProductionMapDefinition,
    pub allowed: bool,
    /// Executed/cooperative orders need the existing scoped visibility rules.
    pub needs_authority: bool,
}

pub(crate) struct PreparationOrderCatalog {
    pub responsibilities: Value,
    pub sequence: Vec<String>,
    pub candidates: Vec<PreparationOrderCandidate>,
}

impl PostgresPreparationStore {
    /// Tuple identities detect committed label/material edits without reading
    /// or decoding map/template JSON. Production authority is separately bound
    /// to its live revision. Include every template because legacy customer
    /// fallbacks can match a product/width from outside this apparatus.
    pub(crate) async fn order_list_metadata_stamp(
        &self,
        apparatus: &str,
    ) -> Result<String, PreparationError> {
        Ok(sqlx::query_scalar(
            "WITH candidates AS MATERIALIZED (
                SELECT m.id,m.xmin::text AS version,m.ctid::text AS location
                FROM mini_production_maps m
                WHERE m.id LIKE 'zakaz-%' AND m.lifecycle_status IN ('released','in_progress')
                  AND m.id IN (SELECT map_id FROM mini_production_map_nodes
                    WHERE canonical_apparatus_id=$1 OR canonical_alternative_apparatus_id=$1)
             ) SELECT md5(jsonb_build_array(
                (SELECT COALESCE(string_agg(id||':'||version||':'||location,',' ORDER BY id),'')
                 FROM candidates),
                (SELECT COALESCE(string_agg(p.id||':'||p.xmin::text||':'||p.ctid::text,',' ORDER BY p.id),'')
                 FROM mini_order_products p WHERE p.order_id IN (SELECT id FROM candidates)),
                (SELECT COALESCE(string_agg(t.id||':'||t.xmin::text||':'||t.ctid::text,',' ORDER BY t.id),'')
                 FROM mini_quick_order_templates t),
                (SELECT COALESCE(string_agg(q.xmin::text||':'||q.ctid::text,','),'')
                 FROM mini_queue_sequences q WHERE q.canonical_apparatus_id=$1)
             )::text)",
        )
        .bind(apparatus)
        .fetch_one(&self.pool)
        .await?)
    }

    pub(crate) async fn order_list_catalog(
        &self,
        owner: &str,
        apparatus: &str,
    ) -> Result<PreparationOrderCatalog, PreparationError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let responsibilities: Vec<Value> = sqlx::query_scalar(
            "SELECT jsonb_build_object('material_id',material_id,'material_name',material_name)
             FROM mini_preparation_material_responsibilities
             WHERE principal_role='tayyorlov_masteri' AND principal_ref=$1
             ORDER BY lower(material_id),lower(material_name)",
        )
        .bind(owner)
        .fetch_all(&mut *tx)
        .await?;
        if responsibilities.is_empty() {
            tx.commit().await?;
            return Ok(PreparationOrderCatalog {
                responsibilities: json!({"materials":[]}),
                sequence: Vec::new(),
                candidates: Vec::new(),
            });
        }
        let sequence: Option<Value> = sqlx::query_scalar(
            "SELECT order_ids FROM mini_queue_sequences WHERE canonical_apparatus_id=$1",
        )
        .bind(apparatus)
        .fetch_optional(&mut *tx)
        .await?;
        // The indexed node projection is only a candidate filter. Shared Rust
        // topology and alternative selection still decide actual membership.
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id',m.id,'product_code',COALESCE(m.map_json->>'product_code',''),
                'title',COALESCE(m.map_json->>'title',''),'code',COALESCE(m.map_json->>'code',''),
                'order_number',COALESCE(m.map_json->>'order_number',''),
                'customer_name',COALESCE(m.map_json->>'customer_name',''),
                'width_mm',CASE WHEN jsonb_typeof(m.map_json->'width_mm')='number'
                    THEN m.map_json->'width_mm' ELSE NULL END,
                'image_id',COALESCE(m.map_json->>'image_id',''),
                'nodes',COALESCE((SELECT jsonb_agg(jsonb_build_object(
                    'id',n->>'id','kind',n->>'kind','title',COALESCE(n->>'title',''),
                    'apparatus_id',COALESCE(n->>'apparatus_id',''),
                    'item_code',COALESCE(n->>'item_code',''),
                    'alternative_group_id',COALESCE(n->>'alternative_group_id',''),
                    'alternative_assigned_apparatus_id',COALESCE(n->>'alternative_assigned_apparatus_id','')
                 ) ORDER BY ordinal) FROM jsonb_array_elements(m.map_json->'nodes')
                   WITH ORDINALITY node(n,ordinal)),'[]'::jsonb),
                'edges',COALESCE(m.map_json->'edges','[]'::jsonb)
             ) AS summary,
             (jsonb_typeof(m.map_json->'order_kg')='number'
              AND (m.map_json->>'order_kg')::numeric>0 AND EXISTS (
                SELECT 1 FROM mini_preparation_material_responsibilities r
                WHERE r.principal_role='tayyorlov_masteri' AND r.principal_ref=$1 AND (
                  EXISTS (SELECT 1 FROM mini_order_products p
                    LEFT JOIN LATERAL jsonb_array_elements(COALESCE(p.layers_json,'[]'::jsonb)) l ON true
                    WHERE p.order_id=m.id AND (
                      lower(COALESCE(l->>'material_id',''))=lower(r.material_id)
                      OR lower(COALESCE(l->>'material',''))=lower(r.material_name)
                      OR lower(COALESCE(p.first_layer_material,''))=lower(r.material_name)
                      OR lower(COALESCE(p.second_layer_material,''))=lower(r.material_name)
                      OR lower(COALESCE(p.third_layer_material,''))=lower(r.material_name)))
                  OR EXISTS (SELECT 1 FROM mini_quick_order_templates t
                    LEFT JOIN LATERAL jsonb_array_elements(COALESCE(t.payload_json->'layers','[]'::jsonb)) l ON true
                    WHERE btrim(COALESCE(t.payload_json->>'source_map_id',''))=m.id
                      AND NOT EXISTS (SELECT 1 FROM mini_order_products p WHERE p.order_id=m.id)
                      AND (lower(COALESCE(l->>'material_id',''))=lower(r.material_id)
                        OR lower(COALESCE(l->>'material',''))=lower(r.material_name)
                        OR lower(COALESCE(t.payload_json->>'first_layer_material',''))=lower(r.material_name)
                        OR lower(COALESCE(t.payload_json->>'second_layer_material',''))=lower(r.material_name)
                        OR lower(COALESCE(t.payload_json->>'third_layer_material',''))=lower(r.material_name)))
                ))) AS allowed,
             (EXISTS (SELECT 1 FROM mini_order_run_sessions s WHERE s.order_id=m.id)
              OR EXISTS (SELECT 1 FROM mini_queue_states q WHERE q.order_id=m.id AND q.state<>'pending')
              OR EXISTS (SELECT 1 FROM mini_order_control_states c WHERE c.order_id=m.id AND c.state<>'active')
              OR EXISTS (SELECT 1 FROM mini_stage_order_controls c WHERE c.order_id=m.id)
              OR EXISTS (SELECT 1 FROM mini_print_preflight_holds h WHERE h.order_id=m.id
                         AND h.status IN ('held','running','passed'))
              OR EXISTS (SELECT 1 FROM mini_progress_batches b WHERE b.order_id=m.id)) AS needs_authority
             FROM mini_production_maps m
             WHERE m.id LIKE 'zakaz-%' AND m.lifecycle_status IN ('released','in_progress')
               AND m.id IN (SELECT map_id FROM mini_production_map_nodes
                   WHERE canonical_apparatus_id=$2 OR canonical_alternative_apparatus_id=$2)
             ORDER BY m.updated_at DESC,m.id ASC",
        ).bind(owner).bind(apparatus).fetch_all(&mut *tx).await?;
        let mut candidates = Vec::with_capacity(rows.len());
        for row in rows {
            let payload: Value = row.try_get("summary")?;
            match serde_json::from_value(payload) {
                Ok(map) => candidates.push(PreparationOrderCandidate {
                    map,
                    allowed: row.try_get::<Option<bool>, _>("allowed")?.unwrap_or(false),
                    needs_authority: row.try_get("needs_authority")?,
                }),
                Err(error) => tracing::warn!(?error, "skipping invalid preparation order topology"),
            }
        }
        let mut maps = candidates
            .iter()
            .filter(|candidate| candidate.allowed)
            .map(|candidate| candidate.map.clone())
            .collect::<Vec<_>>();
        super::order_customers::fill_customers(&mut tx, &mut maps).await?;
        for (candidate, map) in candidates
            .iter_mut()
            .filter(|candidate| candidate.allowed)
            .zip(maps)
        {
            candidate.map = map;
        }
        tx.commit().await?;
        Ok(PreparationOrderCatalog {
            responsibilities: json!({"materials":responsibilities}),
            sequence: sequence
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default(),
            candidates,
        })
    }

    pub(crate) async fn order_list_detail_allowed(
        &self,
        owner: &str,
        id: &str,
    ) -> Result<bool, PreparationError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM mini_production_maps m WHERE m.id=$1
              AND m.lifecycle_status IN ('released','in_progress')
              AND jsonb_typeof(m.map_json->'order_kg')='number'
              AND (m.map_json->>'order_kg')::numeric>0)",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        let allowed = active && super::order_matches_responsibility(&mut tx, owner, id).await?;
        tx.commit().await?;
        Ok(allowed)
    }

    pub(crate) async fn fill_order_customer(
        &self,
        map: &mut ProductionMapDefinition,
    ) -> Result<(), PreparationError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
            .execute(&mut *tx)
            .await?;
        super::order_customers::fill_customers(&mut tx, std::slice::from_mut(map)).await?;
        tx.commit().await?;
        Ok(())
    }
}
