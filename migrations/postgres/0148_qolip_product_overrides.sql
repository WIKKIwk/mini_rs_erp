-- Product composition changes never rewrite a shared physical set.
ALTER TABLE public.mini_qolip_product_specs
    ADD CONSTRAINT mini_qolip_specs_code_reference UNIQUE (qolip_code);

CREATE TABLE public.mini_qolip_product_overrides (
    item_code TEXT NOT NULL REFERENCES public.mini_items(code) ON UPDATE CASCADE ON DELETE CASCADE,
    qolip_code TEXT NOT NULL REFERENCES public.mini_qolip_product_specs(qolip_code) ON UPDATE CASCADE ON DELETE CASCADE,
    item_code_key TEXT GENERATED ALWAYS AS (lower(btrim(item_code))) STORED,
    qolip_code_key TEXT GENERATED ALWAYS AS (lower(btrim(qolip_code))) STORED,
    mode TEXT NOT NULL CHECK (mode IN ('include', 'exclude')),
    created_by_role TEXT NOT NULL,
    created_by_ref TEXT NOT NULL,
    created_by_name TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (item_code_key, qolip_code_key)
);
CREATE INDEX idx_mini_qolip_overrides_code ON public.mini_qolip_product_overrides(qolip_code_key);
ALTER TABLE public.mini_qolip_product_overrides OWNER TO mini_rs_erp_owner;
REVOKE ALL ON public.mini_qolip_product_overrides FROM PUBLIC;
GRANT SELECT, INSERT, UPDATE, DELETE ON public.mini_qolip_product_overrides TO mini_rs_erp;

-- One source of truth for catalog reads, scanners and transactional validation.
CREATE VIEW public.mini_qolip_effective_product_molds AS
WITH base AS (
    SELECT link.item_code, link.item_code_key, spec.qolip_code,
           lower(btrim(spec.qolip_code)) AS qolip_code_key
    FROM public.mini_qolip_set_products link
    JOIN public.mini_qolip_product_specs spec ON spec.qolip_set_id = link.set_id
    JOIN public.mini_items item ON item.code = link.item_code
    WHERE (lower(btrim(spec.item_code)) <> link.item_code_key
           OR lower(btrim(spec.item_group)) = lower(btrim(item.item_group)))
    UNION ALL
    SELECT spec.item_code, lower(btrim(spec.item_code)), spec.qolip_code, lower(btrim(spec.qolip_code))
    FROM public.mini_qolip_product_specs spec
    WHERE spec.qolip_set_id IS NULL
)
SELECT base.*, false AS is_extension
FROM base
WHERE NOT EXISTS (
    SELECT 1 FROM public.mini_qolip_product_overrides override
    WHERE override.item_code_key = base.item_code_key
      AND override.qolip_code_key = base.qolip_code_key AND override.mode = 'exclude'
)
UNION ALL
SELECT override.item_code, override.item_code_key, override.qolip_code, override.qolip_code_key, true
FROM public.mini_qolip_product_overrides override
WHERE override.mode = 'include' AND NOT EXISTS (
    SELECT 1 FROM base
    WHERE base.item_code_key = override.item_code_key AND base.qolip_code_key = override.qolip_code_key
);
ALTER VIEW public.mini_qolip_effective_product_molds OWNER TO mini_rs_erp_owner;
REVOKE ALL ON public.mini_qolip_effective_product_molds FROM PUBLIC;
GRANT SELECT ON public.mini_qolip_effective_product_molds TO mini_rs_erp;
