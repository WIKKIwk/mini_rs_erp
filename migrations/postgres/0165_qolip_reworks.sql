-- Per-code peredelka visits preserve the catalog and the reserved home location.
CREATE TABLE public.mini_qolip_reworks (
    id TEXT PRIMARY KEY CHECK (btrim(id) <> ''),
    qolip_code TEXT NOT NULL CHECK (btrim(qolip_code) <> ''),
    qolip_code_key TEXT GENERATED ALWAYS AS (lower(btrim(qolip_code))) STORED,
    qolip_set_id TEXT NOT NULL,
    warehouse TEXT NOT NULL CHECK (btrim(warehouse) <> ''),
    item_code TEXT NOT NULL,
    item_name TEXT NOT NULL,
    color TEXT NOT NULL,
    size INTEGER NOT NULL CHECK (size > 0),
    sent_at TIMESTAMPTZ NOT NULL,
    returned_at TIMESTAMPTZ,
    location_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    created_by_role TEXT NOT NULL,
    created_by_ref TEXT NOT NULL,
    returned_by_role TEXT,
    returned_by_ref TEXT,
    CHECK (returned_at IS NULL OR returned_at >= sent_at)
);
CREATE UNIQUE INDEX mini_qolip_one_open_rework
    ON public.mini_qolip_reworks(qolip_code_key) WHERE returned_at IS NULL;
CREATE INDEX mini_qolip_reworks_warehouse_sent
    ON public.mini_qolip_reworks(lower(btrim(warehouse)), sent_at DESC, id);

CREATE TABLE public.mini_qolip_rework_requests (
    actor_key TEXT NOT NULL,
    request_id TEXT NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('send', 'return')),
    request_json JSONB NOT NULL,
    response_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (actor_key, request_id)
);
ALTER TABLE public.mini_qolip_reworks OWNER TO mini_rs_erp_owner;
ALTER TABLE public.mini_qolip_rework_requests OWNER TO mini_rs_erp_owner;
REVOKE ALL ON public.mini_qolip_reworks, public.mini_qolip_rework_requests FROM PUBLIC;
GRANT SELECT, INSERT ON public.mini_qolip_reworks, public.mini_qolip_rework_requests TO mini_rs_erp;
GRANT UPDATE (returned_at, returned_by_role, returned_by_ref)
    ON public.mini_qolip_reworks TO mini_rs_erp;

-- All direct warehouse/catalog writers share the physical resource lock.
-- Stored home stock is hidden while away; receiving/moving/renaming/deleting it
-- must not create a second copy or remove its still-required set membership.
CREATE FUNCTION public.mini_qolip_guard_rework_code() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE codes TEXT[] := ARRAY[]::TEXT[]; code TEXT;
BEGIN
    IF TG_OP <> 'INSERT' THEN codes := array_append(codes, lower(btrim(OLD.qolip_code))); END IF;
    IF TG_OP <> 'DELETE' THEN codes := array_append(codes, lower(btrim(NEW.qolip_code))); END IF;
    FOR code IN SELECT DISTINCT value FROM unnest(codes) value ORDER BY value LOOP
        PERFORM pg_advisory_xact_lock(hashtextextended('qolip:' || code, 0));
        IF EXISTS (SELECT 1 FROM public.mini_qolip_reworks r
                   WHERE r.qolip_code_key = code AND r.returned_at IS NULL) THEN
            RAISE EXCEPTION 'qolip_in_rework' USING ERRCODE = '23514';
        END IF;
    END LOOP;
    IF TG_OP = 'DELETE' THEN RETURN OLD; ELSE RETURN NEW; END IF;
END;
$$;
ALTER FUNCTION public.mini_qolip_guard_rework_code() OWNER TO mini_rs_erp_owner;
CREATE TRIGGER mini_qolip_rework_specs_guard BEFORE INSERT OR UPDATE OR DELETE
    ON public.mini_qolip_product_specs FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_rework_code();
CREATE TRIGGER mini_qolip_rework_locations_guard BEFORE INSERT OR UPDATE OR DELETE
    ON public.mini_qolip_locations FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_rework_code();
CREATE TRIGGER mini_qolip_rework_checkouts_guard BEFORE INSERT OR UPDATE OR DELETE
    ON public.mini_qolip_checkouts FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_rework_code();
CREATE TRIGGER mini_qolip_rework_overrides_guard BEFORE INSERT OR UPDATE OR DELETE
    ON public.mini_qolip_product_overrides FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_rework_code();

CREATE FUNCTION public.mini_qolip_guard_rework_set() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE ids TEXT[] := ARRAY[]::TEXT[]; code TEXT;
BEGIN
    IF TG_OP <> 'INSERT' THEN ids := array_append(ids, OLD.set_id); END IF;
    IF TG_OP <> 'DELETE' THEN ids := array_append(ids, NEW.set_id); END IF;
    FOR code IN SELECT DISTINCT r.qolip_code_key FROM public.mini_qolip_reworks r
                WHERE r.qolip_set_id = ANY(ids) AND r.returned_at IS NULL ORDER BY 1 LOOP
        PERFORM pg_advisory_xact_lock(hashtextextended('qolip:' || code, 0));
        IF EXISTS (SELECT 1 FROM public.mini_qolip_reworks r
                   WHERE r.qolip_code_key = code AND r.returned_at IS NULL) THEN
            RAISE EXCEPTION 'qolip_in_rework' USING ERRCODE = '23514';
        END IF;
    END LOOP;
    IF TG_OP = 'DELETE' THEN RETURN OLD; ELSE RETURN NEW; END IF;
END;
$$;
ALTER FUNCTION public.mini_qolip_guard_rework_set() OWNER TO mini_rs_erp_owner;
CREATE TRIGGER mini_qolip_rework_set_links_guard BEFORE INSERT OR UPDATE OR DELETE
    ON public.mini_qolip_set_products FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_rework_set();
CREATE TRIGGER mini_qolip_rework_sets_guard BEFORE UPDATE OR DELETE
    ON public.mini_qolip_sets FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_rework_set();

CREATE FUNCTION public.mini_qolip_guard_rework_home() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND NEW.name IS NOT DISTINCT FROM OLD.name
        AND NEW.parent_warehouse IS NOT DISTINCT FROM OLD.parent_warehouse THEN RETURN NEW; END IF;
    IF EXISTS (SELECT 1 FROM public.mini_qolip_reworks r
               LEFT JOIN public.mini_qolip_locations l ON lower(btrim(l.qolip_code)) = r.qolip_code_key
               WHERE r.returned_at IS NULL AND (
                   lower(btrim(r.warehouse)) = lower(btrim(OLD.name))
                   OR lower(btrim(l.block)) = lower(btrim(OLD.name)))) THEN
        RAISE EXCEPTION 'qolip_in_rework' USING ERRCODE = '23514';
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; ELSE RETURN NEW; END IF;
END;
$$;
ALTER FUNCTION public.mini_qolip_guard_rework_home() OWNER TO mini_rs_erp_owner;
CREATE TRIGGER mini_qolip_rework_home_guard BEFORE UPDATE OR DELETE
    ON public.mini_warehouses FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_guard_rework_home();
