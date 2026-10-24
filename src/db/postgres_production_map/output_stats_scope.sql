-- Scope is applied before discovering historical identities. Never inner-join
-- a live map/order/catalog to decide whether an old output still exists.
SELECT (EXTRACT(EPOCH FROM transaction_timestamp()) * 1000000)::bigint,
       COALESCE((SELECT jsonb_agg(jsonb_build_object('id', id, 'name', name) ORDER BY id)
         FROM (
           SELECT ids.id, COALESCE(NULLIF(btrim(a.name), ''), ids.id) AS name
           FROM (
             SELECT id FROM mini_apparatus
             WHERE ($1::text[] IS NULL OR id = ANY($1)) AND ($2::text IS NULL OR id = $2)
             UNION
             SELECT DISTINCT canonical_apparatus_id FROM mini_progress_batches
             WHERE canonical_apparatus_id IS NOT NULL
               AND ($1::text[] IS NULL OR canonical_apparatus_id = ANY($1))
               AND ($2::text IS NULL OR canonical_apparatus_id = $2)
               AND (
                 $2::text IS NOT NULL
                 OR (production_completed_at >= $3::text::timestamptz
                     AND production_completed_at < $4::text::timestamptz)
                 OR production_completed_at IS NULL OR NOT isfinite(production_completed_at)
                 OR production_completed_at <= to_timestamp(0)
                 OR payload_json #>> '{production_timing_source,completion}' = 'unknown'
               )
               AND COALESCE(payload_json->>'origin', '') <> 'layer_remainder'
               AND order_id NOT LIKE 'training-%' AND order_id NOT LIKE 'demo-%'
           ) ids
           LEFT JOIN mini_apparatus a ON a.id = ids.id
           ORDER BY ids.id LIMIT 50001
         ) names), '[]'::jsonb)
