WITH receipts AS MATERIALIZED (
    SELECT upper(btrim(stock.barcode)) AS barcode, lower(receipt.barcode) AS epc,
           receipt.name AS receipt_name, receipt.payload_json->>'actor_ref' AS owner_ref
    FROM mini_raw_material_stock stock
    JOIN mini_gscale_receipts receipt ON receipt.name=stock.source_receipt_id
    WHERE lower(stock.barcode)=ANY($1::text[]) AND receipt.status='submitted'
      AND receipt.payload_json->>'actor_role'='tayyorlov_masteri'
), owners AS (
    SELECT DISTINCT r.owner_ref FROM receipts r
    WHERE EXISTS (SELECT 1 FROM mini_preparation_output_notes n
        WHERE n.owner_ref=r.owner_ref AND n.target_kind='batch' AND n.note<>'')
), raw_batches AS (
    SELECT o.owner_ref,b.batch_id,b.payload_json,b.updated_at AS recorded_at,0 AS priority
    FROM owners o JOIN mini_rps_batches b ON b.owner_key='tayyorlov_masteri:'||o.owner_ref
    UNION ALL
    SELECT o.owner_ref,b.batch_id,b.payload_json,b.completed_at,1
    FROM owners o JOIN mini_rps_batch_history b ON b.owner_key='tayyorlov_masteri:'||o.owner_ref
), batches AS (
    SELECT DISTINCT ON (owner_ref,batch_id) owner_ref,batch_id,payload_json,
        COALESCE(NULLIF(payload_json->>'created_at','')::timestamptz,recorded_at) AS started_at
    FROM raw_batches ORDER BY owner_ref,batch_id,priority,recorded_at DESC
), membership AS (
    SELECT DISTINCT ON (b.owner_ref,r.epc) b.owner_ref,r.epc,b.batch_id
    FROM batches b CROSS JOIN LATERAL jsonb_array_elements(
        CASE WHEN jsonb_typeof(b.payload_json->'prints')='array'
            THEN b.payload_json->'prints' ELSE '[]'::jsonb END) printed(entry)
    JOIN receipts r ON r.owner_ref=b.owner_ref AND r.epc=lower(printed.entry->>'epc')
    ORDER BY b.owner_ref,r.epc,COALESCE(NULLIF(printed.entry->>'printed_at','')::timestamptz,b.started_at),b.batch_id
)
