-- Mold ownership comes from the RECEIVING CLERK's assigned warehouse.
-- A block/cell placement is neither required nor evidence of ownership.
DROP TRIGGER IF EXISTS mini_qolip_specs_persist_warehouse ON mini_qolip_product_specs;

CREATE OR REPLACE FUNCTION mini_qolip_assigned_warehouse(creator_role text, creator_ref text)
RETURNS text LANGUAGE sql STABLE AS $$
    SELECT CASE WHEN count(DISTINCT lower(owner)) = 1 THEN min(owner) END
    FROM (
        SELECT COALESCE(NULLIF(btrim(w.parent_warehouse), ''), w.name) AS owner
        FROM mini_warehouse_assignments a
        JOIN mini_warehouses w ON lower(w.name) = lower(a.warehouse_name)
        WHERE lower(a.principal_role) = lower(creator_role)
          AND a.principal_ref = creator_ref AND a.assignment_kind = 'warehouse'
    ) assigned
$$;

-- Canonicalize old records that predate product-spec receipts, including issued molds.
-- Keep original creator identity; do not derive ownership from the destination block.
WITH legacy AS (
    SELECT item_code, item_name, qolip_code, size, created_by_role, created_by_ref,
           created_by_name, payload_json, created_at
    FROM mini_qolip_locations
    UNION ALL
    SELECT item_code, item_name, qolip_code, size, issued_by_role, issued_by_ref,
           issued_by_name, payload_json, created_at
    FROM mini_qolip_checkouts
), first_receipt AS (
    SELECT DISTINCT ON (lower(qolip_code)) *
    FROM legacy
    ORDER BY lower(qolip_code), created_at, created_by_role, created_by_ref
)
INSERT INTO mini_qolip_product_specs (
    item_code, item_name, item_group, qolip_code, size,
    created_by_role, created_by_ref, created_by_name, payload_json, created_at
)
SELECT l.item_code, l.item_name, COALESCE(i.item_group, ''), l.qolip_code, l.size,
       l.created_by_role, l.created_by_ref, l.created_by_name,
       (COALESCE(l.payload_json, '{}'::jsonb) - 'warehouse') ||
           jsonb_build_object('warehouse', COALESCE(
               mini_qolip_assigned_warehouse(l.created_by_role, l.created_by_ref), '')),
       l.created_at
FROM first_receipt l
LEFT JOIN mini_items i ON lower(i.code) = lower(l.item_code)
WHERE NOT EXISTS (
    SELECT 1 FROM mini_qolip_product_specs s WHERE lower(s.qolip_code) = lower(l.qolip_code)
)
ON CONFLICT (lower(qolip_code)) DO NOTHING;

UPDATE mini_qolip_product_specs s
SET payload_json = jsonb_set(COALESCE(s.payload_json, '{}'::jsonb), '{warehouse}',
        to_jsonb(mini_qolip_assigned_warehouse(s.created_by_role, s.created_by_ref)), true)
WHERE btrim(COALESCE(s.payload_json->>'warehouse', '')) = ''
  AND mini_qolip_assigned_warehouse(s.created_by_role, s.created_by_ref) IS NOT NULL;

-- Missing/ambiguous historical assignments never grant shared access.
CREATE INDEX IF NOT EXISTS mini_qolip_specs_warehouse_idx
    ON mini_qolip_product_specs (lower(btrim(payload_json->>'warehouse')));

CREATE OR REPLACE FUNCTION mini_qolip_persist_warehouse()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE owner text;
BEGIN
    owner := NULLIF(btrim(NEW.payload_json->>'warehouse'), '');
    IF TG_OP = 'UPDATE' AND NULLIF(btrim(OLD.payload_json->>'warehouse'), '') IS NOT NULL THEN
        IF owner IS NOT NULL AND lower(owner) <> lower(btrim(OLD.payload_json->>'warehouse')) THEN
            RAISE EXCEPTION 'qolip_warehouse_mismatch' USING ERRCODE = '23514';
        END IF;
        owner := btrim(OLD.payload_json->>'warehouse');
    END IF;
    IF owner IS NULL THEN
        owner := mini_qolip_assigned_warehouse(NEW.created_by_role, NEW.created_by_ref);
    END IF;
    IF owner IS NULL THEN
        RAISE EXCEPTION 'qolip_warehouse_required' USING ERRCODE = '23514';
    END IF;
    NEW.payload_json := jsonb_set(COALESCE(NEW.payload_json, '{}'::jsonb),
        '{warehouse}', to_jsonb(owner), true);
    RETURN NEW;
END
$$;

CREATE TRIGGER mini_qolip_specs_persist_warehouse
BEFORE INSERT OR UPDATE ON mini_qolip_product_specs
FOR EACH ROW EXECUTE FUNCTION mini_qolip_persist_warehouse();
