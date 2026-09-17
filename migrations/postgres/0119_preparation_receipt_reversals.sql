SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE mini_preparation_operations
    DROP CONSTRAINT IF EXISTS mini_preparation_operations_kind_check;
ALTER TABLE mini_preparation_operations
    ADD CONSTRAINT mini_preparation_operations_kind_check
    CHECK (kind IN ('material', 'receipt', 'consumption', 'receipt_reversal'));

-- A receipt reversal is an immutable, one-to-one audit link to the original
-- receipt. The stock row is soft-deleted and the append-only raw-material
-- ledger receives the compensating negative event; neither original history
-- row is rewritten or removed.
CREATE TABLE mini_preparation_receipt_reversals (
    operation_id TEXT PRIMARY KEY
        REFERENCES mini_preparation_operations(id) ON DELETE RESTRICT,
    receipt_id TEXT NOT NULL UNIQUE
        REFERENCES mini_preparation_receipts(id) ON DELETE RESTRICT,
    stock_id TEXT NOT NULL UNIQUE
        REFERENCES mini_raw_material_stock(id) ON DELETE RESTRICT,
    owner_ref TEXT NOT NULL
        REFERENCES mini_system_users(id) ON DELETE RESTRICT,
    reversed_kg NUMERIC(18,6) NOT NULL
        CHECK (reversed_kg > 0 AND reversed_kg <> 'NaN'::numeric),
    reason TEXT NOT NULL
        CHECK (btrim(reason) <> '' AND length(reason) <= 500),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX mini_preparation_receipt_reversals_owner_history
    ON mini_preparation_receipt_reversals(owner_ref, created_at DESC);

CREATE TRIGGER mini_preparation_receipt_reversals_immutable
    BEFORE UPDATE OR DELETE ON mini_preparation_receipt_reversals
    FOR EACH ROW EXECUTE FUNCTION mini_raw_material_events_block_mutation();

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        GRANT SELECT, INSERT
            ON mini_preparation_receipt_reversals TO mini_rs_erp;
        REVOKE UPDATE, DELETE
            ON mini_preparation_receipt_reversals FROM mini_rs_erp;
    END IF;
END $$;
