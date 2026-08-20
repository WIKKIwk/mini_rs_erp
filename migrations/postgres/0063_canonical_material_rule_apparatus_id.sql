-- Make the canonical apparatus identity authoritative for material rules while
-- retaining the legacy apparatus column and payload for display/compatibility.
-- The shape predicate mirrors validate_id_shape in
-- src/core/apparatus_standard/mod.rs: apparatus:<namespace>:<opaque-key>, with
-- lowercase ASCII [a-z0-9._-] segments and at most 128 UTF-8 bytes.

ALTER TABLE mini_apparatus_material_rules
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;

ALTER TABLE mini_apparatus_material_rules
    ADD CONSTRAINT mini_apparatus_material_rules_canonical_apparatus_id_shape_check
    CHECK (
        canonical_apparatus_id IS NULL OR (
            octet_length(canonical_apparatus_id) <= 128
            AND canonical_apparatus_id = btrim(canonical_apparatus_id)
            AND canonical_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );

-- Only exact canonical IDs are resolved. No normalization, fuzzy matching,
-- name/title inference, or synthesis is performed; legacy display values that
-- are not already canonical remain NULL and therefore stay out of live reads.
UPDATE mini_apparatus_material_rules r
SET canonical_apparatus_id = a.id
FROM mini_apparatus a
WHERE r.canonical_apparatus_id IS NULL
  AND r.apparatus = a.id
  AND a.id = btrim(a.id)
  AND octet_length(a.id) <= 128
  AND a.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$';

-- Queue-policy and capacity-profile upserts also target canonical identity.
-- Keep duplicate legacy rows visible and fail closed; never delete or merge
-- them to make a unique index pass.
CREATE OR REPLACE VIEW mini_canonical_apparatus_upsert_duplicate_diagnostics AS
SELECT 'mini_apparatus_material_rules' AS source_table,
       canonical_apparatus_id,
       count(*) AS duplicate_rows
FROM mini_apparatus_material_rules
WHERE canonical_apparatus_id IS NOT NULL
GROUP BY canonical_apparatus_id
HAVING count(*) > 1
UNION ALL
SELECT 'mini_apparatus_queue_policies',
       canonical_apparatus_id,
       count(*)
FROM mini_apparatus_queue_policies
WHERE canonical_apparatus_id IS NOT NULL
GROUP BY canonical_apparatus_id
HAVING count(*) > 1
UNION ALL
SELECT 'mini_apparatus_capacity_profiles',
       canonical_apparatus_id,
       count(*)
FROM mini_apparatus_capacity_profiles
WHERE canonical_apparatus_id IS NOT NULL
GROUP BY canonical_apparatus_id
HAVING count(*) > 1;

ALTER TABLE mini_apparatus_material_rules
    ADD CONSTRAINT mini_apparatus_material_rules_canonical_apparatus_fk
    FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;

CREATE INDEX IF NOT EXISTS idx_mini_apparatus_material_rules_canonical_apparatus
    ON mini_apparatus_material_rules (canonical_apparatus_id);

DO $$
DECLARE duplicate_id TEXT;
BEGIN
    SELECT canonical_apparatus_id
    INTO duplicate_id
    FROM mini_canonical_apparatus_upsert_duplicate_diagnostics
    WHERE source_table = 'mini_apparatus_material_rules'
    ORDER BY canonical_apparatus_id
    LIMIT 1;

    IF duplicate_id IS NOT NULL THEN
        RAISE EXCEPTION
            '0063 canonical unique preflight failed for mini_apparatus_material_rules: duplicate canonical_apparatus_id=%; inspect mini_canonical_apparatus_upsert_duplicate_diagnostics',
            duplicate_id;
    END IF;
END
$$;

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_apparatus_material_rules_canonical_unique
    ON mini_apparatus_material_rules (canonical_apparatus_id);

DO $$
DECLARE duplicate_id TEXT;
BEGIN
    SELECT canonical_apparatus_id
    INTO duplicate_id
    FROM mini_canonical_apparatus_upsert_duplicate_diagnostics
    WHERE source_table = 'mini_apparatus_queue_policies'
    ORDER BY canonical_apparatus_id
    LIMIT 1;

    IF duplicate_id IS NOT NULL THEN
        RAISE EXCEPTION
            '0063 canonical unique preflight failed for mini_apparatus_queue_policies: duplicate canonical_apparatus_id=%; inspect mini_canonical_apparatus_upsert_duplicate_diagnostics',
            duplicate_id;
    END IF;
END
$$;

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_apparatus_queue_policies_canonical_unique
    ON mini_apparatus_queue_policies (canonical_apparatus_id);

DO $$
DECLARE duplicate_id TEXT;
BEGIN
    SELECT canonical_apparatus_id
    INTO duplicate_id
    FROM mini_canonical_apparatus_upsert_duplicate_diagnostics
    WHERE source_table = 'mini_apparatus_capacity_profiles'
    ORDER BY canonical_apparatus_id
    LIMIT 1;

    IF duplicate_id IS NOT NULL THEN
        RAISE EXCEPTION
            '0063 canonical unique preflight failed for mini_apparatus_capacity_profiles: duplicate canonical_apparatus_id=%; inspect mini_canonical_apparatus_upsert_duplicate_diagnostics',
            duplicate_id;
    END IF;
END
$$;

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_apparatus_capacity_profiles_canonical_unique
    ON mini_apparatus_capacity_profiles (canonical_apparatus_id);

-- Preserve the existing aggregate diagnostic surface and add material rules.
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
FROM mini_apparatus_order_transfers
UNION ALL SELECT 'mini_apparatus_material_rules',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL AND btrim(apparatus) <> ''),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL AND (
           canonical_apparatus_id <> btrim(canonical_apparatus_id) OR octet_length(canonical_apparatus_id) > 128
           OR canonical_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
           OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_apparatus_id)))
FROM mini_apparatus_material_rules;

-- Row-level diagnostics distinguish unresolved, malformed, and orphaned
-- canonical references without changing the existing aggregate view contract.
CREATE OR REPLACE VIEW mini_apparatus_material_rule_canonical_diagnostics AS
SELECT apparatus,
       canonical_apparatus_id,
       CASE
           WHEN canonical_apparatus_id IS NULL THEN 'unresolved'
           WHEN canonical_apparatus_id <> btrim(canonical_apparatus_id)
               OR octet_length(canonical_apparatus_id) > 128
               OR canonical_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
               THEN 'invalid'
           WHEN NOT EXISTS (
               SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_apparatus_id
           ) THEN 'orphan'
       END AS issue
FROM mini_apparatus_material_rules
WHERE (canonical_apparatus_id IS NULL AND btrim(apparatus) <> '')
   OR canonical_apparatus_id IS NOT NULL AND (
       canonical_apparatus_id <> btrim(canonical_apparatus_id)
       OR octet_length(canonical_apparatus_id) > 128
       OR canonical_apparatus_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
       OR NOT EXISTS (SELECT 1 FROM mini_apparatus a WHERE a.id = canonical_apparatus_id)
   );
