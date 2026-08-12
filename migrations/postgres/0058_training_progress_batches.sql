CREATE TABLE IF NOT EXISTS mini_training_progress_batches (
    batch_id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL,
    apparatus TEXT NOT NULL,
    qr_payload TEXT NOT NULL UNIQUE,
    payload_json JSONB NOT NULL,
    generated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_mini_training_progress_batches_order_id
    ON mini_training_progress_batches (order_id, generated_at DESC);

