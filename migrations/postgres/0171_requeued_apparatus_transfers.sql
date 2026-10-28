-- A requeued session may have been frozen before it produced any WIP.
-- Preserve that absence instead of requiring a synthetic progress batch.
ALTER TABLE mini_apparatus_order_transfers
    ALTER COLUMN progress_batch_id DROP NOT NULL;
