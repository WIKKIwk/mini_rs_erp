-- Rezka progress records carry the measured roll diameter in millimetres.
-- It is nullable for non-Rezka progress, but any recorded value must be
-- finite and strictly positive; request validation remains in the service.
ALTER TABLE mini_order_progress_events
    ADD COLUMN IF NOT EXISTS diameter NUMERIC;
ALTER TABLE mini_progress_batches
    ADD COLUMN IF NOT EXISTS diameter NUMERIC;

ALTER TABLE mini_order_progress_events
    DROP CONSTRAINT IF EXISTS mini_order_progress_events_diameter_positive;
ALTER TABLE mini_order_progress_events
    ADD CONSTRAINT mini_order_progress_events_diameter_positive
    CHECK (
        diameter IS NULL
        OR (
            diameter > 0
            AND diameter <> 'NaN'::numeric
            AND diameter <> 'Infinity'::numeric
            AND diameter <> '-Infinity'::numeric
        )
    );

ALTER TABLE mini_progress_batches
    DROP CONSTRAINT IF EXISTS mini_progress_batches_diameter_positive;
ALTER TABLE mini_progress_batches
    ADD CONSTRAINT mini_progress_batches_diameter_positive
    CHECK (
        diameter IS NULL
        OR (
            diameter > 0
            AND diameter <> 'NaN'::numeric
            AND diameter <> 'Infinity'::numeric
            AND diameter <> '-Infinity'::numeric
        )
    );
