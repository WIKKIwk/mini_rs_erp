-- 0092 canonicalizes the Rezka merge lineage into the existing payload_json
-- authority and drops the typed mirror tables.
--
-- Runtime already reads lineage only from payload_json:
--   mini_order_run_sessions.payload_json.input_lineage
--   mini_order_run_sessions.payload_json.rezka_active_partial_rolls
--   mini_progress_batches.payload_json.source_input_links
-- The typed tables (created by 0085) were DELETE+INSERT mirrors of that same
-- payload state with no production reader. Backfill rows whose canonical array
-- is missing or empty from the mirrors (canonical payload always wins on
-- conflict), then drop the mirrors. Legacy scalar fields (input_progress_*,
-- parent_batch_id columns) are intentionally left untouched.

-- 1. Session input lineage: typed mirror rows -> payload input_lineage.
-- Canonical payload wins: rows that already carry a non-empty array keep it.
UPDATE mini_order_run_sessions AS s
SET payload_json = jsonb_set(s.payload_json, '{input_lineage}', agg.links, true)
FROM (
    SELECT
        session_id,
        jsonb_agg(
            jsonb_strip_nulls(jsonb_build_object(
                'input_batch_id', input_batch_id,
                'input_qr_payload', input_qr_payload,
                'source_apparatus', source_apparatus,
                'source_kind', source_kind,
                'stage_node_id', stage_node_id,
                'sequence_no', sequence_no,
                'status', status,
                'linked_at_unix', floor(extract(epoch FROM linked_at))::bigint,
                'processed_at_unix', floor(extract(epoch FROM processed_at))::bigint
            ))
            ORDER BY sequence_no
        ) AS links
    FROM mini_order_run_input_links
    GROUP BY session_id
) AS agg
WHERE s.session_id = agg.session_id
  AND (
      NOT (s.payload_json ? 'input_lineage')
      OR (s.payload_json -> 'input_lineage') IS NULL
      OR jsonb_typeof(s.payload_json -> 'input_lineage') <> 'array'
      OR jsonb_array_length(s.payload_json -> 'input_lineage') = 0
  );

-- 2. Session input lineage: legacy scalar fallback for rows with no mirror.
-- Mirrors the pre-cutover runtime fallback: one sequence-1 link, completed
-- sessions link as processed at updated_at, others as in_use at started_at.
-- An explicit scalar source kind wins when present (runtime precedence);
-- otherwise the kind comes from the table that unambiguously owns the parent.
-- Ambiguous or missing parents stay absent instead of being guessed.
UPDATE mini_order_run_sessions AS s
SET payload_json = jsonb_set(
    s.payload_json,
    '{input_lineage}',
    jsonb_build_array(
        jsonb_strip_nulls(jsonb_build_object(
            'input_batch_id', btrim(s.payload_json ->> 'input_progress_batch_id'),
            'input_qr_payload', COALESCE(s.payload_json ->> 'input_progress_qr_payload', ''),
            'source_apparatus', COALESCE(s.payload_json ->> 'input_progress_apparatus', ''),
            'source_kind', CASE
                WHEN btrim(COALESCE(s.payload_json ->> 'input_wip_source_kind', ''))
                    IN ('progress_batch', 'opening_wip')
                THEN btrim(s.payload_json ->> 'input_wip_source_kind')
                WHEN EXISTS (
                    SELECT 1 FROM mini_opening_wip_batches opening_batch
                    WHERE opening_batch.batch_id = btrim(s.payload_json ->> 'input_progress_batch_id')
                ) THEN 'opening_wip'
                ELSE 'progress_batch'
            END,
            'stage_node_id', COALESCE(s.stage_node_id, ''),
            'sequence_no', 1,
            'status', CASE WHEN s.status = 'completed' THEN 'processed' ELSE 'in_use' END,
            'linked_at_unix', floor(extract(epoch FROM s.started_at))::bigint,
            'processed_at_unix', CASE
                WHEN s.status = 'completed'
                THEN floor(extract(epoch FROM s.updated_at))::bigint
                ELSE NULL
            END
        ))
    ),
    true
)
WHERE (
      NOT (s.payload_json ? 'input_lineage')
      OR (s.payload_json -> 'input_lineage') IS NULL
      OR jsonb_typeof(s.payload_json -> 'input_lineage') <> 'array'
      OR jsonb_array_length(s.payload_json -> 'input_lineage') = 0
  )
  AND btrim(COALESCE(s.payload_json ->> 'input_progress_batch_id', '')) <> ''
  AND NOT EXISTS (
      SELECT 1 FROM mini_order_run_input_links mirror
      WHERE mirror.session_id = s.session_id
  )
  AND (
      EXISTS (
          SELECT 1 FROM mini_progress_batches progress_batch
          WHERE progress_batch.batch_id = btrim(s.payload_json ->> 'input_progress_batch_id')
      )
      <>
      EXISTS (
          SELECT 1 FROM mini_opening_wip_batches opening_batch
          WHERE opening_batch.batch_id = btrim(s.payload_json ->> 'input_progress_batch_id')
      )
  );

