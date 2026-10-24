-- All values and totals share this one snapshot/pipeline. batch_id is the
-- primary key; no session/event/paddon join can multiply physical outputs.
WITH eligible AS MATERIALIZED (
    SELECT b.*,
           (production_completed_at IS NOT NULL AND isfinite(production_completed_at)
            AND production_completed_at > to_timestamp(0)
            AND COALESCE(payload_json #>> '{production_timing_source,completion}', '') <> 'unknown') AS timed
    FROM mini_progress_batches b
    WHERE canonical_apparatus_id = ANY($4::text[])
      AND COALESCE(payload_json->>'origin', '') <> 'layer_remainder'
      AND order_id NOT LIKE 'training-%' AND order_id NOT LIKE 'demo-%'
      AND ((action = 'pause' AND status IN ('paused', 'resumed'))
        OR (action = 'detach_roll' AND status IN ('roll_detached', 'resumed'))
        OR (action IN ('roll_complete', 'complete') AND status = 'completed'))
      AND ((production_completed_at >= $1::text::timestamptz
            AND production_completed_at < $2::text::timestamptz
            AND production_completed_at < $3::text::timestamptz)
        OR production_completed_at IS NULL OR NOT isfinite(production_completed_at)
        OR production_completed_at <= to_timestamp(0)
        OR payload_json #>> '{production_timing_source,completion}' = 'unknown')
), correction_values AS (
    SELECT c.batch_id, c.new_revision, c.id,
      CASE WHEN jsonb_typeof(old_values->'finished_goods_kg') = 'number'
           THEN round((old_values->>'finished_goods_kg')::numeric, 6) END AS old_kg,
      CASE WHEN jsonb_typeof(new_values->'finished_goods_kg') = 'number'
           THEN round((new_values->>'finished_goods_kg')::numeric, 6) END AS new_kg
    FROM mini_progress_batch_corrections c JOIN eligible b USING (batch_id)
    WHERE b.timed AND c.new_revision > 0 AND c.new_revision <= b.revision
), kg_changes AS (
    -- Same kg-only audit rule as paddon/weights.rs: a later description, meter
    -- or tare correction must not replace independently measured gross.
    SELECT DISTINCT ON (batch_id) batch_id,
           CASE WHEN new_kg >= 0 AND new_kg < 1000000000000 THEN new_kg END AS gross
    FROM correction_values
    WHERE (CASE WHEN old_kg >= 0 AND old_kg < 1000000000000 THEN old_kg END)
      IS DISTINCT FROM (CASE WHEN new_kg >= 0 AND new_kg < 1000000000000 THEN new_kg END)
    ORDER BY batch_id, new_revision DESC, id DESC
), raw_quantities AS (
    SELECT b.*,
      CASE WHEN finished_goods_meter IS NOT NULL THEN finished_goods_meter
           WHEN lower(btrim(uom)) IN ('m', 'meter', 'metr') THEN produced_qty END AS raw_meters,
      CASE WHEN c.batch_id IS NOT NULL THEN c.gross
           WHEN b.payload_json->'gross_qty' IS NOT NULL AND b.payload_json->'gross_qty' <> 'null'::jsonb
             THEN CASE WHEN jsonb_typeof(b.payload_json->'gross_qty') = 'number'
                       THEN round((b.payload_json->>'gross_qty')::numeric, 6) END
           ELSE finished_goods_kg END AS gross
    FROM eligible b LEFT JOIN kg_changes c USING (batch_id)
    WHERE b.timed
), quantities AS (
    SELECT *,
      CASE WHEN raw_meters >= 0 AND raw_meters < 1000000000000
           THEN raw_meters END AS meters,
      -- Gross zero is an unknown measurement in the existing ERP contract.
      -- A *net* zero (gross == explicitly known tare) is a real known zero.
      CASE WHEN gross > 0 AND gross < 1000000000000
                 AND bobina_kg >= 0 AND bobina_kg <= gross
           THEN gross - bobina_kg END AS net_kg
    FROM raw_quantities
), buckets AS (
    SELECT (ordinality - 1)::integer AS ordinal,
           value->>'start' AS start, value->>'end' AS "end",
           (value->>'is_partial')::boolean AS is_partial,
           (value->>'start_utc')::timestamptz AS lo,
           (value->>'end_utc')::timestamptz AS hi
    FROM jsonb_array_elements($5::jsonb) WITH ORDINALITY
), aggregated AS (
    SELECT q.canonical_apparatus_id AS apparatus_id, b.ordinal,
           count(*) AS batch_count,
           COALESCE(sum(q.meters), 0) AS meters,
           count(*) FILTER (WHERE q.meters IS NULL) AS meters_missing,
           COALESCE(sum(q.net_kg), 0) AS net_kg,
           count(*) FILTER (WHERE q.net_kg IS NULL) AS kg_missing
    FROM quantities q JOIN buckets b ON q.production_completed_at >= b.lo AND q.production_completed_at < b.hi
    GROUP BY q.canonical_apparatus_id, b.ordinal
), selected AS (
    SELECT value->>'id' AS id, value->>'name' AS name FROM jsonb_array_elements($6::jsonb)
), points AS (
    SELECT s.id, b.*,
           COALESCE(a.batch_count, 0) AS batch_count,
           COALESCE(a.meters, 0) AS meters, COALESCE(a.meters_missing, 0) AS meters_missing,
           COALESCE(a.net_kg, 0) AS net_kg, COALESCE(a.kg_missing, 0) AS kg_missing
    FROM selected s CROSS JOIN buckets b
    LEFT JOIN aggregated a ON a.apparatus_id = s.id AND a.ordinal = b.ordinal
), series AS (
    SELECT s.id,
      jsonb_build_object('apparatus_id', s.id, 'apparatus_name', s.name,
        'totals', jsonb_build_object(
          'batch_count', COALESCE(sum(p.batch_count), 0),
          'meters', jsonb_build_object('value', CASE WHEN COALESCE(sum(p.meters_missing), 0) = 0 THEN COALESCE(sum(p.meters), 0) END,
            'known_value', COALESCE(sum(p.meters), 0), 'missing_count', COALESCE(sum(p.meters_missing), 0)),
          'net_kg', jsonb_build_object('value', CASE WHEN COALESCE(sum(p.kg_missing), 0) = 0 THEN COALESCE(sum(p.net_kg), 0) END,
            'known_value', COALESCE(sum(p.net_kg), 0), 'missing_count', COALESCE(sum(p.kg_missing), 0))),
        'points', COALESCE(jsonb_agg(jsonb_build_object(
          'start', p.start, 'end', p."end", 'is_partial', p.is_partial, 'batch_count', p.batch_count,
          'meters', jsonb_build_object('value', CASE WHEN p.meters_missing = 0 THEN p.meters END,
            'known_value', p.meters, 'missing_count', p.meters_missing),
          'net_kg', jsonb_build_object('value', CASE WHEN p.kg_missing = 0 THEN p.net_kg END,
            'known_value', p.net_kg, 'missing_count', p.kg_missing)) ORDER BY p.ordinal)
          FILTER (WHERE p.ordinal IS NOT NULL), '[]'::jsonb)) AS value
    FROM selected s LEFT JOIN points p ON p.id = s.id GROUP BY s.id, s.name
), coverage AS (
    SELECT count(*) FILTER (WHERE NOT timed) AS unknown_count,
           count(*) FILTER (WHERE timed AND COALESCE(payload_json #>> '{production_timing_source,completion}', '') NOT IN ('', 'output_event')) AS approximate_count
    FROM eligible
)
SELECT ($7::jsonb || jsonb_build_object(
    'coverage', jsonb_build_object('complete', c.unknown_count = 0,
      'approximate_timestamp_count', c.approximate_count,
      'excluded_unknown_timestamp_count', c.unknown_count,
      'warnings', '[]'::jsonb
        || CASE WHEN c.approximate_count > 0 THEN jsonb_build_array(jsonb_build_object(
             'code', 'approximate_production_timestamps', 'count', c.approximate_count)) ELSE '[]'::jsonb END
        || CASE WHEN c.unknown_count > 0 THEN jsonb_build_array(jsonb_build_object(
             'code', 'excluded_unknown_production_timestamps', 'count', c.unknown_count)) ELSE '[]'::jsonb END),
    'series', COALESCE((SELECT jsonb_agg(value ORDER BY id) FROM series), '[]'::jsonb)))::text
FROM coverage c
