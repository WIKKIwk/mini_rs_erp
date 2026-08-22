-- Canonical apparatus FK support indexes.
--
-- Migrations 0063-0067 add and validate the typed canonical references.  The
-- references introduced by 0065 below did not all receive child-side indexes;
-- without these indexes, deleting or updating a master row requires a scan of
-- the referencing table.  This follow-up is schema-only and does not rewrite
-- or remove data.

CREATE INDEX IF NOT EXISTS idx_mini_apparatus_order_transfers_canonical_to
    ON mini_apparatus_order_transfers (canonical_to_apparatus_id);

CREATE INDEX IF NOT EXISTS idx_mini_production_map_nodes_canonical_apparatus
    ON mini_production_map_nodes (canonical_apparatus_id);
CREATE INDEX IF NOT EXISTS idx_mini_production_map_nodes_canonical_alternative
    ON mini_production_map_nodes (canonical_alternative_apparatus_id);

CREATE INDEX IF NOT EXISTS idx_mini_progress_batches_canonical_apparatus
    ON mini_progress_batches (canonical_apparatus_id);
CREATE INDEX IF NOT EXISTS idx_mini_progress_batches_canonical_current_apparatus
    ON mini_progress_batches (canonical_current_apparatus_id);
CREATE INDEX IF NOT EXISTS idx_mini_progress_batches_canonical_next_apparatus
    ON mini_progress_batches (canonical_next_apparatus_id);
CREATE INDEX IF NOT EXISTS idx_mini_progress_batches_canonical_used_by_apparatus
    ON mini_progress_batches (canonical_used_by_apparatus_id);
CREATE INDEX IF NOT EXISTS idx_mini_progress_batches_canonical_processed_by_apparatus
    ON mini_progress_batches (canonical_processed_by_apparatus_id);

CREATE INDEX IF NOT EXISTS idx_mini_training_queue_events_canonical_apparatus
    ON mini_training_queue_events (canonical_apparatus_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_training_raw_assignments_canonical_apparatus
    ON mini_training_raw_material_assignments (canonical_apparatus_id, order_id);
CREATE INDEX IF NOT EXISTS idx_mini_training_input_batches_canonical_apparatus
    ON mini_training_input_batches (canonical_apparatus_id, generated_at DESC);

CREATE INDEX IF NOT EXISTS idx_mini_laminatsiya_astatka_canonical_apparatus
    ON mini_laminatsiya_astatka_reports
        (canonical_apparatus_id, to_at DESC, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_rezka_astatka_canonical_apparatus
    ON mini_rezka_astatka_reports
        (canonical_apparatus_id, to_at DESC, created_at DESC);
