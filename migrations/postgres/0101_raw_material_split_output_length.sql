SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- Record length in meters for split outputs while preserving historical splits where length was not entered.
ALTER TABLE mini_raw_material_split_outputs
    ADD COLUMN length_m NUMERIC(18,6),
    ADD CONSTRAINT mini_raw_material_split_output_length CHECK (
        length_m IS NULL OR (length_m > 0 AND length_m <> 'NaN'::numeric)
    );

-- Also record length_m directly on raw material stock items
ALTER TABLE mini_raw_material_stock
    ADD COLUMN length_m NUMERIC(18,6),
    ADD CONSTRAINT mini_raw_material_stock_length CHECK (
        length_m IS NULL OR (length_m > 0 AND length_m <> 'NaN'::numeric)
    );
