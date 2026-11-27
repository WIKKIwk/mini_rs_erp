SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- Updates on an order's destination can also change the order codes shown on
-- a recipe shared from a different product or material. Resolve both paths.
CREATE INDEX mini_preparation_catalog_live_product
    ON public.mini_preparation_formula_variants(product_code, owner_ref)
    WHERE catalog_source_id IS NULL;
CREATE INDEX mini_preparation_catalog_live_material
    ON public.mini_preparation_formula_variants(lower(material_id), owner_ref)
    WHERE catalog_source_id IS NULL;
CREATE INDEX mini_preparation_selection_live_product
    ON public.mini_preparation_order_formula_selections(product_code);
CREATE INDEX mini_preparation_selection_live_material
    ON public.mini_preparation_order_formula_selections(lower(material_id), owner_ref);

CREATE FUNCTION public.mini_notify_preparation_formula_rows() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND OLD IS NOT DISTINCT FROM NEW THEN
        RETURN NULL;
    END IF;

    PERFORM pg_notify('mini_preparation_formula_live', jsonb_build_array(owner_ref, product_code)::text)
    FROM (SELECT DISTINCT owner_ref, product_code
          FROM (VALUES (OLD.owner_ref, OLD.product_code), (NEW.owner_ref, NEW.product_code))
                   AS changed(owner_ref, product_code)
          WHERE owner_ref IS NOT NULL) AS scopes;

    IF TG_TABLE_NAME IN ('mini_preparation_order_formula_selections', 'mini_preparation_order_formula_exclusions') THEN
        PERFORM pg_notify('mini_preparation_formula_live', jsonb_build_array(root.owner_ref, root.product_code)::text)
        FROM public.mini_preparation_formula_variants v
        JOIN public.mini_preparation_formula_variants root ON root.id=COALESCE(v.catalog_source_id,v.id)
        WHERE v.id IN (OLD.variant_id, NEW.variant_id);
    ELSIF TG_TABLE_NAME = 'mini_preparation_formula_variants' THEN
        PERFORM pg_notify('mini_preparation_formula_live', jsonb_build_array(root.owner_ref, root.product_code)::text)
        FROM public.mini_preparation_formula_variants root
        WHERE root.id IN (OLD.catalog_source_id, NEW.catalog_source_id);
    END IF;
    RETURN NULL;
END;
$$;

CREATE FUNCTION public.mini_notify_preparation_formula_dependencies(
    products text[], materials text[], target_owner text
) RETURNS void
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    PERFORM pg_notify('mini_preparation_formula_live', jsonb_build_array(owner_ref, product_code)::text)
    FROM (
        SELECT v.owner_ref,v.product_code
        FROM public.mini_preparation_formula_variants v
        WHERE v.catalog_source_id IS NULL
          AND (v.product_code=ANY(products)
               OR (lower(v.material_id)=ANY(materials) AND (target_owner IS NULL OR v.owner_ref=target_owner)))
        UNION
        SELECT root.owner_ref,root.product_code
        FROM public.mini_preparation_order_formula_selections chosen
        JOIN public.mini_preparation_formula_variants selected ON selected.id=chosen.variant_id
        JOIN public.mini_preparation_formula_variants root ON root.id=COALESCE(selected.catalog_source_id,selected.id)
        WHERE chosen.product_code=ANY(products)
           OR (lower(chosen.material_id)=ANY(materials) AND (target_owner IS NULL OR chosen.owner_ref=target_owner))
    ) AS affected;
END;
$$;

CREATE FUNCTION public.mini_notify_preparation_formula_sources() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    products text[] := ARRAY[]::text[];
    order_ids text[];
