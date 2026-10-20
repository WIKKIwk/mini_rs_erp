-- A product-local mold has a physical identity but creates no shared base set.
ALTER TABLE public.mini_qolip_sets
    ADD COLUMN is_product_local BOOLEAN NOT NULL DEFAULT false;
GRANT INSERT (set_id, item_code_key, warehouse_key, display_code, is_product_local)
    ON public.mini_qolip_sets TO mini_rs_erp;

CREATE OR REPLACE FUNCTION public.mini_qolip_link_initial_product() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM public.mini_qolip_sets
               WHERE set_id = NEW.qolip_set_id AND is_product_local) THEN
        RETURN NEW;
    END IF;
    IF TG_OP = 'INSERT' OR NEW.qolip_set_id IS DISTINCT FROM OLD.qolip_set_id THEN
        INSERT INTO public.mini_qolip_set_products
            (set_id, item_code, created_by_role, created_by_ref, created_by_name)
        SELECT NEW.qolip_set_id, item.code, NEW.created_by_role, NEW.created_by_ref, NEW.created_by_name
        FROM public.mini_items item WHERE lower(btrim(item.code)) = lower(btrim(NEW.item_code))
        ON CONFLICT DO NOTHING;
        -- The code remains stable after creation, including when the last member is removed.
        UPDATE public.mini_qolip_sets SET display_code = NEW.qolip_code
        WHERE set_id = NEW.qolip_set_id AND display_code = '';
    ELSIF lower(btrim(NEW.item_code)) <> lower(btrim(OLD.item_code)) THEN
        INSERT INTO public.mini_qolip_set_products
            (set_id, item_code, created_by_role, created_by_ref, created_by_name)
        SELECT NEW.qolip_set_id, item.code, NEW.created_by_role, NEW.created_by_ref, NEW.created_by_name
        FROM public.mini_items item WHERE lower(btrim(item.code)) = lower(btrim(NEW.item_code))
        ON CONFLICT DO NOTHING;
        DELETE FROM public.mini_qolip_set_products
        WHERE set_id = NEW.qolip_set_id AND item_code_key = lower(btrim(OLD.item_code));
    END IF;
    RETURN NEW;
END;
$$;
ALTER FUNCTION public.mini_qolip_link_initial_product() OWNER TO mini_rs_erp_owner;
