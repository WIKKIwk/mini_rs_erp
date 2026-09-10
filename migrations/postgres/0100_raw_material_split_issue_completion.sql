SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- Measured waste is unchanged; a saved issue accounts for the signed difference.
ALTER TABLE mini_raw_material_splits
    ADD COLUMN issue_id TEXT UNIQUE REFERENCES mini_raw_material_split_issues(id) ON DELETE RESTRICT,
    ADD COLUMN difference_kg NUMERIC(22,6) NOT NULL DEFAULT 0,
    DROP CONSTRAINT mini_raw_material_splits_check,
    ADD CONSTRAINT mini_raw_material_splits_accounted_balance CHECK (
        difference_kg <> 'NaN'::numeric
        AND source_kg = output_kg + waste_kg + difference_kg
        AND (issue_id IS NOT NULL OR difference_kg = 0)
    );

-- Bind the exception to this exact roll, actor and measurements at the DB boundary.
CREATE FUNCTION mini_raw_material_split_issue_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.issue_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM mini_raw_material_split_issues i
        WHERE i.id = NEW.issue_id AND i.owner_ref = NEW.owner_ref
            AND i.parent_stock_id = NEW.parent_stock_id
            AND i.source_kg = NEW.source_kg AND i.output_kg = NEW.output_kg
            AND i.waste_kg = NEW.waste_kg AND i.difference_kg = NEW.difference_kg
    ) THEN
        RAISE EXCEPTION 'raw_material_split_issue_mismatch';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER mini_raw_material_split_issue_guard
    BEFORE INSERT ON mini_raw_material_splits
    FOR EACH ROW EXECUTE FUNCTION mini_raw_material_split_issue_guard();
