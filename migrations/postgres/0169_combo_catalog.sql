-- A combo revision is a non-stock production identity. Its component SKUs
-- remain the physical finished outputs and every layout revision owns a SKU.
DO $$
DECLARE finished_group TEXT; existing_parent TEXT;
BEGIN
    INSERT INTO public.mini_item_groups(name, parent_item_group, is_group)
        VALUES ('All Item Groups', NULL, TRUE) ON CONFLICT (name) DO NOTHING;
    SELECT name INTO finished_group FROM public.mini_item_groups
        WHERE lower(btrim(name)) = 'tayyor mahsulot' ORDER BY name LIMIT 1;
    IF finished_group IS NULL THEN
        finished_group := 'Tayyor mahsulot';
        INSERT INTO public.mini_item_groups(name, parent_item_group, is_group)
            VALUES (finished_group, 'All Item Groups', TRUE);
    END IF;
    SELECT parent_item_group INTO existing_parent FROM public.mini_item_groups WHERE name = 'Kombo';
    IF FOUND AND (existing_parent IS DISTINCT FROM finished_group OR EXISTS (
        SELECT 1 FROM public.mini_item_groups WHERE name = 'Kombo'
            AND payload_json->>'kind' IS DISTINCT FROM 'combo_group'
    )) THEN
        RAISE EXCEPTION 'combo_group_already_used';
    END IF;
    INSERT INTO public.mini_item_groups(name, parent_item_group, is_group, payload_json)
        VALUES ('Kombo', finished_group, TRUE, '{"kind":"combo_group"}'::jsonb)
        ON CONFLICT (name) DO NOTHING;
END $$;

