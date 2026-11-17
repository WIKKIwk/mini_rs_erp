SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- Mutable order metadata stays separate from immutable production receipts and recipes.
-- Historical journal orders can outlive their planning row, so order_id has no FK.
CREATE TABLE mini_preparation_production_order_notes (
    order_id TEXT PRIMARY KEY CHECK (btrim(order_id) <> ''),
    note TEXT NOT NULL DEFAULT '' CHECK (char_length(note) <= 2000),
    revision BIGINT NOT NULL CHECK (revision > 0),
    updated_by_ref TEXT NOT NULL CHECK (btrim(updated_by_ref) <> ''),
    updated_by_name TEXT NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON mini_preparation_production_order_notes TO mini_rs_erp;
    END IF;
END $$;
