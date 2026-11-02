-- Recover the physical roll from its actual activation receipt, independently
-- of the worker or shift. Simultaneous bulk activations have no known order.
WITH activations AS (
    SELECT r.barcode, r.order_id, r.canonical_apparatus_id, s.id AS stock_id,
           max(e.occurred_at) AS activated_at
    FROM mini_raw_material_assignments r
    JOIN mini_raw_material_stock s ON lower(s.barcode)=lower(r.barcode)
    JOIN mini_apparatus a ON a.id=r.canonical_apparatus_id
    JOIN mini_raw_material_events e ON lower(e.barcode)=lower(s.barcode)
        AND e.order_id=r.order_id AND e.canonical_apparatus_id=r.canonical_apparatus_id
        AND e.event_type='usage_started'
        AND e.stock_status_before IS DISTINCT FROM 'in_use'
    WHERE a.execution_profile_json->>'operation'='laminate'
      AND s.reserved_order_id=r.order_id AND s.status IN ('in_use','consumed')
      AND s.uom='kg' AND s.length_m>0
      AND NOT EXISTS (SELECT 1 FROM mini_layer_remainder_operations o
                      WHERE o.source_kind='raw_material' AND o.source_id=s.id)
    GROUP BY r.barcode, r.order_id, r.canonical_apparatus_id, s.id
), ranked AS (
    SELECT *, rank() OVER (PARTITION BY order_id, canonical_apparatus_id
                           ORDER BY activated_at DESC) AS activation_rank
    FROM activations
), current_rolls AS (
    SELECT *, count(*) OVER (PARTITION BY order_id, canonical_apparatus_id) AS roll_count
    FROM ranked WHERE activation_rank=1
)
UPDATE mini_raw_material_assignments r
SET payload_json=r.payload_json || jsonb_build_object('lamination_input_stock_id', c.stock_id)
FROM current_rolls c
WHERE c.roll_count=1 AND r.barcode=c.barcode AND r.order_id=c.order_id
  AND r.canonical_apparatus_id=c.canonical_apparatus_id
  AND NOT EXISTS (SELECT 1 FROM mini_raw_material_assignments bound
                  WHERE bound.order_id=c.order_id
                    AND bound.canonical_apparatus_id=c.canonical_apparatus_id
                    AND bound.payload_json ? 'lamination_input_stock_id');
