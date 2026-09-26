-- Reusable maps survive order reset and do not reserve an order number.
CREATE OR REPLACE FUNCTION public.mini_reset_order_number_sequence()
RETURNS VOID LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended('mini-rs-erp:emergency-reset:orders', 0));
    IF EXISTS (SELECT 1 FROM public.mini_orders)
       OR EXISTS (
           SELECT 1 FROM public.mini_production_maps
           WHERE lower(btrim(id)) NOT LIKE 'template-%'
       ) THEN
        RAISE EXCEPTION 'orders must be reset before restarting numbering' USING ERRCODE = '55000';
    END IF;
    ALTER SEQUENCE public.mini_production_order_number_seq RESTART WITH 1;
END;
$$;

ALTER FUNCTION public.mini_reset_order_number_sequence() OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_reset_order_number_sequence() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION public.mini_reset_order_number_sequence() TO mini_rs_erp;
