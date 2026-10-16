-- Product compatibility is independent of the physical set's source identity.
ALTER TABLE public.mini_qolip_sets ADD COLUMN display_code TEXT NOT NULL DEFAULT '';
-- A catalog SKU correction does not replace a physical set. Retain warehouse
-- ownership and the original product as provenance, while keeping set IDs fixed.
ALTER TABLE public.mini_qolip_sets ADD CONSTRAINT mini_qolip_sets_warehouse_unique UNIQUE (set_id, warehouse_key);
ALTER TABLE public.mini_qolip_product_specs DROP CONSTRAINT mini_qolip_specs_set_scope_fk;
ALTER TABLE public.mini_qolip_product_specs ADD CONSTRAINT mini_qolip_specs_set_scope_fk
    FOREIGN KEY (qolip_set_id, qolip_set_warehouse_key)
    REFERENCES public.mini_qolip_sets(set_id, warehouse_key) ON UPDATE RESTRICT ON DELETE RESTRICT;

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
    IF set_key IS NULL THEN
        set_key := 'legacy:' || length(product_key)::text || ':' || product_key || ':' || owner_key;
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
ALTER FUNCTION public.mini_qolip_register_set() OWNER TO mini_rs_erp_owner;

-- Promote existing, attributable physical catalog entries without moving stock.
-- Unknown historical ownership stays outside the shared catalog.
WITH legacy AS (
    SELECT location.qolip_code, location.item_code, location.item_name, location.size,
        location.created_by_role, location.created_by_ref, location.created_by_name,
        mini_qolip_assigned_warehouse(location.created_by_role, location.created_by_ref) AS warehouse,
        COALESCE(location.payload_json->>'color', '') AS color, location.created_at, location.updated_at, 0 AS priority
    FROM public.mini_qolip_locations location
    UNION ALL
    SELECT checkout.qolip_code, checkout.item_code, checkout.item_name, checkout.size,
        checkout.issued_by_role, checkout.issued_by_ref, checkout.issued_by_name,
        mini_qolip_assigned_warehouse(checkout.issued_by_role, checkout.issued_by_ref),
        COALESCE(checkout.payload_json->>'color', ''), checkout.issued_at, checkout.updated_at, 1
    FROM public.mini_qolip_checkouts checkout WHERE checkout.status = 'open'
), chosen AS (
    SELECT DISTINCT ON (lower(qolip_code)) * FROM legacy
    WHERE NULLIF(btrim(warehouse), '') IS NOT NULL
    ORDER BY lower(qolip_code), priority, updated_at DESC
)
INSERT INTO public.mini_qolip_product_specs(item_code, item_name, item_group, qolip_code, size,
    created_by_role, created_by_ref, created_by_name, payload_json, created_at, updated_at)
SELECT item.code, chosen.item_name, item.item_group, chosen.qolip_code, chosen.size,
    chosen.created_by_role, chosen.created_by_ref, chosen.created_by_name,
    jsonb_build_object('warehouse', chosen.warehouse, 'color', chosen.color), chosen.created_at, chosen.updated_at
FROM chosen JOIN public.mini_items item ON lower(btrim(item.code)) = lower(btrim(chosen.item_code))
WHERE NOT EXISTS (SELECT 1 FROM public.mini_qolip_product_specs spec WHERE lower(spec.qolip_code) = lower(chosen.qolip_code))
ON CONFLICT (lower(qolip_code)) DO NOTHING;

UPDATE public.mini_qolip_sets sets SET display_code = source.code
FROM (
    SELECT qolip_set_id, (array_agg(qolip_code ORDER BY length(qolip_code) DESC, qolip_code DESC))[1] AS code
    FROM public.mini_qolip_product_specs WHERE qolip_set_id IS NOT NULL
    GROUP BY qolip_set_id
) source WHERE sets.set_id = source.qolip_set_id;
CREATE INDEX idx_mini_qolip_sets_display_code ON public.mini_qolip_sets (lower(display_code));
GRANT UPDATE (display_code) ON public.mini_qolip_sets TO mini_rs_erp;

