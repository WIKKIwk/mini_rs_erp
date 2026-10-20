-- Sets may be created before any product uses them. A NULL source belongs to
-- the set itself; product associations still live in mini_qolip_set_products.
SELECT pg_advisory_xact_lock(hashtextextended('qolip:product-composition', 0));
LOCK TABLE public.mini_qolip_product_specs, public.mini_qolip_sets,
    public.mini_qolip_set_products IN ACCESS EXCLUSIVE MODE;

ALTER TABLE public.mini_qolip_sets
    ALTER COLUMN item_code_key DROP NOT NULL,
    ADD CONSTRAINT mini_qolip_local_set_requires_product
        CHECK (item_code_key IS NOT NULL OR NOT is_product_local);

-- Keep the existing non-null wire fields. Only a registered standalone set
-- may have an entirely absent source; partially filled product data is invalid.
ALTER TABLE public.mini_qolip_product_specs
    DROP CONSTRAINT mini_qolip_product_specs_item_code_not_blank,
    DROP CONSTRAINT mini_qolip_product_specs_item_name_not_blank,
    ADD CONSTRAINT mini_qolip_specs_product_or_standalone CHECK (
        (btrim(item_code) <> '' AND btrim(item_name) <> '')
        OR (item_code = '' AND item_name = '' AND item_group = '' AND qolip_set_id IS NOT NULL)
    );

CREATE OR REPLACE FUNCTION public.mini_qolip_register_set()
RETURNS trigger LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    set_key TEXT := NULLIF(btrim(NEW.payload_json->>'qolip_set_id'), '');
    product_key TEXT := NULLIF(lower(btrim(NEW.item_code)), '');
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
        IF (product_key IS NULL) IS DISTINCT FROM (NULLIF(btrim(OLD.item_code), '') IS NULL) THEN
            RAISE EXCEPTION 'qolip_set_source_immutable' USING ERRCODE = '23514';
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
        WHERE set_id = set_key
          AND item_code_key IS NOT DISTINCT FROM product_key
          AND warehouse_key = owner_key
    ) THEN
        RAISE EXCEPTION 'qolip_set_scope_conflict' USING ERRCODE = '23514';
    END IF;
    NEW.payload_json := jsonb_set(NEW.payload_json, '{qolip_set_id}', to_jsonb(set_key), true);
    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION public.mini_qolip_link_initial_product() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    IF btrim(NEW.item_code) = '' OR EXISTS (
        SELECT 1 FROM public.mini_qolip_sets WHERE set_id = NEW.qolip_set_id AND is_product_local
    ) THEN
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
