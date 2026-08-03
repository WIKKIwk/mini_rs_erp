-- Rezka records every laminated input roll as a separate progress action.
-- The order queue remains in progress while roll_complete creates the
-- frame-level WIPs; complete remains reserved for the final input roll.

ALTER TABLE mini_queue_action_events
    DROP CONSTRAINT IF EXISTS mini_queue_action_events_action_allowed;
ALTER TABLE mini_queue_action_events
    ADD CONSTRAINT mini_queue_action_events_action_allowed
    CHECK (action IN ('start', 'pause', 'resume', 'roll_complete', 'complete'));

ALTER TABLE mini_order_progress_events
    DROP CONSTRAINT IF EXISTS mini_order_progress_events_action_allowed;
ALTER TABLE mini_order_progress_events
    ADD CONSTRAINT mini_order_progress_events_action_allowed
    CHECK (action IN ('start', 'pause', 'resume', 'roll_complete', 'complete'));

ALTER TABLE mini_progress_batches
    DROP CONSTRAINT IF EXISTS mini_progress_batches_action_allowed;
ALTER TABLE mini_progress_batches
    ADD CONSTRAINT mini_progress_batches_action_allowed
    CHECK (action IN ('pause', 'roll_complete', 'complete'));
