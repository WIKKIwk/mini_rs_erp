SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- An empty recipe can have an order name before there is a composition to pin.
ALTER TABLE public.mini_preparation_order_formula_selections
    ADD COLUMN is_pinned BOOLEAN NOT NULL DEFAULT TRUE;
GRANT UPDATE (is_pinned) ON public.mini_preparation_order_formula_selections TO mini_rs_erp;

-- Only reopen drafts with no recorded composition. An order can intentionally
-- delete every component of a previously populated recipe.
UPDATE public.mini_preparation_order_formula_selections s
SET is_pinned = FALSE
WHERE NOT EXISTS (
    SELECT 1 FROM public.mini_preparation_formula_revisions r
    WHERE r.variant_id = s.variant_id AND r.formulas <> '[]'::jsonb
)
AND NOT EXISTS (
    SELECT 1 FROM public.mini_preparation_formulas f WHERE f.variant_id = s.variant_id
);

-- Recover variants that were created empty and edited entirely in one order.
-- Skip variants with recorded catalog components, which carry a set_id.
WITH recoverable AS (
    SELECT s.variant_id, s.owner_ref, s.product_code, s.material_id,
           material.material_name, r.formulas
    FROM public.mini_preparation_order_formula_selections s
    JOIN public.mini_preparation_formula_revisions r
      ON r.variant_id = s.variant_id AND r.id = s.revision_id
    JOIN public.mini_preparation_formula_variants v ON v.id = s.variant_id
    JOIN public.mini_production_maps m ON m.id = s.order_id
      AND COALESCE(NULLIF(m.map_json->>'product_code', ''), m.product_code) = s.product_code
    CROSS JOIN LATERAL (
        SELECT mr.material_name
        FROM public.mini_preparation_material_responsibilities mr
        WHERE mr.principal_role = 'tayyorlov_masteri' AND mr.principal_ref = s.owner_ref
          AND lower(mr.material_id) = lower(s.material_id)
        ORDER BY mr.material_id
        LIMIT 1
    ) material
    WHERE v.request_name IS NOT NULL AND r.formulas <> '[]'::jsonb
      AND NOT EXISTS (
          SELECT 1 FROM public.mini_preparation_order_formula_exclusions x
          WHERE x.owner_ref = s.owner_ref AND x.order_id = s.order_id AND x.variant_id = s.variant_id
      )
      AND NOT EXISTS (
          SELECT 1 FROM public.mini_preparation_order_formula_selections other
          WHERE other.owner_ref = s.owner_ref AND other.variant_id = s.variant_id AND other.order_id <> s.order_id
      )
      AND EXISTS (
          SELECT 1 FROM public.mini_preparation_formula_revisions empty
          WHERE empty.variant_id = s.variant_id AND empty.formulas = '[]'::jsonb AND empty.id < r.id
      )
      AND NOT EXISTS (
          SELECT 1 FROM public.mini_preparation_formulas f WHERE f.variant_id = s.variant_id
      )
      AND NOT EXISTS (
          SELECT 1 FROM public.mini_preparation_formula_revisions history
          CROSS JOIN LATERAL jsonb_array_elements(history.formulas) component
          WHERE history.variant_id = s.variant_id AND component->>'set_id' IS NOT NULL
      )
      AND NOT EXISTS (
          SELECT 1 FROM jsonb_array_elements(r.formulas) component
          WHERE component->>'variant_id' IS DISTINCT FROM s.variant_id
             OR COALESCE(component->>'formula_code', '') !~ '^9503[0-9]{8}$'
             OR EXISTS (
                 SELECT 1 FROM public.mini_preparation_formulas existing
                 WHERE existing.formula_code = component->>'formula_code'
             )
      )
)
INSERT INTO public.mini_preparation_formulas
    (owner_ref, product_code, material_id, material_name, variant_id, name, lines, share_percent, formula_code)
SELECT d.owner_ref, d.product_code, d.material_id, d.material_name, d.variant_id,
       c->>'name', c->'lines', (c->>'share_percent')::numeric, c->>'formula_code'
FROM recoverable d
CROSS JOIN LATERAL jsonb_array_elements(d.formulas) c;
