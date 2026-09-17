SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- A preparation-owned material is created from a specific exclusive warehouse.
-- Keep the binding on the catalog row so sibling/parent warehouses do not
-- inherit the material merely because they belong to the same master.
ALTER TABLE mini_preparation_materials
    ADD COLUMN IF NOT EXISTS warehouse_name TEXT;

-- Recover the creation warehouse for rows written by the preparation
-- operation log. This keeps the new scope correct for existing materials
-- without guessing from current warehouse assignments.
WITH created_materials AS (
    SELECT DISTINCT ON (response_json->>'item_code')
           response_json->>'item_code' AS item_code,
           btrim(response_json->>'warehouse') AS warehouse_name
    FROM mini_preparation_operations
    WHERE kind = 'material'
      AND btrim(COALESCE(response_json->>'item_code', '')) <> ''
      AND btrim(COALESCE(response_json->>'warehouse', '')) <> ''
    ORDER BY response_json->>'item_code', created_at, id
)
UPDATE mini_preparation_materials material
SET warehouse_name = warehouse.name
FROM created_materials created
JOIN mini_warehouses warehouse
  ON lower(warehouse.name) = lower(created.warehouse_name)
WHERE material.item_code = created.item_code
  AND material.warehouse_name IS NULL;

-- Older rows may have a receipt but no retained material creation operation.
-- Infer a binding only when every preparation receipt for the material is in
-- one warehouse; ambiguous historical rows remain legacy-scoped.
WITH receipt_warehouses AS (
    SELECT receipt.item_code, min(warehouse.name) AS warehouse_name
    FROM mini_preparation_receipts receipt
    JOIN mini_raw_material_stock stock ON stock.id = receipt.stock_id
    JOIN mini_warehouses warehouse
      ON lower(warehouse.name) = lower(stock.warehouse)
    GROUP BY receipt.item_code
    HAVING count(DISTINCT lower(stock.warehouse)) = 1
)
UPDATE mini_preparation_materials material
SET warehouse_name = receipt_warehouse.warehouse_name
FROM receipt_warehouses receipt_warehouse
WHERE material.item_code = receipt_warehouse.item_code
  AND material.warehouse_name IS NULL;

ALTER TABLE mini_preparation_materials
    ADD CONSTRAINT mini_preparation_materials_warehouse_name_fkey
    FOREIGN KEY (warehouse_name) REFERENCES mini_warehouses(name)
    ON UPDATE CASCADE
    ON DELETE RESTRICT NOT VALID;
ALTER TABLE mini_preparation_materials
    VALIDATE CONSTRAINT mini_preparation_materials_warehouse_name_fkey;

CREATE INDEX IF NOT EXISTS idx_mini_preparation_materials_warehouse
    ON mini_preparation_materials (warehouse_name)
    WHERE warehouse_name IS NOT NULL;
