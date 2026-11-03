SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- An older server could reuse a copy as the source of another copy.
-- Follow proven copy links to the original recipe, preserving every order selection.
WITH RECURSIVE recipes AS MATERIALIZED (
    SELECT v.id, v.owner_ref, v.name, v.product_code, v.material_id, v.request_id,
           public.mini_preparation_recipe_key(parts.formulas) AS recipe_key
    FROM public.mini_preparation_formula_variants v
    CROSS JOIN LATERAL (
        SELECT jsonb_agg(jsonb_build_object(
            'name', f.name, 'share_percent', f.share_percent, 'lines', f.lines
        )) AS formulas
        FROM public.mini_preparation_formulas f WHERE f.variant_id = v.id
    ) parts
    WHERE parts.formulas IS NOT NULL
), copy_edges AS MATERIALIZED (
    SELECT copy.id AS copy_id, parent.id AS parent_id, parent.request_id AS parent_request
    FROM recipes copy
    JOIN recipes parent ON parent.owner_ref = copy.owner_ref AND parent.id <> copy.id
      AND parent.name = copy.name AND parent.recipe_key = copy.recipe_key
      AND copy.request_id = 'reuse:' || encode(sha256(convert_to(
        '[' || to_json(copy.product_code)::text || ',' || to_json(copy.material_id)::text ||
        ',' || to_json(parent.id)::text || ']', 'UTF8')), 'hex')
    WHERE copy.request_id LIKE 'reuse:%'
      AND EXISTS (
          SELECT 1 FROM public.mini_preparation_order_formula_selections s
          WHERE s.owner_ref = copy.owner_ref AND s.variant_id = copy.id AND s.is_pinned
      )
), family AS (
    SELECT copy_id, parent_id AS source_id FROM copy_edges
    WHERE COALESCE(parent_request, '') NOT LIKE 'reuse:%'
    UNION ALL
    SELECT edge.copy_id, family.source_id
    FROM family JOIN copy_edges edge ON edge.parent_id = family.copy_id
)
UPDATE public.mini_preparation_formula_variants v
SET catalog_source_id = family.source_id
FROM family
WHERE v.id = family.copy_id AND v.catalog_source_id IS NULL;
