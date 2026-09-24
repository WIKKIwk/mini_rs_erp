-- Outbox event log for live sequence streaming and replay recovery.
-- Keeps small deterministic deltas alongside full sequences.
ALTER TABLE mini_queue_sequences ADD COLUMN IF NOT EXISTS revision BIGINT NOT NULL DEFAULT 1;

CREATE TABLE IF NOT EXISTS mini_queue_events (
    id BIGSERIAL PRIMARY KEY,
    canonical_apparatus_id TEXT NOT NULL REFERENCES mini_apparatus(id),
    revision BIGINT NOT NULL,
    base_revision BIGINT NOT NULL,
    event_type TEXT NOT NULL DEFAULT 'delta',
    ops JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_mini_queue_events_apparatus_revision UNIQUE (canonical_apparatus_id, revision)
);

CREATE INDEX IF NOT EXISTS idx_mini_queue_events_lookup
    ON mini_queue_events (canonical_apparatus_id, revision);

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        GRANT SELECT, INSERT ON mini_queue_events TO mini_rs_erp;
        GRANT USAGE, SELECT ON SEQUENCE mini_queue_events_id_seq TO mini_rs_erp;
    END IF;
END $$;
