-- Keep legacy `pause` records intact while allowing worker roll removal to be
-- persisted with its own canonical action and execution status.

ALTER TABLE mini_queue_action_events
    DROP CONSTRAINT IF EXISTS mini_queue_action_events_action_allowed;
ALTER TABLE mini_queue_action_events
    ADD CONSTRAINT mini_queue_action_events_action_allowed
    CHECK (action IN ('start', 'pause', 'detach_roll', 'resume', 'roll_complete', 'complete'));

ALTER TABLE mini_order_run_sessions
    DROP CONSTRAINT IF EXISTS mini_order_run_sessions_status_allowed;
ALTER TABLE mini_order_run_sessions
    ADD CONSTRAINT mini_order_run_sessions_status_allowed
    CHECK (status IN ('active', 'paused', 'roll_detached', 'completed'));

ALTER TABLE mini_order_progress_events
    DROP CONSTRAINT IF EXISTS mini_order_progress_events_action_allowed;
ALTER TABLE mini_order_progress_events
    ADD CONSTRAINT mini_order_progress_events_action_allowed
    CHECK (action IN ('start', 'pause', 'detach_roll', 'resume', 'roll_complete', 'complete'));

ALTER TABLE mini_progress_batches
    DROP CONSTRAINT IF EXISTS mini_progress_batches_action_allowed;
ALTER TABLE mini_progress_batches
    ADD CONSTRAINT mini_progress_batches_action_allowed
    CHECK (action IN ('pause', 'detach_roll', 'roll_complete', 'complete'));

ALTER TABLE mini_progress_batches
    DROP CONSTRAINT IF EXISTS mini_progress_batches_status_allowed;
ALTER TABLE mini_progress_batches
    ADD CONSTRAINT mini_progress_batches_status_allowed
    CHECK (status IN ('paused', 'roll_detached', 'completed', 'resumed'));

DROP INDEX IF EXISTS idx_mini_order_run_sessions_one_open;
CREATE UNIQUE INDEX idx_mini_order_run_sessions_one_open
    ON mini_order_run_sessions(lower(apparatus), order_id)
    WHERE status IN ('active', 'paused', 'roll_detached');
