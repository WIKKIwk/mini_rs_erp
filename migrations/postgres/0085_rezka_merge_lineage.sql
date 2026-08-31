-- A Rezka output roll may span multiple upstream WIPs. Keep the splice order,
-- the still-mounted partial rolls, and every completed output's source set as
-- normalized audit data. Quantity contribution is intentionally not stored at
-- the merge boundary because that value is not measured by the worker flow.

CREATE TABLE IF NOT EXISTS mini_order_run_input_links (
    session_id TEXT NOT NULL REFERENCES mini_order_run_sessions(session_id) ON DELETE CASCADE,
    order_id TEXT NOT NULL,
    target_apparatus TEXT NOT NULL,
    input_batch_id TEXT NOT NULL,
    input_qr_payload TEXT NOT NULL DEFAULT '',
    source_apparatus TEXT NOT NULL DEFAULT '',
    source_kind TEXT NOT NULL,
    stage_node_id TEXT NOT NULL DEFAULT '',
    sequence_no INTEGER NOT NULL,
    status TEXT NOT NULL,
    linked_at TIMESTAMPTZ NOT NULL,
    processed_at TIMESTAMPTZ,
    PRIMARY KEY (session_id, input_batch_id),
    CONSTRAINT mini_order_run_input_links_order_not_blank
        CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_order_run_input_links_target_not_blank
        CHECK (btrim(target_apparatus) <> ''),
    CONSTRAINT mini_order_run_input_links_batch_not_blank
        CHECK (btrim(input_batch_id) <> ''),
    CONSTRAINT mini_order_run_input_links_source_kind_allowed
        CHECK (source_kind IN ('progress_batch', 'opening_wip')),
    CONSTRAINT mini_order_run_input_links_sequence_positive
        CHECK (sequence_no > 0),
    CONSTRAINT mini_order_run_input_links_status_allowed
        CHECK (status IN ('in_use', 'processed')),
    CONSTRAINT mini_order_run_input_links_processed_at_consistent CHECK (
        (status = 'in_use' AND processed_at IS NULL)
        OR (status = 'processed' AND processed_at IS NOT NULL)
    ),
    CONSTRAINT mini_order_run_input_links_sequence_unique
        UNIQUE (session_id, sequence_no)
);

CREATE TABLE IF NOT EXISTS mini_rezka_active_partial_rolls (
    session_id TEXT NOT NULL REFERENCES mini_order_run_sessions(session_id) ON DELETE CASCADE,
    order_id TEXT NOT NULL,
    apparatus TEXT NOT NULL,
    slot_index INTEGER NOT NULL,
    generation INTEGER NOT NULL,
    contained_kadr_count INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    source_input_batch_ids TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    started_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (session_id, slot_index),
    CONSTRAINT mini_rezka_active_partial_rolls_order_not_blank
        CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_rezka_active_partial_rolls_apparatus_not_blank
        CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_rezka_active_partial_rolls_slot_positive
        CHECK (slot_index > 0),
    CONSTRAINT mini_rezka_active_partial_rolls_generation_positive
        CHECK (generation > 0),
    CONSTRAINT mini_rezka_active_partial_rolls_kadr_positive
        CHECK (contained_kadr_count > 0),
    CONSTRAINT mini_rezka_active_partial_rolls_status_allowed
        CHECK (status = 'active'),
    CONSTRAINT mini_rezka_active_partial_rolls_sources_not_empty
        CHECK (cardinality(source_input_batch_ids) > 0),
    CONSTRAINT mini_rezka_active_partial_rolls_time_consistent
        CHECK (updated_at >= started_at)
);

CREATE TABLE IF NOT EXISTS mini_progress_batch_input_links (
    output_batch_id TEXT NOT NULL REFERENCES mini_progress_batches(batch_id) ON DELETE CASCADE,
    session_id TEXT NOT NULL,
    order_id TEXT NOT NULL,
    input_batch_id TEXT NOT NULL,
    input_qr_payload TEXT NOT NULL DEFAULT '',
    source_apparatus TEXT NOT NULL DEFAULT '',
    source_kind TEXT NOT NULL,
    sequence_no INTEGER NOT NULL,
    PRIMARY KEY (output_batch_id, input_batch_id),
    CONSTRAINT mini_progress_batch_input_links_session_not_blank
        CHECK (btrim(session_id) <> ''),
    CONSTRAINT mini_progress_batch_input_links_order_not_blank
        CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_progress_batch_input_links_input_not_blank
        CHECK (btrim(input_batch_id) <> ''),
    CONSTRAINT mini_progress_batch_input_links_source_kind_allowed
        CHECK (source_kind IN ('progress_batch', 'opening_wip')),
    CONSTRAINT mini_progress_batch_input_links_sequence_positive
        CHECK (sequence_no > 0),
    CONSTRAINT mini_progress_batch_input_links_sequence_unique
        UNIQUE (output_batch_id, sequence_no)
);

