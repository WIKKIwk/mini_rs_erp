-- Database invariants for concurrent production writes.
-- These indexes intentionally fail the migration when legacy data already
-- violates an identity rule; silently choosing a winner would corrupt the
-- production projection.

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_apparatus_factory_map_object_id_unique
    ON mini_apparatus (btrim(payload_json->>'factory_map_object_id'))
    WHERE btrim(COALESCE(payload_json->>'factory_map_object_id', '')) <> '';

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_apparatus_material_rules_lower_apparatus
    ON mini_apparatus_material_rules (lower(apparatus));

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_raw_material_stock_lower_barcode
    ON mini_raw_material_stock (lower(barcode));

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_raw_material_assignments_lower_barcode
    ON mini_raw_material_assignments (lower(barcode));

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_queue_action_events_pending_completion
    ON mini_queue_action_events (lower(apparatus), order_id)
    WHERE action = 'complete'
      AND payload_json->>'completion_request' = 'true'
      AND COALESCE(payload_json->>'completion_request_status', 'pending') = 'pending';
