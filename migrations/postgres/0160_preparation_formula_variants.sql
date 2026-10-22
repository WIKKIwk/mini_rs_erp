SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

CREATE TABLE public.mini_preparation_formula_variants (
    id TEXT PRIMARY KEY DEFAULT ('fv-' || gen_random_uuid()::text),
    owner_ref TEXT NOT NULL REFERENCES public.mini_system_users(id) ON DELETE RESTRICT,
    product_code TEXT NOT NULL CHECK (btrim(product_code) <> '' AND char_length(product_code) <= 160),
    material_id TEXT NOT NULL CHECK (char_length(material_id) <= 128),
    name TEXT NOT NULL CHECK (btrim(name) <> '' AND char_length(name) <= 160),
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    request_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, owner_ref, product_code, material_id)
);
ALTER TABLE public.mini_preparation_formula_variants OWNER TO mini_rs_erp_owner;
REVOKE ALL ON public.mini_preparation_formula_variants FROM PUBLIC, mini_rs_erp;
GRANT SELECT, INSERT ON public.mini_preparation_formula_variants TO mini_rs_erp;
CREATE UNIQUE INDEX mini_preparation_variant_name
    ON public.mini_preparation_formula_variants(owner_ref, product_code, material_id, lower(name));
CREATE UNIQUE INDEX mini_preparation_variant_default
    ON public.mini_preparation_formula_variants(owner_ref, product_code, material_id) WHERE is_default;
CREATE UNIQUE INDEX mini_preparation_variant_request
    ON public.mini_preparation_formula_variants(owner_ref, request_id) WHERE request_id IS NOT NULL;

INSERT INTO public.mini_preparation_formula_variants(owner_ref, product_code, material_id, name, is_default)
SELECT DISTINCT owner_ref, product_code, material_id, 'Asosiy', TRUE
FROM public.mini_preparation_formulas;

ALTER TABLE public.mini_preparation_formulas ADD COLUMN variant_id TEXT;
UPDATE public.mini_preparation_formulas f SET variant_id = v.id
FROM public.mini_preparation_formula_variants v
WHERE v.owner_ref=f.owner_ref AND v.product_code=f.product_code AND v.material_id=f.material_id;
ALTER TABLE public.mini_preparation_formulas
    ALTER COLUMN variant_id SET NOT NULL,
    DROP CONSTRAINT mini_preparation_formulas_pkey,
    ADD PRIMARY KEY (owner_ref, product_code, material_id, variant_id, name),
    ADD CONSTRAINT mini_preparation_formula_variant_scope_fk
        FOREIGN KEY (variant_id, owner_ref, product_code, material_id)
        REFERENCES public.mini_preparation_formula_variants(id, owner_ref, product_code, material_id)
        ON DELETE RESTRICT ON UPDATE RESTRICT;
CREATE INDEX mini_preparation_formulas_variant ON public.mini_preparation_formulas(variant_id);

-- Older clients continue operating on one explicit default variant.
CREATE FUNCTION public.mini_preparation_default_formula_variant(owner_key TEXT, product TEXT, material TEXT)
RETURNS TEXT LANGUAGE sql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
    INSERT INTO public.mini_preparation_formula_variants(owner_ref, product_code, material_id, name, is_default)
    VALUES (owner_key, product, material, 'Asosiy', TRUE)
    ON CONFLICT (owner_ref, product_code, material_id) WHERE is_default
    DO UPDATE SET is_default=TRUE RETURNING id
$$;
ALTER FUNCTION public.mini_preparation_default_formula_variant(TEXT,TEXT,TEXT) OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_preparation_default_formula_variant(TEXT,TEXT,TEXT) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION public.mini_preparation_default_formula_variant(TEXT,TEXT,TEXT) TO mini_rs_erp;

CREATE FUNCTION public.mini_preparation_assign_formula_variant() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp AS $$
BEGIN
    IF NEW.variant_id IS NULL OR btrim(NEW.variant_id) = '' THEN
        NEW.variant_id := public.mini_preparation_default_formula_variant(NEW.owner_ref, NEW.product_code, NEW.material_id);
    END IF;
    RETURN NEW;
END
$$;
ALTER FUNCTION public.mini_preparation_assign_formula_variant() OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_preparation_assign_formula_variant() FROM PUBLIC, mini_rs_erp;
CREATE TRIGGER mini_preparation_assign_formula_variant
    BEFORE INSERT ON public.mini_preparation_formulas
    FOR EACH ROW EXECUTE FUNCTION public.mini_preparation_assign_formula_variant();
