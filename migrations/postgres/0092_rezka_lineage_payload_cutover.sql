-- 0092 canonicalizes the Rezka merge lineage into the existing payload_json
-- authority and drops the typed mirror tables.
--
-- Runtime already reads lineage only from payload_json:
--   mini_order_run_sessions.payload_json.input_lineage
--   mini_order_run_sessions.payload_json.rezka_active_partial_rolls
--   mini_progress_batches.payload_json.source_input_links
-- The typed tables (created by 0085) were DELETE+INSERT mirrors of that same
-- payload state with no production reader. Historical rows whose canonical
-- array is missing or fails the current Rust parser shape are backfilled from
-- the mirrors (canonical payload always wins when valid), then the mirrors are
-- dropped. Legacy scalar fields (input_progress_*, parent_batch_id columns)
-- are intentionally left untouched.
--
-- Precedence per row: valid canonical payload is kept; malformed canonical
-- payload is replaced from valid mirror rows; missing payload is backfilled;
-- unambiguous legacy scalars synthesize exactly the former runtime fallback;
-- anything else aborts the migration instead of losing history.

-- One-time migration safety helpers (dropped at the end of this file).
-- They mirror the current Rust parser invariants for OrderRunInputLink,
-- ProgressBatchInputLink, RezkaActivePartialRoll and
-- rezka_merge_state_is_consistent. They are NOT runtime validators.

CREATE FUNCTION rezka_0092_is_integral_in_range(
    value jsonb,
    min_value numeric,
    max_value numeric
) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE AS $$
BEGIN
    IF jsonb_typeof(value) IS DISTINCT FROM 'number' THEN
        RETURN FALSE;
    END IF;
    IF (value ->> 0)::numeric <> trunc((value ->> 0)::numeric) THEN
        RETURN FALSE;
    END IF;
    IF (value ->> 0)::numeric < min_value OR (value ->> 0)::numeric > max_value THEN
        RETURN FALSE;
    END IF;
    RETURN TRUE;
END;
$$;

