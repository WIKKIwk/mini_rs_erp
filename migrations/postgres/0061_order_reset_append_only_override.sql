-- Order reset is the only destructive workflow allowed to remove order-derived
-- raw-material events. Normal writes remain append-only; the transaction-local
-- setting is enabled only by the order reset store after the full backup step.
CREATE OR REPLACE FUNCTION mini_raw_material_events_block_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF current_setting('mini_rs_erp.order_reset', true) = 'on' THEN
        IF TG_OP = 'DELETE' THEN
            RETURN OLD;
        END IF;
        RETURN NEW;
    END IF;

    RAISE EXCEPTION 'mini_raw_material_events is append-only';
END;
$$;
