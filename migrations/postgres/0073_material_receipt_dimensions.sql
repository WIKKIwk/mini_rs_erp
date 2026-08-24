ALTER TABLE mini_gscale_receipts
    ADD COLUMN width_mm NUMERIC(18,6),
    ADD COLUMN micron NUMERIC(18,6),
    ADD CONSTRAINT mini_gscale_receipts_dimensions_valid CHECK (
        (width_mm IS NULL AND micron IS NULL)
        OR (width_mm > 0 AND micron > 0)
    );

ALTER TABLE mini_raw_material_stock
    ADD COLUMN width_mm NUMERIC(18,6),
    ADD COLUMN micron NUMERIC(18,6),
    ADD CONSTRAINT mini_raw_material_stock_dimensions_valid CHECK (
        (width_mm IS NULL AND micron IS NULL)
        OR (width_mm > 0 AND micron > 0)
    );
