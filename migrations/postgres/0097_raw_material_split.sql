SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE mini_system_users DROP CONSTRAINT mini_system_users_role_allowed;
ALTER TABLE mini_system_users ADD CONSTRAINT mini_system_users_role_allowed
    CHECK (role IN ('qolipchi', 'boyoqchi', 'tayyorlov_masteri', 'homashyo_rezkachi'));

-- A split parent has no remaining stock. Historical receipt/ledger retains its original weight.
ALTER TABLE mini_raw_material_stock DROP CONSTRAINT mini_raw_material_stock_qty_positive;
ALTER TABLE mini_raw_material_stock ADD CONSTRAINT mini_raw_material_stock_qty_positive
    CHECK ((qty > 0 OR (qty = 0 AND status = 'consumed')) AND qty <> 'NaN'::numeric);

CREATE TABLE mini_raw_material_splits (
    id TEXT PRIMARY KEY,
    owner_ref TEXT NOT NULL REFERENCES mini_system_users(id) ON DELETE RESTRICT,
    request_id TEXT NOT NULL CHECK (length(request_id) BETWEEN 8 AND 128),
    parent_stock_id TEXT NOT NULL UNIQUE REFERENCES mini_raw_material_stock(id) ON DELETE RESTRICT,
    source_kg NUMERIC(18,6) NOT NULL CHECK (source_kg > 0 AND source_kg <> 'NaN'::numeric),
    output_kg NUMERIC(18,6) NOT NULL CHECK (output_kg > 0 AND output_kg <> 'NaN'::numeric),
    waste_kg NUMERIC(18,6) NOT NULL CHECK (waste_kg >= 0 AND waste_kg <> 'NaN'::numeric),
    request_json JSONB NOT NULL,
    response_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (owner_ref, request_id),
    CHECK (source_kg = output_kg + waste_kg)
);
CREATE TABLE mini_raw_material_split_outputs (
    split_id TEXT NOT NULL REFERENCES mini_raw_material_splits(id) ON DELETE RESTRICT,
    stock_id TEXT PRIMARY KEY REFERENCES mini_raw_material_stock(id) ON DELETE RESTRICT,
    kg NUMERIC(18,6) NOT NULL CHECK (kg > 0 AND kg <> 'NaN'::numeric),
    width_mm NUMERIC(18,6) NOT NULL CHECK (width_mm > 0 AND width_mm <> 'NaN'::numeric)
);
CREATE INDEX mini_raw_material_splits_owner_history ON mini_raw_material_splits(owner_ref, created_at DESC);
-- Existing receipt retries and other writers cannot resurrect a split parent.
CREATE FUNCTION mini_raw_material_split_parent_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM mini_raw_material_splits WHERE parent_stock_id=OLD.id)
       AND (NEW.qty <> 0 OR NEW.status <> 'consumed' OR NEW.barcode <> OLD.barcode) THEN
        RAISE EXCEPTION 'raw_material_split_parent_consumed';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER mini_raw_material_split_parent_guard BEFORE UPDATE ON mini_raw_material_stock
    FOR EACH ROW EXECUTE FUNCTION mini_raw_material_split_parent_guard();
CREATE TRIGGER mini_raw_material_splits_immutable BEFORE UPDATE OR DELETE ON mini_raw_material_splits
    FOR EACH ROW EXECUTE FUNCTION mini_raw_material_events_block_mutation();
CREATE TRIGGER mini_raw_material_split_outputs_immutable BEFORE UPDATE OR DELETE ON mini_raw_material_split_outputs
    FOR EACH ROW EXECUTE FUNCTION mini_raw_material_events_block_mutation();

ALTER TABLE mini_raw_material_events DROP CONSTRAINT mini_rme_event_type_allowed;
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_event_type_allowed CHECK (
    event_type IN ('receipt_posted','order_reserved','order_unreserved','usage_started','consumption_posted',
        'adjustment_increase','adjustment_decrease','transfer_in','transfer_out','stock_corrected','stock_deleted',
        'split_consumed','split_produced'));
ALTER TABLE mini_raw_material_events DROP CONSTRAINT mini_rme_source_type_allowed;
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_source_type_allowed CHECK (
    source_type IN ('gscale_receipt','order_assignment','consumption','manual_adjustment','warehouse_transfer',
        'system','stock_correction','stock_delete','raw_material_split'));
ALTER TABLE mini_raw_material_events DROP CONSTRAINT mini_rme_qty_sign_allowed;
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_qty_sign_allowed CHECK (
    CASE WHEN event_type IN ('receipt_posted','adjustment_increase','transfer_in','split_produced') THEN qty_delta > 0
         WHEN event_type IN ('consumption_posted','adjustment_decrease','transfer_out','stock_deleted','split_consumed') THEN qty_delta < 0
         WHEN event_type IN ('order_reserved','order_unreserved','usage_started') THEN qty_delta = 0
         WHEN event_type = 'stock_corrected' THEN TRUE ELSE FALSE END);
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_split_consistent CHECK (
    (event_type NOT IN ('split_consumed','split_produced') AND source_type <> 'raw_material_split')
    OR (source_type = 'raw_material_split' AND order_id IS NULL AND apparatus IS NULL
        AND ((event_type = 'split_consumed' AND stock_status_before = 'available' AND stock_status_after = 'consumed')
          OR (event_type = 'split_produced' AND stock_status_before IS NULL AND stock_status_after = 'available'))));
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        GRANT SELECT, INSERT ON mini_raw_material_splits, mini_raw_material_split_outputs TO mini_rs_erp;
    END IF;
END $$;
