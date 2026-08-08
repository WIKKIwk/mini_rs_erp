-- Worker WIP corrections keep the current batch as the read projection while
-- preserving every change as an append-only audit record.

SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE mini_progress_batches
    ADD COLUMN IF NOT EXISTS revision BIGINT NOT NULL DEFAULT 1;

ALTER TABLE mini_progress_batches
    DROP CONSTRAINT IF EXISTS mini_progress_batches_revision_positive;
ALTER TABLE mini_progress_batches
    ADD CONSTRAINT mini_progress_batches_revision_positive
    CHECK (revision > 0) NOT VALID;
ALTER TABLE mini_progress_batches
    VALIDATE CONSTRAINT mini_progress_batches_revision_positive;

CREATE TABLE IF NOT EXISTS mini_progress_batch_corrections (
    id BIGSERIAL PRIMARY KEY,
    batch_id TEXT NOT NULL REFERENCES mini_progress_batches(batch_id),
    previous_revision BIGINT NOT NULL,
    new_revision BIGINT NOT NULL,
    reason TEXT NOT NULL,
    actor_role TEXT NOT NULL,
    actor_ref TEXT NOT NULL,
    actor_display_name TEXT NOT NULL,
    old_values JSONB NOT NULL,
    new_values JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_progress_batch_corrections_reason_required
        CHECK (btrim(reason) <> ''),
    CONSTRAINT mini_progress_batch_corrections_revision_step
        CHECK (previous_revision > 0 AND new_revision = previous_revision + 1),
    CONSTRAINT mini_progress_batch_corrections_batch_revision_unique
        UNIQUE (batch_id, new_revision)
);

CREATE INDEX IF NOT EXISTS mini_progress_batch_corrections_batch_created_idx
    ON mini_progress_batch_corrections (batch_id, created_at DESC, id DESC);

GRANT SELECT, INSERT
    ON TABLE mini_progress_batch_corrections TO mini_rs_erp;
GRANT USAGE, SELECT
    ON SEQUENCE mini_progress_batch_corrections_id_seq TO mini_rs_erp;