BEGIN
    CASE TG_TABLE_NAME
        WHEN 'mini_preparation_material_responsibilities' THEN
            IF TG_OP = 'UPDATE' AND
               ROW(OLD.principal_role,OLD.principal_ref,OLD.material_id,OLD.material_name)
               IS NOT DISTINCT FROM ROW(NEW.principal_role,NEW.principal_ref,NEW.material_id,NEW.material_name) THEN
                RETURN NULL;
            END IF;
            IF OLD.principal_role = 'tayyorlov_masteri' THEN
                PERFORM public.mini_notify_preparation_formula_dependencies(
                    products, ARRAY[lower(OLD.material_id)], OLD.principal_ref);
            END IF;
            IF NEW.principal_role = 'tayyorlov_masteri' AND
               ROW(OLD.principal_role,OLD.principal_ref,lower(OLD.material_id))
               IS DISTINCT FROM ROW(NEW.principal_role,NEW.principal_ref,lower(NEW.material_id)) THEN
                PERFORM public.mini_notify_preparation_formula_dependencies(
                    products, ARRAY[lower(NEW.material_id)], NEW.principal_ref);
            END IF;
            RETURN NULL;
        WHEN 'mini_calculate_materials' THEN
            IF TG_OP = 'UPDATE' AND ROW(OLD.id,OLD.payload_json->>'name')
               IS NOT DISTINCT FROM ROW(NEW.id,NEW.payload_json->>'name') THEN
                RETURN NULL;
            END IF;
            PERFORM public.mini_notify_preparation_formula_dependencies(
                products, ARRAY[lower(OLD.id),lower(NEW.id)], NULL);
            RETURN NULL;
        WHEN 'mini_items' THEN
            IF TG_OP = 'UPDATE' AND ROW(OLD.code,OLD.name) IS NOT DISTINCT FROM ROW(NEW.code,NEW.name) THEN
                RETURN NULL;
            END IF;
            products := ARRAY[OLD.code,NEW.code];
        WHEN 'mini_production_maps' THEN
            IF TG_OP = 'UPDATE' AND
                ROW(OLD.id,OLD.code,OLD.order_number,OLD.product_code,OLD.title,OLD.created_at,OLD.map_json->>'product_code')
                IS NOT DISTINCT FROM
                ROW(NEW.id,NEW.code,NEW.order_number,NEW.product_code,NEW.title,NEW.created_at,NEW.map_json->>'product_code') THEN
                RETURN NULL;
            END IF;
            products := ARRAY[
                CASE WHEN OLD.id NOT LIKE 'template-%' THEN OLD.product_code END,
                CASE WHEN OLD.id NOT LIKE 'template-%' THEN OLD.map_json->>'product_code' END,
                CASE WHEN NEW.id NOT LIKE 'template-%' THEN NEW.product_code END,
                CASE WHEN NEW.id NOT LIKE 'template-%' THEN NEW.map_json->>'product_code' END
            ];
        WHEN 'mini_order_products' THEN
            IF TG_OP = 'UPDATE' AND
                ROW(OLD.order_id,OLD.layers_json,OLD.first_layer_material,OLD.second_layer_material,OLD.third_layer_material)
                IS NOT DISTINCT FROM
                ROW(NEW.order_id,NEW.layers_json,NEW.first_layer_material,NEW.second_layer_material,NEW.third_layer_material) THEN
                RETURN NULL;
            END IF;
            order_ids := ARRAY[OLD.order_id,NEW.order_id];
        WHEN 'mini_quick_order_templates' THEN
            IF TG_OP = 'UPDATE' AND
                ROW(OLD.id,OLD.item_code,OLD.product_name,OLD.saved_at,OLD.payload_json->>'source_map_id',
                    OLD.payload_json->'layers',OLD.payload_json->>'first_layer_material',
                    OLD.payload_json->>'second_layer_material',OLD.payload_json->>'third_layer_material')
                IS NOT DISTINCT FROM
                ROW(NEW.id,NEW.item_code,NEW.product_name,NEW.saved_at,NEW.payload_json->>'source_map_id',
                    NEW.payload_json->'layers',NEW.payload_json->>'first_layer_material',
                    NEW.payload_json->>'second_layer_material',NEW.payload_json->>'third_layer_material') THEN
                RETURN NULL;
            END IF;
            products := ARRAY[OLD.item_code,NEW.item_code];
            order_ids := ARRAY[btrim(OLD.payload_json->>'source_map_id'),btrim(NEW.payload_json->>'source_map_id')];
    END CASE;
    IF order_ids IS NOT NULL THEN
        products := products || ARRAY(
            SELECT product FROM public.mini_production_maps m,
                LATERAL unnest(ARRAY[m.product_code,m.map_json->>'product_code']) AS keys(product)
            WHERE m.id=ANY(order_ids) AND m.id NOT LIKE 'template-%'
        );
    END IF;
    IF cardinality(array_remove(products, NULL)) > 0 THEN
        PERFORM public.mini_notify_preparation_formula_dependencies(products, ARRAY[]::text[], NULL);
    END IF;
    RETURN NULL;
END;
$$;

CREATE FUNCTION public.mini_reset_preparation_formula_live() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    PERFORM pg_notify('mini_preparation_formula_live', 'reset');
    RETURN NULL;
END;
$$;

ALTER FUNCTION public.mini_notify_preparation_formula_rows() OWNER TO mini_rs_erp_owner;
ALTER FUNCTION public.mini_notify_preparation_formula_sources() OWNER TO mini_rs_erp_owner;
ALTER FUNCTION public.mini_notify_preparation_formula_dependencies(text[],text[],text) OWNER TO mini_rs_erp_owner;
ALTER FUNCTION public.mini_reset_preparation_formula_live() OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_notify_preparation_formula_rows(),
    public.mini_notify_preparation_formula_sources(),
    public.mini_notify_preparation_formula_dependencies(text[],text[],text),
    public.mini_reset_preparation_formula_live() FROM PUBLIC;

DO $$
DECLARE
    relation text;
BEGIN
    FOREACH relation IN ARRAY ARRAY['mini_preparation_formulas','mini_preparation_formula_sets',
        'mini_preparation_formula_variants','mini_preparation_order_formula_selections',
        'mini_preparation_order_formula_exclusions']
    LOOP
        EXECUTE format('CREATE TRIGGER mini_preparation_formula_live_rows AFTER INSERT OR DELETE OR UPDATE ON public.%I
            FOR EACH ROW EXECUTE FUNCTION public.mini_notify_preparation_formula_rows()', relation);
        EXECUTE format('CREATE TRIGGER mini_preparation_formula_live_reset AFTER TRUNCATE ON public.%I
            FOR EACH STATEMENT EXECUTE FUNCTION public.mini_reset_preparation_formula_live()', relation);
    END LOOP;

    FOREACH relation IN ARRAY ARRAY['mini_production_maps','mini_order_products','mini_quick_order_templates',
        'mini_items','mini_calculate_materials','mini_preparation_material_responsibilities']
    LOOP
        EXECUTE format('CREATE TRIGGER mini_preparation_formula_live_sources AFTER INSERT OR DELETE OR UPDATE ON public.%I
            FOR EACH ROW EXECUTE FUNCTION public.mini_notify_preparation_formula_sources()', relation);
        EXECUTE format('CREATE TRIGGER mini_preparation_formula_live_reset AFTER TRUNCATE ON public.%I
            FOR EACH STATEMENT EXECUTE FUNCTION public.mini_reset_preparation_formula_live()', relation);
    END LOOP;
END;
$$;
