-- Producing apparatus is independent of location, which changes at warehouse receipt.
-- Empty ownership preserves historical packages whose evidence is absent or conflicts.
ALTER TABLE mini_paddons ADD COLUMN apparatus_id TEXT NOT NULL DEFAULT '';

WITH evidence AS (
    SELECT p.code, p.location AS apparatus_id
    FROM mini_paddons p JOIN mini_apparatus a ON a.id=p.location
    UNION ALL
    SELECT paddon_code, apparatus_id FROM mini_active_rezka_paddons WHERE paddon_code IS NOT NULL
    UNION ALL
    SELECT source_code, apparatus_id FROM mini_paddon_print_successors
    UNION ALL
    SELECT paddon_code, apparatus_id FROM mini_paddon_print_successors
    UNION ALL
    SELECT p.code, b.canonical_apparatus_id
    FROM mini_paddons p
    JOIN mini_paddon_items i ON i.paddon_id=p.id
    JOIN mini_progress_batches b ON b.batch_id=i.progress_batch_id
    WHERE b.canonical_apparatus_id IS NOT NULL
), unambiguous AS (
    SELECT code, MIN(apparatus_id) AS apparatus_id
    FROM evidence WHERE btrim(apparatus_id) <> ''
    GROUP BY code HAVING COUNT(DISTINCT apparatus_id)=1
)
UPDATE mini_paddons p SET apparatus_id=e.apparatus_id
FROM unambiguous e JOIN mini_apparatus a ON a.id=e.apparatus_id
WHERE p.code=e.code AND a.execution_profile_json->>'operation'='cut';

CREATE INDEX idx_mini_paddons_apparatus_updated
    ON mini_paddons(apparatus_id, updated_at DESC, code ASC);
