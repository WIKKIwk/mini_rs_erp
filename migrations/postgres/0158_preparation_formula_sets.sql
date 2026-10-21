SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- A master's formulas for one product form one durable set across material
-- scopes and orders. Keep the registry when its last formula is removed.
CREATE SEQUENCE public.mini_preparation_formula_set_id_seq AS BIGINT
    MINVALUE 1 MAXVALUE 99999999 NO CYCLE;
ALTER SEQUENCE public.mini_preparation_formula_set_id_seq OWNER TO mini_rs_erp_owner;
REVOKE ALL ON SEQUENCE public.mini_preparation_formula_set_id_seq FROM PUBLIC, mini_rs_erp;

CREATE FUNCTION public.mini_preparation_next_formula_set_id() RETURNS TEXT
LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
    SELECT '9502' || lpad(nextval('public.mini_preparation_formula_set_id_seq')::TEXT, 8, '0')
$$;
ALTER FUNCTION public.mini_preparation_next_formula_set_id() OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_preparation_next_formula_set_id() FROM PUBLIC, mini_rs_erp;

CREATE FUNCTION public.mini_preparation_formula_set_default_name(product TEXT) RETURNS TEXT
LANGUAGE sql STABLE SET search_path = pg_catalog, pg_temp AS $$
    SELECT left(COALESCE(
        (SELECT NULLIF(btrim(i.name), '') FROM public.mini_items i WHERE i.code = product),
        (SELECT NULLIF(btrim(t.product_name), '') FROM public.mini_quick_order_templates t
         WHERE t.item_code = product AND btrim(t.product_name) <> ''
         ORDER BY t.saved_at DESC, t.id LIMIT 1),
        (SELECT NULLIF(btrim(m.title), '') FROM public.mini_production_maps m
         WHERE m.product_code = product AND m.id NOT LIKE 'template-%'
         ORDER BY m.created_at DESC, m.id LIMIT 1),
        'Mahsulot ' || product), 160)
$$;
ALTER FUNCTION public.mini_preparation_formula_set_default_name(TEXT) OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_preparation_formula_set_default_name(TEXT) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION public.mini_preparation_formula_set_default_name(TEXT) TO mini_rs_erp;

CREATE TABLE public.mini_preparation_formula_sets (
    set_id TEXT PRIMARY KEY DEFAULT public.mini_preparation_next_formula_set_id()
        CHECK (set_id ~ '^9502[0-9]{8}$'),
    owner_ref TEXT NOT NULL REFERENCES public.mini_system_users(id) ON DELETE RESTRICT,
    product_code TEXT NOT NULL CHECK (btrim(product_code) <> '' AND char_length(product_code) <= 160),
    name TEXT NOT NULL CHECK (btrim(name) <> '' AND char_length(name) <= 160),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (owner_ref, product_code),
    UNIQUE (set_id, owner_ref, product_code)
);
ALTER TABLE public.mini_preparation_formula_sets OWNER TO mini_rs_erp_owner;
ALTER SEQUENCE public.mini_preparation_formula_set_id_seq
    OWNED BY public.mini_preparation_formula_sets.set_id;
REVOKE ALL ON public.mini_preparation_formula_sets FROM PUBLIC, mini_rs_erp;
GRANT SELECT ON public.mini_preparation_formula_sets TO mini_rs_erp;
GRANT UPDATE (name, updated_at) ON public.mini_preparation_formula_sets TO mini_rs_erp;

INSERT INTO public.mini_preparation_formula_sets(owner_ref, product_code, name, created_at, updated_at)
SELECT owner_ref, product_code, public.mini_preparation_formula_set_default_name(product_code),
       min(created_at), max(updated_at)
FROM public.mini_preparation_formulas
GROUP BY owner_ref, product_code
ORDER BY min(created_at), owner_ref, product_code;

ALTER TABLE public.mini_preparation_formulas ADD COLUMN set_id TEXT;
UPDATE public.mini_preparation_formulas f SET set_id = s.set_id
FROM public.mini_preparation_formula_sets s
WHERE s.owner_ref = f.owner_ref AND s.product_code = f.product_code;
ALTER TABLE public.mini_preparation_formulas
    ALTER COLUMN set_id SET NOT NULL,
    ADD CONSTRAINT mini_preparation_formula_set_scope_fk
        FOREIGN KEY (set_id, owner_ref, product_code)
        REFERENCES public.mini_preparation_formula_sets(set_id, owner_ref, product_code)
        ON UPDATE RESTRICT ON DELETE RESTRICT;
CREATE INDEX mini_preparation_formulas_set ON public.mini_preparation_formulas(set_id);

-- Existing formula writers also receive an ID. Concurrent first inserts use
-- the owner/product uniqueness constraint; changing a name never rebinds a set.
CREATE FUNCTION public.mini_preparation_register_formula_set() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
DECLARE
    identity TEXT;
BEGIN
    SELECT s.set_id INTO identity FROM public.mini_preparation_formula_sets s
    WHERE s.owner_ref = NEW.owner_ref AND s.product_code = NEW.product_code;
    IF identity IS NULL THEN
        INSERT INTO public.mini_preparation_formula_sets(owner_ref, product_code, name)
        VALUES (NEW.owner_ref, NEW.product_code,
                public.mini_preparation_formula_set_default_name(NEW.product_code))
        ON CONFLICT (owner_ref, product_code)
        DO UPDATE SET product_code = EXCLUDED.product_code
        RETURNING set_id INTO identity;
    END IF;
    IF NEW.set_id IS NOT NULL AND NEW.set_id <> identity THEN
        RAISE EXCEPTION 'preparation_formula_set_scope' USING ERRCODE = '23514';
    END IF;
    NEW.set_id := identity;
    RETURN NEW;
END;
$$;
ALTER FUNCTION public.mini_preparation_register_formula_set() OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_preparation_register_formula_set() FROM PUBLIC, mini_rs_erp;
CREATE TRIGGER mini_preparation_register_formula_set
    BEFORE INSERT ON public.mini_preparation_formulas
    FOR EACH ROW EXECUTE FUNCTION public.mini_preparation_register_formula_set();
