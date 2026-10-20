-- The readable 9501 + 8 digit identifier is the primary key itself.
-- The migration runner wraps this entire migration in one transaction.
SELECT pg_advisory_xact_lock(hashtextextended('qolip:product-composition', 0));
LOCK TABLE public.mini_qolip_product_specs, public.mini_qolip_sets,
    public.mini_qolip_set_products IN ACCESS EXCLUSIVE MODE;

CREATE SEQUENCE public.mini_qolip_set_id_seq AS BIGINT
    MINVALUE 1 MAXVALUE 99999999 NO CYCLE;
ALTER SEQUENCE public.mini_qolip_set_id_seq OWNER TO mini_rs_erp_owner;
REVOKE ALL ON SEQUENCE public.mini_qolip_set_id_seq FROM PUBLIC, mini_rs_erp;
SELECT setval('public.mini_qolip_set_id_seq',
    greatest(COALESCE(max(substring(set_id FROM 5)::BIGINT), 0), 1),
    COALESCE(max(substring(set_id FROM 5)::BIGINT), 0) > 0)
FROM public.mini_qolip_sets WHERE set_id ~ '^9501[0-9]{8}$';

CREATE FUNCTION public.mini_qolip_next_set_id() RETURNS TEXT
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    candidate TEXT;
BEGIN
    LOOP
        candidate := '9501' || lpad(nextval('public.mini_qolip_set_id_seq')::TEXT, 8, '0');
        EXIT WHEN NOT EXISTS (
            SELECT 1 FROM public.mini_qolip_sets WHERE set_id = candidate
            UNION ALL
            SELECT 1 FROM public.mini_qolip_product_specs WHERE qolip_code = candidate
            UNION ALL
            SELECT 1 FROM public.mini_qolip_locations WHERE qolip_code = candidate
            UNION ALL
            SELECT 1 FROM public.mini_qolip_checkouts WHERE qolip_code = candidate
        );
    END LOOP;
    RETURN candidate;
END;
$$;
ALTER FUNCTION public.mini_qolip_next_set_id() OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_qolip_next_set_id() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION public.mini_qolip_next_set_id() TO mini_rs_erp;

