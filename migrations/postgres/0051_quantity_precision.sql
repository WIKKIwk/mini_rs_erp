-- Keep every operational continuous quantity on one storage invariant.
-- UI formatting is intentionally separate from database precision.

ALTER TABLE mini_gscale_receipts
    ALTER COLUMN qty TYPE NUMERIC(18, 6)
        USING round(qty, 6)::NUMERIC(18, 6);

ALTER TABLE mini_raw_material_stock
    ALTER COLUMN qty TYPE NUMERIC(18, 6)
        USING round(qty, 6)::NUMERIC(18, 6);

ALTER TABLE mini_finished_goods_stock
    ALTER COLUMN qty TYPE NUMERIC(18, 6)
        USING round(qty, 6)::NUMERIC(18, 6);

ALTER TABLE mini_raw_material_events
    ALTER COLUMN qty_delta TYPE NUMERIC(18, 6)
        USING round(qty_delta, 6)::NUMERIC(18, 6);

ALTER TABLE mini_orders
    DROP CONSTRAINT IF EXISTS mini_orders_roll_count_positive;
ALTER TABLE mini_orders
    ALTER COLUMN kg TYPE NUMERIC(18, 6)
        USING round(kg, 6)::NUMERIC(18, 6),
    ALTER COLUMN width_mm TYPE NUMERIC(18, 6)
        USING round(width_mm, 6)::NUMERIC(18, 6),
    ALTER COLUMN roll_count TYPE INTEGER
        USING CASE WHEN roll_count IS NULL THEN NULL ELSE round(roll_count)::INTEGER END;
ALTER TABLE mini_orders
    ADD CONSTRAINT mini_orders_roll_count_positive
        CHECK (roll_count IS NULL OR roll_count > 0);

ALTER TABLE mini_production_maps
    DROP CONSTRAINT IF EXISTS mini_production_maps_roll_count_positive;
ALTER TABLE mini_production_maps
    ALTER COLUMN width_mm TYPE NUMERIC(18, 6)
        USING round(width_mm, 6)::NUMERIC(18, 6),
    ALTER COLUMN roll_count TYPE INTEGER
        USING CASE WHEN roll_count IS NULL THEN NULL ELSE round(roll_count)::INTEGER END;
ALTER TABLE mini_production_maps
    ADD CONSTRAINT mini_production_maps_roll_count_positive
        CHECK (roll_count IS NULL OR roll_count > 0);

ALTER TABLE mini_order_progress_events
    ALTER COLUMN produced_qty TYPE NUMERIC(18, 6)
        USING round(produced_qty, 6)::NUMERIC(18, 6),
    ALTER COLUMN return_ink_kg TYPE NUMERIC(18, 6)
        USING round(return_ink_kg, 6)::NUMERIC(18, 6),
    ALTER COLUMN lamination_print_leftover_rolls TYPE NUMERIC(18, 6)
        USING round(lamination_print_leftover_rolls, 6)::NUMERIC(18, 6),
    ALTER COLUMN lamination_film_leftover_rolls TYPE NUMERIC(18, 6)
        USING round(lamination_film_leftover_rolls, 6)::NUMERIC(18, 6),
    ALTER COLUMN rezka_bosma_waste TYPE NUMERIC(18, 6)
        USING round(rezka_bosma_waste, 6)::NUMERIC(18, 6),
    ALTER COLUMN rezka_lamination_waste TYPE NUMERIC(18, 6)
        USING round(rezka_lamination_waste, 6)::NUMERIC(18, 6),
    ALTER COLUMN rezka_edge_waste TYPE NUMERIC(18, 6)
        USING round(rezka_edge_waste, 6)::NUMERIC(18, 6),
    ALTER COLUMN total_waste TYPE NUMERIC(18, 6)
        USING round(total_waste, 6)::NUMERIC(18, 6),
    ALTER COLUMN finished_goods_kg TYPE NUMERIC(18, 6)
        USING round(finished_goods_kg, 6)::NUMERIC(18, 6),
    ALTER COLUMN finished_goods_meter TYPE NUMERIC(18, 6)
        USING round(finished_goods_meter, 6)::NUMERIC(18, 6),
    ALTER COLUMN diameter TYPE NUMERIC(18, 6)
        USING round(diameter, 6)::NUMERIC(18, 6),
    ALTER COLUMN bobina_kg TYPE NUMERIC(18, 6)
        USING round(bobina_kg, 6)::NUMERIC(18, 6);

ALTER TABLE mini_progress_batches
    ALTER COLUMN produced_qty TYPE NUMERIC(18, 6)
        USING round(produced_qty, 6)::NUMERIC(18, 6),
    ALTER COLUMN return_ink_kg TYPE NUMERIC(18, 6)
        USING round(return_ink_kg, 6)::NUMERIC(18, 6),
    ALTER COLUMN lamination_print_leftover_rolls TYPE NUMERIC(18, 6)
        USING round(lamination_print_leftover_rolls, 6)::NUMERIC(18, 6),
    ALTER COLUMN lamination_film_leftover_rolls TYPE NUMERIC(18, 6)
        USING round(lamination_film_leftover_rolls, 6)::NUMERIC(18, 6),
    ALTER COLUMN rezka_bosma_waste TYPE NUMERIC(18, 6)
        USING round(rezka_bosma_waste, 6)::NUMERIC(18, 6),
    ALTER COLUMN rezka_lamination_waste TYPE NUMERIC(18, 6)
        USING round(rezka_lamination_waste, 6)::NUMERIC(18, 6),
    ALTER COLUMN rezka_edge_waste TYPE NUMERIC(18, 6)
        USING round(rezka_edge_waste, 6)::NUMERIC(18, 6),
    ALTER COLUMN total_waste TYPE NUMERIC(18, 6)
        USING round(total_waste, 6)::NUMERIC(18, 6),
    ALTER COLUMN finished_goods_kg TYPE NUMERIC(18, 6)
        USING round(finished_goods_kg, 6)::NUMERIC(18, 6),
    ALTER COLUMN finished_goods_meter TYPE NUMERIC(18, 6)
        USING round(finished_goods_meter, 6)::NUMERIC(18, 6),
    ALTER COLUMN diameter TYPE NUMERIC(18, 6)
        USING round(diameter, 6)::NUMERIC(18, 6),
    ALTER COLUMN bobina_kg TYPE NUMERIC(18, 6)
        USING round(bobina_kg, 6)::NUMERIC(18, 6);

