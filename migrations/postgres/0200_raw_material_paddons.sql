CREATE TABLE mini_raw_material_paddons (
    id TEXT PRIMARY KEY,
    code TEXT NOT NULL UNIQUE,
    owner_role TEXT NOT NULL CHECK (owner_role IN ('tayyorlov_masteri', 'material_taminotchi')),
    owner_ref TEXT NOT NULL CHECK (btrim(owner_ref) <> ''),
    warehouse_id TEXT REFERENCES mini_warehouses(id) ON DELETE SET NULL,
    request_id TEXT NOT NULL CHECK (length(request_id) BETWEEN 8 AND 128),
    request_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (owner_role, owner_ref, request_id)
);

-- Each unit has one stock row. The paddon itself contributes no extra stock.
CREATE TABLE mini_raw_material_paddon_units (
    paddon_id TEXT NOT NULL REFERENCES mini_raw_material_paddons(id) ON DELETE RESTRICT,
    line_no INTEGER NOT NULL CHECK (line_no > 0),
    unit_no INTEGER NOT NULL CHECK (unit_no > 0),
    stock_id TEXT NOT NULL UNIQUE REFERENCES mini_raw_material_stock(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL CHECK (kind IN ('bag', 'roll')),
    lot_number TEXT NOT NULL,
    unit_kg NUMERIC(18,6) NOT NULL CHECK (unit_kg > 0 AND unit_kg <> 'NaN'::numeric),
    PRIMARY KEY (paddon_id, line_no, unit_no)
);

CREATE INDEX mini_raw_material_paddon_owner_created
    ON mini_raw_material_paddons (owner_role, owner_ref, created_at DESC, id DESC);

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        GRANT SELECT, INSERT ON mini_raw_material_paddons, mini_raw_material_paddon_units TO mini_rs_erp;
    END IF;
END $$;