-- Recovery history only: application lookups never use the old identifiers.
CREATE TABLE public.mini_qolip_set_id_migrations (
    old_set_id TEXT PRIMARY KEY,
    old_display_code TEXT NOT NULL,
    old_public_code TEXT NOT NULL,
    new_set_id TEXT NOT NULL UNIQUE CHECK (new_set_id ~ '^9501[0-9]{8}$'),
    migrated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO public.mini_qolip_set_id_migrations
    (old_set_id, old_display_code, old_public_code, new_set_id)
SELECT set_id, display_code, set_code, public.mini_qolip_next_set_id()
FROM public.mini_qolip_sets
WHERE set_id !~ '^9501[0-9]{8}$'
ORDER BY created_at, set_id;
ALTER TABLE public.mini_qolip_set_id_migrations OWNER TO mini_rs_erp_owner;
REVOKE ALL ON public.mini_qolip_set_id_migrations FROM PUBLIC, mini_rs_erp;

CREATE TEMP TABLE qolip_bindings_before ON COMMIT DROP AS
SELECT * FROM public.mini_qolip_effective_product_molds;
CREATE TEMP TABLE qolip_links_before ON COMMIT DROP AS
SELECT COALESCE(history.new_set_id, link.set_id) AS set_id, link.item_code_key
FROM public.mini_qolip_set_products link
LEFT JOIN public.mini_qolip_set_id_migrations history ON history.old_set_id = link.set_id;

-- Generated foreign-key columns cannot use ON UPDATE CASCADE. Rebuild only
-- these two constraints after changing the registry, payloads and links.
ALTER TABLE public.mini_qolip_product_specs DROP CONSTRAINT mini_qolip_specs_set_scope_fk;
ALTER TABLE public.mini_qolip_set_products DROP CONSTRAINT mini_qolip_set_products_set_id_fkey;
DROP TRIGGER mini_qolip_specs_register_set ON public.mini_qolip_product_specs;
DROP TRIGGER mini_qolip_specs_initial_product ON public.mini_qolip_product_specs;
DROP TRIGGER mini_qolip_sets_assign_code ON public.mini_qolip_sets;
DROP FUNCTION public.mini_qolip_assign_set_code();

-- Rewrite exact ID values, including nested session/progress snapshots and
-- stored idempotent responses. No substring replacement of user text.
CREATE FUNCTION pg_temp.qolip_rekey_json(value JSONB) RETURNS JSONB
LANGUAGE plpgsql STABLE AS $$
DECLARE
    result JSONB;
    replacement TEXT;
BEGIN
    CASE jsonb_typeof(value)
    WHEN 'string' THEN
        SELECT new_set_id INTO replacement FROM public.mini_qolip_set_id_migrations
        WHERE old_set_id = value #>> '{}';
        RETURN COALESCE(to_jsonb(replacement), value);
    WHEN 'array' THEN
        SELECT COALESCE(jsonb_agg(pg_temp.qolip_rekey_json(element) ORDER BY ordinal), '[]'::JSONB)
        INTO result FROM jsonb_array_elements(value) WITH ORDINALITY AS entries(element, ordinal);
    WHEN 'object' THEN
        SELECT COALESCE(jsonb_object_agg(
            COALESCE(history.new_set_id, entry.key), pg_temp.qolip_rekey_json(entry.value)), '{}'::JSONB)
        INTO result FROM jsonb_each(value) entry
        LEFT JOIN public.mini_qolip_set_id_migrations history ON history.old_set_id = entry.key;
        IF value ? 'display_code' AND value ? 'set_id' THEN
            SELECT new_set_id INTO replacement FROM public.mini_qolip_set_id_migrations
            WHERE old_set_id = value->>'set_id';
            IF replacement IS NOT NULL THEN
                result := jsonb_set(result, '{display_code}', to_jsonb(replacement));
            END IF;
        END IF;
    ELSE
        result := value;
    END CASE;
    RETURN result;
END;
$$;

-- Some lineage lives in immutable receipts. Disable each affected table's
-- user triggers only while rewriting ID references under an exclusive lock,
-- then restore each trigger's exact original state. Foreign keys stay active.
DO $$
DECLARE
    column_info RECORD;
    saved_trigger RECORD;
    has_changes BOOLEAN;
    trigger_states JSONB;
BEGIN
    FOR column_info IN
        SELECT c.oid, n.nspname, c.relname, a.attname
        FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
        JOIN pg_attribute a ON a.attrelid = c.oid
        WHERE n.nspname = 'public' AND c.relname LIKE 'mini\_%' ESCAPE '\'
          AND c.relkind = 'r' AND a.attnum > 0 AND NOT a.attisdropped
          AND a.attgenerated = '' AND a.atttypid = 'jsonb'::regtype
        ORDER BY c.oid, a.attnum
    LOOP
        EXECUTE format(
            'SELECT EXISTS (SELECT 1 FROM %I.%I WHERE %I::text ~ ''"(qolip_set_id|set_id|qolip_set_snapshot)"''
             AND %I IS DISTINCT FROM pg_temp.qolip_rekey_json(%I))',
            column_info.nspname, column_info.relname, column_info.attname,
            column_info.attname, column_info.attname) INTO has_changes;
        IF NOT has_changes THEN CONTINUE; END IF;
        EXECUTE format('LOCK TABLE %I.%I IN ACCESS EXCLUSIVE MODE',
            column_info.nspname, column_info.relname);
        SELECT COALESCE(jsonb_agg(jsonb_build_object('name', tgname, 'state', tgenabled)), '[]'::JSONB)
        INTO trigger_states FROM pg_trigger
        WHERE tgrelid = column_info.oid AND NOT tgisinternal AND tgenabled <> 'D';
        FOR saved_trigger IN SELECT * FROM jsonb_to_recordset(trigger_states) AS x(name TEXT, state TEXT)
        LOOP
            EXECUTE format('ALTER TABLE %I.%I DISABLE TRIGGER %I',
                column_info.nspname, column_info.relname, saved_trigger.name);
        END LOOP;
        EXECUTE format(
            'UPDATE %I.%I SET %I = pg_temp.qolip_rekey_json(%I)
             WHERE %I::text ~ ''"(qolip_set_id|set_id|qolip_set_snapshot)"''
             AND %I IS DISTINCT FROM pg_temp.qolip_rekey_json(%I)',
            column_info.nspname, column_info.relname, column_info.attname, column_info.attname,
            column_info.attname, column_info.attname, column_info.attname);
        FOR saved_trigger IN SELECT * FROM jsonb_to_recordset(trigger_states) AS x(name TEXT, state TEXT)
        LOOP
            EXECUTE format('ALTER TABLE %I.%I ENABLE %s TRIGGER %I',
                column_info.nspname, column_info.relname,
                CASE saved_trigger.state WHEN 'A' THEN 'ALWAYS' WHEN 'R' THEN 'REPLICA' ELSE '' END,
                saved_trigger.name);
        END LOOP;
    END LOOP;