CREATE TABLE public.mini_qolip_set_products (
    set_id TEXT NOT NULL REFERENCES public.mini_qolip_sets(set_id) ON DELETE RESTRICT,
    item_code TEXT NOT NULL REFERENCES public.mini_items(code) ON UPDATE CASCADE ON DELETE CASCADE,
    item_code_key TEXT GENERATED ALWAYS AS (lower(btrim(item_code))) STORED,
    created_by_role TEXT NOT NULL,
    created_by_ref TEXT NOT NULL,
    created_by_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (set_id, item_code_key)
);
CREATE INDEX idx_mini_qolip_set_products_item ON public.mini_qolip_set_products(item_code_key, set_id);
ALTER TABLE public.mini_qolip_set_products OWNER TO mini_rs_erp_owner;
REVOKE ALL ON public.mini_qolip_set_products FROM PUBLIC;
GRANT SELECT, INSERT, DELETE ON public.mini_qolip_set_products TO mini_rs_erp;

INSERT INTO public.mini_qolip_set_products(set_id, item_code, created_by_role, created_by_ref, created_by_name)
SELECT DISTINCT ON (spec.qolip_set_id, lower(btrim(item.code)))
    spec.qolip_set_id, item.code, spec.created_by_role, spec.created_by_ref, spec.created_by_name
FROM public.mini_qolip_product_specs spec
JOIN public.mini_items item ON lower(btrim(item.code)) = lower(btrim(spec.item_code))
WHERE spec.qolip_set_id IS NOT NULL
ORDER BY spec.qolip_set_id, lower(btrim(item.code)), spec.created_at;

CREATE FUNCTION public.mini_qolip_link_initial_product() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    IF TG_OP = 'INSERT' OR NEW.qolip_set_id IS DISTINCT FROM OLD.qolip_set_id THEN
        INSERT INTO public.mini_qolip_set_products(set_id, item_code, created_by_role, created_by_ref, created_by_name)
        SELECT NEW.qolip_set_id, item.code, NEW.created_by_role, NEW.created_by_ref, NEW.created_by_name
        FROM public.mini_items item WHERE lower(btrim(item.code)) = lower(btrim(NEW.item_code))
        ON CONFLICT DO NOTHING;
        UPDATE public.mini_qolip_sets SET display_code = NEW.qolip_code WHERE set_id = NEW.qolip_set_id;
    ELSE
        IF lower(btrim(NEW.item_code)) <> lower(btrim(OLD.item_code)) THEN
            INSERT INTO public.mini_qolip_set_products(set_id, item_code, created_by_role, created_by_ref, created_by_name)
            SELECT NEW.qolip_set_id, item.code, NEW.created_by_role, NEW.created_by_ref, NEW.created_by_name
            FROM public.mini_items item WHERE lower(btrim(item.code)) = lower(btrim(NEW.item_code))
            ON CONFLICT DO NOTHING;
            DELETE FROM public.mini_qolip_set_products
            WHERE set_id = NEW.qolip_set_id AND item_code_key = lower(btrim(OLD.item_code));
        END IF;
        IF NEW.qolip_code IS DISTINCT FROM OLD.qolip_code THEN
            UPDATE public.mini_qolip_sets SET display_code = NEW.qolip_code
            WHERE set_id = NEW.qolip_set_id AND lower(display_code) = lower(OLD.qolip_code);
        END IF;
    END IF;
    RETURN NEW;
END;
$$;
ALTER FUNCTION public.mini_qolip_link_initial_product() OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_qolip_link_initial_product() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION public.mini_qolip_link_initial_product() TO mini_rs_erp;
CREATE TRIGGER mini_qolip_specs_initial_product AFTER INSERT OR UPDATE ON public.mini_qolip_product_specs
FOR EACH ROW EXECUTE FUNCTION public.mini_qolip_link_initial_product();
