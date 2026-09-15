ALTER TABLE mini_orders
    ADD COLUMN calculation_json JSONB,
    ADD COLUMN calculation_revision BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN calculation_edit_log JSONB NOT NULL DEFAULT '[]'::jsonb;

-- Telegram completion is an immutable, order-specific copy of the input.
UPDATE mini_orders o SET calculation_json = p.completion_json->'template'
FROM mini_pending_orders p
WHERE p.id = o.id AND p.completion_json IS NOT NULL;
