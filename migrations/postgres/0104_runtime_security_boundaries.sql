SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- Apply with an operator/migration credential, never the HTTP login. Scope
-- ownership changes to this database and its public mini_* objects only.
DO $$
DECLARE obj RECORD; creator TEXT; target RECORD;
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        RAISE EXCEPTION 'required runtime role mini_rs_erp does not exist';
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp'
               AND (rolsuper OR rolcreaterole OR rolcreatedb OR rolbypassrls)) THEN
        RAISE EXCEPTION 'runtime role must not have administrative privileges';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp_owner') THEN
        CREATE ROLE mini_rs_erp_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS;
    END IF;
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp_owner'
               AND (rolcanlogin OR rolsuper OR rolcreaterole OR rolcreatedb OR rolbypassrls))
       OR pg_has_role('mini_rs_erp', 'mini_rs_erp_owner', 'MEMBER') THEN
        RAISE EXCEPTION 'mini_rs_erp_owner must be a non-login owner unavailable to runtime';
    END IF;

    -- Do not silently transfer an unrelated database to the ERP role.
    IF (SELECT datdba FROM pg_database WHERE datname = current_database())
       NOT IN ('mini_rs_erp'::regrole, 'mini_rs_erp_owner'::regrole, current_user::regrole) THEN
        RAISE EXCEPTION 'unexpected database owner; operator review required';
    END IF;
    EXECUTE format('ALTER DATABASE %I OWNER TO mini_rs_erp_owner', current_database());
    EXECUTE format('REVOKE CREATE ON DATABASE %I FROM mini_rs_erp, PUBLIC', current_database());
    EXECUTE format('GRANT CONNECT, TEMPORARY ON DATABASE %I TO mini_rs_erp', current_database());
    ALTER SCHEMA public OWNER TO mini_rs_erp_owner;
    REVOKE CREATE ON SCHEMA public FROM mini_rs_erp, PUBLIC;
    GRANT USAGE ON SCHEMA public TO mini_rs_erp;

    FOR obj IN SELECT oid, relname, relkind FROM pg_class
        WHERE relnamespace = 'public'::regnamespace AND relname LIKE 'mini\_%'
          AND relkind IN ('r', 'p', 'v', 'm')
    LOOP
        EXECUTE format('ALTER %s public.%I OWNER TO mini_rs_erp_owner',
            CASE obj.relkind WHEN 'v' THEN 'VIEW' WHEN 'm' THEN 'MATERIALIZED VIEW' ELSE 'TABLE' END,
            obj.relname);
        EXECUTE format('REVOKE ALL ON TABLE public.%I FROM mini_rs_erp, PUBLIC', obj.relname);
        EXECUTE format('GRANT SELECT ON TABLE public.%I TO mini_rs_erp', obj.relname);
        IF obj.relkind IN ('r', 'p') AND obj.relname <> 'mini_schema_migrations' THEN
            EXECUTE format('GRANT INSERT, UPDATE, DELETE ON TABLE public.%I TO mini_rs_erp', obj.relname);
        END IF;
    END LOOP;
    FOR obj IN SELECT relname FROM pg_class WHERE relnamespace = 'public'::regnamespace
        AND relname LIKE 'mini\_%' AND relkind = 'S'
    LOOP
        EXECUTE format('ALTER SEQUENCE public.%I OWNER TO mini_rs_erp_owner', obj.relname);
        EXECUTE format('REVOKE ALL ON SEQUENCE public.%I FROM mini_rs_erp, PUBLIC', obj.relname);
        EXECUTE format('GRANT USAGE, SELECT, UPDATE ON SEQUENCE public.%I TO mini_rs_erp', obj.relname);
    END LOOP;
    FOR obj IN SELECT oid::regprocedure AS signature FROM pg_proc
        WHERE pronamespace = 'public'::regnamespace AND proname LIKE 'mini\_%' AND prokind = 'f'
    LOOP
        EXECUTE format('ALTER FUNCTION %s OWNER TO mini_rs_erp_owner', obj.signature);
        EXECUTE format('REVOKE ALL ON FUNCTION %s FROM PUBLIC', obj.signature);
        EXECUTE format('GRANT EXECUTE ON FUNCTION %s TO mini_rs_erp', obj.signature);
    END LOOP;

    FOR target IN SELECT * FROM (VALUES
        ('mini_canonical_apparatus_identities', 'apparatus_id'),
        ('mini_canonical_apparatus_revisions', 'apparatus_id'),
        ('mini_preparation_allocations', 'operation_id'),
        ('mini_preparation_operations', 'id'),
        ('mini_preparation_receipts', 'id'),
        ('mini_raw_material_events', 'id'),
        ('mini_raw_material_split_issues', 'id'),
        ('mini_raw_material_split_outputs', 'split_id'),
        ('mini_raw_material_splits', 'id')
    ) AS protected(table_name, key_column)
    LOOP
        IF NOT EXISTS (SELECT 1 FROM pg_trigger t JOIN pg_proc p ON p.oid = t.tgfoid
            WHERE t.tgrelid = to_regclass(format('public.%I', target.table_name))
              AND NOT t.tgisinternal AND t.tgenabled IN ('O', 'A')
              AND (t.tgtype::integer & 27) = 27 AND p.pronamespace = 'public'::regnamespace
              AND p.proname IN ('mini_raw_material_events_block_mutation',
                               'mini_reject_canonical_identity_or_revision_mutation')) THEN
            RAISE EXCEPTION 'immutable guard missing on %', target.table_name;
        END IF;
        EXECUTE format('REVOKE UPDATE, DELETE ON public.%I FROM mini_rs_erp', target.table_name);
        -- Both FK directions need row locks. The owner retains its own UPDATE;
        -- runtime only gets one column for explicit row-locking SELECTs.
        EXECUTE format('GRANT UPDATE (%I) ON public.%I TO mini_rs_erp', target.key_column, target.table_name);
        EXECUTE format('GRANT SELECT, INSERT, UPDATE, DELETE ON public.%I TO mini_rs_erp_owner', target.table_name);
    END LOOP;

    FOREACH creator IN ARRAY ARRAY[current_user::text, 'mini_rs_erp_owner'] LOOP
        EXECUTE format('ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO mini_rs_erp', creator);
        EXECUTE format('ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO mini_rs_erp', creator);
        EXECUTE format('ALTER DEFAULT PRIVILEGES FOR ROLE %I REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC', creator);
        EXECUTE format('ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA public GRANT EXECUTE ON FUNCTIONS TO mini_rs_erp', creator);
    END LOOP;