CREATE TABLE public.mini_combo_masters (
    id TEXT PRIMARY KEY CHECK (btrim(id) <> ''),
    customer_ref TEXT NOT NULL REFERENCES public.mini_customers(ref) ON UPDATE RESTRICT ON DELETE RESTRICT,
    name TEXT NOT NULL CHECK (name = btrim(name) AND char_length(name) BETWEEN 1 AND 160),
    normalized_name TEXT NOT NULL CHECK (btrim(normalized_name) <> ''),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (customer_ref, normalized_name)
);
CREATE TABLE public.mini_combo_revisions (
    combo_id TEXT NOT NULL REFERENCES public.mini_combo_masters(id) ON UPDATE RESTRICT ON DELETE RESTRICT,
    revision BIGINT NOT NULL CHECK (revision > 0),
    item_code TEXT NOT NULL UNIQUE REFERENCES public.mini_items(code) ON UPDATE RESTRICT ON DELETE RESTRICT,
    layout_fingerprint TEXT NOT NULL CHECK (layout_fingerprint ~ '^[0-9a-f]{64}$'),
    reference_json JSONB NOT NULL CHECK (jsonb_typeof(reference_json) = 'object'),
    template_json JSONB NOT NULL CHECK (jsonb_typeof(template_json) = 'object'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (combo_id, revision),
    UNIQUE (combo_id, layout_fingerprint),
    CHECK (reference_json ?& ARRAY['id','item_code','name','revision','layout_fingerprint']
        AND reference_json->>'id' = combo_id AND reference_json->>'item_code' = item_code
        AND reference_json->'revision' = to_jsonb(revision)
        AND reference_json->>'layout_fingerprint' = layout_fingerprint
        AND jsonb_typeof(reference_json->'name') = 'string'),
    CHECK (template_json ?& ARRAY['combo','item_code','name','product','products']
        AND template_json->'combo' = reference_json
        AND template_json->>'item_code' = item_code
        AND template_json->>'name' = reference_json->>'name'
        AND template_json->>'product' = reference_json->>'name'
        AND jsonb_typeof(template_json->'products') = 'array'
        AND jsonb_array_length(template_json->'products') > 0)
);
CREATE INDEX mini_combo_revisions_products_gin ON public.mini_combo_revisions
    USING gin ((template_json->'products') jsonb_path_ops);
ALTER TABLE public.mini_combo_masters OWNER TO mini_rs_erp_owner;
ALTER TABLE public.mini_combo_revisions OWNER TO mini_rs_erp_owner;
REVOKE ALL ON public.mini_combo_masters, public.mini_combo_revisions FROM PUBLIC, mini_rs_erp;
GRANT SELECT, INSERT ON public.mini_combo_masters, public.mini_combo_revisions TO mini_rs_erp;

CREATE FUNCTION public.mini_combo_revision_immutable() RETURNS trigger
LANGUAGE plpgsql SET search_path = public, pg_temp AS $$
BEGIN
    RAISE EXCEPTION 'combo_revision_immutable';
END $$;
CREATE TRIGGER mini_combo_revision_immutable BEFORE UPDATE OR DELETE
ON public.mini_combo_revisions FOR EACH ROW EXECUTE FUNCTION public.mini_combo_revision_immutable();
CREATE TRIGGER mini_combo_master_immutable BEFORE UPDATE OR DELETE
ON public.mini_combo_masters FOR EACH ROW EXECUTE FUNCTION public.mini_combo_revision_immutable();

CREATE FUNCTION public.mini_combo_catalog_guard() RETURNS trigger
LANGUAGE plpgsql SET search_path = public, pg_temp AS $$
BEGIN
    IF TG_OP <> 'INSERT' AND (TG_OP = 'DELETE' OR NEW.code IS DISTINCT FROM OLD.code)
        AND EXISTS (SELECT 1 FROM public.mini_combo_revisions
            WHERE template_json->'products' @> jsonb_build_array(jsonb_build_object('item_code', OLD.code))) THEN
        RAISE EXCEPTION 'combo_component_in_use';
    END IF;
    IF TG_OP <> 'INSERT' AND EXISTS (
        SELECT 1 FROM public.mini_combo_revisions WHERE item_code = OLD.code
    ) THEN
        IF TG_OP = 'DELETE' THEN RAISE EXCEPTION 'combo_catalog_immutable'; END IF;
        IF (NEW.code, NEW.name, NEW.uom, NEW.item_group, NEW.payload_json)
            IS DISTINCT FROM (OLD.code, OLD.name, OLD.uom, OLD.item_group, OLD.payload_json) THEN
            RAISE EXCEPTION 'combo_catalog_immutable';
        END IF;
    END IF;
    IF TG_OP <> 'DELETE' AND lower(btrim(NEW.item_group)) = 'kombo'
        AND (NEW.item_group <> 'Kombo' OR NEW.payload_json->>'kind' IS DISTINCT FROM 'combo') THEN
        RAISE EXCEPTION 'combo_catalog_reserved';
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER mini_combo_catalog_guard BEFORE INSERT OR UPDATE OR DELETE
ON public.mini_items FOR EACH ROW EXECUTE FUNCTION public.mini_combo_catalog_guard();

CREATE FUNCTION public.mini_combo_catalog_registered() RETURNS trigger
LANGUAGE plpgsql SET search_path = public, pg_temp AS $$
BEGIN
    IF NEW.item_group = 'Kombo' OR NEW.payload_json->>'kind' = 'combo' THEN
        IF NEW.item_group <> 'Kombo' OR NEW.payload_json->>'kind' IS DISTINCT FROM 'combo'
            OR NOT EXISTS (SELECT 1 FROM public.mini_combo_revisions revision
                WHERE revision.item_code = NEW.code
                    AND revision.reference_json = NEW.payload_json->'combo') THEN
            RAISE EXCEPTION 'combo_catalog_revision_required';
        END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE CONSTRAINT TRIGGER mini_combo_catalog_registered AFTER INSERT OR UPDATE
ON public.mini_items DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION public.mini_combo_catalog_registered();

CREATE FUNCTION public.mini_combo_group_guard() RETURNS trigger
LANGUAGE plpgsql SET search_path = public, pg_temp AS $$
BEGIN
    IF TG_OP <> 'INSERT' AND lower(btrim(OLD.name)) = 'kombo' THEN
        IF TG_OP = 'DELETE' THEN RAISE EXCEPTION 'combo_group_reserved'; END IF;
        IF (NEW.name, NEW.parent_item_group, NEW.is_group)
            IS DISTINCT FROM (OLD.name, OLD.parent_item_group, OLD.is_group) THEN
            RAISE EXCEPTION 'combo_group_reserved';
        END IF;
    END IF;
    IF TG_OP <> 'DELETE' AND lower(btrim(NEW.name)) = 'kombo' AND NEW.name <> 'Kombo' THEN
        RAISE EXCEPTION 'combo_group_reserved';
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER mini_combo_group_guard BEFORE INSERT OR UPDATE OR DELETE
ON public.mini_item_groups FOR EACH ROW EXECUTE FUNCTION public.mini_combo_group_guard();

CREATE FUNCTION public.mini_combo_stock_guard() RETURNS trigger
LANGUAGE plpgsql SET search_path = public, pg_temp AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM public.mini_combo_revisions
        WHERE lower(item_code) = lower(btrim(NEW.item_code))) THEN
        RAISE EXCEPTION 'combo_is_not_finished_stock';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER mini_combo_stock_guard BEFORE INSERT OR UPDATE OF item_code
