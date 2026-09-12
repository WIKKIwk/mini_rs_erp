\set ON_ERROR_STOP on
\if :{?runtime_role}
\else
    \set runtime_role mini_rs_erp
\endif
BEGIN READ ONLY;
SET LOCAL statement_timeout = '15s';
SELECT set_config('mini_rs_erp.audit_runtime_role', :'runtime_role', true);
DO $$
DECLARE
    runtime_role TEXT := current_setting('mini_rs_erp.audit_runtime_role');
    relation RECORD;
    owner_oid OID := to_regrole('mini_rs_erp_owner');
    hardened BOOLEAN := EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp_owner')
        AND to_regprocedure('public.mini_reset_order_events(text[])') IS NOT NULL;
    append_only TEXT[] := ARRAY[
        'mini_canonical_apparatus_identities', 'mini_canonical_apparatus_revisions',
        'mini_preparation_allocations', 'mini_preparation_operations', 'mini_preparation_receipts',
        'mini_raw_material_events', 'mini_raw_material_split_issues',
        'mini_raw_material_split_outputs', 'mini_raw_material_splits'
    ];
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = runtime_role) THEN
        RAISE EXCEPTION 'runtime role % does not exist', runtime_role;
    END IF;
    IF hardened THEN
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = runtime_role
                   AND (rolsuper OR rolcreaterole OR rolcreatedb OR rolbypassrls))
           OR pg_has_role(runtime_role, 'mini_rs_erp_owner', 'MEMBER')
           OR has_schema_privilege(runtime_role, 'public', 'CREATE')
           OR has_database_privilege(runtime_role, current_database(), 'CREATE') THEN
            RAISE EXCEPTION 'runtime can bypass the ownership boundary';
        END IF;
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp_owner'
                   AND (rolcanlogin OR rolsuper OR rolcreaterole OR rolcreatedb OR rolbypassrls)) THEN
            RAISE EXCEPTION 'unsafe schema owner role';
        END IF;
    END IF;
    FOR relation IN
        SELECT c.oid, c.relname, c.relkind, c.relowner FROM pg_class c
        WHERE c.relnamespace = 'public'::regnamespace
          AND c.relname LIKE 'mini\_%' AND c.relkind IN ('r', 'p', 'S')
    LOOP
        IF hardened AND relation.relowner <> owner_oid THEN
            RAISE EXCEPTION 'unexpected owner on %', relation.relname;
        END IF;
        IF relation.relkind = 'S' THEN
            IF NOT has_sequence_privilege(runtime_role, relation.oid, 'USAGE')
               OR NOT has_sequence_privilege(runtime_role, relation.oid, 'SELECT')
               OR NOT has_sequence_privilege(runtime_role, relation.oid, 'UPDATE') THEN
                RAISE EXCEPTION 'runtime sequence privilege missing on %', relation.relname;
            END IF;
            CONTINUE;
        END IF;
        IF hardened AND relation.relname = 'mini_schema_migrations' THEN
            IF NOT has_table_privilege(runtime_role, relation.oid, 'SELECT')
               OR has_table_privilege(runtime_role, relation.oid, 'INSERT,UPDATE,DELETE,TRUNCATE') THEN
                RAISE EXCEPTION 'runtime migration history must be read-only';
            END IF;
            CONTINUE;
        END IF;
        IF NOT has_table_privilege(runtime_role, relation.oid, 'SELECT')
           OR NOT has_table_privilege(runtime_role, relation.oid, 'INSERT') THEN
            RAISE EXCEPTION 'runtime read/insert privilege missing on %', relation.relname;
        END IF;
        IF relation.relname = ANY(append_only) THEN
            IF has_table_privilege(runtime_role, relation.oid, 'UPDATE')
               OR has_table_privilege(runtime_role, relation.oid, 'DELETE') THEN
                RAISE EXCEPTION 'append-only table has broad mutation privileges: %', relation.relname;
            END IF;
            IF NOT EXISTS (
                SELECT 1 FROM pg_trigger WHERE tgrelid = relation.oid AND NOT tgisinternal
                  AND tgenabled IN ('O', 'A') AND (tgtype::integer & 27) = 27
            ) THEN
                RAISE EXCEPTION 'immutable guard missing on %', relation.relname;
            END IF;
            IF hardened AND (SELECT count(*) FROM pg_attribute
                WHERE attrelid = relation.oid AND attnum > 0 AND NOT attisdropped
                  AND has_column_privilege(runtime_role, relation.oid, attnum, 'UPDATE')) <> 1 THEN
                RAISE EXCEPTION 'append-only row locks require exactly one runtime UPDATE column: %', relation.relname;
            END IF;
        ELSIF NOT has_table_privilege(runtime_role, relation.oid, 'UPDATE')
           OR NOT has_table_privilege(runtime_role, relation.oid, 'DELETE') THEN
            RAISE EXCEPTION 'runtime write privilege missing on %', relation.relname;
        END IF;
        IF EXISTS (SELECT 1 FROM pg_constraint WHERE contype = 'f'
                   AND (confrelid = relation.oid OR conrelid = relation.oid))
           AND NOT has_any_column_privilege(relation.relowner, relation.oid, 'UPDATE') THEN
            RAISE EXCEPTION 'FK owner cannot acquire KEY SHARE on %', relation.relname;
        END IF;
    END LOOP;
    FOR relation IN SELECT p.oid, p.proname, p.proowner, p.prosecdef, p.proconfig, p.proacl
        FROM pg_proc p WHERE p.pronamespace = 'public'::regnamespace AND p.proname LIKE 'mini\_%'
    LOOP
        IF NOT has_function_privilege(runtime_role, relation.oid, 'EXECUTE') THEN
            RAISE EXCEPTION 'runtime function EXECUTE missing on %', relation.proname;
        END IF;
        IF hardened AND relation.proname IN ('mini_reset_order_events', 'mini_reset_order_number_sequence') THEN
            IF NOT relation.prosecdef OR relation.proowner <> owner_oid
               OR NOT COALESCE(relation.proconfig @> ARRAY['search_path=pg_catalog, pg_temp'], false)
               OR EXISTS (SELECT 1 FROM aclexplode(COALESCE(relation.proacl, acldefault('f', relation.proowner)))
                          WHERE grantee = 0 AND privilege_type = 'EXECUTE') THEN
                RAISE EXCEPTION 'unsafe reset definer function %', relation.proname;
            END IF;
        END IF;
    END LOOP;
    IF EXISTS (SELECT 1 FROM pg_class WHERE relnamespace = 'public'::regnamespace
               AND relkind IN ('v','m') AND relname LIKE 'mini\_%'
               AND NOT has_table_privilege(runtime_role, oid, 'SELECT')) THEN
        RAISE EXCEPTION 'runtime view SELECT missing';
    END IF;
END;
$$;
SELECT current_database() AS database, :'runtime_role' AS runtime_role,
    (SELECT count(*) FROM pg_tables WHERE schemaname = 'public') AS public_tables,
    (SELECT count(*) FROM pg_indexes WHERE schemaname = 'public') AS public_indexes,
    (SELECT count(*) FROM pg_index i JOIN pg_class c ON c.oid = i.indrelid
       WHERE c.relnamespace = 'public'::regnamespace AND (NOT i.indisvalid OR NOT i.indisready)) AS invalid_indexes,
    (SELECT count(*) FROM pg_constraint WHERE connamespace = 'public'::regnamespace AND NOT convalidated) AS unvalidated_constraints,
    (SELECT count(*) FROM mini_canonical_apparatus_projection_drift) AS apparatus_projection_drift;
ROLLBACK;
