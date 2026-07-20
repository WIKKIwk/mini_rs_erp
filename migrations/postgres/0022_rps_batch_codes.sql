-- Public batch codes are immutable, globally unique 24-character hex
-- identifiers. Internal owner-scoped batch IDs remain unchanged.
CREATE TABLE IF NOT EXISTS mini_rps_batch_identities (
    batch_code CHAR(24) PRIMARY KEY,
    owner_key TEXT NOT NULL,
    batch_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (owner_key, batch_id),
    CONSTRAINT mini_rps_batch_identities_code_format
        CHECK (batch_code ~ '^42[0-9A-F]{22}$'),
    CONSTRAINT mini_rps_batch_identities_owner_not_blank
        CHECK (btrim(owner_key) <> ''),
    CONSTRAINT mini_rps_batch_identities_batch_not_blank
        CHECK (btrim(batch_id) <> '')
);

-- Backfill every real batch once. Prefix 42 reserves a separate namespace from
-- product EPCs, the hash keeps retries deterministic and avoids changing IDs.
WITH known_batches AS (
    SELECT owner_key, batch_id, updated_at AS created_at
    FROM mini_rps_batches
    UNION ALL
    SELECT owner_key, batch_id, completed_at AS created_at
    FROM mini_rps_batch_history
), canonical_batches AS (
    SELECT owner_key, batch_id, min(created_at) AS created_at
    FROM known_batches
    WHERE btrim(owner_key) <> '' AND btrim(batch_id) <> ''
    GROUP BY owner_key, batch_id
)
INSERT INTO mini_rps_batch_identities (batch_code, owner_key, batch_id, created_at)
SELECT
    '42' || upper(substr(md5(owner_key || chr(31) || batch_id), 1, 22)),
    owner_key,
    batch_id,
    created_at
FROM canonical_batches
ON CONFLICT (owner_key, batch_id) DO NOTHING;

-- Keep the public code in the persisted session payload consumed by mobile.
UPDATE mini_rps_batches AS batch
SET payload_json = jsonb_set(
    batch.payload_json,
    '{batch_code}',
    to_jsonb(identity.batch_code::TEXT),
    true
)
FROM mini_rps_batch_identities AS identity
WHERE identity.owner_key = batch.owner_key
  AND identity.batch_id = batch.batch_id
  AND batch.payload_json->>'batch_code' IS DISTINCT FROM identity.batch_code::TEXT;

UPDATE mini_rps_batch_history AS history
SET payload_json = jsonb_set(
    history.payload_json,
    '{batch_code}',
    to_jsonb(identity.batch_code::TEXT),
    true
)
FROM mini_rps_batch_identities AS identity
WHERE identity.owner_key = history.owner_key
  AND identity.batch_id = history.batch_id
  AND history.payload_json->>'batch_code' IS DISTINCT FROM identity.batch_code::TEXT;
