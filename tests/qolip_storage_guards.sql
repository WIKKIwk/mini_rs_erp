-- psql -v ON_ERROR_STOP=1 -d qolip_storage_guards_<unique> -f this_file
-- NEW disposable database only; fixture checks DB enforcement independently
-- of service validation, with the actual unprivileged runtime role.
\set ON_ERROR_STOP on
DO $$ BEGIN
    IF current_database() NOT LIKE 'qolip_storage_guards_%' THEN
        RAISE EXCEPTION 'An isolated qolip_storage_guards_* database is required';
    END IF;
    BEGIN CREATE ROLE mini_rs_erp_owner NOLOGIN; EXCEPTION WHEN duplicate_object THEN NULL; END;
    BEGIN CREATE ROLE mini_rs_erp LOGIN; EXCEPTION WHEN duplicate_object THEN NULL; END;
END $$;
BEGIN;
CREATE TABLE mini_qolip_product_specs(qolip_code text UNIQUE, qolip_set_id text, size integer);
CREATE TABLE mini_qolip_locations(qolip_code text, block text, quantity integer);
CREATE TABLE mini_qolip_checkouts(qolip_code text, status text);
CREATE TABLE mini_qolip_reworks(qolip_code text);
CREATE TABLE mini_qolip_product_overrides(qolip_code text);
CREATE TABLE mini_qolip_sets(set_id text PRIMARY KEY);
CREATE TABLE mini_qolip_set_products(set_id text);
CREATE TABLE mini_order_run_sessions(status text, payload_json jsonb);
CREATE TABLE mini_warehouses(name text, parent_warehouse text);
DO $$ DECLARE relation text; BEGIN
    FOREACH relation IN ARRAY ARRAY['mini_qolip_product_specs','mini_qolip_locations','mini_qolip_checkouts',
        'mini_qolip_reworks','mini_qolip_product_overrides','mini_qolip_sets','mini_qolip_set_products',
        'mini_order_run_sessions','mini_warehouses'] LOOP
        EXECUTE format('ALTER TABLE %I OWNER TO mini_rs_erp_owner', relation);
    END LOOP;
END $$;
GRANT SELECT,INSERT,UPDATE,DELETE ON ALL TABLES IN SCHEMA public TO mini_rs_erp;
\ir ../migrations/postgres/0173_qolip_paddons.sql
\ir ../migrations/postgres/0174_qolip_storage_guards.sql
CREATE FUNCTION pg_temp.expect_block(statement text, expected text) RETURNS void LANGUAGE plpgsql AS $$
BEGIN
    BEGIN EXECUTE statement;
    EXCEPTION WHEN OTHERS THEN
        IF SQLERRM NOT LIKE '%' || expected || '%' THEN RAISE; END IF;
        RETURN;
    END;
    RAISE EXCEPTION 'Statement unexpectedly succeeded: %', statement;
END $$;
SET ROLE mini_rs_erp;
INSERT INTO mini_warehouses VALUES ('Mold warehouse',''),('Block A','Mold warehouse');
INSERT INTO mini_qolip_sets VALUES ('set-a'),('alternative');
INSERT INTO mini_qolip_product_specs VALUES ('PACKED','set-a',420),('ARCHIVED','set-a',420),('FREE','alternative',420);
INSERT INTO mini_qolip_locations VALUES ('PACKED','Block A',3),('ARCHIVED','Block A',1),('FREE','Block A',1);
INSERT INTO mini_qolip_paddons(code,warehouse,created_by_role,created_by_ref,created_by_name)
VALUES ('QP-00001','Mold warehouse','qolipchi','worker','Worker');
INSERT INTO mini_qolip_paddon_members(id,paddon_code,qolip_code,added_by_ref,added_by_name)
VALUES ('member-one','QP-00001','PACKED','worker','Worker');
INSERT INTO mini_qolip_archive_entries(id,kind,code,warehouse,snapshot,archived_by_role,archived_by_ref,archived_by_name)
VALUES ('visit-one','qolip','ARCHIVED','Mold warehouse','{}','qolipchi','worker','Worker');
DO $$ BEGIN
    ASSERT mini_qolip_storage_state('FREE') = '';
    ASSERT mini_qolip_storage_state('UNKNOWN') = '';
    ASSERT mini_qolip_storage_state('  pAcKeD ') = 'paddon';
    ASSERT mini_qolip_storage_state('archived') = 'archived';
    ASSERT (SELECT quantity FROM mini_qolip_locations WHERE qolip_code='PACKED') = 3;
