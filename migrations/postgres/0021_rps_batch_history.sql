-- Completed RPS batches are immutable identities. Their payload keeps the exact
-- print EPC list; every EPC can be reconciled with mini_gscale_receipts.barcode.
CREATE TABLE IF NOT EXISTS mini_rps_batch_history (
    batch_id TEXT NOT NULL,
    owner_key TEXT NOT NULL,
    owner_role TEXT NOT NULL,
    owner_ref TEXT NOT NULL,
    item_code TEXT NOT NULL DEFAULT '',
    warehouse TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL,
    completed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (owner_key, batch_id),
    CONSTRAINT mini_rps_batch_history_batch_not_blank CHECK (btrim(batch_id) <> ''),
    CONSTRAINT mini_rps_batch_history_owner_not_blank CHECK (btrim(owner_key) <> ''),
    CONSTRAINT mini_rps_batch_history_owner_role_not_blank CHECK (btrim(owner_role) <> ''),
    CONSTRAINT mini_rps_batch_history_owner_ref_not_blank CHECK (btrim(owner_ref) <> ''),
    CONSTRAINT mini_rps_batch_history_payload_object
        CHECK (jsonb_typeof(payload_json) = 'object')
);

CREATE INDEX IF NOT EXISTS idx_mini_rps_batch_history_owner_completed
    ON mini_rps_batch_history (owner_key, completed_at DESC, batch_id DESC);

-- Preserve the last stopped session already present before this migration.
INSERT INTO mini_rps_batch_history (
    batch_id,
    owner_key,
    owner_role,
    owner_ref,
    item_code,
    warehouse,
    payload_json,
    completed_at
)
SELECT
    batch_id,
    owner_key,
    owner_role,
    owner_ref,
    item_code,
    warehouse,
    payload_json,
    updated_at
FROM mini_rps_batches
WHERE NOT active
ON CONFLICT (owner_key, batch_id) DO NOTHING;