CREATE INDEX IF NOT EXISTS idx_mini_order_run_input_links_order
    ON mini_order_run_input_links (order_id, target_apparatus, session_id, sequence_no);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_order_run_input_links_one_in_use
    ON mini_order_run_input_links (session_id)
    WHERE status = 'in_use';

CREATE INDEX IF NOT EXISTS idx_mini_rezka_active_partial_rolls_order
    ON mini_rezka_active_partial_rolls (order_id, apparatus, session_id, slot_index);

CREATE INDEX IF NOT EXISTS idx_mini_progress_batch_input_links_input
    ON mini_progress_batch_input_links (input_batch_id, output_batch_id);

-- Preserve the legacy scalar input as lineage sequence 1. Source kind comes
-- from the table that actually owns the batch; ambiguous or missing parents
-- stay absent instead of being guessed. New writes replace this backfill with
-- the typed payload and normalized rows atomically.
INSERT INTO mini_order_run_input_links (
    session_id, order_id, target_apparatus,
    input_batch_id, input_qr_payload, source_apparatus, source_kind,
    stage_node_id, sequence_no, status, linked_at, processed_at
)
SELECT session_id,
       order_id,
       COALESCE(NULLIF(canonical_apparatus_id, ''), apparatus),
       btrim(payload_json->>'input_progress_batch_id'),
       COALESCE(payload_json->>'input_progress_qr_payload', ''),
       COALESCE(payload_json->>'input_progress_apparatus', ''),
       CASE
           WHEN EXISTS (
               SELECT 1
               FROM mini_opening_wip_batches opening_batch
               WHERE opening_batch.batch_id = btrim(
                   mini_order_run_sessions.payload_json->>'input_progress_batch_id'
               )
           ) THEN 'opening_wip'
           ELSE 'progress_batch'
       END,
       COALESCE(payload_json->>'stage_node_id', ''),
       1,
       CASE WHEN status = 'completed' THEN 'processed' ELSE 'in_use' END,
       started_at,
       CASE WHEN status = 'completed' THEN updated_at ELSE NULL END
FROM mini_order_run_sessions
WHERE btrim(COALESCE(payload_json->>'input_progress_batch_id', '')) <> ''
  AND (
      EXISTS (
          SELECT 1
          FROM mini_progress_batches progress_batch
          WHERE progress_batch.batch_id = btrim(
              mini_order_run_sessions.payload_json->>'input_progress_batch_id'
          )
      )
      <>
      EXISTS (
          SELECT 1
          FROM mini_opening_wip_batches opening_batch
          WHERE opening_batch.batch_id = btrim(
              mini_order_run_sessions.payload_json->>'input_progress_batch_id'
          )
      )
  )
ON CONFLICT (session_id, input_batch_id) DO NOTHING;

-- Existing output batches keep their unambiguous scalar parent as the first
-- lineage item, including opening-WIP parents.
INSERT INTO mini_progress_batch_input_links (
    output_batch_id, session_id, order_id,
    input_batch_id, input_qr_payload, source_apparatus, source_kind, sequence_no
)
SELECT batch_id,
       session_id,
       order_id,
       btrim(parent_batch_id),
       '',
       '',
       CASE
           WHEN EXISTS (
               SELECT 1
               FROM mini_opening_wip_batches opening_batch
               WHERE opening_batch.batch_id = btrim(mini_progress_batches.parent_batch_id)
           ) THEN 'opening_wip'
           ELSE 'progress_batch'
       END,
       1
FROM mini_progress_batches
WHERE btrim(parent_batch_id) <> ''
  AND (
      EXISTS (
          SELECT 1
          FROM mini_progress_batches input_progress_batch
          WHERE input_progress_batch.batch_id = btrim(mini_progress_batches.parent_batch_id)
      )
      <>
      EXISTS (
          SELECT 1
          FROM mini_opening_wip_batches opening_batch
          WHERE opening_batch.batch_id = btrim(mini_progress_batches.parent_batch_id)
      )
  )
ON CONFLICT (output_batch_id, input_batch_id) DO NOTHING;
