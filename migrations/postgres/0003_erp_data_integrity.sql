-- ERP quantities are exact decimal values. Nine fractional digits match the
-- configurable upper precision used by mature ERP systems while retaining
-- ample integer range for warehouse totals.
ALTER TABLE mini_gscale_receipts
    DROP CONSTRAINT IF EXISTS mini_gscale_receipts_qty_positive;
ALTER TABLE mini_gscale_receipts
    ALTER COLUMN qty TYPE NUMERIC(24, 9) USING qty::numeric(24, 9);
ALTER TABLE mini_gscale_receipts
    ADD CONSTRAINT mini_gscale_receipts_qty_positive
    CHECK (qty > 0 AND qty <> 'NaN'::numeric);

ALTER TABLE mini_raw_material_stock
    DROP CONSTRAINT IF EXISTS mini_raw_material_stock_qty_positive;
ALTER TABLE mini_raw_material_stock
    ALTER COLUMN qty TYPE NUMERIC(24, 9) USING qty::numeric(24, 9);
ALTER TABLE mini_raw_material_stock
    ADD CONSTRAINT mini_raw_material_stock_qty_positive
    CHECK (qty > 0 AND qty <> 'NaN'::numeric);

ALTER TABLE mini_finished_goods_stock
    DROP CONSTRAINT IF EXISTS mini_finished_goods_stock_qty_positive;
ALTER TABLE mini_finished_goods_stock
    ALTER COLUMN qty TYPE NUMERIC(24, 9) USING qty::numeric(24, 9);
ALTER TABLE mini_finished_goods_stock
    ADD CONSTRAINT mini_finished_goods_stock_qty_positive
    CHECK (qty > 0 AND qty <> 'NaN'::numeric);

ALTER TABLE mini_raw_material_events
    ALTER COLUMN qty_delta TYPE NUMERIC(24, 9) USING qty_delta::numeric(24, 9);
ALTER TABLE mini_raw_material_events
    ADD CONSTRAINT mini_raw_material_events_qty_finite
    CHECK (qty_delta <> 'NaN'::numeric);

ALTER TABLE mini_orders
    DROP CONSTRAINT IF EXISTS mini_orders_kg_non_negative,
    DROP CONSTRAINT IF EXISTS mini_orders_width_positive,
    DROP CONSTRAINT IF EXISTS mini_orders_roll_count_positive;
ALTER TABLE mini_orders
    ALTER COLUMN kg TYPE NUMERIC(24, 9) USING kg::numeric(24, 9),
    ALTER COLUMN width_mm TYPE NUMERIC(24, 9) USING width_mm::numeric(24, 9),
    ALTER COLUMN roll_count TYPE NUMERIC(24, 9) USING roll_count::numeric(24, 9);
ALTER TABLE mini_orders
    ADD CONSTRAINT mini_orders_kg_non_negative
        CHECK (kg >= 0 AND kg <> 'NaN'::numeric),
    ADD CONSTRAINT mini_orders_width_positive
        CHECK (width_mm IS NULL OR (width_mm > 0 AND width_mm <> 'NaN'::numeric)),
    ADD CONSTRAINT mini_orders_roll_count_positive
        CHECK (roll_count IS NULL OR (roll_count > 0 AND roll_count <> 'NaN'::numeric));

ALTER TABLE mini_production_maps
    DROP CONSTRAINT IF EXISTS mini_production_maps_width_positive,
    DROP CONSTRAINT IF EXISTS mini_production_maps_roll_count_positive;
ALTER TABLE mini_production_maps
    ALTER COLUMN width_mm TYPE NUMERIC(24, 9) USING width_mm::numeric(24, 9),
    ALTER COLUMN roll_count TYPE NUMERIC(24, 9) USING roll_count::numeric(24, 9);
ALTER TABLE mini_production_maps
    ADD CONSTRAINT mini_production_maps_width_positive
        CHECK (width_mm IS NULL OR (width_mm > 0 AND width_mm <> 'NaN'::numeric)),
    ADD CONSTRAINT mini_production_maps_roll_count_positive
        CHECK (roll_count IS NULL OR (roll_count > 0 AND roll_count <> 'NaN'::numeric));

ALTER TABLE mini_order_progress_events
    DROP CONSTRAINT IF EXISTS mini_order_progress_events_qty_non_negative;