END;
$$;

UPDATE public.mini_qolip_set_products link SET set_id = history.new_set_id
FROM public.mini_qolip_set_id_migrations history WHERE link.set_id = history.old_set_id;
UPDATE public.mini_engine_events event SET entity_id = history.new_set_id
FROM public.mini_qolip_set_id_migrations history
WHERE event.domain = 'qolip' AND event.entity_id = history.old_set_id;
UPDATE public.mini_qolip_sets sets SET set_id = history.new_set_id
FROM public.mini_qolip_set_id_migrations history WHERE sets.set_id = history.old_set_id;

ALTER TABLE public.mini_qolip_sets
    DROP COLUMN set_code,
    DROP COLUMN display_code,
    ALTER COLUMN set_id SET DEFAULT public.mini_qolip_next_set_id(),
    ADD CONSTRAINT mini_qolip_sets_id_format CHECK (set_id ~ '^9501[0-9]{8}$');
ALTER SEQUENCE public.mini_qolip_set_id_seq OWNED BY public.mini_qolip_sets.set_id;
ALTER TABLE public.mini_qolip_product_specs ADD CONSTRAINT mini_qolip_specs_set_scope_fk
    FOREIGN KEY (qolip_set_id, qolip_set_warehouse_key)
    REFERENCES public.mini_qolip_sets(set_id, warehouse_key) ON UPDATE RESTRICT ON DELETE RESTRICT;
ALTER TABLE public.mini_qolip_set_products ADD CONSTRAINT mini_qolip_set_products_set_id_fkey
    FOREIGN KEY (set_id) REFERENCES public.mini_qolip_sets(set_id) ON UPDATE RESTRICT ON DELETE RESTRICT;

CREATE OR REPLACE FUNCTION public.mini_qolip_register_set()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    set_key TEXT := NULLIF(btrim(NEW.payload_json->>'qolip_set_id'), '');
    product_key TEXT := lower(btrim(NEW.item_code));
    owner_key TEXT := NULLIF(lower(btrim(NEW.payload_json->>'warehouse')), '');
    old_set_key TEXT;
