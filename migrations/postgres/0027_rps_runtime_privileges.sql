-- RPS tables were introduced after the original runtime ownership migration.
-- Fail startup instead of running with a role that can read the active batch
-- but cannot reserve identities or archive a stopped batch.
DO $$
DECLARE
    table_name TEXT;
    table_owner TEXT;
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        RAISE EXCEPTION 'required runtime role mini_rs_erp does not exist';
    END IF;

    FOREACH table_name IN ARRAY ARRAY[
        'mini_rps_batches',
        'mini_rps_batch_history',
        'mini_rps_batch_identities'
    ]
    LOOP
        IF to_regclass(format('public.%I', table_name)) IS NULL THEN
            RAISE EXCEPTION 'required RPS table public.% does not exist', table_name;
        END IF;

        SELECT tableowner
        INTO table_owner
        FROM pg_tables
        WHERE schemaname = 'public' AND tablename = table_name;

        IF table_owner = current_user
            OR pg_has_role(current_user, table_owner, 'MEMBER')
        THEN
            EXECUTE format(
                'GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.%I TO mini_rs_erp',
                table_name
            );
        ELSIF NOT has_table_privilege(
            'mini_rs_erp',
            format('public.%I', table_name),
            'SELECT,INSERT,UPDATE,DELETE'
        ) THEN
            RAISE EXCEPTION
                'migration user % cannot grant RPS privileges on public.% owned by %; run migrations with MINI_ERP_MIGRATION_DATABASE_URL using the table owner',
                current_user,
                table_name,
                table_owner;
        END IF;
    END LOOP;
END;
$$;
