WITH raw_batches AS (
    SELECT 'batch:'||batch_id AS batch_id, payload_json, updated_at AS recorded_at, 0 AS priority
    FROM mini_rps_batches WHERE owner_key='tayyorlov_masteri:'||$1 AND owner_role='tayyorlov_masteri' AND owner_ref=$1
    UNION ALL
    SELECT 'batch:'||batch_id, payload_json, completed_at, 1
    FROM mini_rps_batch_history WHERE owner_key='tayyorlov_masteri:'||$1 AND owner_role='tayyorlov_masteri' AND owner_ref=$1
), batches AS MATERIALIZED (
    SELECT DISTINCT ON (batch_id) batch_id, payload_json,
        COALESCE(payload_json#>>'{order_assignment,order_id}','') AS order_id,
        COALESCE((payload_json->>'active')::boolean,false) AS active,
        COALESCE(NULLIF(payload_json->>'created_at','')::timestamptz,recorded_at) AS started_at,
        COALESCE(NULLIF(payload_json->>'updated_at','')::timestamptz,recorded_at) AS updated_at
    FROM raw_batches ORDER BY batch_id,priority,recorded_at DESC
), batch_prints AS MATERIALIZED (
    SELECT DISTINCT ON (lower(printed.entry->>'epc'))
        lower(printed.entry->>'epc') AS epc_key,b.batch_id,b.order_id,printed.entry
    FROM batches b CROSS JOIN LATERAL jsonb_array_elements(
        CASE WHEN jsonb_typeof(b.payload_json->'prints')='array'
            THEN b.payload_json->'prints' ELSE '[]'::jsonb END) printed(entry)
    WHERE btrim(COALESCE(printed.entry->>'epc',''))<>''
    ORDER BY lower(printed.entry->>'epc'),
        COALESCE(NULLIF(printed.entry->>'printed_at','')::timestamptz,b.started_at),b.batch_id
), confirmed AS MATERIALIZED (
    SELECT r.barcode AS epc,r.name AS receipt_name,
        COALESCE(NULLIF(e.order_id,''),NULLIF(r.payload_json#>>'{order_assignment,order_id}',''),'') AS order_id,
        COALESCE(NULLIF(e.item_code,''),r.item_code) AS item_code,
        COALESCE(NULLIF(e.item_name,''),NULLIF(r.payload_json->>'item_name',''),r.item_code) AS item_name,
        COALESCE(NULLIF(e.warehouse,''),r.warehouse) AS warehouse,
        COALESCE(e.qty_delta,NULLIF(p.entry->>'net_qty','')::numeric,r.qty)::numeric AS net_kg,
        NULLIF(p.entry->>'gross_qty','')::numeric AS gross_kg,
        COALESCE(NULLIF(e.payload_json->>'length_m','')::numeric,r.length_m) AS length_m,
        COALESCE(NULLIF(e.payload_json->>'width_mm','')::numeric,r.width_mm) AS width_mm,
        COALESCE(NULLIF(e.payload_json->>'micron','')::numeric,r.micron) AS micron,
        COALESCE(e.occurred_at,NULLIF(p.entry->>'printed_at','')::timestamptz,r.submitted_at,r.created_at) AS printed_at,
        COALESCE(NULLIF(e.payload_json->'preparation_formula','null'::jsonb),
            NULLIF(r.payload_json#>'{order_assignment,preparation_formula}','null'::jsonb)) AS preparation_formula,
        p.batch_id AS recorded_batch_id,p.order_id AS batch_order_id
    FROM mini_gscale_receipts r
    LEFT JOIN batch_prints p ON p.epc_key=lower(r.barcode)
    LEFT JOIN LATERAL (
        SELECT event.order_id,event.item_code,event.item_name,event.warehouse,event.qty_delta,
            event.payload_json,event.occurred_at
        FROM mini_raw_material_events event
        WHERE event.barcode_key=lower(r.barcode) AND event.event_type='receipt_posted'
            AND event.source_type='gscale_receipt'
        ORDER BY event.occurred_at,event.event_id LIMIT 1
    ) e ON true
    WHERE r.status='submitted' AND r.payload_json->>'actor_role'='tayyorlov_masteri'
        AND r.payload_json->>'actor_ref'=$1
), rolls AS MATERIALIZED (
    SELECT *,CASE WHEN recorded_batch_id IS NOT NULL AND batch_order_id=order_id
        THEN recorded_batch_id ELSE 'receipt:'||epc END AS batch_id
    FROM confirmed WHERE order_id<>''
), roll_stats AS (
    SELECT batch_id,count(*) AS roll_count,sum(net_kg) AS total_net_kg,
        COALESCE(sum(length_m),0) AS total_length_m,max(printed_at) AS last_activity_at,
        string_agg(DISTINCT item_code,', ' ORDER BY item_code) AS item_code,
        string_agg(DISTINCT item_name,', ' ORDER BY item_name) AS item_name,
        string_agg(DISTINCT warehouse,', ' ORDER BY warehouse) AS warehouse
    FROM rolls GROUP BY batch_id
), journal_batches AS MATERIALIZED (
    SELECT b.batch_id,b.order_id,COALESCE(s.item_code,b.payload_json->>'item_code','') AS item_code,
        COALESCE(s.item_name,NULLIF(b.payload_json->>'item_name',''),b.payload_json->>'item_code','') AS item_name,
        COALESCE(s.warehouse,b.payload_json->>'warehouse','') AS warehouse,b.active,b.started_at,
        CASE WHEN b.active THEN NULL ELSE b.updated_at END AS ended_at,
        GREATEST(b.updated_at,s.last_activity_at) AS last_activity_at,
        COALESCE(s.roll_count,0) AS roll_count,COALESCE(s.total_net_kg,0) AS total_net_kg,
        COALESCE(s.total_length_m,0) AS total_length_m,
        NULLIF(b.payload_json#>'{order_assignment,preparation_formula}','null'::jsonb) AS preparation_formula
    FROM batches b LEFT JOIN roll_stats s ON s.batch_id=b.batch_id WHERE b.order_id<>''
    UNION ALL
    SELECT r.batch_id,r.order_id,r.item_code,r.item_name,r.warehouse,false,r.printed_at,r.printed_at,
        r.printed_at,1,r.net_kg,COALESCE(r.length_m,0),r.preparation_formula
    FROM rolls r WHERE r.batch_id='receipt:'||r.epc
), order_stats AS (
    SELECT b.order_id,count(*) AS batch_count,count(*) FILTER (WHERE active) AS active_batch_count,
        sum(roll_count) AS roll_count,sum(total_net_kg) AS total_net_kg,
        sum(total_length_m) AS total_length_m,max(last_activity_at) AS last_activity_at,
        string_agg(concat_ws(' ',item_code,item_name,preparation_formula->>'variant_name'),' ') AS materials_search
    FROM journal_batches b GROUP BY b.order_id
), journal_orders AS MATERIALIZED (
    SELECT s.*,COALESCE(NULLIF(m.code,''),NULLIF(m.order_number,''),NULLIF(o.code,''),NULLIF(o.order_number,''),s.order_id) AS order_code,
        COALESCE(NULLIF(m.title,''),NULLIF(o.product_name,''),s.order_id) AS order_title,
        COALESCE(NULLIF(m.map_json->>'customer_name',''),o.customer_name,'') AS customer_name
    FROM order_stats s LEFT JOIN mini_production_maps m ON m.id=s.order_id
    LEFT JOIN mini_orders o ON o.id=s.order_id
)
