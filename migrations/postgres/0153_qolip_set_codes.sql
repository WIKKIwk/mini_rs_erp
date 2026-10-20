-- Public set codes are independent of internal IDs and member mold codes.
CREATE SEQUENCE public.mini_qolip_set_code_seq;
ALTER SEQUENCE public.mini_qolip_set_code_seq OWNER TO mini_rs_erp_owner;
REVOKE ALL ON SEQUENCE public.mini_qolip_set_code_seq FROM PUBLIC, mini_rs_erp;

ALTER TABLE public.mini_qolip_sets ADD COLUMN set_code TEXT;

CREATE FUNCTION public.mini_qolip_assign_set_code()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    serial TEXT;
BEGIN
    IF TG_OP = 'UPDATE' AND OLD.set_code IS NOT NULL THEN
        IF NEW.set_code IS DISTINCT FROM OLD.set_code THEN
            RAISE EXCEPTION 'qolip_set_code_immutable' USING ERRCODE = '23514';
        END IF;
        RETURN NEW;
    END IF;
    -- Registering another member must keep the existing code without consuming
    -- another number (the registry insert uses ON CONFLICT DO NOTHING).
    IF TG_OP = 'INSERT' THEN
        SELECT set_code INTO NEW.set_code
        FROM public.mini_qolip_sets WHERE set_id = NEW.set_id;
        IF FOUND THEN RETURN NEW; END IF;
    END IF;
    LOOP
        serial := nextval('public.mini_qolip_set_code_seq')::TEXT;
        NEW.set_code := 'K-' || lpad(serial, greatest(6, length(serial)), '0');
        EXIT WHEN NOT EXISTS (
            SELECT 1 FROM public.mini_qolip_product_specs WHERE lower(qolip_code) = lower(NEW.set_code)
            UNION ALL
            SELECT 1 FROM public.mini_qolip_locations WHERE lower(qolip_code) = lower(NEW.set_code)
            UNION ALL
            SELECT 1 FROM public.mini_qolip_checkouts WHERE lower(qolip_code) = lower(NEW.set_code)
            UNION ALL
            SELECT 1 FROM public.mini_qolip_sets
            WHERE lower(display_code) = lower(NEW.set_code) OR set_code = NEW.set_code
        );
    END LOOP;
    RETURN NEW;
END;
$$;
ALTER FUNCTION public.mini_qolip_assign_set_code() OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_qolip_assign_set_code() FROM PUBLIC;

CREATE TRIGGER mini_qolip_sets_assign_code
BEFORE INSERT OR UPDATE OF set_code ON public.mini_qolip_sets
FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_assign_set_code();

-- Backfill existing sets once, including legacy and empty registry entries.
-- Their IDs and product links are retained, and codes are never recycled.
DO $$
DECLARE
    existing RECORD;
BEGIN
    FOR existing IN SELECT set_id FROM public.mini_qolip_sets ORDER BY created_at, set_id
    LOOP
        UPDATE public.mini_qolip_sets SET set_code = NULL WHERE set_id = existing.set_id;
    END LOOP;
END;
$$;

ALTER TABLE public.mini_qolip_sets
    ALTER COLUMN set_code SET NOT NULL,
    ADD CONSTRAINT mini_qolip_sets_code_format CHECK (set_code ~ '^K-[0-9]{6,}$'),
    ADD CONSTRAINT mini_qolip_sets_code_unique UNIQUE (set_code);
ALTER SEQUENCE public.mini_qolip_set_code_seq OWNED BY public.mini_qolip_sets.set_code;
