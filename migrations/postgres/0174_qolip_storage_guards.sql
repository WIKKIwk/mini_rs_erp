-- Pallet/archive custody is independent of production use and peredelka.
-- Keep composition rows intact: an unavailable member must not silently make
-- an incomplete production set appear complete.
CREATE FUNCTION public.mini_qolip_storage_state(code TEXT) RETURNS TEXT
LANGUAGE sql STABLE SET search_path = pg_catalog, pg_temp AS $$
    SELECT CASE
        WHEN EXISTS (SELECT 1 FROM public.mini_qolip_archive_entries a
                     WHERE a.kind = 'qolip' AND a.code_key = lower(btrim($1)) AND a.restored_at IS NULL)
            THEN 'archived'
        WHEN EXISTS (SELECT 1 FROM public.mini_qolip_paddon_members m
                     JOIN public.mini_qolip_paddons p ON p.code = m.paddon_code
                     WHERE m.qolip_code_key = lower(btrim($1)) AND m.removed_at IS NULL AND p.archived_at IS NOT NULL)
            THEN 'archived'
        WHEN EXISTS (SELECT 1 FROM public.mini_qolip_paddon_members m
                     WHERE m.qolip_code_key = lower(btrim($1)) AND m.removed_at IS NULL)
            THEN 'paddon'
        ELSE '' END;
$$;
ALTER FUNCTION public.mini_qolip_storage_state(TEXT) OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_qolip_storage_state(TEXT) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION public.mini_qolip_storage_state(TEXT) TO mini_rs_erp;

CREATE FUNCTION public.mini_qolip_require_free_storage(code TEXT) RETURNS void
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE state TEXT;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended('qolip:' || lower(btrim(code)), 0));
    state := public.mini_qolip_storage_state(code);
    IF state = 'archived' THEN RAISE EXCEPTION 'qolip_archived' USING ERRCODE = '23514'; END IF;
    IF state = 'paddon' THEN RAISE EXCEPTION 'qolip_in_paddon' USING ERRCODE = '23514'; END IF;
END;
$$;
ALTER FUNCTION public.mini_qolip_require_free_storage(TEXT) OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_qolip_require_free_storage(TEXT) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION public.mini_qolip_require_free_storage(TEXT) TO mini_rs_erp;

CREATE FUNCTION public.mini_qolip_guard_storage_code() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE codes TEXT[] := ARRAY[]::TEXT[]; code TEXT;
BEGIN
    IF TG_OP <> 'INSERT' THEN codes := array_append(codes, lower(btrim(OLD.qolip_code))); END IF;
    IF TG_OP <> 'DELETE' THEN codes := array_append(codes, lower(btrim(NEW.qolip_code))); END IF;
    FOR code IN SELECT DISTINCT value FROM unnest(codes) value ORDER BY value LOOP
        PERFORM public.mini_qolip_require_free_storage(code);
    END LOOP;
    IF TG_OP = 'DELETE' THEN RETURN OLD; ELSE RETURN NEW; END IF;
END;
$$;
ALTER FUNCTION public.mini_qolip_guard_storage_code() OWNER TO mini_rs_erp_owner;
CREATE TRIGGER mini_qolip_storage_specs_guard BEFORE INSERT OR UPDATE OR DELETE
    ON public.mini_qolip_product_specs FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_storage_code();
CREATE TRIGGER mini_qolip_storage_locations_guard BEFORE INSERT OR UPDATE OR DELETE
    ON public.mini_qolip_locations FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_storage_code();
CREATE TRIGGER mini_qolip_storage_checkouts_guard BEFORE INSERT OR UPDATE OR DELETE
    ON public.mini_qolip_checkouts FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_storage_code();
CREATE TRIGGER mini_qolip_storage_reworks_guard BEFORE INSERT OR UPDATE OR DELETE
    ON public.mini_qolip_reworks FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_storage_code();
CREATE TRIGGER mini_qolip_storage_overrides_guard BEFORE INSERT OR UPDATE OR DELETE
    ON public.mini_qolip_product_overrides FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_storage_code();

