SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- Order editing must exclude concurrent history inserts, but the runtime
-- account must not gain table-wide UPDATE/DELETE on append-only history.
-- Only these two fixed lock targets are allowed; there is no caller SQL.
CREATE OR REPLACE FUNCTION public.mini_lock_order_edit_history(history_table TEXT)
RETURNS VOID LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    CASE history_table
        WHEN 'mini_raw_material_events' THEN
            LOCK TABLE public.mini_raw_material_events IN SHARE ROW EXCLUSIVE MODE;
        WHEN 'mini_preparation_operations' THEN
            LOCK TABLE public.mini_preparation_operations IN SHARE ROW EXCLUSIVE MODE;
        ELSE
            RAISE EXCEPTION 'unsupported order edit history table' USING ERRCODE = '22023';
    END CASE;
END;
$$;

ALTER FUNCTION public.mini_lock_order_edit_history(TEXT) OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_lock_order_edit_history(TEXT) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION public.mini_lock_order_edit_history(TEXT) TO mini_rs_erp;
