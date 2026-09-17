SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- A preparation material can be used in one or more explicitly linked
-- warehouses.  The stable warehouse id is authoritative; the legacy
-- warehouse_name column remains only as a compatibility mirror for old rows.
CREATE TABLE IF NOT EXISTS mini_preparation_material_warehouse_scopes (
    item_code TEXT NOT NULL
        REFERENCES mini_preparation_materials(item_code)
        ON UPDATE CASCADE ON DELETE CASCADE,
    warehouse_id TEXT NOT NULL
        REFERENCES mini_warehouses(id)
        ON UPDATE CASCADE ON DELETE RESTRICT,
    scope_kind TEXT NOT NULL DEFAULT 'exclusive',
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by_role TEXT NOT NULL DEFAULT '',
    created_by_ref TEXT NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_prep_material_warehouse_scope_kind_allowed
        CHECK (scope_kind IN ('exclusive', 'shared')),
    CONSTRAINT mini_prep_material_warehouse_scope_actor_valid
        CHECK (btrim(created_by_role) <> '' AND btrim(created_by_ref) <> ''),
    PRIMARY KEY (item_code, warehouse_id)
);

CREATE INDEX IF NOT EXISTS idx_prep_material_warehouse_scope_warehouse
    ON mini_preparation_material_warehouse_scopes (warehouse_id, item_code)
    WHERE active;

-- Existing material rows already have the best available creation binding in
-- 0115.  Promote that binding to the stable id relation without guessing for
-- ambiguous historical rows.
INSERT INTO mini_preparation_material_warehouse_scopes (
    item_code, warehouse_id, scope_kind, active,
    created_by_role, created_by_ref
)
SELECT
    material.item_code,
    warehouse.id,
    'exclusive',
    true,
    'migration',
    '0117'
FROM mini_preparation_materials material
JOIN mini_warehouses warehouse
  ON lower(warehouse.name) = lower(material.warehouse_name)
WHERE btrim(COALESCE(material.warehouse_name, '')) <> ''
ON CONFLICT (item_code, warehouse_id) DO NOTHING;

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE
            ON mini_preparation_material_warehouse_scopes TO mini_rs_erp;
    END IF;
END $$;
