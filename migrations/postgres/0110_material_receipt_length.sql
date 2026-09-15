SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- Preserve the roll length on receipt drafts/submissions so the printed label
-- and the raw-material stock row share one authoritative value.
ALTER TABLE mini_gscale_receipts
    ADD COLUMN length_m NUMERIC(18,6),
    ADD CONSTRAINT mini_gscale_receipts_length CHECK (
        length_m IS NULL OR (length_m > 0 AND length_m <> 'NaN'::numeric)
    );
