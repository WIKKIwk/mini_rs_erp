SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- A material's named formulas are screw mixtures in one combined recipe.
-- Existing multi-screw recipes require an explicit allocation by the master.
ALTER TABLE public.mini_preparation_formulas
    ADD COLUMN share_percent NUMERIC(9,6)
        CHECK (share_percent > 0 AND share_percent <= 100);

UPDATE public.mini_preparation_formulas f SET share_percent = 100
WHERE (SELECT count(*) FROM public.mini_preparation_formulas other
    WHERE other.owner_ref = f.owner_ref AND other.product_code = f.product_code
      AND lower(other.material_id) = lower(f.material_id)) = 1;
