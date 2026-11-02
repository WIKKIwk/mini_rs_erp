-- An opening roll's residual keeps its intake passport and scan contract.
ALTER TABLE mini_opening_wip_batches
    ADD COLUMN parent_batch_id TEXT REFERENCES mini_opening_wip_batches(batch_id);
CREATE INDEX mini_opening_wip_parent_batch ON mini_opening_wip_batches(parent_batch_id)
    WHERE parent_batch_id IS NOT NULL;

ALTER TABLE mini_layer_remainder_operations
    DROP CONSTRAINT mini_layer_remainder_operations_source_kind_check;
ALTER TABLE mini_layer_remainder_operations
    ADD CONSTRAINT mini_layer_remainder_operations_source_kind_check
    CHECK (source_kind IN ('progress_batch', 'opening_wip', 'raw_material'));

-- Returning a residual retires the parent QR, including recovery/correction paths.
CREATE FUNCTION mini_opening_wip_remainder_source_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM mini_layer_remainder_operations
               WHERE source_kind = 'opening_wip' AND source_id = OLD.batch_id)
       AND (NEW.wip_status <> 'processed'
            OR NEW.qr_payload IS DISTINCT FROM OLD.qr_payload
            OR NEW.quantity IS DISTINCT FROM OLD.quantity
            OR NEW.finished_goods_meter IS DISTINCT FROM OLD.finished_goods_meter
            OR NEW.finished_goods_kg IS DISTINCT FROM OLD.finished_goods_kg
            OR NEW.bobina_kg IS DISTINCT FROM OLD.bobina_kg
            OR NEW.intake_id IS DISTINCT FROM OLD.intake_id
            OR NEW.parent_batch_id IS DISTINCT FROM OLD.parent_batch_id) THEN
        RAISE EXCEPTION 'layer_remainder_source_used';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER mini_opening_wip_remainder_source_guard
    BEFORE UPDATE ON mini_opening_wip_batches
    FOR EACH ROW EXECUTE FUNCTION mini_opening_wip_remainder_source_guard();