ON public.mini_finished_goods_stock FOR EACH ROW EXECUTE FUNCTION public.mini_combo_stock_guard();
CREATE TRIGGER mini_combo_stock_guard BEFORE INSERT OR UPDATE OF item_code
ON public.mini_raw_material_stock FOR EACH ROW EXECUTE FUNCTION public.mini_combo_stock_guard();

CREATE FUNCTION public.mini_combo_customer_guard() RETURNS trigger
LANGUAGE plpgsql SET search_path = public, pg_temp AS $$
DECLARE expected_customer TEXT; code TEXT;
BEGIN
    IF TG_OP = 'UPDATE' AND (NEW.item_code, NEW.customer_ref)
        IS DISTINCT FROM (OLD.item_code, OLD.customer_ref) AND EXISTS (
            SELECT 1 FROM public.mini_combo_revisions WHERE item_code = OLD.item_code
        ) THEN RAISE EXCEPTION 'combo_customer_immutable'; END IF;
    IF TG_OP = 'DELETE' THEN code := OLD.item_code; ELSE code := NEW.item_code; END IF;
    SELECT master.customer_ref INTO expected_customer
        FROM public.mini_combo_revisions revision JOIN public.mini_combo_masters master
        ON master.id = revision.combo_id WHERE revision.item_code = code;
    IF FOUND THEN
        IF TG_OP = 'DELETE' THEN RAISE EXCEPTION 'combo_customer_immutable'; END IF;
        IF NEW.customer_ref <> expected_customer THEN RAISE EXCEPTION 'combo_customer_immutable'; END IF;
        IF TG_OP = 'UPDATE' AND (NEW.item_code, NEW.customer_ref)
            IS DISTINCT FROM (OLD.item_code, OLD.customer_ref) THEN
            RAISE EXCEPTION 'combo_customer_immutable';
        END IF;
    END IF;
    IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER mini_combo_customer_guard BEFORE INSERT OR UPDATE OR DELETE
ON public.mini_customer_items FOR EACH ROW EXECUTE FUNCTION public.mini_combo_customer_guard();
REVOKE ALL ON FUNCTION public.mini_combo_revision_immutable(), public.mini_combo_catalog_guard(),
    public.mini_combo_catalog_registered(), public.mini_combo_group_guard(), public.mini_combo_stock_guard(),
    public.mini_combo_customer_guard() FROM PUBLIC;