ALTER TABLE mini_order_progress_events
    ALTER COLUMN produced_qty TYPE NUMERIC(24, 9) USING produced_qty::numeric(24, 9),
    ALTER COLUMN return_ink_kg TYPE NUMERIC(24, 9) USING return_ink_kg::numeric(24, 9),
    ALTER COLUMN lamination_print_leftover_rolls TYPE NUMERIC(24, 9) USING lamination_print_leftover_rolls::numeric(24, 9),
    ALTER COLUMN lamination_film_leftover_rolls TYPE NUMERIC(24, 9) USING lamination_film_leftover_rolls::numeric(24, 9),
    ALTER COLUMN rezka_bosma_waste TYPE NUMERIC(24, 9) USING rezka_bosma_waste::numeric(24, 9),
    ALTER COLUMN rezka_lamination_waste TYPE NUMERIC(24, 9) USING rezka_lamination_waste::numeric(24, 9),
    ALTER COLUMN rezka_edge_waste TYPE NUMERIC(24, 9) USING rezka_edge_waste::numeric(24, 9),
    ALTER COLUMN total_waste TYPE NUMERIC(24, 9) USING total_waste::numeric(24, 9),
    ALTER COLUMN finished_goods_kg TYPE NUMERIC(24, 9) USING finished_goods_kg::numeric(24, 9),
    ALTER COLUMN finished_goods_meter TYPE NUMERIC(24, 9) USING finished_goods_meter::numeric(24, 9);
ALTER TABLE mini_order_progress_events
    ADD CONSTRAINT mini_order_progress_events_qty_non_negative
    CHECK (produced_qty >= 0 AND produced_qty <> 'NaN'::numeric);

ALTER TABLE mini_progress_batches
    DROP CONSTRAINT IF EXISTS mini_progress_batches_qty_positive;
ALTER TABLE mini_progress_batches
    ALTER COLUMN produced_qty TYPE NUMERIC(24, 9) USING produced_qty::numeric(24, 9),
    ALTER COLUMN return_ink_kg TYPE NUMERIC(24, 9) USING return_ink_kg::numeric(24, 9),
    ALTER COLUMN lamination_print_leftover_rolls TYPE NUMERIC(24, 9) USING lamination_print_leftover_rolls::numeric(24, 9),
    ALTER COLUMN lamination_film_leftover_rolls TYPE NUMERIC(24, 9) USING lamination_film_leftover_rolls::numeric(24, 9),
    ALTER COLUMN rezka_bosma_waste TYPE NUMERIC(24, 9) USING rezka_bosma_waste::numeric(24, 9),
    ALTER COLUMN rezka_lamination_waste TYPE NUMERIC(24, 9) USING rezka_lamination_waste::numeric(24, 9),
    ALTER COLUMN rezka_edge_waste TYPE NUMERIC(24, 9) USING rezka_edge_waste::numeric(24, 9),
    ALTER COLUMN total_waste TYPE NUMERIC(24, 9) USING total_waste::numeric(24, 9),
    ALTER COLUMN finished_goods_kg TYPE NUMERIC(24, 9) USING finished_goods_kg::numeric(24, 9),
    ALTER COLUMN finished_goods_meter TYPE NUMERIC(24, 9) USING finished_goods_meter::numeric(24, 9);
ALTER TABLE mini_progress_batches
    ADD CONSTRAINT mini_progress_batches_qty_positive
    CHECK (produced_qty > 0 AND produced_qty <> 'NaN'::numeric);

ALTER TABLE mini_order_progress_events
    ADD CONSTRAINT mini_order_progress_events_metrics_non_negative CHECK (
        (return_ink_kg IS NULL OR (return_ink_kg >= 0 AND return_ink_kg <> 'NaN'::numeric))
        AND (lamination_print_leftover_rolls IS NULL OR (lamination_print_leftover_rolls >= 0 AND lamination_print_leftover_rolls <> 'NaN'::numeric))
        AND (lamination_film_leftover_rolls IS NULL OR (lamination_film_leftover_rolls >= 0 AND lamination_film_leftover_rolls <> 'NaN'::numeric))
        AND (rezka_bosma_waste IS NULL OR (rezka_bosma_waste >= 0 AND rezka_bosma_waste <> 'NaN'::numeric))
        AND (rezka_lamination_waste IS NULL OR (rezka_lamination_waste >= 0 AND rezka_lamination_waste <> 'NaN'::numeric))
        AND (rezka_edge_waste IS NULL OR (rezka_edge_waste >= 0 AND rezka_edge_waste <> 'NaN'::numeric))
        AND (total_waste IS NULL OR (total_waste >= 0 AND total_waste <> 'NaN'::numeric))
        AND (finished_goods_kg IS NULL OR (finished_goods_kg >= 0 AND finished_goods_kg <> 'NaN'::numeric))
        AND (finished_goods_meter IS NULL OR (finished_goods_meter >= 0 AND finished_goods_meter <> 'NaN'::numeric))
    );