-- 3. Active Rezka partial rolls: typed mirror rows -> payload field.
UPDATE mini_order_run_sessions AS s
SET payload_json = jsonb_set(s.payload_json, '{rezka_active_partial_rolls}', agg.rolls, true)
FROM (
    SELECT
        session_id,
        jsonb_agg(
            jsonb_build_object(
                'slot_index', slot_index,
                'generation', generation,
                'contained_kadr_count', contained_kadr_count,
                'status', status,
                'source_input_batch_ids', to_jsonb(source_input_batch_ids),
                'started_at_unix', floor(extract(epoch FROM started_at))::bigint,
                'updated_at_unix', floor(extract(epoch FROM updated_at))::bigint
            )
            ORDER BY slot_index
        ) AS rolls
    FROM mini_rezka_active_partial_rolls
    GROUP BY session_id
) AS agg
WHERE s.session_id = agg.session_id
  AND (
      NOT (s.payload_json ? 'rezka_active_partial_rolls')
      OR (s.payload_json -> 'rezka_active_partial_rolls') IS NULL
      OR jsonb_typeof(s.payload_json -> 'rezka_active_partial_rolls') <> 'array'
      OR jsonb_array_length(s.payload_json -> 'rezka_active_partial_rolls') = 0
  );

-- 4. Output batch source lineage: typed mirror rows -> payload source_input_links.
UPDATE mini_progress_batches AS b
SET payload_json = jsonb_set(b.payload_json, '{source_input_links}', agg.links, true)
FROM (
    SELECT
        output_batch_id,
        jsonb_agg(
            jsonb_build_object(
                'input_batch_id', input_batch_id,
                'input_qr_payload', input_qr_payload,
                'source_apparatus', source_apparatus,
                'source_kind', source_kind,
                'sequence_no', sequence_no
            )
            ORDER BY sequence_no
        ) AS links
    FROM mini_progress_batch_input_links
    GROUP BY output_batch_id
) AS agg
WHERE b.batch_id = agg.output_batch_id
  AND (
      NOT (b.payload_json ? 'source_input_links')
      OR (b.payload_json -> 'source_input_links') IS NULL
      OR jsonb_typeof(b.payload_json -> 'source_input_links') <> 'array'
      OR jsonb_array_length(b.payload_json -> 'source_input_links') = 0
  );

-- 5. Output batch source lineage: legacy parent_batch_id column fallback.
-- Mirrors the pre-cutover runtime fallback exactly: one sequence-1 link with
-- empty QR/apparatus, source kind from the unambiguous owner table.
UPDATE mini_progress_batches AS b
SET payload_json = jsonb_set(
    b.payload_json,
    '{source_input_links}',
    jsonb_build_array(
        jsonb_build_object(
            'input_batch_id', btrim(b.parent_batch_id),
            'input_qr_payload', '',
            'source_apparatus', '',
            'source_kind', CASE
                WHEN EXISTS (
                    SELECT 1 FROM mini_opening_wip_batches opening_batch
                    WHERE opening_batch.batch_id = btrim(b.parent_batch_id)
                ) THEN 'opening_wip'
                ELSE 'progress_batch'
            END,
            'sequence_no', 1
        )
    ),
    true
)
WHERE (
      NOT (b.payload_json ? 'source_input_links')
      OR (b.payload_json -> 'source_input_links') IS NULL
      OR jsonb_typeof(b.payload_json -> 'source_input_links') <> 'array'
      OR jsonb_array_length(b.payload_json -> 'source_input_links') = 0
  )
  AND btrim(b.parent_batch_id) <> ''
  AND NOT EXISTS (
      SELECT 1 FROM mini_progress_batch_input_links mirror
      WHERE mirror.output_batch_id = b.batch_id
  )
  AND (
      EXISTS (
          SELECT 1 FROM mini_progress_batches input_progress_batch
          WHERE input_progress_batch.batch_id = btrim(b.parent_batch_id)
      )
      <>
      EXISTS (
          SELECT 1 FROM mini_opening_wip_batches opening_batch
          WHERE opening_batch.batch_id = btrim(b.parent_batch_id)
      )
  );

-- 6. Fail closed: every mirrored row must now be represented by a non-empty
-- canonical payload array. Anything else means history would be lost on DROP.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_order_run_input_links mirror
        JOIN mini_order_run_sessions s USING (session_id)
        WHERE NOT (
            (s.payload_json ? 'input_lineage')
            AND jsonb_typeof(s.payload_json -> 'input_lineage') = 'array'
            AND jsonb_array_length(s.payload_json -> 'input_lineage') > 0
        )
    ) THEN
        RAISE EXCEPTION '0092: mini_order_run_input_links rows lack canonical input_lineage';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_rezka_active_partial_rolls mirror
        JOIN mini_order_run_sessions s USING (session_id)
        WHERE NOT (
            (s.payload_json ? 'rezka_active_partial_rolls')
            AND jsonb_typeof(s.payload_json -> 'rezka_active_partial_rolls') = 'array'
            AND jsonb_array_length(s.payload_json -> 'rezka_active_partial_rolls') > 0
        )
    ) THEN
        RAISE EXCEPTION '0092: mini_rezka_active_partial_rolls rows lack canonical payload rolls';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_progress_batch_input_links mirror
        JOIN mini_progress_batches b ON b.batch_id = mirror.output_batch_id
        WHERE NOT (
            (b.payload_json ? 'source_input_links')
            AND jsonb_typeof(b.payload_json -> 'source_input_links') = 'array'
            AND jsonb_array_length(b.payload_json -> 'source_input_links') > 0
        )
    ) THEN
        RAISE EXCEPTION '0092: mini_progress_batch_input_links rows lack canonical source_input_links';
    END IF;
END
$$;

-- 7. Drop the mirrors. Payload_json is now the single lineage authority.
DROP TABLE IF EXISTS mini_progress_batch_input_links;
DROP TABLE IF EXISTS mini_rezka_active_partial_rolls;
DROP TABLE IF EXISTS mini_order_run_input_links;
