-- Frozen orders are open work that must not occupy an active queue slot.
-- Keep the legacy paused state for ordinary pause/handoff flows, while
-- allowing the explicit frozen transition and frozen run-session status.

ALTER TABLE mini_queue_states
    DROP CONSTRAINT IF EXISTS mini_queue_states_state_allowed;
ALTER TABLE mini_queue_states
    ADD CONSTRAINT mini_queue_states_state_allowed
    CHECK (state IN ('pending', 'in_progress', 'paused', 'frozen', 'completed'));

ALTER TABLE mini_queue_action_events
    DROP CONSTRAINT IF EXISTS mini_queue_action_events_action_allowed;
ALTER TABLE mini_queue_action_events
    DROP CONSTRAINT IF EXISTS mini_queue_action_events_from_state_allowed;
ALTER TABLE mini_queue_action_events
    DROP CONSTRAINT IF EXISTS mini_queue_action_events_to_state_allowed;
ALTER TABLE mini_queue_action_events
    ADD CONSTRAINT mini_queue_action_events_action_allowed
    CHECK (action IN ('start', 'pause', 'freeze', 'detach_roll', 'resume', 'roll_complete', 'complete'));
ALTER TABLE mini_queue_action_events
    ADD CONSTRAINT mini_queue_action_events_from_state_allowed
    CHECK (from_state IN ('pending', 'in_progress', 'paused', 'frozen', 'completed'));
ALTER TABLE mini_queue_action_events
    ADD CONSTRAINT mini_queue_action_events_to_state_allowed
    CHECK (to_state IN ('pending', 'in_progress', 'paused', 'frozen', 'completed'));

ALTER TABLE mini_order_run_sessions
    DROP CONSTRAINT IF EXISTS mini_order_run_sessions_status_allowed;
ALTER TABLE mini_order_run_sessions
    ADD CONSTRAINT mini_order_run_sessions_status_allowed
    CHECK (status IN ('active', 'paused', 'frozen', 'roll_detached', 'completed'));

ALTER TABLE mini_order_progress_events
    DROP CONSTRAINT IF EXISTS mini_order_progress_events_action_allowed;
ALTER TABLE mini_order_progress_events
    ADD CONSTRAINT mini_order_progress_events_action_allowed
    CHECK (action IN ('start', 'pause', 'freeze', 'detach_roll', 'resume', 'roll_complete', 'complete'));

DROP INDEX IF EXISTS idx_mini_order_run_sessions_one_open;
CREATE UNIQUE INDEX idx_mini_order_run_sessions_one_open
    ON mini_order_run_sessions(lower(apparatus), order_id)
    WHERE status IN ('active', 'paused', 'frozen', 'roll_detached');
