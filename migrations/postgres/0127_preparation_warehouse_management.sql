-- Persist creator ownership separately from mutable warehouse assignments.
ALTER TABLE mini_warehouses ADD COLUMN preparation_owner_ref TEXT;

-- Legacy creation did not record a creator. Adopt only an unambiguously
-- exclusive child of the same master's exclusive parent; shared/root rows
-- never acquire management rights through this compatibility backfill.
UPDATE mini_warehouses child
SET preparation_owner_ref = mine.principal_ref
FROM mini_warehouse_assignments mine, mini_warehouses parent
WHERE NOT child.is_group AND NOT parent.is_group
  AND lower(child.parent_warehouse) = lower(parent.name)
  AND mine.assignment_kind = 'warehouse'
  AND lower(mine.warehouse_name) = lower(child.name)
  AND mine.principal_role = 'tayyorlov_masteri'
  AND EXISTS (
      SELECT 1 FROM mini_warehouse_assignments p
      WHERE p.assignment_kind = 'warehouse' AND p.warehouse_name = parent.name
        AND p.principal_role = mine.principal_role AND p.principal_ref = mine.principal_ref
  )
  AND NOT EXISTS (
      SELECT 1 FROM mini_warehouse_assignments other
      WHERE other.assignment_kind = 'warehouse'
        AND lower(other.warehouse_name) IN (lower(child.name), lower(parent.name))
        AND (other.principal_role <> mine.principal_role OR other.principal_ref <> mine.principal_ref)
  );

-- Stable references for history display; the original commands remain immutable.
CREATE TABLE mini_preparation_warehouse_history_names (
    operation_id TEXT PRIMARY KEY REFERENCES mini_preparation_operations(id) ON DELETE RESTRICT,
    warehouse_id TEXT NOT NULL REFERENCES mini_warehouses(id) ON DELETE RESTRICT
);
INSERT INTO mini_preparation_warehouse_history_names(operation_id, warehouse_id)
SELECT operation.id, warehouse.id
FROM mini_preparation_operations operation
JOIN mini_warehouses warehouse
  ON warehouse.name = operation.response_json->>'warehouse';

CREATE FUNCTION mini_preparation_link_history_warehouse() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO mini_preparation_warehouse_history_names(operation_id, warehouse_id)
    SELECT NEW.id, id FROM mini_warehouses WHERE name = NEW.response_json->>'warehouse';
    RETURN NEW;
END $$;
CREATE TRIGGER mini_preparation_link_history_warehouse
AFTER INSERT ON mini_preparation_operations
FOR EACH ROW EXECUTE FUNCTION mini_preparation_link_history_warehouse();

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        GRANT SELECT, INSERT, UPDATE ON mini_preparation_warehouse_history_names TO mini_rs_erp;
    END IF;
END $$;
