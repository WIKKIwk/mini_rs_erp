-- Stage canonical apparatus identity without removing legacy display/audit values.
--
-- Canonical ApparatusId predicate (mirrors validate_id_shape in
-- src/core/apparatus_standard/mod.rs): no surrounding whitespace, at most 128
-- UTF-8 bytes, exact `apparatus:<namespace>:<opaque-key>` shape, and only
-- lowercase ASCII letters, digits, '-', '_', or '.' in each non-empty segment.
-- Backfill is deliberately exact: only a legacy value that is already an exact
-- canonical-valid mini_apparatus.id is copied. No id is normalized, guessed,
-- display-derived, or synthesized.

ALTER TABLE mini_worker_groups
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_queue_sequences
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_queue_states
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_apparatus_queue_policies
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_queue_action_events
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_order_run_sessions
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_order_progress_events
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_training_queue_states
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_training_progress_batches
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_apparatus_capacity_profiles
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_apparatus_downtimes
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_apparatus_schedule_reservations
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_apparatus_order_transfers
    ADD COLUMN IF NOT EXISTS canonical_from_apparatus_id TEXT,
    ADD COLUMN IF NOT EXISTS canonical_to_apparatus_id TEXT;

-- Canonical references are nullable during legacy cleanup, but every value
-- that is present must already satisfy the Rust ApparatusId shape. The master
-- table guard is staged NOT VALID so existing legacy IDs remain untouched and
-- visible through the diagnostic view below; new inserts and updates are
-- checked immediately.
ALTER TABLE mini_apparatus
    ADD CONSTRAINT mini_apparatus_id_canonical_shape_check
    CHECK (
        id IS NULL OR (
            octet_length(id) <= 128
            AND id = btrim(id)
            AND id !~ '[[:space:][:cntrl:]]'
            AND id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    ) NOT VALID;

CREATE OR REPLACE VIEW mini_apparatus_legacy_id_shape_diagnostics AS
SELECT id,
       octet_length(id) AS id_bytes,
       'invalid_canonical_shape' AS issue
FROM mini_apparatus
WHERE id IS NULL OR id <> btrim(id)
   OR octet_length(id) > 128
   OR id ~ '[[:space:][:cntrl:]]'
   OR id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$';

ALTER TABLE mini_worker_groups
    ADD CONSTRAINT mini_worker_groups_canonical_apparatus_id_shape_check
    CHECK (
        canonical_apparatus_id IS NULL OR (
            octet_length(canonical_apparatus_id) <= 128
            AND canonical_apparatus_id = btrim(canonical_apparatus_id)
            AND canonical_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );
ALTER TABLE mini_queue_sequences
    ADD CONSTRAINT mini_queue_sequences_canonical_apparatus_id_shape_check
    CHECK (
        canonical_apparatus_id IS NULL OR (
            octet_length(canonical_apparatus_id) <= 128
            AND canonical_apparatus_id = btrim(canonical_apparatus_id)
            AND canonical_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );
ALTER TABLE mini_queue_states
    ADD CONSTRAINT mini_queue_states_canonical_apparatus_id_shape_check
    CHECK (
        canonical_apparatus_id IS NULL OR (
            octet_length(canonical_apparatus_id) <= 128
            AND canonical_apparatus_id = btrim(canonical_apparatus_id)
            AND canonical_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );
ALTER TABLE mini_apparatus_queue_policies
    ADD CONSTRAINT mini_apparatus_queue_policies_canonical_apparatus_id_shape_check
    CHECK (
        canonical_apparatus_id IS NULL OR (
            octet_length(canonical_apparatus_id) <= 128
            AND canonical_apparatus_id = btrim(canonical_apparatus_id)
            AND canonical_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );
ALTER TABLE mini_queue_action_events
    ADD CONSTRAINT mini_queue_action_events_canonical_apparatus_id_shape_check
    CHECK (
        canonical_apparatus_id IS NULL OR (
            octet_length(canonical_apparatus_id) <= 128
            AND canonical_apparatus_id = btrim(canonical_apparatus_id)
            AND canonical_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );
ALTER TABLE mini_order_run_sessions
    ADD CONSTRAINT mini_order_run_sessions_canonical_apparatus_id_shape_check
    CHECK (
        canonical_apparatus_id IS NULL OR (
            octet_length(canonical_apparatus_id) <= 128
            AND canonical_apparatus_id = btrim(canonical_apparatus_id)
            AND canonical_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );
ALTER TABLE mini_order_progress_events
    ADD CONSTRAINT mini_order_progress_events_canonical_apparatus_id_shape_check
    CHECK (
        canonical_apparatus_id IS NULL OR (
            octet_length(canonical_apparatus_id) <= 128
            AND canonical_apparatus_id = btrim(canonical_apparatus_id)
            AND canonical_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );
ALTER TABLE mini_training_queue_states
    ADD CONSTRAINT mini_training_queue_states_canonical_apparatus_id_shape_check
    CHECK (
        canonical_apparatus_id IS NULL OR (
            octet_length(canonical_apparatus_id) <= 128
            AND canonical_apparatus_id = btrim(canonical_apparatus_id)
            AND canonical_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );
ALTER TABLE mini_training_progress_batches
    ADD CONSTRAINT mini_training_progress_batches_canonical_apparatus_id_shape_check
    CHECK (
        canonical_apparatus_id IS NULL OR (
            octet_length(canonical_apparatus_id) <= 128
            AND canonical_apparatus_id = btrim(canonical_apparatus_id)
            AND canonical_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );
ALTER TABLE mini_apparatus_capacity_profiles
    ADD CONSTRAINT mini_apparatus_capacity_profiles_canonical_apparatus_id_shape_check
    CHECK (
        canonical_apparatus_id IS NULL OR (
            octet_length(canonical_apparatus_id) <= 128
            AND canonical_apparatus_id = btrim(canonical_apparatus_id)
            AND canonical_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );
ALTER TABLE mini_apparatus_downtimes
    ADD CONSTRAINT mini_apparatus_downtimes_canonical_apparatus_id_shape_check
    CHECK (
        canonical_apparatus_id IS NULL OR (
            octet_length(canonical_apparatus_id) <= 128
            AND canonical_apparatus_id = btrim(canonical_apparatus_id)
            AND canonical_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );
ALTER TABLE mini_apparatus_schedule_reservations
    ADD CONSTRAINT mini_apparatus_schedule_reservations_canonical_apparatus_id_shape_check
    CHECK (
        canonical_apparatus_id IS NULL OR (
            octet_length(canonical_apparatus_id) <= 128
            AND canonical_apparatus_id = btrim(canonical_apparatus_id)
            AND canonical_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );
ALTER TABLE mini_apparatus_order_transfers
    ADD CONSTRAINT mini_apparatus_order_transfers_canonical_from_apparatus_id_shape_check
    CHECK (
        canonical_from_apparatus_id IS NULL OR (
            octet_length(canonical_from_apparatus_id) <= 128
            AND canonical_from_apparatus_id = btrim(canonical_from_apparatus_id)
            AND canonical_from_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_from_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );
ALTER TABLE mini_apparatus_order_transfers
    ADD CONSTRAINT mini_apparatus_order_transfers_canonical_to_apparatus_id_shape_check
    CHECK (
        canonical_to_apparatus_id IS NULL OR (
            octet_length(canonical_to_apparatus_id) <= 128
            AND canonical_to_apparatus_id = btrim(canonical_to_apparatus_id)
            AND canonical_to_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_to_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );

-- Deterministic backfill only. A legacy stored value is copied only when it is
-- itself an exact canonical-valid master id; unmatched and noncanonical legacy
-- values remain NULL.
UPDATE mini_worker_groups w SET canonical_apparatus_id = a.id
FROM mini_apparatus a WHERE w.canonical_apparatus_id IS NULL
  AND a.id = btrim(a.id) AND octet_length(a.id) <= 128
  AND a.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
  AND w.apparatus = a.id;
UPDATE mini_queue_sequences q SET canonical_apparatus_id = a.id
FROM mini_apparatus a WHERE q.canonical_apparatus_id IS NULL
  AND a.id = btrim(a.id) AND octet_length(a.id) <= 128
  AND a.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
  AND q.apparatus = a.id;
UPDATE mini_queue_states q SET canonical_apparatus_id = a.id
FROM mini_apparatus a WHERE q.canonical_apparatus_id IS NULL
  AND a.id = btrim(a.id) AND octet_length(a.id) <= 128
  AND a.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
  AND q.apparatus = a.id;
UPDATE mini_apparatus_queue_policies q SET canonical_apparatus_id = a.id
FROM mini_apparatus a WHERE q.canonical_apparatus_id IS NULL
  AND a.id = btrim(a.id) AND octet_length(a.id) <= 128
  AND a.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
  AND q.apparatus = a.id;
UPDATE mini_queue_action_events q SET canonical_apparatus_id = a.id
FROM mini_apparatus a WHERE q.canonical_apparatus_id IS NULL
  AND a.id = btrim(a.id) AND octet_length(a.id) <= 128
  AND a.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
  AND q.apparatus = a.id;
UPDATE mini_order_run_sessions q SET canonical_apparatus_id = a.id
FROM mini_apparatus a WHERE q.canonical_apparatus_id IS NULL
  AND a.id = btrim(a.id) AND octet_length(a.id) <= 128
  AND a.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
  AND q.apparatus = a.id;
UPDATE mini_order_progress_events q SET canonical_apparatus_id = a.id
FROM mini_apparatus a WHERE q.canonical_apparatus_id IS NULL
  AND a.id = btrim(a.id) AND octet_length(a.id) <= 128
  AND a.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
  AND q.apparatus = a.id;
UPDATE mini_training_queue_states q SET canonical_apparatus_id = a.id
FROM mini_apparatus a WHERE q.canonical_apparatus_id IS NULL
  AND a.id = btrim(a.id) AND octet_length(a.id) <= 128
  AND a.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
  AND q.apparatus = a.id;
UPDATE mini_training_progress_batches q SET canonical_apparatus_id = a.id
FROM mini_apparatus a WHERE q.canonical_apparatus_id IS NULL
  AND a.id = btrim(a.id) AND octet_length(a.id) <= 128
  AND a.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
  AND q.apparatus = a.id;
UPDATE mini_apparatus_capacity_profiles q SET canonical_apparatus_id = a.id
FROM mini_apparatus a WHERE q.canonical_apparatus_id IS NULL
  AND a.id = btrim(a.id) AND octet_length(a.id) <= 128
  AND a.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
  AND q.apparatus_id = a.id;
UPDATE mini_apparatus_downtimes q SET canonical_apparatus_id = a.id
FROM mini_apparatus a WHERE q.canonical_apparatus_id IS NULL
  AND a.id = btrim(a.id) AND octet_length(a.id) <= 128
  AND a.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
  AND q.apparatus_id = a.id;
UPDATE mini_apparatus_schedule_reservations q SET canonical_apparatus_id = a.id
FROM mini_apparatus a WHERE q.canonical_apparatus_id IS NULL
  AND a.id = btrim(a.id) AND octet_length(a.id) <= 128
  AND a.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
  AND q.apparatus_id = a.id;
UPDATE mini_apparatus_order_transfers q SET canonical_from_apparatus_id = a.id
FROM mini_apparatus a WHERE q.canonical_from_apparatus_id IS NULL
  AND a.id = btrim(a.id) AND octet_length(a.id) <= 128
  AND a.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
  AND q.from_apparatus = a.id;
UPDATE mini_apparatus_order_transfers q SET canonical_to_apparatus_id = a.id
FROM mini_apparatus a WHERE q.canonical_to_apparatus_id IS NULL
  AND a.id = btrim(a.id) AND octet_length(a.id) <= 128
  AND a.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
  AND q.to_apparatus = a.id;

ALTER TABLE mini_worker_groups ADD CONSTRAINT mini_worker_groups_canonical_apparatus_fk
    FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_queue_sequences ADD CONSTRAINT mini_queue_sequences_canonical_apparatus_fk
    FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_queue_states ADD CONSTRAINT mini_queue_states_canonical_apparatus_fk
    FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_apparatus_queue_policies ADD CONSTRAINT mini_apparatus_queue_policies_canonical_apparatus_fk
    FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_queue_action_events ADD CONSTRAINT mini_queue_action_events_canonical_apparatus_fk
    FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_order_run_sessions ADD CONSTRAINT mini_order_run_sessions_canonical_apparatus_fk
    FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_order_progress_events ADD CONSTRAINT mini_order_progress_events_canonical_apparatus_fk
    FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_training_queue_states ADD CONSTRAINT mini_training_queue_states_canonical_apparatus_fk
    FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_training_progress_batches ADD CONSTRAINT mini_training_progress_batches_canonical_apparatus_fk
    FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_apparatus_capacity_profiles ADD CONSTRAINT mini_apparatus_capacity_profiles_canonical_apparatus_fk
    FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_apparatus_downtimes ADD CONSTRAINT mini_apparatus_downtimes_canonical_apparatus_fk
    FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_apparatus_schedule_reservations ADD CONSTRAINT mini_apparatus_schedule_reservations_canonical_apparatus_fk
    FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_apparatus_order_transfers ADD CONSTRAINT mini_apparatus_order_transfers_canonical_from_fk
    FOREIGN KEY (canonical_from_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_apparatus_order_transfers ADD CONSTRAINT mini_apparatus_order_transfers_canonical_to_fk
    FOREIGN KEY (canonical_to_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;

CREATE INDEX IF NOT EXISTS idx_mini_worker_groups_canonical_apparatus
    ON mini_worker_groups (canonical_apparatus_id);
CREATE INDEX IF NOT EXISTS idx_mini_queue_sequences_canonical_apparatus
    ON mini_queue_sequences (canonical_apparatus_id);
CREATE INDEX IF NOT EXISTS idx_mini_queue_states_canonical_apparatus
    ON mini_queue_states (canonical_apparatus_id);
CREATE INDEX IF NOT EXISTS idx_mini_apparatus_queue_policies_canonical_apparatus
    ON mini_apparatus_queue_policies (canonical_apparatus_id);

CREATE OR REPLACE VIEW mini_canonical_apparatus_queue_policy_duplicate_diagnostics AS
SELECT canonical_apparatus_id,
       count(*) AS duplicate_rows
FROM mini_apparatus_queue_policies
WHERE canonical_apparatus_id IS NOT NULL
GROUP BY canonical_apparatus_id
HAVING count(*) > 1;

DO $$
DECLARE duplicate_id TEXT;
BEGIN
    SELECT canonical_apparatus_id
    INTO duplicate_id
    FROM mini_canonical_apparatus_queue_policy_duplicate_diagnostics
    ORDER BY canonical_apparatus_id
    LIMIT 1;

    IF duplicate_id IS NOT NULL THEN
        RAISE EXCEPTION
            '0063 canonical unique preflight failed for mini_apparatus_queue_policies: duplicate canonical_apparatus_id=%; inspect mini_canonical_apparatus_queue_policy_duplicate_diagnostics',
            duplicate_id;
    END IF;
END
$$;

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_apparatus_queue_policies_canonical_apparatus_unique
    ON mini_apparatus_queue_policies (canonical_apparatus_id);
CREATE INDEX IF NOT EXISTS idx_mini_queue_action_events_canonical_apparatus
    ON mini_queue_action_events (canonical_apparatus_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_order_run_sessions_canonical_apparatus
    ON mini_order_run_sessions (canonical_apparatus_id);
CREATE INDEX IF NOT EXISTS idx_mini_order_progress_events_canonical_apparatus
    ON mini_order_progress_events (canonical_apparatus_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_training_queue_states_canonical_apparatus
    ON mini_training_queue_states (canonical_apparatus_id);
CREATE INDEX IF NOT EXISTS idx_mini_training_progress_batches_canonical_apparatus
    ON mini_training_progress_batches (canonical_apparatus_id);
CREATE INDEX IF NOT EXISTS idx_mini_apparatus_capacity_profiles_canonical_apparatus
    ON mini_apparatus_capacity_profiles (canonical_apparatus_id);
CREATE INDEX IF NOT EXISTS idx_mini_apparatus_downtimes_canonical_apparatus
    ON mini_apparatus_downtimes (canonical_apparatus_id, starts_at, ends_at);
CREATE INDEX IF NOT EXISTS idx_mini_apparatus_schedule_reservations_canonical_apparatus
    ON mini_apparatus_schedule_reservations (canonical_apparatus_id, starts_at, ends_at);
CREATE INDEX IF NOT EXISTS idx_mini_apparatus_order_transfers_canonical_from_to
    ON mini_apparatus_order_transfers (canonical_from_apparatus_id, canonical_to_apparatus_id);

-- Operational validation surface: NULL canonical IDs with nonblank legacy
-- values are unresolved; non-NULL values are invalid when they fail the same
-- shape predicate or do not reference a master row. Keep both signals visible
-- before any future NOT NULL cutover or FK validation.
CREATE OR REPLACE VIEW mini_unresolved_apparatus_reference_counts AS
SELECT 'mini_worker_groups' AS source_table,
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL AND btrim(apparatus) <> '') AS unresolved_rows,
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL AND (
           canonical_apparatus_id <> btrim(canonical_apparatus_id)
           OR octet_length(canonical_apparatus_id) > 128
           OR canonical_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
           OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_apparatus_id)
       )) AS invalid_canonical_rows
FROM mini_worker_groups
UNION ALL SELECT 'mini_queue_sequences',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL AND btrim(apparatus) <> ''),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL AND (
           canonical_apparatus_id <> btrim(canonical_apparatus_id) OR octet_length(canonical_apparatus_id) > 128
           OR canonical_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
           OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_apparatus_id)))
FROM mini_queue_sequences
UNION ALL SELECT 'mini_queue_states',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL AND btrim(apparatus) <> ''),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL AND (
           canonical_apparatus_id <> btrim(canonical_apparatus_id) OR octet_length(canonical_apparatus_id) > 128
           OR canonical_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
           OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_apparatus_id)))
FROM mini_queue_states
UNION ALL SELECT 'mini_apparatus_queue_policies',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL AND btrim(apparatus) <> ''),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL AND (
           canonical_apparatus_id <> btrim(canonical_apparatus_id) OR octet_length(canonical_apparatus_id) > 128
           OR canonical_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
           OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_apparatus_id)))
FROM mini_apparatus_queue_policies
UNION ALL SELECT 'mini_queue_action_events',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL AND btrim(apparatus) <> ''),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL AND (
           canonical_apparatus_id <> btrim(canonical_apparatus_id) OR octet_length(canonical_apparatus_id) > 128
           OR canonical_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
           OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_apparatus_id)))
FROM mini_queue_action_events
UNION ALL SELECT 'mini_order_run_sessions',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL AND btrim(apparatus) <> ''),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL AND (
           canonical_apparatus_id <> btrim(canonical_apparatus_id) OR octet_length(canonical_apparatus_id) > 128
           OR canonical_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
           OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_apparatus_id)))
FROM mini_order_run_sessions
UNION ALL SELECT 'mini_order_progress_events',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL AND btrim(apparatus) <> ''),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL AND (
           canonical_apparatus_id <> btrim(canonical_apparatus_id) OR octet_length(canonical_apparatus_id) > 128
           OR canonical_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
           OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_apparatus_id)))
