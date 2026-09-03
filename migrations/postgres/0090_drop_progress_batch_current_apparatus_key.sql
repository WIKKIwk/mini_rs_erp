-- 1. Backfill canonical_current_apparatus_id from current_apparatus_key if missing and matches a valid apparatus ID
UPDATE mini_progress_batches
SET canonical_current_apparatus_id = current_apparatus_key
WHERE (canonical_current_apparatus_id IS NULL OR btrim(canonical_current_apparatus_id) = '')
  AND btrim(current_apparatus_key) <> ''
  AND EXISTS (SELECT 1 FROM mini_apparatus WHERE id = current_apparatus_key);

-- Ensure current_apparatus has the canonical ID if it was empty
UPDATE mini_progress_batches
SET current_apparatus = canonical_current_apparatus_id
WHERE (current_apparatus IS NULL OR btrim(current_apparatus) = '')
  AND canonical_current_apparatus_id IS NOT NULL
  AND btrim(canonical_current_apparatus_id) <> '';

-- 2. Drop obsolete index built around current_apparatus_key
DROP INDEX IF EXISTS idx_mini_progress_batches_wip_status_apparatus_key;

-- 3. Rebuild index on actual canonical current apparatus identity
CREATE INDEX IF NOT EXISTS idx_mini_progress_batches_wip_status_canonical_current_apparatus
    ON mini_progress_batches (wip_status, canonical_current_apparatus_id, updated_at DESC);

-- 4. Drop the redundant current_apparatus_key column
ALTER TABLE mini_progress_batches
    DROP COLUMN IF EXISTS current_apparatus_key;