-- Archived/restored objects retain their identity for history and QR lookup.
CREATE FUNCTION public.mini_qolip_guard_storage_history() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND NEW.qolip_code IS NOT DISTINCT FROM OLD.qolip_code THEN RETURN NEW; END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('qolip:' || lower(btrim(OLD.qolip_code)), 0));
    IF EXISTS (SELECT 1 FROM public.mini_qolip_paddon_members WHERE qolip_code_key = lower(btrim(OLD.qolip_code)))
       OR EXISTS (SELECT 1 FROM public.mini_qolip_archive_entries
                  WHERE kind = 'qolip' AND code_key = lower(btrim(OLD.qolip_code))) THEN
        RAISE EXCEPTION 'qolip_history_locked' USING ERRCODE = '23514';
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; ELSE RETURN NEW; END IF;
END;
$$;
ALTER FUNCTION public.mini_qolip_guard_storage_history() OWNER TO mini_rs_erp_owner;
CREATE TRIGGER mini_qolip_storage_history_guard BEFORE UPDATE OR DELETE
    ON public.mini_qolip_product_specs FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_storage_history();

CREATE FUNCTION public.mini_qolip_guard_storage_set() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE ids TEXT[] := ARRAY[]::TEXT[]; code TEXT;
BEGIN
    IF TG_OP <> 'INSERT' THEN ids := array_append(ids, OLD.set_id); END IF;
    IF TG_OP <> 'DELETE' THEN ids := array_append(ids, NEW.set_id); END IF;
    FOR code IN SELECT DISTINCT lower(btrim(s.qolip_code)) FROM public.mini_qolip_product_specs s
                WHERE s.qolip_set_id = ANY(ids) ORDER BY 1 LOOP
        PERFORM public.mini_qolip_require_free_storage(code);
    END LOOP;
    IF TG_OP = 'DELETE' THEN RETURN OLD; ELSE RETURN NEW; END IF;
END;
$$;
ALTER FUNCTION public.mini_qolip_guard_storage_set() OWNER TO mini_rs_erp_owner;
CREATE TRIGGER mini_qolip_storage_set_links_guard BEFORE INSERT OR UPDATE OR DELETE
    ON public.mini_qolip_set_products FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_storage_set();
CREATE TRIGGER mini_qolip_storage_sets_guard BEFORE UPDATE OR DELETE
    ON public.mini_qolip_sets FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_storage_set();

-- Defense in depth for starts/resumes/reactivation, including catalog-only
-- molds that do not require a checkout/location row under the existing policy.
CREATE FUNCTION public.mini_qolip_guard_storage_session() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE code TEXT;
BEGIN
    IF NEW.status NOT IN ('active','paused','frozen','roll_detached')
       OR NEW.payload_json->>'qolip_lock_owner' IS DISTINCT FROM 'true' THEN RETURN NEW; END IF;
    FOR code IN SELECT DISTINCT lower(btrim(value)) FROM (
        SELECT NEW.payload_json->>'qolip_code' AS value
        UNION ALL
        SELECT jsonb_array_elements_text(CASE WHEN jsonb_typeof(NEW.payload_json->'qolip_codes') = 'array'
            THEN NEW.payload_json->'qolip_codes' ELSE '[]'::jsonb END)
    ) selected WHERE btrim(value) <> '' ORDER BY 1 LOOP
        PERFORM public.mini_qolip_require_free_storage(code);
    END LOOP;
    RETURN NEW;
END;
$$;
ALTER FUNCTION public.mini_qolip_guard_storage_session() OWNER TO mini_rs_erp_owner;
CREATE TRIGGER mini_qolip_storage_session_guard BEFORE INSERT OR UPDATE
    ON public.mini_order_run_sessions FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_storage_session();

CREATE FUNCTION public.mini_qolip_guard_storage_home() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND NEW.name IS NOT DISTINCT FROM OLD.name
       AND NEW.parent_warehouse IS NOT DISTINCT FROM OLD.parent_warehouse THEN RETURN NEW; END IF;
    IF EXISTS (SELECT 1 FROM public.mini_qolip_paddons p WHERE lower(btrim(p.warehouse)) = lower(btrim(OLD.name)))
       OR EXISTS (SELECT 1 FROM public.mini_qolip_archive_entries a
                  WHERE a.restored_at IS NULL AND lower(btrim(a.warehouse)) = lower(btrim(OLD.name)))
       OR EXISTS (SELECT 1 FROM public.mini_qolip_locations l
                  WHERE lower(btrim(l.block)) = lower(btrim(OLD.name))
                    AND public.mini_qolip_storage_state(l.qolip_code) <> '') THEN
        RAISE EXCEPTION 'qolip_history_locked' USING ERRCODE = '23514';
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; ELSE RETURN NEW; END IF;
END;
$$;
ALTER FUNCTION public.mini_qolip_guard_storage_home() OWNER TO mini_rs_erp_owner;
CREATE TRIGGER mini_qolip_storage_home_guard BEFORE UPDATE OR DELETE
    ON public.mini_warehouses FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_storage_home();
