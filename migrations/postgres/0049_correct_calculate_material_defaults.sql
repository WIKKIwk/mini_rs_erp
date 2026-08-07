DELETE FROM mini_calculate_materials
WHERE id = 'builtin-pe-qora';

UPDATE mini_calculate_materials
SET payload_json = jsonb_set(payload_json, '{density_g_cm3}', '0.905'::jsonb, true),
    updated_at = now()
WHERE id IN ('builtin-bopp', 'builtin-mcpp')
  AND payload_json->>'density_g_cm3' IN ('0.9', '0.90', '0.91')
  AND NOT EXISTS (
      SELECT 1
      FROM jsonb_array_elements(COALESCE(payload_json->'variants', '[]'::jsonb)) AS variant
      WHERE variant ? 'actual_gsm'
  );
