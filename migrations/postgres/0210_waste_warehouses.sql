ALTER TABLE mini_warehouses
    ADD COLUMN is_waste BOOLEAN NOT NULL DEFAULT false,
    ADD CONSTRAINT mini_warehouses_waste_is_physical CHECK (NOT is_waste OR NOT is_group);

-- Keep the measurement immutable, including measurements taken before a
-- destination was recorded. Each measurement can be stocked only once.
CREATE TABLE mini_waste_warehouse_receipts (
    cycle_id TEXT PRIMARY KEY REFERENCES mini_waste_bin_receipts(cycle_id),
    warehouse_id TEXT NOT NULL REFERENCES mini_warehouses(id) ON DELETE RESTRICT,
    warehouse_name TEXT NOT NULL CHECK (btrim(warehouse_name) <> ''),
    stocked_by_ref TEXT NOT NULL CHECK (btrim(stocked_by_ref) <> ''),
    stocked_by_display_name TEXT NOT NULL,
    stocked_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX mini_waste_warehouse_receipts_by_warehouse
    ON mini_waste_warehouse_receipts (warehouse_id);

ALTER TABLE mini_waste_warehouse_receipts OWNER TO mini_rs_erp_owner;
REVOKE ALL ON mini_waste_warehouse_receipts FROM PUBLIC;
GRANT SELECT, INSERT ON mini_waste_warehouse_receipts TO mini_rs_erp;

CREATE VIEW mini_waste_warehouse_balances WITH (security_invoker = true) AS
    SELECT w.warehouse_id, sum(r.gross_units - r.tare_units) / 1000000 AS on_hand_kg,
           count(*) AS receipt_count
    FROM mini_waste_warehouse_receipts w
    JOIN mini_waste_bin_receipts r ON r.cycle_id = w.cycle_id
    GROUP BY w.warehouse_id;

ALTER VIEW mini_waste_warehouse_balances OWNER TO mini_rs_erp_owner;
REVOKE ALL ON mini_waste_warehouse_balances FROM PUBLIC;
GRANT SELECT ON mini_waste_warehouse_balances TO mini_rs_erp;
