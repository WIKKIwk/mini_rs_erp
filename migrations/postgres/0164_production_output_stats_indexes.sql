-- Reporting only: no output/session/warehouse data or write semantics change.
SET LOCAL lock_timeout = '2s';
SET LOCAL statement_timeout = '30s';

CREATE INDEX IF NOT EXISTS idx_mini_output_stats_time_producer
    ON mini_progress_batches (production_completed_at, canonical_apparatus_id)
    WHERE COALESCE(payload_json->>'origin', '') <> 'layer_remainder'
      AND order_id NOT LIKE 'training-%' AND order_id NOT LIKE 'demo-%';

-- Unknown times cannot be filtered into a date range. Keep the coverage audit
-- scoped and indexed rather than scanning all historical output payloads.
CREATE INDEX IF NOT EXISTS idx_mini_output_stats_unknown_time_producer
    ON mini_progress_batches (canonical_apparatus_id)
    WHERE production_completed_at IS NULL OR NOT isfinite(production_completed_at)
       OR production_completed_at <= to_timestamp(0)
       OR payload_json #>> '{production_timing_source,completion}' = 'unknown';
