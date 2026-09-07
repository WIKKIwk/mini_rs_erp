SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE mini_system_users DROP CONSTRAINT mini_system_users_role_allowed;
ALTER TABLE mini_system_users ADD CONSTRAINT mini_system_users_role_allowed
    CHECK (role IN ('qolipchi', 'boyoqchi', 'tayyorlov_masteri'));

INSERT INTO mini_item_groups (name, parent_item_group)
VALUES ('All Item Groups', NULL) ON CONFLICT DO NOTHING;
INSERT INTO mini_item_groups (name, parent_item_group)
VALUES ('Tayyorlov homashyolari', 'All Item Groups') ON CONFLICT DO NOTHING;

-- Catalog ownership only. Physical quantity remains in the shared ERP stock.
CREATE TABLE mini_preparation_materials (
    item_code TEXT PRIMARY KEY REFERENCES mini_items(code) ON UPDATE CASCADE ON DELETE RESTRICT,
    owner_ref TEXT NOT NULL REFERENCES mini_system_users(id) ON DELETE RESTRICT,
    name_key TEXT NOT NULL,
    UNIQUE (owner_ref, name_key),
    CHECK (btrim(name_key) <> '')
);

-- Immutable commands double as receipt/recipe history and durable retry results.
CREATE TABLE mini_preparation_operations (
    id TEXT PRIMARY KEY,
    owner_ref TEXT NOT NULL REFERENCES mini_system_users(id) ON DELETE RESTRICT,
    request_id TEXT NOT NULL CHECK (length(request_id) BETWEEN 8 AND 128),
    kind TEXT NOT NULL CHECK (kind IN ('material', 'receipt', 'consumption')),
    order_id TEXT,
    request_json JSONB NOT NULL,
    response_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (owner_ref, request_id),
    CHECK ((kind = 'consumption') = (order_id IS NOT NULL))
);
CREATE UNIQUE INDEX mini_preparation_one_recipe_per_order
    ON mini_preparation_operations (owner_ref, order_id) WHERE kind = 'consumption';

-- Original lot quantities never change when shared stock is partially consumed.
CREATE TABLE mini_preparation_receipts (
    id TEXT PRIMARY KEY REFERENCES mini_preparation_operations(id) ON DELETE RESTRICT,
    stock_id TEXT NOT NULL UNIQUE REFERENCES mini_raw_material_stock(id) ON DELETE RESTRICT,
    item_code TEXT NOT NULL REFERENCES mini_preparation_materials(item_code) ON UPDATE CASCADE ON DELETE RESTRICT,
    owner_ref TEXT NOT NULL REFERENCES mini_system_users(id) ON DELETE RESTRICT,
    initial_kg NUMERIC(18,6) NOT NULL CHECK (initial_kg > 0 AND initial_kg <> 'NaN'::numeric),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX mini_preparation_receipt_owner_item ON mini_preparation_receipts(owner_ref, item_code);

CREATE TABLE mini_preparation_allocations (
    operation_id TEXT NOT NULL REFERENCES mini_preparation_operations(id) ON DELETE RESTRICT,
    receipt_id TEXT NOT NULL REFERENCES mini_preparation_receipts(id) ON DELETE RESTRICT,
    kg NUMERIC(18,6) NOT NULL CHECK (kg > 0 AND kg <> 'NaN'::numeric),
    PRIMARY KEY (operation_id, receipt_id)
);

CREATE TRIGGER mini_preparation_operations_immutable BEFORE UPDATE OR DELETE ON mini_preparation_operations
    FOR EACH ROW EXECUTE FUNCTION mini_raw_material_events_block_mutation();
CREATE TRIGGER mini_preparation_receipts_immutable BEFORE UPDATE OR DELETE ON mini_preparation_receipts
    FOR EACH ROW EXECUTE FUNCTION mini_raw_material_events_block_mutation();
CREATE TRIGGER mini_preparation_allocations_immutable BEFORE UPDATE OR DELETE ON mini_preparation_allocations
    FOR EACH ROW EXECUTE FUNCTION mini_raw_material_events_block_mutation();

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        GRANT SELECT, INSERT ON mini_preparation_materials, mini_preparation_operations,
            mini_preparation_receipts, mini_preparation_allocations TO mini_rs_erp;
    END IF;
END $$;
