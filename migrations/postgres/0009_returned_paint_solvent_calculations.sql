WITH entered_values AS (
    SELECT
        request.id,
        lower(btrim(item.value ->> 'usage')) AS usage,
        lower(btrim(item.value ->> 'category')) AS category,
        lower(btrim(field.key)) AS field_name,
        (field.value #>> '{}')::NUMERIC AS amount
    FROM mini_returned_paint_requests AS request
    CROSS JOIN LATERAL jsonb_array_elements(request.items_json) AS item(value)
    CROSS JOIN LATERAL jsonb_each(item.value -> 'values') AS field(key, value)
    WHERE lower(btrim(item.value ->> 'category')) IN ('colors', 'solvents')
), totals AS (
    SELECT
        request.id,
        COALESCE(SUM(value.amount) FILTER (
            WHERE value.usage = 'rasxot'
              AND value.category = 'colors'
              AND value.field_name = 'mix'
        ), 0::NUMERIC) AS rasxot_mix_total,
        COALESCE(SUM(value.amount) FILTER (
            WHERE value.usage = 'astatka'
              AND value.category = 'colors'
              AND value.field_name = 'mix'
        ), 0::NUMERIC) AS astatka_mix_total,
        COALESCE(SUM(value.amount) FILTER (
            WHERE value.usage = 'rasxot'
              AND value.category = 'colors'
              AND value.field_name <> 'mix'
        ), 0::NUMERIC) AS rasxot_direct_paint,
        COALESCE(SUM(value.amount) FILTER (
            WHERE value.usage = 'astatka'
              AND value.category = 'colors'
              AND value.field_name <> 'mix'
        ), 0::NUMERIC) AS astatka_direct_paint,
        COALESCE(SUM(value.amount) FILTER (
            WHERE value.usage = 'rasxot'
              AND value.category = 'solvents'
        ), 0::NUMERIC) AS rasxot_direct_alcohol,
        COALESCE(SUM(value.amount) FILTER (
            WHERE value.usage = 'astatka'
              AND value.category = 'solvents'
        ), 0::NUMERIC) AS astatka_direct_alcohol
    FROM mini_returned_paint_requests AS request
    LEFT JOIN entered_values AS value ON value.id = request.id
    GROUP BY request.id
), calculations AS (
    SELECT
        id,
        ROUND(rasxot_mix_total, 12) AS rasxot_mix_total,
        ROUND(astatka_mix_total, 12) AS astatka_mix_total,
        ROUND(
            rasxot_direct_alcohol + (rasxot_mix_total * 0.30::NUMERIC),
            12
        ) AS rasxot_alcohol,
        ROUND(
            astatka_direct_alcohol + (astatka_mix_total * 0.30::NUMERIC),
            12
        ) AS astatka_alcohol,
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
  AND calculation.rasxot_alcohol
      BETWEEN 0::NUMERIC AND 999999999999999999::NUMERIC
  AND calculation.astatka_alcohol
      BETWEEN 0::NUMERIC AND 999999999999999999::NUMERIC
  AND calculation.rasxot_pure_paint
      BETWEEN 0::NUMERIC AND 999999999999999999::NUMERIC
  AND calculation.astatka_pure_paint
      BETWEEN 0::NUMERIC AND 999999999999999999::NUMERIC
  AND calculation.rasxot_alcohol >= calculation.astatka_alcohol
  AND calculation.rasxot_pure_paint >= calculation.astatka_pure_paint;
