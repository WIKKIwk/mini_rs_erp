ALTER TABLE mini_production_maps
    ADD COLUMN standard_print_side_defaulted boolean NOT NULL DEFAULT false;

-- Persist the displayed legacy default without treating it as a customer choice.
UPDATE mini_production_maps
SET map_json = jsonb_set(map_json, '{standard_print_side}', 'true'::jsonb),
    standard_print_side_defaulted = true
WHERE btrim(id) LIKE 'zakaz-%'
  AND btrim(id) NOT LIKE 'zakaz-draft-%'
  AND map_json->>'standard_print_side' IS NULL;
