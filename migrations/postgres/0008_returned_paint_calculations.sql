ALTER TABLE mini_returned_paint_requests
    ADD COLUMN IF NOT EXISTS rasxot_mix_total NUMERIC(30, 12),
    ADD COLUMN IF NOT EXISTS astatka_mix_total NUMERIC(30, 12),
    ADD COLUMN IF NOT EXISTS rasxot_alcohol NUMERIC(30, 12),
    ADD COLUMN IF NOT EXISTS astatka_alcohol NUMERIC(30, 12),
    ADD COLUMN IF NOT EXISTS final_used_alcohol NUMERIC(30, 12),
    ADD COLUMN IF NOT EXISTS rasxot_pure_paint NUMERIC(30, 12),
    ADD COLUMN IF NOT EXISTS astatka_pure_paint NUMERIC(30, 12),
    ADD COLUMN IF NOT EXISTS final_used_paint NUMERIC(30, 12);

WITH color_values AS (
    SELECT
        request.id,
        lower(btrim(item.value ->> 'usage')) AS usage,
        lower(btrim(field.key)) AS field_name,
        (field.value #>> '{}')::NUMERIC AS amount
    FROM mini_returned_paint_requests AS request
    CROSS JOIN LATERAL jsonb_array_elements(request.items_json) AS item(value)
    CROSS JOIN LATERAL jsonb_each(item.value -> 'values') AS field(key, value)
    WHERE lower(btrim(item.value ->> 'category')) = 'colors'
), totals AS (
    SELECT
        request.id,
        COALESCE(SUM(value.amount) FILTER (
            WHERE value.usage = 'rasxot' AND value.field_name = 'mix'
        ), 0::NUMERIC) AS rasxot_mix_total,
        COALESCE(SUM(value.amount) FILTER (
            WHERE value.usage = 'astatka' AND value.field_name = 'mix'
        ), 0::NUMERIC) AS astatka_mix_total,
        COALESCE(SUM(value.amount) FILTER (
            WHERE value.usage = 'rasxot' AND value.field_name <> 'mix'
        ), 0::NUMERIC) AS rasxot_direct_paint,
        COALESCE(SUM(value.amount) FILTER (
            WHERE value.usage = 'astatka' AND value.field_name <> 'mix'
        ), 0::NUMERIC) AS astatka_direct_paint
    FROM mini_returned_paint_requests AS request
    LEFT JOIN color_values AS value ON value.id = request.id
    GROUP BY request.id
), calculations AS (
    SELECT
        id,
        ROUND(rasxot_mix_total, 12) AS rasxot_mix_total,
        ROUND(astatka_mix_total, 12) AS astatka_mix_total,
        ROUND(rasxot_mix_total * 0.30::NUMERIC, 12) AS rasxot_alcohol,
        ROUND(astatka_mix_total * 0.30::NUMERIC, 12) AS astatka_alcohol,
        ROUND(
            rasxot_direct_paint + (rasxot_mix_total * 0.70::NUMERIC),
            12
        ) AS rasxot_pure_paint,
        ROUND(
            astatka_direct_paint + (astatka_mix_total * 0.70::NUMERIC),
            12
        ) AS astatka_pure_paint
    FROM totals
)
UPDATE mini_returned_paint_requests AS request
SET
    rasxot_mix_total = calculation.rasxot_mix_total,
    astatka_mix_total = calculation.astatka_mix_total,
    rasxot_alcohol = calculation.rasxot_alcohol,
    astatka_alcohol = calculation.astatka_alcohol,
    final_used_alcohol = calculation.rasxot_alcohol - calculation.astatka_alcohol,
    rasxot_pure_paint = calculation.rasxot_pure_paint,
    astatka_pure_paint = calculation.astatka_pure_paint,
    final_used_paint = calculation.rasxot_pure_paint - calculation.astatka_pure_paint
FROM calculations AS calculation
WHERE request.id = calculation.id
  AND calculation.rasxot_mix_total
      BETWEEN 0::NUMERIC AND 999999999999999999::NUMERIC
  AND calculation.astatka_mix_total
      BETWEEN 0::NUMERIC AND 999999999999999999::NUMERIC
  AND calculation.rasxot_pure_paint
      BETWEEN 0::NUMERIC AND 999999999999999999::NUMERIC
  AND calculation.astatka_pure_paint
      BETWEEN 0::NUMERIC AND 999999999999999999::NUMERIC
  AND calculation.rasxot_alcohol >= calculation.astatka_alcohol
  AND calculation.rasxot_pure_paint >= calculation.astatka_pure_paint;

ALTER TABLE mini_returned_paint_requests
    DROP CONSTRAINT IF EXISTS mini_returned_paint_calculation_all_or_none,
    ADD CONSTRAINT mini_returned_paint_calculation_all_or_none CHECK (
        (rasxot_mix_total IS NULL
            AND astatka_mix_total IS NULL
            AND rasxot_alcohol IS NULL
            AND astatka_alcohol IS NULL
            AND final_used_alcohol IS NULL
            AND rasxot_pure_paint IS NULL
            AND astatka_pure_paint IS NULL
            AND final_used_paint IS NULL)
        OR
        (rasxot_mix_total IS NOT NULL
            AND astatka_mix_total IS NOT NULL
            AND rasxot_alcohol IS NOT NULL
            AND astatka_alcohol IS NOT NULL
            AND final_used_alcohol IS NOT NULL
            AND rasxot_pure_paint IS NOT NULL
            AND astatka_pure_paint IS NOT NULL
            AND final_used_paint IS NOT NULL)
    ),
    DROP CONSTRAINT IF EXISTS mini_returned_paint_calculation_non_negative,
    ADD CONSTRAINT mini_returned_paint_calculation_non_negative CHECK (
        rasxot_mix_total >= 0
        AND astatka_mix_total >= 0
        AND rasxot_alcohol >= 0
        AND astatka_alcohol >= 0
        AND final_used_alcohol >= 0
        AND rasxot_pure_paint >= 0
        AND astatka_pure_paint >= 0
        AND final_used_paint >= 0
    ),
    DROP CONSTRAINT IF EXISTS mini_returned_paint_calculation_consistent,
    ADD CONSTRAINT mini_returned_paint_calculation_consistent CHECK (
        final_used_alcohol = rasxot_alcohol - astatka_alcohol
        AND final_used_paint = rasxot_pure_paint - astatka_pure_paint
    );
