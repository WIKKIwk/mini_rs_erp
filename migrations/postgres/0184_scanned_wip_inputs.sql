-- Input receipts are separate from hot queue/list snapshots. Keep them after
-- an unused session is undone so the same QR's later claim has a new identity.
CREATE TABLE mini_scanned_wip_inputs (
    claim_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    order_id TEXT NOT NULL,
    canonical_apparatus_id TEXT NOT NULL,
    stage_node_id TEXT NOT NULL,
    batch_id TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('progress_batch','opening_wip')),
    qr_payload TEXT NOT NULL,
    source_apparatus TEXT NOT NULL,
    worker_ref TEXT NOT NULL,
    worker_display_name TEXT NOT NULL,
    linked_at_unix BIGINT NOT NULL,
    work_cursor BIGINT NOT NULL DEFAULT 0,
    material_cursor BIGINT NOT NULL DEFAULT 0,
    before_json JSONB NOT NULL,
    claimed_json JSONB NOT NULL,
    map_json JSONB NOT NULL,
    returned_at_unix BIGINT,
    returned_by TEXT,
    return_token TEXT UNIQUE
);
CREATE INDEX mini_scanned_wip_scope_idx ON mini_scanned_wip_inputs
    (order_id, canonical_apparatus_id, stage_node_id, linked_at_unix DESC, claim_id);
CREATE UNIQUE INDEX mini_scanned_wip_session_idx ON mini_scanned_wip_inputs
    (session_id, batch_id) WHERE returned_at_unix IS NULL;

GRANT SELECT, INSERT, UPDATE ON mini_scanned_wip_inputs TO mini_rs_erp;
