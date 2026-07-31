-- Emergency apparatus failover keeps the order execution identity together:
-- queue state, paused progress, operator session, map assignment, and material
-- assignment are committed with one durable audit/idempotency record.
CREATE TABLE IF NOT EXISTS mini_apparatus_order_transfers (
    transfer_id TEXT PRIMARY KEY,
    idempotency_key TEXT NOT NULL UNIQUE,
    order_id TEXT NOT NULL,
    from_apparatus TEXT NOT NULL,
    to_apparatus TEXT NOT NULL,
    reason TEXT NOT NULL,
    actor_role TEXT NOT NULL,
    actor_ref TEXT NOT NULL DEFAULT '',
    actor_display_name TEXT NOT NULL DEFAULT '',
    session_id TEXT NOT NULL,
    progress_batch_id TEXT NOT NULL,
    material_barcodes JSONB NOT NULL DEFAULT '[]'::jsonb,
    payload_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_apparatus_order_transfers_transfer_id_not_blank
        CHECK (btrim(transfer_id) <> ''),
    CONSTRAINT mini_apparatus_order_transfers_idempotency_not_blank
        CHECK (btrim(idempotency_key) <> ''),
    CONSTRAINT mini_apparatus_order_transfers_order_not_blank
        CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_apparatus_order_transfers_from_not_blank
        CHECK (btrim(from_apparatus) <> ''),
    CONSTRAINT mini_apparatus_order_transfers_to_not_blank
        CHECK (btrim(to_apparatus) <> ''),
    CONSTRAINT mini_apparatus_order_transfers_reason_not_blank
        CHECK (btrim(reason) <> ''),
    CONSTRAINT mini_apparatus_order_transfers_session_not_blank
        CHECK (btrim(session_id) <> ''),
    CONSTRAINT mini_apparatus_order_transfers_batch_not_blank
        CHECK (btrim(progress_batch_id) <> ''),
    CONSTRAINT mini_apparatus_order_transfers_material_array
        CHECK (jsonb_typeof(material_barcodes) = 'array')
);

CREATE INDEX IF NOT EXISTS idx_mini_apparatus_order_transfers_order_time
    ON mini_apparatus_order_transfers (order_id, created_at DESC);