CREATE FUNCTION rezka_0092_valid_input_lineage(payload jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE AS $$
DECLARE
    arr jsonb;
    el jsonb;
    batch_ids text[] := '{}';
    seqs numeric[] := '{}';
    active_count int := 0;
    v_batch text;
    v_seq numeric;
    v_status text;
    v_proc jsonb;
BEGIN
    IF payload IS NULL OR NOT (payload ? 'input_lineage') THEN
        RETURN FALSE;
    END IF;
    arr := payload -> 'input_lineage';
    IF jsonb_typeof(arr) IS DISTINCT FROM 'array' OR jsonb_array_length(arr) = 0 THEN
        RETURN FALSE;
    END IF;
    FOR el IN SELECT * FROM jsonb_array_elements(arr) LOOP
        IF jsonb_typeof(el) IS DISTINCT FROM 'object' THEN
            RETURN FALSE;
        END IF;
        v_batch := btrim(COALESCE(el ->> 'input_batch_id', ''));
        IF v_batch = '' OR v_batch = ANY (batch_ids) THEN
            RETURN FALSE;
        END IF;
        batch_ids := batch_ids || v_batch;
        IF (el ->> 'source_kind') NOT IN ('progress_batch', 'opening_wip') THEN
            RETURN FALSE;
        END IF;
        v_status := el ->> 'status';
        IF v_status NOT IN ('in_use', 'processed') THEN
            RETURN FALSE;
        END IF;
        IF NOT rezka_0092_is_integral_in_range(el -> 'sequence_no', 1, 4294967295) THEN
            RETURN FALSE;
        END IF;
        v_seq := (el -> 'sequence_no' ->> 0)::numeric;
        IF v_seq = ANY (seqs) THEN
            RETURN FALSE;
        END IF;
        seqs := seqs || v_seq;
        IF jsonb_typeof(el -> 'input_qr_payload') IS DISTINCT FROM 'string'
            OR jsonb_typeof(el -> 'source_apparatus') IS DISTINCT FROM 'string'
            OR jsonb_typeof(el -> 'stage_node_id') IS DISTINCT FROM 'string'
            OR NOT rezka_0092_is_integral_in_range(
                el -> 'linked_at_unix', -9223372036854775808, 9223372036854775807
            )
        THEN
            RETURN FALSE;
        END IF;
        v_proc := el -> 'processed_at_unix';
        IF v_status = 'in_use' THEN
            IF v_proc IS NOT NULL AND v_proc <> 'null'::jsonb THEN
                RETURN FALSE;
            END IF;
        ELSE
            IF NOT rezka_0092_is_integral_in_range(
                v_proc, -9223372036854775808, 9223372036854775807
            ) THEN
                RETURN FALSE;
            END IF;
        END IF;
        IF v_status = 'in_use' THEN
            active_count := active_count + 1;
        END IF;
    END LOOP;
    IF active_count > 1 THEN
        RETURN FALSE;
    END IF;
    RETURN TRUE;
END;
$$;

CREATE FUNCTION rezka_0092_valid_output_links(payload jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE AS $$
DECLARE
    arr jsonb;
    el jsonb;
    batch_ids text[] := '{}';
    seqs numeric[] := '{}';
    v_batch text;
    v_seq numeric;
BEGIN
    IF payload IS NULL OR NOT (payload ? 'source_input_links') THEN
        RETURN FALSE;
    END IF;
    arr := payload -> 'source_input_links';
    IF jsonb_typeof(arr) IS DISTINCT FROM 'array' OR jsonb_array_length(arr) = 0 THEN
        RETURN FALSE;
    END IF;
    FOR el IN SELECT * FROM jsonb_array_elements(arr) LOOP
        IF jsonb_typeof(el) IS DISTINCT FROM 'object' THEN
            RETURN FALSE;
        END IF;
        v_batch := btrim(COALESCE(el ->> 'input_batch_id', ''));
        IF v_batch = '' OR v_batch = ANY (batch_ids) THEN
            RETURN FALSE;
        END IF;
        batch_ids := batch_ids || v_batch;
        IF (el ->> 'source_kind') NOT IN ('progress_batch', 'opening_wip') THEN
            RETURN FALSE;
        END IF;
        IF NOT rezka_0092_is_integral_in_range(el -> 'sequence_no', 1, 4294967295) THEN
            RETURN FALSE;
        END IF;
        v_seq := (el -> 'sequence_no' ->> 0)::numeric;
        IF v_seq = ANY (seqs) THEN
            RETURN FALSE;
        END IF;
        seqs := seqs || v_seq;
        IF jsonb_typeof(el -> 'input_qr_payload') IS DISTINCT FROM 'string'
            OR jsonb_typeof(el -> 'source_apparatus') IS DISTINCT FROM 'string'
        THEN
            RETURN FALSE;
        END IF;
    END LOOP;
    RETURN TRUE;
END;
$$;

CREATE FUNCTION rezka_0092_valid_rolls(payload jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE AS $$
DECLARE
    arr jsonb;
    el jsonb;
    slots numeric[] := '{}';
    v_slot numeric;
    srcs jsonb;
    src_el jsonb;
    seen text[] := '{}';
    v_src text;
BEGIN
    IF payload IS NULL OR NOT (payload ? 'rezka_active_partial_rolls') THEN
        RETURN FALSE;
    END IF;
    arr := payload -> 'rezka_active_partial_rolls';
    IF jsonb_typeof(arr) IS DISTINCT FROM 'array' OR jsonb_array_length(arr) = 0 THEN
        RETURN FALSE;
    END IF;
    FOR el IN SELECT * FROM jsonb_array_elements(arr) LOOP
        IF jsonb_typeof(el) IS DISTINCT FROM 'object' THEN
            RETURN FALSE;
        END IF;
        IF NOT rezka_0092_is_integral_in_range(el -> 'slot_index', 1, 4294967295) THEN
            RETURN FALSE;
        END IF;
        v_slot := (el -> 'slot_index' ->> 0)::numeric;
        IF v_slot = ANY (slots) THEN
            RETURN FALSE;
        END IF;
        slots := slots || v_slot;
        IF NOT rezka_0092_is_integral_in_range(el -> 'generation', 1, 4294967295)
            OR NOT rezka_0092_is_integral_in_range(el -> 'contained_kadr_count', 1, 4294967295)
            OR (el ->> 'status') IS DISTINCT FROM 'active'
            OR NOT rezka_0092_is_integral_in_range(
                el -> 'started_at_unix', -9223372036854775808, 9223372036854775807
            )
            OR NOT rezka_0092_is_integral_in_range(
                el -> 'updated_at_unix', -9223372036854775808, 9223372036854775807
            )
        THEN
            RETURN FALSE;
        END IF;
        srcs := el -> 'source_input_batch_ids';
        IF srcs IS NULL THEN
            CONTINUE;
        END IF;
        IF jsonb_typeof(srcs) IS DISTINCT FROM 'array' THEN
            RETURN FALSE;
        END IF;
        seen := '{}';
        FOR src_el IN SELECT * FROM jsonb_array_elements(srcs) LOOP
            IF jsonb_typeof(src_el) IS DISTINCT FROM 'string' THEN
                RETURN FALSE;
            END IF;
            v_src := btrim(src_el #>> '{}');
            IF v_src = '' OR v_src = ANY (seen) THEN
                RETURN FALSE;
            END IF;
            seen := seen || v_src;
        END LOOP;
    END LOOP;
    RETURN TRUE;
END;
$$;

CREATE FUNCTION rezka_0092_lineage_consistent(payload jsonb) RETURNS boolean
LANGUAGE plpgsql IMMUTABLE AS $$
DECLARE
    lin jsonb;
    rls jsonb;
    el jsonb;
    roll jsonb;
    ids text[] := '{}';
    active text := NULL;
    src_el jsonb;
    src text;
    found_active boolean;
BEGIN
    IF payload IS NULL
        OR jsonb_typeof(payload -> 'input_lineage') IS DISTINCT FROM 'array'
    THEN
        RETURN FALSE;
    END IF;
    lin := payload -> 'input_lineage';
    FOR el IN SELECT * FROM jsonb_array_elements(lin) LOOP
        IF jsonb_typeof(el) IS DISTINCT FROM 'object' THEN
            RETURN FALSE;
        END IF;
        ids := ids || btrim(COALESCE(el ->> 'input_batch_id', ''));
        IF (el ->> 'status') = 'in_use' THEN
            IF active IS NOT NULL THEN
                RETURN FALSE;
            END IF;
            active := btrim(COALESCE(el ->> 'input_batch_id', ''));
        END IF;
    END LOOP;
    IF NOT (payload ? 'rezka_active_partial_rolls')
        OR (payload -> 'rezka_active_partial_rolls') IS NULL
    THEN
        RETURN TRUE;
    END IF;
    rls := payload -> 'rezka_active_partial_rolls';
    IF jsonb_typeof(rls) IS DISTINCT FROM 'array' THEN
        RETURN FALSE;
    END IF;
    FOR roll IN SELECT * FROM jsonb_array_elements(rls) LOOP
        IF jsonb_typeof(roll) IS DISTINCT FROM 'object' THEN
            RETURN FALSE;
        END IF;
        IF (roll -> 'source_input_batch_ids') IS NULL THEN
            IF active IS NOT NULL THEN
                RETURN FALSE;
            END IF;
            CONTINUE;
        END IF;
        IF jsonb_typeof(roll -> 'source_input_batch_ids') IS DISTINCT FROM 'array' THEN
            RETURN FALSE;
        END IF;
        IF active IS NULL THEN
            IF (SELECT count(*) FROM jsonb_array_elements(roll -> 'source_input_batch_ids')) > 0
            THEN
                RETURN FALSE;
            END IF;
        ELSE
            found_active := FALSE;
            FOR src_el IN SELECT * FROM jsonb_array_elements(roll -> 'source_input_batch_ids') LOOP
                IF jsonb_typeof(src_el) IS DISTINCT FROM 'string' THEN
                    RETURN FALSE;
                END IF;
                src := btrim(src_el #>> '{}');
                IF NOT (src = ANY (ids)) THEN
                    RETURN FALSE;
                END IF;
                IF src = active THEN
                    found_active := TRUE;
                END IF;
            END LOOP;
            IF NOT found_active THEN
                RETURN FALSE;
            END IF;
        END IF;
    END LOOP;
    RETURN TRUE;
END;
$$;

-- 1. Session input lineage: typed mirror rows -> payload input_lineage.
-- Canonical payload wins: rows that already carry valid lineage keep it.
-- Malformed canonical payload is replaced from the valid mirror.
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
  AND NOT rezka_0092_valid_input_lineage(s.payload_json);

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
WHERE NOT rezka_0092_valid_input_lineage(s.payload_json)
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
  AND NOT rezka_0092_valid_rolls(s.payload_json);

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
  AND NOT rezka_0092_valid_output_links(b.payload_json);

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
WHERE NOT rezka_0092_valid_output_links(b.payload_json)
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

-- 6. Fail closed: every mirrored row must now be represented by valid canonical
-- payload, and lineage+rolls must satisfy the merge consistency invariant.
-- Anything else means history would be lost or corrupted on DROP.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_order_run_input_links mirror
        JOIN mini_order_run_sessions s USING (session_id)
        WHERE NOT rezka_0092_valid_input_lineage(s.payload_json)
    ) THEN
        RAISE EXCEPTION '0092: mini_order_run_input_links rows lack valid canonical input_lineage';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_rezka_active_partial_rolls mirror
        JOIN mini_order_run_sessions s USING (session_id)
        WHERE NOT rezka_0092_valid_rolls(s.payload_json)
    ) THEN
        RAISE EXCEPTION '0092: mini_rezka_active_partial_rolls rows lack valid canonical payload rolls';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_progress_batch_input_links mirror
        JOIN mini_progress_batches b ON b.batch_id = mirror.output_batch_id
        WHERE NOT rezka_0092_valid_output_links(b.payload_json)
    ) THEN
        RAISE EXCEPTION '0092: mini_progress_batch_input_links rows lack valid canonical source_input_links';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_order_run_sessions s
        WHERE (
            EXISTS (
                SELECT 1 FROM mini_order_run_input_links mirror
                WHERE mirror.session_id = s.session_id
            )
            OR EXISTS (
                SELECT 1 FROM mini_rezka_active_partial_rolls mirror
                WHERE mirror.session_id = s.session_id
            )
        )
        AND NOT rezka_0092_lineage_consistent(s.payload_json)
    ) THEN
        RAISE EXCEPTION '0092: session lineage violates the merge consistency invariant';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_order_run_sessions s
        WHERE (s.payload_json ? 'input_lineage')
        AND NOT rezka_0092_valid_input_lineage(s.payload_json)
    ) THEN
        RAISE EXCEPTION '0092: unrecoverable input_lineage payload would be left malformed';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_order_run_sessions s
        WHERE (s.payload_json ? 'rezka_active_partial_rolls')
        AND NOT rezka_0092_valid_rolls(s.payload_json)
    ) THEN
        RAISE EXCEPTION '0092: unrecoverable rezka_active_partial_rolls payload would be left malformed';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_progress_batches b
        WHERE (b.payload_json ? 'source_input_links')
        AND NOT rezka_0092_valid_output_links(b.payload_json)
    ) THEN
        RAISE EXCEPTION '0092: unrecoverable source_input_links payload would be left malformed';
    END IF;
END
$$;

-- 7. Drop the mirrors. Payload_json is now the single lineage authority.
DROP TABLE IF EXISTS mini_progress_batch_input_links;
DROP TABLE IF EXISTS mini_rezka_active_partial_rolls;
DROP TABLE IF EXISTS mini_order_run_input_links;

-- 8. Drop the one-time migration helpers. No lasting footprint.
DROP FUNCTION IF EXISTS rezka_0092_lineage_consistent(jsonb);
DROP FUNCTION IF EXISTS rezka_0092_valid_rolls(jsonb);
DROP FUNCTION IF EXISTS rezka_0092_valid_output_links(jsonb);
DROP FUNCTION IF EXISTS rezka_0092_valid_input_lineage(jsonb);
DROP FUNCTION IF EXISTS rezka_0092_is_integral_in_range(jsonb, numeric, numeric);
