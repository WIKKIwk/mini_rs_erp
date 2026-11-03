SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- The creation request remains replayable after a catalog rename.
ALTER TABLE public.mini_preparation_formula_variants ADD COLUMN request_name TEXT;
UPDATE public.mini_preparation_formula_variants SET request_name = name WHERE request_id IS NOT NULL;
GRANT UPDATE (name) ON public.mini_preparation_formula_variants TO mini_rs_erp;

CREATE TABLE public.mini_preparation_formula_revisions (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    variant_id TEXT NOT NULL REFERENCES public.mini_preparation_formula_variants(id),
    content_key BYTEA NOT NULL CHECK (octet_length(content_key) = 32),
    formulas JSONB NOT NULL CHECK (jsonb_typeof(formulas) = 'array'),
    UNIQUE (variant_id, content_key),
    UNIQUE (variant_id, id)
);
ALTER TABLE public.mini_preparation_formula_revisions OWNER TO mini_rs_erp_owner;
REVOKE ALL ON public.mini_preparation_formula_revisions FROM PUBLIC, mini_rs_erp;
GRANT SELECT, INSERT ON public.mini_preparation_formula_revisions TO mini_rs_erp;

CREATE TABLE public.mini_preparation_order_formula_selections (
    owner_ref TEXT NOT NULL,
    order_id TEXT NOT NULL,
    variant_id TEXT NOT NULL,
    product_code TEXT NOT NULL,
    material_id TEXT NOT NULL,
    revision_id BIGINT NOT NULL,
    display_name TEXT NOT NULL CHECK (btrim(display_name) <> '' AND char_length(display_name) <= 160),
    PRIMARY KEY (owner_ref, order_id, variant_id),
    FOREIGN KEY (variant_id, owner_ref, product_code, material_id)
        REFERENCES public.mini_preparation_formula_variants(id, owner_ref, product_code, material_id),
    FOREIGN KEY (variant_id, revision_id)
        REFERENCES public.mini_preparation_formula_revisions(variant_id, id)
);
ALTER TABLE public.mini_preparation_order_formula_selections OWNER TO mini_rs_erp_owner;
REVOKE ALL ON public.mini_preparation_order_formula_selections FROM PUBLIC, mini_rs_erp;
GRANT SELECT, INSERT ON public.mini_preparation_order_formula_selections TO mini_rs_erp;
GRANT UPDATE (display_name, revision_id) ON public.mini_preparation_order_formula_selections TO mini_rs_erp;

-- Display names, timestamps, row order and decimal formatting are metadata.
CREATE FUNCTION public.mini_preparation_recipe_key(formulas JSONB) RETURNS BYTEA
LANGUAGE sql IMMUTABLE STRICT SET search_path = pg_catalog AS $$
    SELECT sha256(convert_to(COALESCE(jsonb_agg(jsonb_build_array(
        f->>'name', trim_scale((f->>'share_percent')::numeric),
        (SELECT COALESCE(jsonb_agg(jsonb_build_array(
            l->>'item_code', trim_scale((l->>'percent')::numeric))
            ORDER BY l->>'item_code' COLLATE "C"), '[]'::jsonb)
         FROM jsonb_array_elements(f->'lines') l)
    ) ORDER BY f->>'name' COLLATE "C"), '[]'::jsonb)::text, 'UTF8'))
    FROM jsonb_array_elements(formulas) f
$$;
ALTER FUNCTION public.mini_preparation_recipe_key(JSONB) OWNER TO mini_rs_erp_owner;
REVOKE ALL ON FUNCTION public.mini_preparation_recipe_key(JSONB) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION public.mini_preparation_recipe_key(JSONB) TO mini_rs_erp;

-- Where a legacy order already ran a recipe, keep its last recorded selection.
-- Unstarted legacy orders have no selection to recover.
CREATE TEMP TABLE preparation_legacy_recipe_selections ON COMMIT DROP AS
WITH snapshots AS (
    SELECT owner_ref, payload_json->'order_assignment' AS assignment
    FROM public.mini_rps_batches WHERE owner_role = 'tayyorlov_masteri'
    UNION ALL
    SELECT owner_ref, payload_json->'order_assignment'
    FROM public.mini_rps_batch_history WHERE owner_role = 'tayyorlov_masteri'
), valid AS (
    SELECT v.owner_ref, v.id AS variant_id, v.product_code, v.material_id,
           m.id AS order_id, recipe->>'variant_name' AS display_name,
           recipe->'formulas' AS formulas, recipe->>'captured_at' AS captured_at
    FROM snapshots s
    CROSS JOIN LATERAL (SELECT s.assignment->'preparation_formula' AS recipe) r
    JOIN public.mini_preparation_formula_variants v
      ON v.id = recipe->>'variant_id' AND v.owner_ref = s.owner_ref
     AND v.product_code = recipe->>'product_code' AND v.material_id = recipe->>'material_id'
    JOIN public.mini_production_maps m ON m.id = s.assignment->>'order_id'
     AND COALESCE(NULLIF(m.map_json->>'product_code',''),m.product_code) = v.product_code
    WHERE recipe->>'schema_version' = '1'
      AND recipe->>'owner_ref' = v.owner_ref
      AND btrim(recipe->>'variant_name') <> ''
      AND char_length(recipe->>'variant_name') <= 160
      AND jsonb_typeof(recipe->'formulas') = 'array'
)
SELECT DISTINCT ON (owner_ref, order_id, variant_id)
       owner_ref, order_id, variant_id, product_code, material_id, display_name,
       formulas, public.mini_preparation_recipe_key(formulas) AS content_key
FROM valid ORDER BY owner_ref, order_id, variant_id, captured_at DESC NULLS LAST;

INSERT INTO public.mini_preparation_formula_revisions (variant_id, content_key, formulas)
SELECT variant_id, content_key, formulas FROM preparation_legacy_recipe_selections
ON CONFLICT (variant_id, content_key) DO NOTHING;

INSERT INTO public.mini_preparation_order_formula_selections
    (owner_ref, order_id, variant_id, product_code, material_id, revision_id, display_name)
SELECT s.owner_ref, s.order_id, s.variant_id, s.product_code, s.material_id, r.id, s.display_name
FROM preparation_legacy_recipe_selections s
JOIN public.mini_preparation_formula_revisions r
  ON r.variant_id = s.variant_id AND r.content_key = s.content_key;