FROM mini_order_progress_events
UNION ALL SELECT 'mini_training_queue_states',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL AND btrim(apparatus) <> ''),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL AND (
           canonical_apparatus_id <> btrim(canonical_apparatus_id) OR octet_length(canonical_apparatus_id) > 128
           OR canonical_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
           OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_apparatus_id)))
FROM mini_training_queue_states
UNION ALL SELECT 'mini_training_progress_batches',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL AND btrim(apparatus) <> ''),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL AND (
           canonical_apparatus_id <> btrim(canonical_apparatus_id) OR octet_length(canonical_apparatus_id) > 128
           OR canonical_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
           OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_apparatus_id)))
FROM mini_training_progress_batches
UNION ALL SELECT 'mini_apparatus_capacity_profiles',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL AND (btrim(apparatus_id) <> '' OR btrim(apparatus) <> '')),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL AND (
           canonical_apparatus_id <> btrim(canonical_apparatus_id) OR octet_length(canonical_apparatus_id) > 128
           OR canonical_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
           OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_apparatus_id)))
FROM mini_apparatus_capacity_profiles
UNION ALL SELECT 'mini_apparatus_downtimes',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL AND (btrim(apparatus_id) <> '' OR btrim(apparatus) <> '')),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL AND (
           canonical_apparatus_id <> btrim(canonical_apparatus_id) OR octet_length(canonical_apparatus_id) > 128
           OR canonical_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
           OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_apparatus_id)))
