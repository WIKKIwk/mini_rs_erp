WITH scoped AS (
    SELECT p.id, p.code, p.name, p.created_at, w.name AS warehouse
    FROM mini_raw_material_paddons p
    JOIN mini_warehouses w ON w.id = p.warehouse_id
    WHERE p.owner_role = $1 AND p.owner_ref = $2 AND NOT w.is_group
      AND ($7::text IS NULL OR p.code = $7)
      AND EXISTS (
          SELECT 1 FROM mini_warehouse_assignments a
          WHERE a.assignment_kind = 'warehouse'
            AND a.principal_role = $1 AND a.principal_ref = $2
            AND lower(a.warehouse_name) = lower(w.name)
      )
), units AS (
    SELECT p.id, u.line_no, u.kind, u.lot_number, u.unit_kg,
           COALESCE(NULLIF(s.item_name, ''), s.item_code, NULLIF(u.item_name, ''), u.item_code) AS item_name,
           u.issue_id IS NULL AND (s.id IS NULL OR (
               lower(s.warehouse) = lower(p.warehouse) AND {AVAILABLE_STOCK} AND s.qty > 0
           )) AS available,
           s.qty
    FROM scoped p
    JOIN mini_raw_material_paddon_units u ON u.paddon_id = p.id
    LEFT JOIN mini_raw_material_stock s ON s.id = u.stock_id
), lines AS (
    SELECT id, line_no, kind, lot_number, min(item_name) AS item_name, unit_kg,
           count(*) AS initial_count,
           count(*) FILTER (WHERE available) AS remaining_count,
           count(*) FILTER (WHERE available AND qty IS NULL) AS unweighed_count,
           COALESCE(sum(qty) FILTER (WHERE available), 0::numeric(18,6)) AS remaining_kg
    FROM units GROUP BY id, line_no, kind, lot_number, unit_kg
), totals AS (
    SELECT id, sum(remaining_kg) AS remaining_kg, sum(remaining_count) AS remaining_count,
           sum(unweighed_count) AS unweighed_count,
           jsonb_agg(jsonb_build_object(
              'line_no', line_no, 'item_name', item_name, 'lot_number', lot_number, 'kind', kind,
              'initial_count', initial_count, 'remaining_count', remaining_count,
              'unweighed_count', unweighed_count, 'unit_kg', unit_kg::text, 'remaining_kg', remaining_kg::text
           ) ORDER BY line_no, item_name) AS contents,
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
