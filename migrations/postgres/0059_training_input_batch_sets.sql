ALTER TABLE mini_training_input_batches
    DROP CONSTRAINT IF EXISTS mini_training_input_batches_pkey;

ALTER TABLE mini_training_input_batches
    ADD PRIMARY KEY USING INDEX idx_mini_training_input_batches_batch_id;

CREATE INDEX IF NOT EXISTS idx_mini_training_input_batches_order_apparatus
    ON mini_training_input_batches (order_id, lower(apparatus), generated_at ASC);
