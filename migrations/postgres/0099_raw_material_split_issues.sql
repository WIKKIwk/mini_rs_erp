SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- Audit only. Reports never create roll stock, ledger movements or printable outputs.
CREATE TABLE mini_raw_material_split_issues (
    id TEXT PRIMARY KEY,
    owner_ref TEXT NOT NULL REFERENCES mini_system_users(id) ON DELETE RESTRICT,
    request_id TEXT NOT NULL CHECK (length(request_id) BETWEEN 8 AND 128),
    parent_stock_id TEXT NOT NULL REFERENCES mini_raw_material_stock(id) ON DELETE RESTRICT,
    source_kg NUMERIC(18,6) NOT NULL CHECK (source_kg > 0 AND source_kg <> 'NaN'::numeric),
    output_kg NUMERIC(22,6) NOT NULL CHECK (output_kg > 0 AND output_kg <> 'NaN'::numeric),
    waste_kg NUMERIC(18,6) CHECK (waste_kg >= 0 AND waste_kg <> 'NaN'::numeric),
    difference_kg NUMERIC(22,6),
    note TEXT NOT NULL CHECK (length(btrim(note)) BETWEEN 1 AND 1000),
    request_json JSONB NOT NULL,
    response_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (owner_ref, request_id),
    CHECK ((waste_kg IS NULL AND difference_kg IS NULL)
        OR (waste_kg IS NOT NULL AND difference_kg IS NOT NULL
            AND difference_kg = source_kg - output_kg - waste_kg
            AND (waste_kg = 0 OR difference_kg <> 0)))
);
CREATE INDEX mini_raw_material_split_issues_history
    ON mini_raw_material_split_issues(owner_ref, created_at DESC);
CREATE TRIGGER mini_raw_material_split_issues_immutable
    BEFORE UPDATE OR DELETE ON mini_raw_material_split_issues
    FOR EACH ROW EXECUTE FUNCTION mini_raw_material_events_block_mutation();
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        GRANT SELECT, INSERT ON mini_raw_material_split_issues TO mini_rs_erp;
    END IF;
END $$;