END $$;
SELECT pg_temp.expect_block('INSERT INTO mini_qolip_checkouts VALUES (''PACKED'',''open'')','qolip_in_paddon');
SELECT pg_temp.expect_block('UPDATE mini_qolip_locations SET quantity=2 WHERE qolip_code=''PACKED''','qolip_in_paddon');
SELECT pg_temp.expect_block('INSERT INTO mini_qolip_reworks VALUES (''PACKED'')','qolip_in_paddon');
SELECT pg_temp.expect_block('UPDATE mini_qolip_product_specs SET size=430 WHERE qolip_code=''ARCHIVED''','qolip_archived');
SELECT pg_temp.expect_block('INSERT INTO mini_qolip_set_products VALUES (''set-a'')','qolip_archived');
SELECT pg_temp.expect_block('DELETE FROM mini_warehouses WHERE name=''Mold warehouse''','qolip_history_locked');
SELECT pg_temp.expect_block('INSERT INTO mini_order_run_sessions VALUES (''active'',''{"qolip_lock_owner":true,"qolip_codes":["ARCHIVED"]}'')','qolip_archived');
SELECT pg_temp.expect_block('INSERT INTO mini_order_run_sessions VALUES (''paused'',''{"qolip_lock_owner":true,"qolip_code":"PACKED"}'')','qolip_in_paddon');
-- A healthy alternative stays usable; composition must not be filtered away.
INSERT INTO mini_order_run_sessions VALUES ('active','{"qolip_lock_owner":true,"qolip_code":"FREE"}');
INSERT INTO mini_qolip_set_products VALUES ('alternative');
UPDATE mini_qolip_paddons SET locked_at=now(),archived_at=now(),revision=revision+1 WHERE code='QP-00001';
DO $$ BEGIN ASSERT mini_qolip_storage_state('PACKED') = 'archived'; END $$;
UPDATE mini_qolip_paddons SET archived_at=NULL,revision=revision+1 WHERE code='QP-00001';
DO $$ BEGIN
    ASSERT mini_qolip_storage_state('PACKED') = 'paddon';
    ASSERT (SELECT locked_at IS NOT NULL FROM mini_qolip_paddons WHERE code='QP-00001');
END $$;
SELECT pg_temp.expect_block('INSERT INTO mini_qolip_checkouts VALUES (''PACKED'',''open'')','qolip_in_paddon');
UPDATE mini_qolip_paddons SET locked_at=NULL,revision=revision+1 WHERE code='QP-00001';
UPDATE mini_qolip_paddon_members SET removed_at=now(),removed_by_ref='worker',removed_by_name='Worker' WHERE id='member-one';
DO $$ BEGIN ASSERT mini_qolip_storage_state('PACKED') = ''; END $$;
INSERT INTO mini_qolip_checkouts VALUES ('PACKED','open');
UPDATE mini_qolip_archive_entries SET restored_at=now(),restored_by_role='qolipchi',restored_by_ref='worker',restored_by_name='Worker' WHERE id='visit-one';
DO $$ BEGIN ASSERT mini_qolip_storage_state('ARCHIVED') = ''; END $$;
SELECT pg_temp.expect_block('DELETE FROM mini_qolip_product_specs WHERE qolip_code=''PACKED''','qolip_history_locked');
SELECT pg_temp.expect_block('UPDATE mini_qolip_product_specs SET qolip_code=''RENAMED'' WHERE qolip_code=''ARCHIVED''','qolip_history_locked');
INSERT INTO mini_qolip_archive_entries(id,kind,code,warehouse,snapshot,archived_by_role,archived_by_ref,archived_by_name)
VALUES ('visit-two','qolip','ARCHIVED','Mold warehouse','{"cycle":2}','qolipchi','worker','Worker');
DO $$ BEGIN
    ASSERT mini_qolip_storage_state('ARCHIVED') = 'archived';
    ASSERT (SELECT count(*) FROM mini_qolip_archive_entries WHERE code='ARCHIVED') = 2;
    ASSERT (SELECT snapshot FROM mini_qolip_archive_entries WHERE id='visit-one') = '{}'::jsonb;
END $$;
RESET ROLE;
ROLLBACK;
\echo Qolip storage guards passed
