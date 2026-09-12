SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- PostgreSQL checks foreign keys with SELECT ... FOR KEY SHARE as the
-- referenced table's owner. That lock requires UPDATE on at least one column.
-- On installations where mini_rs_erp owns the tables, 0102 revoked that right
-- from the owner too, breaking otherwise permitted INSERTs into child tables.
-- Restore only a key-column privilege; immutable triggers still reject row
-- changes, and table-wide UPDATE and DELETE remain revoked.
DO $$
DECLARE
    target RECORD;
    relation_id REGCLASS;
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        RAISE EXCEPTION 'required runtime role mini_rs_erp does not exist';
    END IF;

    FOR target IN
        SELECT * FROM (VALUES
            ('mini_canonical_apparatus_identities', 'apparatus_id',
             'mini_canonical_identity_immutable',
             'mini_reject_canonical_identity_or_revision_mutation'),
            ('mini_canonical_apparatus_revisions', 'apparatus_id',
             'mini_canonical_revision_immutable',
             'mini_reject_canonical_identity_or_revision_mutation'),
            ('mini_preparation_operations', 'id',
             'mini_preparation_operations_immutable',
             'mini_raw_material_events_block_mutation'),
            ('mini_preparation_receipts', 'id',
             'mini_preparation_receipts_immutable',
             'mini_raw_material_events_block_mutation'),
            ('mini_raw_material_splits', 'id',
             'mini_raw_material_splits_immutable',
             'mini_raw_material_events_block_mutation'),
            ('mini_raw_material_split_issues', 'id',
             'mini_raw_material_split_issues_immutable',
             'mini_raw_material_events_block_mutation')
        ) AS required(table_name, column_name, trigger_name, function_name)
    LOOP
        relation_id := to_regclass(format('public.%I', target.table_name));
        IF relation_id IS NULL OR NOT EXISTS (
            SELECT 1 FROM pg_trigger
            WHERE tgrelid = relation_id
              AND tgname = target.trigger_name
              AND NOT tgisinternal
              AND tgenabled IN ('O', 'A')
              -- BEFORE ROW trigger covering both UPDATE and DELETE.
              AND (tgtype::integer & 27) = 27
              AND tgfoid = to_regprocedure(format('public.%I()', target.function_name))
        ) THEN
            RAISE EXCEPTION 'required immutable guard is unavailable on public.%',
                target.table_name;
        END IF;

        EXECUTE format(
            'GRANT UPDATE (%I) ON TABLE public.%I TO mini_rs_erp',
            target.column_name, target.table_name
        );
    END LOOP;
END;
$$;
