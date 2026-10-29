CREATE TABLE public.mini_preparation_order_formula_exclusions (
    owner_ref TEXT NOT NULL,
    order_id TEXT NOT NULL REFERENCES public.mini_production_maps(id) ON DELETE CASCADE,
    variant_id TEXT NOT NULL,
    product_code TEXT NOT NULL,
    material_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (owner_ref, order_id, variant_id),
    FOREIGN KEY (variant_id, owner_ref, product_code, material_id)
        REFERENCES public.mini_preparation_formula_variants(id, owner_ref, product_code, material_id)
        ON DELETE CASCADE
);
ALTER TABLE public.mini_preparation_order_formula_exclusions OWNER TO mini_rs_erp_owner;
REVOKE ALL ON public.mini_preparation_order_formula_exclusions FROM PUBLIC, mini_rs_erp;
GRANT SELECT, INSERT, DELETE ON public.mini_preparation_order_formula_exclusions TO mini_rs_erp;