ALTER TABLE mini_inventory_transfer_lines
    ALTER COLUMN qty TYPE NUMERIC(18, 6)
        USING round(qty, 6)::NUMERIC(18, 6);

ALTER TABLE mini_inventory_movement_events
    ALTER COLUMN qty TYPE NUMERIC(18, 6)
        USING round(qty, 6)::NUMERIC(18, 6);

ALTER TABLE mini_laminatsiya_astatka_reports
    ALTER COLUMN lamination_print_leftover_rolls TYPE NUMERIC(18, 6)
        USING round(lamination_print_leftover_rolls, 6)::NUMERIC(18, 6),
    ALTER COLUMN lamination_film_leftover_rolls TYPE NUMERIC(18, 6)
        USING round(lamination_film_leftover_rolls, 6)::NUMERIC(18, 6),
    ALTER COLUMN total_waste TYPE NUMERIC(18, 6)
        USING round(total_waste, 6)::NUMERIC(18, 6),
    ALTER COLUMN finished_goods_meter TYPE NUMERIC(18, 6)
        USING round(finished_goods_meter, 6)::NUMERIC(18, 6),
    ALTER COLUMN finished_goods_kg TYPE NUMERIC(18, 6)
        USING round(finished_goods_kg, 6)::NUMERIC(18, 6),
    ALTER COLUMN bobina_kg TYPE NUMERIC(18, 6)
        USING round(bobina_kg, 6)::NUMERIC(18, 6);

ALTER TABLE mini_rezka_astatka_reports
    ALTER COLUMN total_waste TYPE NUMERIC(18, 6)
        USING round(total_waste, 6)::NUMERIC(18, 6),
    ALTER COLUMN rezka_bosma_waste TYPE NUMERIC(18, 6)
        USING round(rezka_bosma_waste, 6)::NUMERIC(18, 6),
    ALTER COLUMN rezka_lamination_waste TYPE NUMERIC(18, 6)
        USING round(rezka_lamination_waste, 6)::NUMERIC(18, 6),
    ALTER COLUMN rezka_edge_waste TYPE NUMERIC(18, 6)
        USING round(rezka_edge_waste, 6)::NUMERIC(18, 6),
    ALTER COLUMN finished_goods_meter TYPE NUMERIC(18, 6)
        USING round(finished_goods_meter, 6)::NUMERIC(18, 6),
    ALTER COLUMN finished_goods_kg TYPE NUMERIC(18, 6)
        USING round(finished_goods_kg, 6)::NUMERIC(18, 6),
    ALTER COLUMN bobina_kg TYPE NUMERIC(18, 6)
        USING round(bobina_kg, 6)::NUMERIC(18, 6);

-- Piece-count quantities must not silently accept fractions. Qolip quantity
-- and paddon sequence values are already INTEGER in their source tables.
ALTER TABLE mini_gscale_receipts
    ADD CONSTRAINT mini_gscale_receipts_dona_integer
        CHECK (lower(btrim(uom)) <> 'dona' OR qty = trunc(qty));
ALTER TABLE mini_raw_material_stock
    ADD CONSTRAINT mini_raw_material_stock_dona_integer
        CHECK (lower(btrim(uom)) <> 'dona' OR qty = trunc(qty));
ALTER TABLE mini_finished_goods_stock
    ADD CONSTRAINT mini_finished_goods_stock_dona_integer
        CHECK (lower(btrim(uom)) <> 'dona' OR qty = trunc(qty));
ALTER TABLE mini_raw_material_events
    ADD CONSTRAINT mini_raw_material_events_dona_integer
        CHECK (lower(btrim(uom)) <> 'dona' OR qty_delta = trunc(qty_delta));
ALTER TABLE mini_order_progress_events
    ADD CONSTRAINT mini_order_progress_events_dona_integer
        CHECK (lower(btrim(uom)) <> 'dona' OR produced_qty = trunc(produced_qty));
ALTER TABLE mini_progress_batches
    ADD CONSTRAINT mini_progress_batches_dona_integer
        CHECK (lower(btrim(uom)) <> 'dona' OR produced_qty = trunc(produced_qty));
ALTER TABLE mini_inventory_transfer_lines
    ADD CONSTRAINT mini_inventory_transfer_lines_dona_integer
        CHECK (lower(btrim(uom)) <> 'dona' OR qty = trunc(qty));
ALTER TABLE mini_inventory_movement_events
    ADD CONSTRAINT mini_inventory_movement_events_dona_integer
        CHECK (lower(btrim(uom)) <> 'dona' OR qty = trunc(qty));
