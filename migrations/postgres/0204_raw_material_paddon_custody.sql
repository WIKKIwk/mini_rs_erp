ALTER TABLE mini_raw_material_paddon_units ADD COLUMN opened BOOLEAN NOT NULL DEFAULT false;
GRANT UPDATE (opened) ON mini_raw_material_paddon_units TO mini_rs_erp;

ALTER TABLE mini_raw_material_stock DROP CONSTRAINT mini_raw_material_stock_status_allowed,
    ADD CONSTRAINT mini_raw_material_stock_status_allowed
    CHECK (status IN ('available','reserved','in_use','consumed','deleted','on_paddon'));
ALTER TABLE mini_raw_material_events DROP CONSTRAINT mini_rme_status_before_allowed,
    ADD CONSTRAINT mini_rme_status_before_allowed CHECK (stock_status_before IN ('available','reserved','in_use','consumed','deleted','on_paddon')),
    DROP CONSTRAINT mini_rme_status_after_allowed,
    ADD CONSTRAINT mini_rme_status_after_allowed CHECK (stock_status_after IN ('available','reserved','in_use','consumed','deleted','on_paddon')),
    DROP CONSTRAINT mini_rme_split_consistent,
    ADD CONSTRAINT mini_rme_split_consistent CHECK (
        (event_type NOT IN ('split_consumed','split_produced') AND source_type <> 'raw_material_split')
        OR (source_type = 'raw_material_split' AND order_id IS NULL AND apparatus IS NULL AND (
            (event_type = 'split_consumed' AND stock_status_before = 'available' AND stock_status_after = 'consumed')
            OR (event_type = 'split_produced' AND stock_status_before IS NULL AND stock_status_after = 'available')
            OR (event_type = 'split_consumed' AND payload_json->>'source' IS NOT DISTINCT FROM 'raw_material_paddon'
                AND stock_status_before = 'on_paddon' AND stock_status_after = 'on_paddon'))));

-- Adopt stock still physically free on its original pallet. Existing commitments finish normally.
UPDATE mini_raw_material_stock s SET status='on_paddon', updated_at=now()
FROM mini_raw_material_paddon_units u
JOIN mini_raw_material_paddons p ON p.id=u.paddon_id
JOIN mini_warehouses w ON w.id=p.warehouse_id
WHERE u.stock_id=s.id AND u.issue_id IS NULL AND lower(s.warehouse)=lower(w.name)
  AND s.status='available' AND s.qty>0 AND s.reserved_order_id=''
  AND coalesce(s.payload_json->>'inventory_transfer_id','')=''
  AND NOT EXISTS (SELECT 1 FROM mini_raw_material_assignments a WHERE lower(a.barcode)=lower(s.barcode))
  AND NOT EXISTS (SELECT 1 FROM mini_inventory_placements pl
      JOIN mini_inventory_locations loc ON loc.id=pl.physical_location_id
      WHERE pl.asset_kind='raw_material' AND pl.asset_ref=s.id AND loc.warehouse_id IS DISTINCT FROM w.id);

CREATE TABLE mini_raw_material_paddon_label_prints (
    issue_id TEXT NOT NULL REFERENCES mini_raw_material_paddon_issues(id),
    barcode TEXT NOT NULL,
    attempt TEXT NOT NULL,
    printed BOOLEAN NOT NULL DEFAULT false,
    PRIMARY KEY (issue_id, barcode)
);
GRANT SELECT, INSERT, UPDATE (attempt, printed) ON mini_raw_material_paddon_label_prints TO mini_rs_erp;

