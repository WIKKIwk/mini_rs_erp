WITH scoped AS (
    SELECT p.id, p.code, p.name, p.created_at, p.warehouse_id, w.name AS warehouse
    FROM mini_raw_material_paddons p
    JOIN mini_warehouses w ON w.id = p.warehouse_id
    WHERE p.owner_role = $1 AND p.owner_ref = $2 AND NOT w.is_group
      AND ($7::text IS NULL OR p.code = $7)
      AND ($8::text IS NULL OR p.warehouse_id = $8)
      AND EXISTS (
          SELECT 1 FROM mini_warehouse_assignments a
          WHERE a.assignment_kind = 'warehouse'
            AND a.principal_role = $1 AND a.principal_ref = $2
            AND lower(a.warehouse_name) = lower(w.name)
      )
), units AS (
    SELECT p.id, u.line_no, u.kind, u.lot_number, u.unit_kg,
           CASE WHEN u.prepared_roll THEN s.barcode END AS prepared_barcode,
           CASE WHEN u.opened THEN u.unit_no END AS open_unit_no,
           COALESCE(u.width_mm,s.width_mm) AS width_mm, COALESCE(u.micron,s.micron) AS micron,
           COALESCE(NULLIF(s.item_name, ''), s.item_code, NULLIF(u.item_name, ''), u.item_code) AS item_name,
           u.issue_id IS NULL AND (s.id IS NULL OR (
               lower(s.warehouse) = lower(p.warehouse) AND {PADDON_UNIT_STOCK} AND s.qty > 0
           )) AS issuable,
           u.issue_id IS NULL AND (
               s.id IS NULL OR (
                   lower(s.warehouse)=lower(p.warehouse)
                   AND (placement.asset_ref IS NULL OR location.warehouse_id=p.warehouse_id)
                   AND (s.status='on_paddon' OR (
                       s.payload_json @> '{"paddon_pending_at_upgrade":true}'::jsonb
                       AND (t.id IS NULL OR t.status IN ('requested','approved'))
                   ))
               )
           ) AS on_pallet,
           s.qty
    FROM scoped p
    JOIN mini_raw_material_paddon_units u ON u.paddon_id = p.id
    LEFT JOIN mini_raw_material_stock s ON s.id = u.stock_id
    LEFT JOIN mini_inventory_placements placement ON placement.asset_kind='raw_material' AND placement.asset_ref=s.id
    LEFT JOIN mini_inventory_locations location ON location.id=placement.physical_location_id
    LEFT JOIN mini_inventory_transfers t ON t.id=s.payload_json->>'inventory_transfer_id'
), lines AS (
    SELECT id, line_no, kind, lot_number, min(item_name) AS item_name, unit_kg, open_unit_no,
           prepared_barcode,
           min(width_mm) AS width_mm, min(micron) AS micron,
           count(*) AS initial_count, count(*) FILTER (WHERE issuable) AS issuable_count,
           count(*) FILTER (WHERE on_pallet) AS remaining_count,
           count(*) FILTER (WHERE issuable AND qty IS NULL) AS unweighed_count,
           COALESCE(sum(qty) FILTER (WHERE on_pallet), 0::numeric(18,6)) AS remaining_kg
    FROM units GROUP BY id, line_no, kind, lot_number, unit_kg, open_unit_no, prepared_barcode
), totals AS (
    SELECT id, sum(remaining_kg) AS remaining_kg, sum(remaining_count) AS remaining_count,
           sum(unweighed_count) AS unweighed_count,
           COALESCE(jsonb_agg(jsonb_build_object(
              'line_no', line_no, 'item_name', item_name, 'lot_number', lot_number, 'kind', kind,
              'initial_count', initial_count, 'remaining_count', remaining_count, 'issuable_count', issuable_count,
              'prepared_barcode', prepared_barcode,
              'open_unit_no', open_unit_no, 'width_mm', width_mm::text, 'micron', micron::text,
              'unweighed_count', unweighed_count, 'unit_kg', unit_kg::text, 'remaining_kg', remaining_kg::text
           ) ORDER BY line_no, open_unit_no NULLS FIRST)
               FILTER (WHERE prepared_barcode IS NULL OR remaining_count > 0), '[]'::jsonb) AS contents,
           bool_or(strpos(lower(item_name), $3) > 0 OR strpos(lower(lot_number), $3) > 0) AS matches
    FROM lines GROUP BY id
)
SELECT jsonb_build_object(
    'paddon_id', p.id, 'paddon_code', p.code, 'name', p.name, 'warehouse', p.warehouse,
    'remaining_kg', t.remaining_kg::text, 'remaining_count', t.remaining_count,
    'unweighed_count', t.unweighed_count, 'contents', t.contents
)
FROM scoped p JOIN totals t ON t.id = p.id
WHERE ($3 = '' OR strpos(lower(p.code), $3) > 0 OR strpos(lower(p.name), $3) > 0 OR t.matches)
  AND ($4 = 'all' OR ($4 = 'available' AND t.remaining_count > 0)
                 OR ($4 = 'empty' AND t.remaining_count = 0))
ORDER BY p.created_at DESC, p.id DESC
LIMIT $5 OFFSET $6