BEGIN
    IF owner_key IS NULL THEN
        RAISE EXCEPTION 'qolip_warehouse_required' USING ERRCODE = '23514';
    END IF;
    IF TG_OP = 'UPDATE' THEN
        old_set_key := NULLIF(btrim(OLD.payload_json->>'qolip_set_id'), '');
        IF old_set_key IS NOT NULL THEN
            IF set_key IS NOT NULL AND set_key <> old_set_key THEN
                RAISE EXCEPTION 'qolip_set_id_immutable' USING ERRCODE = '23514';
            END IF;
            set_key := old_set_key;
        END IF;
    END IF;
    IF set_key IS NULL THEN set_key := public.mini_qolip_next_set_id(); END IF;
    IF set_key !~ '^9501[0-9]{8}$' THEN
        RAISE EXCEPTION 'qolip_set_id_invalid' USING ERRCODE = '23514';
    END IF;
    INSERT INTO public.mini_qolip_sets(set_id, item_code_key, warehouse_key)
    VALUES (set_key, product_key, owner_key) ON CONFLICT (set_id) DO NOTHING;
    IF TG_OP = 'INSERT' AND NOT EXISTS (
        SELECT 1 FROM public.mini_qolip_sets
        WHERE set_id = set_key AND item_code_key = product_key AND warehouse_key = owner_key
    ) THEN
        RAISE EXCEPTION 'qolip_set_scope_conflict' USING ERRCODE = '23514';
    END IF;
    NEW.payload_json := jsonb_set(NEW.payload_json, '{qolip_set_id}', to_jsonb(set_key), true);
    RETURN NEW;
END;
$$;
CREATE TRIGGER mini_qolip_specs_register_set BEFORE INSERT OR UPDATE ON public.mini_qolip_product_specs
FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_register_set();

CREATE OR REPLACE FUNCTION public.mini_qolip_link_initial_product() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM public.mini_qolip_sets WHERE set_id = NEW.qolip_set_id AND is_product_local) THEN
        RETURN NEW;
    END IF;
    IF TG_OP = 'INSERT' OR lower(btrim(NEW.item_code)) <> lower(btrim(OLD.item_code)) THEN
        INSERT INTO public.mini_qolip_set_products
            (set_id, item_code, created_by_role, created_by_ref, created_by_name)
        SELECT NEW.qolip_set_id, item.code, NEW.created_by_role, NEW.created_by_ref, NEW.created_by_name
        FROM public.mini_items item WHERE lower(btrim(item.code)) = lower(btrim(NEW.item_code))
        ON CONFLICT DO NOTHING;
        IF TG_OP = 'UPDATE' THEN
            DELETE FROM public.mini_qolip_set_products
            WHERE set_id = NEW.qolip_set_id AND item_code_key = lower(btrim(OLD.item_code));
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER mini_qolip_specs_initial_product AFTER INSERT OR UPDATE ON public.mini_qolip_product_specs
FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_link_initial_product();

CREATE FUNCTION public.mini_qolip_keep_set_id() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.set_id IS DISTINCT FROM OLD.set_id THEN
        RAISE EXCEPTION 'qolip_set_id_immutable' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;
ALTER FUNCTION public.mini_qolip_keep_set_id() OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_qolip_keep_set_id() FROM PUBLIC;
CREATE TRIGGER mini_qolip_sets_keep_id BEFORE UPDATE OF set_id ON public.mini_qolip_sets
FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_keep_set_id();

DO $$
BEGIN
    IF EXISTS (
        (SELECT * FROM qolip_bindings_before EXCEPT ALL SELECT * FROM public.mini_qolip_effective_product_molds)
        UNION ALL
        (SELECT * FROM public.mini_qolip_effective_product_molds EXCEPT ALL SELECT * FROM qolip_bindings_before)
    ) OR EXISTS (
        (SELECT * FROM qolip_links_before EXCEPT ALL SELECT set_id, item_code_key FROM public.mini_qolip_set_products)
        UNION ALL
        (SELECT set_id, item_code_key FROM public.mini_qolip_set_products EXCEPT ALL SELECT * FROM qolip_links_before)
    ) THEN
        RAISE EXCEPTION 'qolip_set_id_migration_changed_product_bindings';
    END IF;
END;
$$;
DROP FUNCTION pg_temp.qolip_rekey_json(JSONB);
