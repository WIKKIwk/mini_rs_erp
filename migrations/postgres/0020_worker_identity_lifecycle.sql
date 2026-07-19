-- Worker identity is immutable. Names and phone numbers are mutable display/login
-- attributes and must never own production history.
ALTER TABLE mini_workers
    ADD COLUMN IF NOT EXISTS active BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS deactivated_at TIMESTAMPTZ;

ALTER TABLE mini_workers
    DROP CONSTRAINT IF EXISTS mini_workers_name_unique;

DROP INDEX IF EXISTS idx_mini_workers_lower_name;
CREATE INDEX IF NOT EXISTS idx_mini_workers_active_lower_name
    ON mini_workers (lower(name))
    WHERE active;

-- A phone may be assigned to another worker after the previous assignment is
-- closed. Historical ownership is preserved in the effective-dated alias table.
DROP INDEX IF EXISTS idx_mini_workers_phone_key_unique;
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_workers_active_phone_key_unique
    ON mini_workers (phone_key)
    WHERE active AND phone_key <> '';

CREATE TABLE IF NOT EXISTS mini_worker_identity_aliases (
    id BIGSERIAL PRIMARY KEY,
    worker_id TEXT NOT NULL
        REFERENCES mini_workers(id)
        ON UPDATE CASCADE
        ON DELETE RESTRICT,
    alias_type TEXT NOT NULL,
    alias_key TEXT NOT NULL,
    valid_from TIMESTAMPTZ NOT NULL,
    valid_to TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_worker_identity_alias_type_allowed
        CHECK (alias_type IN ('phone')),
    CONSTRAINT mini_worker_identity_alias_key_not_blank
        CHECK (btrim(alias_key) <> ''),
    CONSTRAINT mini_worker_identity_alias_period_valid
        CHECK (valid_to IS NULL OR valid_to >= valid_from)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_worker_identity_alias_open_unique
    ON mini_worker_identity_aliases (alias_type, alias_key)
    WHERE valid_to IS NULL;
CREATE INDEX IF NOT EXISTS idx_mini_worker_identity_alias_worker_period
    ON mini_worker_identity_aliases (worker_id, alias_type, valid_from, valid_to);

INSERT INTO mini_worker_identity_aliases (
    worker_id,
    alias_type,
    alias_key,
    valid_from,
    valid_to
)
SELECT
    worker.id,
    'phone',
    worker.phone_key,
    worker.created_at,
    CASE WHEN worker.active THEN NULL ELSE worker.deactivated_at END
FROM mini_workers AS worker
WHERE worker.phone_key <> ''
  AND NOT EXISTS (
      SELECT 1
      FROM mini_worker_identity_aliases AS alias
      WHERE alias.worker_id = worker.id
        AND alias.alias_type = 'phone'
        AND alias.alias_key = worker.phone_key
        AND alias.valid_from = worker.created_at
  );
