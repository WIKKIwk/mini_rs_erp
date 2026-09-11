SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- Keep the application role usable after upgrades and restores. Migrations are
-- run with the owner credential; the HTTP service runs as mini_rs_erp.
DO $$
DECLARE
    relation_name TEXT;
    sequence_name TEXT;
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        RAISE EXCEPTION 'required runtime role mini_rs_erp does not exist';
    END IF;

    FOR relation_name IN
        SELECT c.relname
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'public'
          AND c.relkind IN ('r', 'p')
    LOOP
        EXECUTE format(
            'GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.%I TO mini_rs_erp',
            relation_name
        );
    END LOOP;

    FOR sequence_name IN
        SELECT c.relname
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'public'
          AND c.relkind = 'S'
    LOOP
        EXECUTE format(
            'GRANT USAGE, SELECT, UPDATE ON SEQUENCE public.%I TO mini_rs_erp',
            sequence_name
        );
    END LOOP;

    -- This endpoint intentionally resets the number sequence in a transaction
    -- and therefore needs ownership, not merely USAGE/UPDATE privileges.
    IF to_regclass('public.mini_production_order_number_seq') IS NOT NULL THEN
        ALTER SEQUENCE public.mini_production_order_number_seq OWNER TO mini_rs_erp;
    END IF;

    -- Preserve append-only protections even though the broad grant above keeps
    -- the runtime role compatible with all existing CRUD repositories.
    FOREACH relation_name IN ARRAY ARRAY[
        'mini_canonical_apparatus_identities',
        'mini_canonical_apparatus_revisions',
        'mini_preparation_allocations',
        'mini_preparation_operations',
        'mini_preparation_receipts',
        'mini_raw_material_events',
        'mini_raw_material_split_issues',
        'mini_raw_material_split_outputs',
        'mini_raw_material_splits'
    ]
    LOOP
        IF to_regclass(format('public.%I', relation_name)) IS NOT NULL THEN
            EXECUTE format(
                'REVOKE UPDATE, DELETE ON TABLE public.%I FROM mini_rs_erp',
                relation_name
            );
        END IF;
    END LOOP;

    -- New objects created by this migration owner inherit the same runtime
    -- access on future upgrades. Existing objects are handled above.
    EXECUTE format(
        'ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA public GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO mini_rs_erp',
        current_user
    );
    EXECUTE format(
        'ALTER DEFAULT PRIVILEGES FOR ROLE %I IN SCHEMA public GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO mini_rs_erp',
        current_user
    );
END;
$$;
