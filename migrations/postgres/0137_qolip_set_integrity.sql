-- A set ID belongs to exactly one product and receiving warehouse. Keep the
-- registry after the last mold is deleted so an old ID cannot be reassigned.
CREATE TABLE public.mini_qolip_sets (
    set_id TEXT PRIMARY KEY CHECK (set_id <> '' AND set_id = btrim(set_id)),
    item_code_key TEXT NOT NULL CHECK (item_code_key <> '' AND item_code_key = lower(btrim(item_code_key))),
    warehouse_key TEXT NOT NULL CHECK (warehouse_key <> '' AND warehouse_key = lower(btrim(warehouse_key))),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_qolip_sets_scope_unique UNIQUE (set_id, item_code_key, warehouse_key)
);

-- Older writers may have created receipts since 0136 without a recorded set.
-- Unresolved historical owners stay unresolved and invisible to clerks.
UPDATE public.mini_qolip_product_specs
SET payload_json = jsonb_set(payload_json, '{qolip_set_id}',
    to_jsonb(('legacy:' || length(lower(btrim(item_code)))::text || ':' || lower(btrim(item_code))
        || ':' || lower(btrim(payload_json->>'warehouse')))::text), true)
WHERE NULLIF(btrim(payload_json->>'qolip_set_id'), '') IS NULL
  AND NULLIF(btrim(payload_json->>'warehouse'), '') IS NOT NULL;

ALTER TABLE public.mini_qolip_product_specs
    ADD COLUMN qolip_set_id TEXT GENERATED ALWAYS AS (NULLIF(btrim(payload_json->>'qolip_set_id'), '')) STORED,
    ADD COLUMN qolip_set_item_key TEXT GENERATED ALWAYS AS (lower(btrim(item_code))) STORED,
    ADD COLUMN qolip_set_warehouse_key TEXT GENERATED ALWAYS AS (NULLIF(lower(btrim(payload_json->>'warehouse')), '')) STORED;

-- A conflicting pre-existing ID fails the migration instead of merging owners.
INSERT INTO public.mini_qolip_sets (set_id, item_code_key, warehouse_key)
SELECT DISTINCT qolip_set_id, qolip_set_item_key, qolip_set_warehouse_key
FROM public.mini_qolip_product_specs
WHERE qolip_set_id IS NOT NULL;

ALTER TABLE public.mini_qolip_product_specs
    ADD CONSTRAINT mini_qolip_specs_set_owner_required
        CHECK ((qolip_set_id IS NULL) = (qolip_set_warehouse_key IS NULL)),
    ADD CONSTRAINT mini_qolip_specs_set_scope_fk
        FOREIGN KEY (qolip_set_id, qolip_set_item_key, qolip_set_warehouse_key)
        REFERENCES public.mini_qolip_sets (set_id, item_code_key, warehouse_key)
        ON UPDATE RESTRICT ON DELETE RESTRICT;

CREATE INDEX idx_mini_qolip_specs_set_scope
    ON public.mini_qolip_product_specs (qolip_set_id, qolip_set_item_key, qolip_set_warehouse_key);

CREATE FUNCTION public.mini_qolip_register_set()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
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
        IF product_key = lower(btrim(OLD.item_code)) AND set_key IS NULL THEN
            set_key := old_set_key;
        ELSIF product_key <> lower(btrim(OLD.item_code))
              AND old_set_key IS NOT NULL AND (set_key IS NULL OR set_key = old_set_key) THEN
            -- Item-code corrections/renames update every member directly. Fork
            -- one stable new ID per old set, preserving its batch boundaries.
            set_key := 'qolip-set:rebind:' || md5(jsonb_build_array(old_set_key, product_key, owner_key)::text);
        END IF;
    END IF;
    IF set_key IS NULL THEN
        set_key := 'legacy:' || length(product_key)::text || ':' || product_key || ':' || owner_key;
    END IF;
    NEW.payload_json := jsonb_set(NEW.payload_json, '{qolip_set_id}', to_jsonb(set_key), true);
    -- The unique ID serializes concurrent first receipts. The composite FK
    -- then rejects a losing writer with a different product or warehouse.
    INSERT INTO public.mini_qolip_sets (set_id, item_code_key, warehouse_key)
    VALUES (set_key, product_key, owner_key)
    ON CONFLICT (set_id) DO NOTHING;
    RETURN NEW;
END;
$$;

-- PostgreSQL runs BEFORE triggers alphabetically: persist_warehouse runs first.
CREATE TRIGGER mini_qolip_specs_register_set
BEFORE INSERT OR UPDATE ON public.mini_qolip_product_specs
FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_register_set();

ALTER TABLE public.mini_qolip_sets OWNER TO mini_rs_erp_owner;
ALTER FUNCTION public.mini_qolip_register_set() OWNER TO mini_rs_erp_owner;
REVOKE ALL ON TABLE public.mini_qolip_sets FROM PUBLIC, mini_rs_erp;
GRANT SELECT ON TABLE public.mini_qolip_sets TO mini_rs_erp;
REVOKE ALL ON FUNCTION public.mini_qolip_register_set() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION public.mini_qolip_register_set() TO mini_rs_erp;
