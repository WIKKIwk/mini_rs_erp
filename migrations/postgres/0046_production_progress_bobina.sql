-- Production progress and astatka reports also retain the bobina weight.
-- These columns remain nullable so existing records and older API clients stay
-- readable; current worker forms send a positive value.
ALTER TABLE mini_order_progress_events
    ADD COLUMN IF NOT EXISTS bobina_kg NUMERIC;
ALTER TABLE mini_progress_batches
    ADD COLUMN IF NOT EXISTS bobina_kg NUMERIC;

ALTER TABLE mini_laminatsiya_astatka_reports
    ADD COLUMN IF NOT EXISTS finished_goods_meter NUMERIC;
ALTER TABLE mini_laminatsiya_astatka_reports
    ADD COLUMN IF NOT EXISTS finished_goods_kg NUMERIC;
ALTER TABLE mini_laminatsiya_astatka_reports
    ADD COLUMN IF NOT EXISTS bobina_kg NUMERIC;

ALTER TABLE mini_rezka_astatka_reports
    ADD COLUMN IF NOT EXISTS finished_goods_meter NUMERIC;
ALTER TABLE mini_rezka_astatka_reports
    ADD COLUMN IF NOT EXISTS finished_goods_kg NUMERIC;
ALTER TABLE mini_rezka_astatka_reports
    ADD COLUMN IF NOT EXISTS bobina_kg NUMERIC;

ALTER TABLE mini_order_progress_events
    DROP CONSTRAINT IF EXISTS mini_order_progress_events_bobina_kg_positive;
ALTER TABLE mini_order_progress_events
    ADD CONSTRAINT mini_order_progress_events_bobina_kg_positive
    CHECK (
        bobina_kg IS NULL
        OR (
            bobina_kg > 0
            AND bobina_kg <> 'NaN'::numeric
            AND bobina_kg <> 'Infinity'::numeric
            AND bobina_kg <> '-Infinity'::numeric
        )
    );

ALTER TABLE mini_progress_batches
    DROP CONSTRAINT IF EXISTS mini_progress_batches_bobina_kg_positive;
ALTER TABLE mini_progress_batches
    ADD CONSTRAINT mini_progress_batches_bobina_kg_positive
    CHECK (
        bobina_kg IS NULL
        OR (
            bobina_kg > 0
            AND bobina_kg <> 'NaN'::numeric
            AND bobina_kg <> 'Infinity'::numeric
            AND bobina_kg <> '-Infinity'::numeric
        )
    );

ALTER TABLE mini_laminatsiya_astatka_reports
    DROP CONSTRAINT IF EXISTS mini_laminatsiya_astatka_reports_finished_goods_meter_positive;
ALTER TABLE mini_laminatsiya_astatka_reports
    ADD CONSTRAINT mini_laminatsiya_astatka_reports_finished_goods_meter_positive
    CHECK (
        finished_goods_meter IS NULL
        OR (
            finished_goods_meter > 0
            AND finished_goods_meter <> 'NaN'::numeric
            AND finished_goods_meter <> 'Infinity'::numeric
            AND finished_goods_meter <> '-Infinity'::numeric
        )
    );
ALTER TABLE mini_laminatsiya_astatka_reports
    DROP CONSTRAINT IF EXISTS mini_laminatsiya_astatka_reports_finished_goods_kg_positive;
ALTER TABLE mini_laminatsiya_astatka_reports
    ADD CONSTRAINT mini_laminatsiya_astatka_reports_finished_goods_kg_positive
    CHECK (
        finished_goods_kg IS NULL
        OR (
            finished_goods_kg > 0
            AND finished_goods_kg <> 'NaN'::numeric
            AND finished_goods_kg <> 'Infinity'::numeric
            AND finished_goods_kg <> '-Infinity'::numeric
        )
    );
ALTER TABLE mini_laminatsiya_astatka_reports
    DROP CONSTRAINT IF EXISTS mini_laminatsiya_astatka_reports_bobina_kg_positive;
ALTER TABLE mini_laminatsiya_astatka_reports
    ADD CONSTRAINT mini_laminatsiya_astatka_reports_bobina_kg_positive
    CHECK (
        bobina_kg IS NULL
        OR (
            bobina_kg > 0
            AND bobina_kg <> 'NaN'::numeric
            AND bobina_kg <> 'Infinity'::numeric
            AND bobina_kg <> '-Infinity'::numeric
        )
    );

ALTER TABLE mini_rezka_astatka_reports
    DROP CONSTRAINT IF EXISTS mini_rezka_astatka_reports_finished_goods_meter_positive;
ALTER TABLE mini_rezka_astatka_reports
    ADD CONSTRAINT mini_rezka_astatka_reports_finished_goods_meter_positive
    CHECK (
        finished_goods_meter IS NULL
        OR (
            finished_goods_meter > 0
            AND finished_goods_meter <> 'NaN'::numeric
            AND finished_goods_meter <> 'Infinity'::numeric
            AND finished_goods_meter <> '-Infinity'::numeric
        )
    );
ALTER TABLE mini_rezka_astatka_reports
    DROP CONSTRAINT IF EXISTS mini_rezka_astatka_reports_finished_goods_kg_positive;
ALTER TABLE mini_rezka_astatka_reports
    ADD CONSTRAINT mini_rezka_astatka_reports_finished_goods_kg_positive
    CHECK (
        finished_goods_kg IS NULL
        OR (
            finished_goods_kg > 0
            AND finished_goods_kg <> 'NaN'::numeric
            AND finished_goods_kg <> 'Infinity'::numeric
            AND finished_goods_kg <> '-Infinity'::numeric
        )
    );
ALTER TABLE mini_rezka_astatka_reports
    DROP CONSTRAINT IF EXISTS mini_rezka_astatka_reports_bobina_kg_positive;
ALTER TABLE mini_rezka_astatka_reports
    ADD CONSTRAINT mini_rezka_astatka_reports_bobina_kg_positive
    CHECK (
        bobina_kg IS NULL
        OR (
            bobina_kg > 0
            AND bobina_kg <> 'NaN'::numeric
            AND bobina_kg <> 'Infinity'::numeric
            AND bobina_kg <> '-Infinity'::numeric
        )
    );
