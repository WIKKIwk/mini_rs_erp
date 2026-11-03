SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE public.mini_preparation_formula_variants
    ADD CONSTRAINT mini_preparation_variant_owner_unique UNIQUE (id, owner_ref),
    ADD COLUMN catalog_source_id TEXT,
    ADD CONSTRAINT mini_preparation_variant_catalog_source_fk
        FOREIGN KEY (catalog_source_id, owner_ref)
        REFERENCES public.mini_preparation_formula_variants(id, owner_ref);

-- A recipe keeps its catalog product and material. Its order selections and
-- exclusions use the destination product and material instead.
ALTER TABLE public.mini_preparation_order_formula_selections
    DROP CONSTRAINT mini_preparation_order_formu_variant_id_owner_ref_product_fkey1,
    DROP CONSTRAINT mini_preparation_order_formula_selections_pkey,
    ADD PRIMARY KEY (owner_ref, order_id, variant_id, product_code, material_id),
    ADD CONSTRAINT mini_preparation_selection_variant_owner_fk
        FOREIGN KEY (variant_id, owner_ref)
        REFERENCES public.mini_preparation_formula_variants(id, owner_ref);
ALTER TABLE public.mini_preparation_order_formula_exclusions
    DROP CONSTRAINT mini_preparation_order_formul_variant_id_owner_ref_product_fkey,
    DROP CONSTRAINT mini_preparation_order_formula_exclusions_pkey,
    ADD PRIMARY KEY (owner_ref, order_id, variant_id, product_code, material_id),
    ADD CONSTRAINT mini_preparation_exclusion_variant_owner_fk
        FOREIGN KEY (variant_id, owner_ref)
        REFERENCES public.mini_preparation_formula_variants(id, owner_ref) ON DELETE CASCADE;

-- Retire duplicate catalog entries from the old order reuse flow. Match the
-- original creation request, catalog name and ingredients before hiding a copy.
-- Existing selections, codes and receipt snapshots retain their identities.
WITH proven_copies AS (
    SELECT copy.id, source.id AS source_id
    FROM public.mini_preparation_formula_variants copy
    JOIN public.mini_preparation_formula_variants source ON source.owner_ref = copy.owner_ref
      AND source.id <> copy.id AND source.name = copy.name
      AND COALESCE(source.request_id, '') NOT LIKE 'reuse:%'
      AND copy.request_id = 'reuse:' || encode(sha256(convert_to(
        '[' || to_json(copy.product_code)::text || ',' || to_json(copy.material_id)::text ||
        ',' || to_json(source.id)::text || ']', 'UTF8')), 'hex')
    WHERE copy.request_id LIKE 'reuse:%'
      AND EXISTS (SELECT 1 FROM public.mini_preparation_order_formula_selections s
                  WHERE s.owner_ref=copy.owner_ref AND s.variant_id=copy.id AND s.is_pinned)
      AND EXISTS (SELECT 1 FROM public.mini_preparation_formulas f WHERE f.variant_id=copy.id)
      AND (
          SELECT public.mini_preparation_recipe_key(jsonb_agg(jsonb_build_object(
              'name',f.name,'share_percent',f.share_percent,'lines',f.lines)))
          FROM public.mini_preparation_formulas f WHERE f.variant_id=copy.id
      ) = (
          SELECT public.mini_preparation_recipe_key(jsonb_agg(jsonb_build_object(
              'name',f.name,'share_percent',f.share_percent,'lines',f.lines)))
          FROM public.mini_preparation_formulas f WHERE f.variant_id=source.id
      )
)
UPDATE public.mini_preparation_formula_variants v
SET catalog_source_id = p.source_id
FROM proven_copies p WHERE p.id = v.id;