FROM mini_apparatus_downtimes
UNION ALL SELECT 'mini_apparatus_schedule_reservations',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL AND (btrim(apparatus_id) <> '' OR btrim(apparatus) <> '')),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL AND (
           canonical_apparatus_id <> btrim(canonical_apparatus_id) OR octet_length(canonical_apparatus_id) > 128
           OR canonical_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
           OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_apparatus_id)))
FROM mini_apparatus_schedule_reservations
UNION ALL SELECT 'mini_apparatus_order_transfers.from',
       count(*) FILTER (WHERE canonical_from_apparatus_id IS NULL AND btrim(from_apparatus) <> ''),
       count(*) FILTER (WHERE canonical_from_apparatus_id IS NOT NULL AND (
           canonical_from_apparatus_id <> btrim(canonical_from_apparatus_id) OR octet_length(canonical_from_apparatus_id) > 128
           OR canonical_from_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
           OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_from_apparatus_id)))
FROM mini_apparatus_order_transfers
UNION ALL SELECT 'mini_apparatus_order_transfers.to',
       count(*) FILTER (WHERE canonical_to_apparatus_id IS NULL AND btrim(to_apparatus) <> ''),
       count(*) FILTER (WHERE canonical_to_apparatus_id IS NOT NULL AND (
           canonical_to_apparatus_id <> btrim(canonical_to_apparatus_id) OR octet_length(canonical_to_apparatus_id) > 128
           OR canonical_to_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
           OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_to_apparatus_id)))
FROM mini_apparatus_order_transfers;
