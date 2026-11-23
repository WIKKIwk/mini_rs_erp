CREATE TABLE mini_raw_material_paddon_issues (
    id TEXT PRIMARY KEY,
    paddon_id TEXT NOT NULL REFERENCES mini_raw_material_paddons(id) ON DELETE RESTRICT,
    request_id TEXT NOT NULL CHECK (length(request_id) BETWEEN 8 AND 128),
    request_json JSONB NOT NULL,
    response_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (paddon_id, request_id)
);

ALTER TABLE mini_raw_material_paddon_units
    ALTER COLUMN stock_id DROP NOT NULL,
    ALTER COLUMN unit_kg DROP NOT NULL,
    ADD COLUMN item_code TEXT REFERENCES mini_items(code) ON UPDATE CASCADE ON DELETE RESTRICT,
    ADD COLUMN item_name TEXT,
    ADD COLUMN width_mm NUMERIC(18,6),
    ADD COLUMN micron NUMERIC(18,6),
    ADD COLUMN issue_id TEXT REFERENCES mini_raw_material_paddon_issues(id) ON DELETE RESTRICT,
    ADD COLUMN unweighed_warehouse_id TEXT REFERENCES mini_warehouses(id) ON DELETE RESTRICT,
    ADD CONSTRAINT mini_raw_material_paddon_unweighed_material CHECK (
        (stock_id IS NOT NULL AND unweighed_warehouse_id IS NULL) OR (
            stock_id IS NULL AND unit_kg IS NULL AND issue_id IS NULL
            AND item_code IS NOT NULL AND unweighed_warehouse_id IS NOT NULL
        )
    );

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        GRANT SELECT, INSERT ON mini_raw_material_paddon_issues TO mini_rs_erp;
        GRANT UPDATE (stock_id, issue_id, item_code, unweighed_warehouse_id) ON mini_raw_material_paddon_units TO mini_rs_erp;
    END IF;
END $$;