END;
$$;

-- A caller-controlled custom GUC is not authorization. Only the protected
-- definer may delete order history; no reset override exists for UPDATE or
-- any of the other append-only tables sharing this trigger function.
CREATE OR REPLACE FUNCTION public.mini_raw_material_events_block_mutation()
RETURNS trigger LANGUAGE plpgsql SECURITY INVOKER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF current_user = 'mini_rs_erp_owner'
       AND current_setting('mini_rs_erp.order_reset', true) = 'on'
       AND TG_TABLE_SCHEMA = 'public' AND TG_TABLE_NAME = 'mini_raw_material_events'
       AND TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RAISE EXCEPTION '% is append-only', TG_TABLE_NAME USING ERRCODE = '55000';
END;
$$;

CREATE OR REPLACE FUNCTION public.mini_reset_order_events(order_ids TEXT[])
RETURNS BIGINT LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    removed BIGINT;
    previous_reset TEXT := current_setting('mini_rs_erp.order_reset', true);
BEGIN
    IF order_ids IS NULL OR EXISTS (
        SELECT 1 FROM unnest(order_ids) AS ids(id) WHERE id IS NULL OR btrim(id) = ''
    ) THEN
        RAISE EXCEPTION 'order reset ids are invalid' USING ERRCODE = '22023';
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('mini-rs-erp:emergency-reset:orders', 0));
    -- Initialize the custom GUC through set_config rather than a function SET
    -- clause: an unregistered placeholder in that clause requires superuser
    -- privileges on some PostgreSQL sessions. Actual authorization is still
    -- the definer identity checked by the trigger, never this flag alone.
    PERFORM set_config('mini_rs_erp.order_reset', 'on', true);
    DELETE FROM public.mini_raw_material_events
    WHERE lower(order_id) IN (SELECT lower(id) FROM unnest(order_ids) AS ids(id));
    GET DIAGNOSTICS removed = ROW_COUNT;
    PERFORM set_config('mini_rs_erp.order_reset', COALESCE(previous_reset, ''), true);
    RETURN removed;
END;
$$;

CREATE OR REPLACE FUNCTION public.mini_reset_order_number_sequence()
RETURNS VOID LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended('mini-rs-erp:emergency-reset:orders', 0));
    -- The backend calls this only after deleting/resetting all order state.
    IF EXISTS (SELECT 1 FROM public.mini_orders)
       OR EXISTS (SELECT 1 FROM public.mini_production_maps) THEN
        RAISE EXCEPTION 'orders must be reset before restarting numbering' USING ERRCODE = '55000';
    END IF;
    ALTER SEQUENCE public.mini_production_order_number_seq RESTART WITH 1;
END;
$$;

ALTER FUNCTION public.mini_raw_material_events_block_mutation() OWNER TO mini_rs_erp_owner;
ALTER FUNCTION public.mini_reset_order_events(TEXT[]) OWNER TO mini_rs_erp_owner;
ALTER FUNCTION public.mini_reset_order_number_sequence() OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_raw_material_events_block_mutation(),
    public.mini_reset_order_events(TEXT[]), public.mini_reset_order_number_sequence() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION public.mini_raw_material_events_block_mutation(),
    public.mini_reset_order_events(TEXT[]), public.mini_reset_order_number_sequence() TO mini_rs_erp;