-- Keep prior commitments intact. Only a cancellation while still at the source
-- puts a unit back under pallet control; a later return transfer does not.
UPDATE mini_raw_material_stock s SET payload_json=s.payload_json || '{"paddon_pending_at_upgrade":true}'::jsonb
FROM mini_raw_material_paddon_units u JOIN mini_raw_material_paddons p ON p.id=u.paddon_id
JOIN mini_warehouses w ON w.id=p.warehouse_id
WHERE u.stock_id=s.id AND u.issue_id IS NULL AND lower(s.warehouse)=lower(w.name) AND s.qty>0
  AND s.status IN ('available','reserved')
  AND (s.status='reserved' OR s.reserved_order_id<>'' OR EXISTS (
       SELECT 1 FROM mini_raw_material_assignments a WHERE lower(a.barcode)=lower(s.barcode)))
  AND (coalesce(s.payload_json->>'inventory_transfer_id','')='' OR EXISTS (
       SELECT 1 FROM mini_inventory_transfers t WHERE t.id=s.payload_json->>'inventory_transfer_id'
       AND t.status IN ('requested','approved')))
  AND NOT EXISTS (SELECT 1 FROM mini_inventory_placements pl JOIN mini_inventory_locations loc ON loc.id=pl.physical_location_id
       WHERE pl.asset_kind='raw_material' AND pl.asset_ref=s.id AND loc.warehouse_id IS DISTINCT FROM w.id);

CREATE FUNCTION mini_restore_packed_stock() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.warehouse<>OLD.warehouse OR NEW.status NOT IN ('available','reserved') THEN
        NEW.payload_json := NEW.payload_json - 'paddon_pending_at_upgrade';
    ELSIF NEW.status='available' AND NEW.reserved_order_id='' AND coalesce(NEW.payload_json->>'inventory_transfer_id','')=''
       AND NOT EXISTS (SELECT 1 FROM mini_raw_material_assignments a WHERE lower(a.barcode)=lower(NEW.barcode))
       AND NOT EXISTS (SELECT 1 FROM mini_inventory_placements pl JOIN mini_inventory_locations loc ON loc.id=pl.physical_location_id
           LEFT JOIN mini_warehouses w ON w.id=loc.warehouse_id WHERE pl.asset_kind='raw_material' AND pl.asset_ref=NEW.id
           AND lower(w.name) IS DISTINCT FROM lower(NEW.warehouse))
    THEN
        NEW.status := 'on_paddon';
        NEW.payload_json := NEW.payload_json - 'paddon_pending_at_upgrade';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER mini_restore_packed_stock BEFORE UPDATE ON mini_raw_material_stock
    FOR EACH ROW WHEN (OLD.payload_json @> '{"paddon_pending_at_upgrade":true}'::jsonb)
    EXECUTE FUNCTION mini_restore_packed_stock();

CREATE FUNCTION mini_finish_pending_paddon_assignment() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    UPDATE mini_raw_material_stock SET updated_at=now() WHERE lower(barcode)=lower(OLD.barcode)
        AND status='available' AND payload_json @> '{"paddon_pending_at_upgrade":true}'::jsonb;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER mini_finish_pending_paddon_assignment AFTER DELETE ON mini_raw_material_assignments
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION mini_finish_pending_paddon_assignment();

ALTER TABLE mini_raw_material_paddon_issues ADD COLUMN print_tracked BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE mini_raw_material_paddon_issues ALTER COLUMN print_tracked SET DEFAULT true;
CREATE VIEW mini_raw_material_paddon_issue_history WITH (security_invoker=true) AS
SELECT i.*, i.response_json || jsonb_build_object(
    'print_tracking', true, 'destination_kind', coalesce(i.request_json #>> '{destination,kind}', 'warehouse'))
    || CASE WHEN i.print_tracked THEN '{}'::jsonb ELSE '{"legacy_print":true}'::jsonb END
    || coalesce((SELECT jsonb_build_object('print_states',jsonb_object_agg(l.barcode,
          jsonb_build_object('attempt',l.attempt,'printed',l.printed)))
         FROM mini_raw_material_paddon_label_prints l WHERE l.issue_id=i.id HAVING count(*)>0),'{}'::jsonb) AS result
FROM mini_raw_material_paddon_issues i;
GRANT SELECT ON mini_raw_material_paddon_issue_history TO mini_rs_erp;
