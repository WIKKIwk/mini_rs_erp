CREATE TABLE IF NOT EXISTS mini_training_queue_states (
    apparatus TEXT NOT NULL,
    order_id TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (apparatus, order_id)
);

CREATE INDEX IF NOT EXISTS idx_mini_training_queue_states_apparatus
    ON mini_training_queue_states (lower(apparatus), updated_at DESC);
