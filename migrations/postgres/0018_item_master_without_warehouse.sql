UPDATE mini_items
SET payload_json = payload_json - 'warehouse' - 'default_warehouse'
WHERE payload_json ? 'warehouse'
   OR payload_json ? 'default_warehouse';

ALTER TABLE mini_items
DROP COLUMN IF EXISTS warehouse;
