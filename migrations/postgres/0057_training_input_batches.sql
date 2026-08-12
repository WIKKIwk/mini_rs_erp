CREATE TABLE IF NOT EXISTS mini_training_input_batches (
    order_id TEXT PRIMARY KEY,
    apparatus TEXT NOT NULL,
    batch_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    qr_payload TEXT NOT NULL,
    generated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_training_input_batches_batch_id
    ON mini_training_input_batches (batch_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_training_input_batches_qr_payload
    ON mini_training_input_batches (qr_payload);