ALTER TABLE mini_progress_batches
    ADD CONSTRAINT mini_progress_batches_metrics_non_negative CHECK (
        (return_ink_kg IS NULL OR (return_ink_kg >= 0 AND return_ink_kg <> 'NaN'::numeric))
        AND (lamination_print_leftover_rolls IS NULL OR (lamination_print_leftover_rolls >= 0 AND lamination_print_leftover_rolls <> 'NaN'::numeric))
        AND (lamination_film_leftover_rolls IS NULL OR (lamination_film_leftover_rolls >= 0 AND lamination_film_leftover_rolls <> 'NaN'::numeric))
        AND (rezka_bosma_waste IS NULL OR (rezka_bosma_waste >= 0 AND rezka_bosma_waste <> 'NaN'::numeric))
        AND (rezka_lamination_waste IS NULL OR (rezka_lamination_waste >= 0 AND rezka_lamination_waste <> 'NaN'::numeric))
        AND (rezka_edge_waste IS NULL OR (rezka_edge_waste >= 0 AND rezka_edge_waste <> 'NaN'::numeric))
        AND (total_waste IS NULL OR (total_waste >= 0 AND total_waste <> 'NaN'::numeric))
        AND (finished_goods_kg IS NULL OR (finished_goods_kg >= 0 AND finished_goods_kg <> 'NaN'::numeric))
        AND (finished_goods_meter IS NULL OR (finished_goods_meter >= 0 AND finished_goods_meter <> 'NaN'::numeric))
    );

-- The calculation template status field is the product form (for example,
-- `rulon`), not the order lifecycle. Keep those concepts separate.
ALTER TABLE mini_orders
    ADD COLUMN IF NOT EXISTS product_form TEXT NOT NULL DEFAULT '';
UPDATE mini_orders
SET product_form = status
WHERE btrim(product_form) = ''
  AND status NOT IN ('draft', 'ready', 'in_progress', 'completed', 'cancelled');
UPDATE mini_orders
SET status = 'draft'
WHERE status NOT IN ('draft', 'ready', 'in_progress', 'completed', 'cancelled');
ALTER TABLE mini_orders
    ADD CONSTRAINT mini_orders_status_allowed
    CHECK (status IN ('draft', 'ready', 'in_progress', 'completed', 'cancelled'));

-- Normalize phone uniqueness in the database, including concurrent writes.
ALTER TABLE mini_customers
    ADD COLUMN IF NOT EXISTS phone_key TEXT
    GENERATED ALWAYS AS (regexp_replace(phone, '[^0-9]', '', 'g')) STORED;
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_customers_phone_key_unique
    ON mini_customers (phone_key)
    WHERE phone_key <> '';

ALTER TABLE mini_workers
    ADD COLUMN IF NOT EXISTS phone_key TEXT
    GENERATED ALWAYS AS (regexp_replace(phone, '[^0-9]', '', 'g')) STORED;
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_workers_phone_key_unique
    ON mini_workers (phone_key)
    WHERE phone_key <> '';

-- Operational rows must reference live master/transaction rows. Immutable
-- event ledgers intentionally retain text references so history survives.
ALTER TABLE mini_raw_material_assignments
    ADD CONSTRAINT mini_raw_material_assignments_order_fkey
        FOREIGN KEY (order_id) REFERENCES mini_production_maps(id) ON DELETE RESTRICT,
    ADD CONSTRAINT mini_raw_material_assignments_item_fkey
        FOREIGN KEY (item_code) REFERENCES mini_items(code) ON UPDATE CASCADE ON DELETE RESTRICT,
    ADD CONSTRAINT mini_raw_material_assignments_stock_fkey
        FOREIGN KEY (barcode) REFERENCES mini_raw_material_stock(barcode) ON UPDATE CASCADE ON DELETE RESTRICT;

ALTER TABLE mini_order_run_sessions
    ADD CONSTRAINT mini_order_run_sessions_order_fkey
        FOREIGN KEY (order_id) REFERENCES mini_production_maps(id) ON DELETE RESTRICT;
ALTER TABLE mini_progress_batches
    ADD CONSTRAINT mini_progress_batches_order_fkey
        FOREIGN KEY (order_id) REFERENCES mini_production_maps(id) ON DELETE RESTRICT;
ALTER TABLE mini_queue_states
    ADD CONSTRAINT mini_queue_states_order_fkey
        FOREIGN KEY (order_id) REFERENCES mini_production_maps(id) ON DELETE RESTRICT;
ALTER TABLE mini_warehouse_assignments
    ADD CONSTRAINT mini_warehouse_assignments_warehouse_fkey
        FOREIGN KEY (warehouse) REFERENCES mini_warehouses(name) ON UPDATE CASCADE ON DELETE RESTRICT;

ALTER TABLE mini_production_maps
    ADD CONSTRAINT mini_production_maps_json_object
    CHECK (jsonb_typeof(map_json) = 'object');
ALTER TABLE mini_raw_material_stock
    ADD CONSTRAINT mini_raw_material_stock_json_object
    CHECK (jsonb_typeof(payload_json) = 'object');
ALTER TABLE mini_raw_material_events
    ADD CONSTRAINT mini_raw_material_events_json_object
    CHECK (jsonb_typeof(payload_json) = 'object');
