ALTER TABLE mini_queue_action_events
    DROP CONSTRAINT IF EXISTS mini_queue_action_events_action_allowed;
ALTER TABLE mini_queue_action_events
    ADD CONSTRAINT mini_queue_action_events_action_allowed
    CHECK (action IN (
        'start', 'pause', 'freeze', 'detach_roll', 'resume',
        'merge', 'roll_complete', 'complete'
    ));

ALTER TABLE mini_order_progress_events
    DROP CONSTRAINT IF EXISTS mini_order_progress_events_action_allowed;
ALTER TABLE mini_order_progress_events
    ADD CONSTRAINT mini_order_progress_events_action_allowed
    CHECK (action IN (
        'start', 'pause', 'freeze', 'detach_roll', 'resume',
        'merge', 'roll_complete', 'complete'
    ));
