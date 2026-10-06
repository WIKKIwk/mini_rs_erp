-- Existing inventories have no recorded batch boundary. Preserve each product/owner as one set.
UPDATE mini_qolip_product_specs
SET payload_json = jsonb_set(COALESCE(payload_json, '{}'::jsonb), '{qolip_set_id}',
    to_jsonb(('legacy:' || length(lower(btrim(item_code)))::text || ':' || lower(btrim(item_code))
        || ':' || lower(btrim(COALESCE(payload_json->>'warehouse', ''))))::text), true)
WHERE COALESCE(btrim(payload_json->>'qolip_set_id'), '') = ''
  AND NULLIF(btrim(payload_json->>'warehouse'), '') IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_mini_qolip_product_specs_set_id
    ON mini_qolip_product_specs ((payload_json->>'qolip_set_id'));
