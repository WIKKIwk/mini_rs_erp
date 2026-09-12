\set ON_ERROR_STOP on
-- Run only in an empty, disposable database named mini_rs_erp_test_runtime_fk_*.
-- The existing mini_rs_erp role is reused; no cluster roles are created/altered.
BEGIN;
SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '15s';
DO $$
BEGIN
    IF current_database() NOT LIKE 'mini\_rs\_erp\_test\_runtime\_fk\_%'
       OR EXISTS (SELECT 1 FROM pg_tables WHERE schemaname = 'public') THEN
        RAISE EXCEPTION 'an empty disposable runtime-FK test database is required';
    END IF;
END;
$$;

CREATE FUNCTION public.mini_reject_canonical_identity_or_revision_mutation()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'canonical rows are append-only' USING ERRCODE = '55000';
END;
$$;
CREATE FUNCTION public.mini_raw_material_events_block_mutation()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'raw material rows are append-only';
END;
$$;

CREATE TEMP TABLE expected_runtime_locks (
    table_name TEXT, column_name TEXT, trigger_name TEXT, function_name TEXT
);
INSERT INTO expected_runtime_locks VALUES
    ('mini_canonical_apparatus_identities', 'apparatus_id', 'mini_canonical_identity_immutable', 'mini_reject_canonical_identity_or_revision_mutation'),
    ('mini_canonical_apparatus_revisions', 'apparatus_id', 'mini_canonical_revision_immutable', 'mini_reject_canonical_identity_or_revision_mutation'),
    ('mini_preparation_operations', 'id', 'mini_preparation_operations_immutable', 'mini_raw_material_events_block_mutation'),
    ('mini_preparation_receipts', 'id', 'mini_preparation_receipts_immutable', 'mini_raw_material_events_block_mutation'),
    ('mini_raw_material_splits', 'id', 'mini_raw_material_splits_immutable', 'mini_raw_material_events_block_mutation'),
    ('mini_raw_material_split_issues', 'id', 'mini_raw_material_split_issues_immutable', 'mini_raw_material_events_block_mutation');
GRANT SELECT ON expected_runtime_locks TO mini_rs_erp;

DO $$
DECLARE target RECORD;
BEGIN
    FOR target IN SELECT * FROM expected_runtime_locks LOOP
        EXECUTE format('CREATE TABLE public.%I (%I TEXT PRIMARY KEY, payload TEXT NOT NULL)',
            target.table_name, target.column_name);
        EXECUTE format('ALTER TABLE public.%I OWNER TO mini_rs_erp', target.table_name);
        EXECUTE format('CREATE TRIGGER %I BEFORE UPDATE OR DELETE ON public.%I
            FOR EACH ROW EXECUTE FUNCTION public.%I()',
            target.trigger_name, target.table_name, target.function_name);
        EXECUTE format('INSERT INTO public.%I VALUES (''existing'', ''unchanged'')', target.table_name);
        EXECUTE format('CREATE TABLE public.%I (parent_id TEXT REFERENCES public.%I(%I))',
            'probe_' || target.table_name, target.table_name, target.column_name);
    END LOOP;
END;
$$;

\ir ../../../migrations/postgres/0102_runtime_privileges.sql
SET LOCAL ROLE mini_rs_erp;
DO $$
DECLARE target RECORD;
BEGIN
    FOR target IN SELECT * FROM expected_runtime_locks LOOP
        BEGIN
            EXECUTE format('INSERT INTO public.%I VALUES (''existing'')', 'probe_' || target.table_name);
            RAISE EXCEPTION 'expected 0102 FK permission failure on %', target.table_name
                USING ERRCODE = 'XX000';
        EXCEPTION WHEN insufficient_privilege THEN
            NULL;
        END;
    END LOOP;
END;
$$;
\echo PASS: reproduced all six foreign-key permission failures under the runtime owner
RESET ROLE;

\ir ../../../migrations/postgres/0103_runtime_foreign_key_locks.sql
\ir ../../../migrations/postgres/0103_runtime_foreign_key_locks.sql
SET LOCAL ROLE mini_rs_erp;
DO $$
DECLARE target RECORD;
BEGIN
    FOR target IN SELECT * FROM expected_runtime_locks LOOP
        IF has_table_privilege(current_user, 'public.' || target.table_name, 'UPDATE')
           OR has_table_privilege(current_user, 'public.' || target.table_name, 'DELETE')
           OR has_column_privilege(current_user, 'public.' || target.table_name, 'payload', 'UPDATE') THEN
            RAISE EXCEPTION 'repair granted broader rights than row locking needs';
        END IF;
        EXECUTE format('INSERT INTO public.%I VALUES (''existing'')', 'probe_' || target.table_name);
        EXECUTE format('SELECT 1 FROM public.%I FOR KEY SHARE', target.table_name);
        BEGIN
            EXECUTE format('UPDATE public.%I SET %I = %I',
                target.table_name, target.column_name, target.column_name);
            RAISE EXCEPTION 'immutable UPDATE was accepted on %', target.table_name
                USING ERRCODE = 'XX000';
        EXCEPTION WHEN SQLSTATE '55000' OR raise_exception THEN
            IF position('append-only' IN SQLERRM) = 0 THEN RAISE; END IF;
        END;
        BEGIN
            EXECUTE format('DELETE FROM public.%I', target.table_name);
            RAISE EXCEPTION 'immutable DELETE was accepted on %', target.table_name
                USING ERRCODE = 'XX000';
        EXCEPTION WHEN insufficient_privilege THEN
            NULL;
        END;
    END LOOP;
END;
$$;
\echo PASS: six FK inserts and row locks succeed; UPDATE guards and DELETE restrictions remain effective
RESET ROLE;

-- A separate migration owner must work too; the runtime is not always owner.
DO $$
DECLARE target RECORD;
BEGIN
    FOR target IN SELECT * FROM expected_runtime_locks LOOP
        EXECUTE format('ALTER TABLE public.%I OWNER TO %I', target.table_name, current_user);
    END LOOP;
END;
$$;
SET LOCAL ROLE mini_rs_erp;
DO $$
DECLARE target RECORD;
BEGIN
    FOR target IN SELECT * FROM expected_runtime_locks LOOP
        EXECUTE format('INSERT INTO public.%I VALUES (''existing'')', 'probe_' || target.table_name);
    END LOOP;
END;
$$;
RESET ROLE;
\echo PASS: distinct migration-owner and runtime roles also support FK inserts

SAVEPOINT missing_guard;
ALTER TABLE public.mini_canonical_apparatus_identities
    DISABLE TRIGGER mini_canonical_identity_immutable;
\set ON_ERROR_STOP off
\ir ../../../migrations/postgres/0103_runtime_foreign_key_locks.sql
\if :ERROR
    \echo PASS: migration refuses to grant rights when an immutable guard is disabled
\else
    \echo FAIL: migration accepted a disabled immutable guard
    \quit 1
\endif
\set ON_ERROR_STOP on
ROLLBACK TO SAVEPOINT missing_guard;
ROLLBACK;
