ALTER TABLE mini_inventory_movement_events
    DROP CONSTRAINT IF EXISTS mini_inventory_movement_events_type_allowed;

ALTER TABLE mini_inventory_movement_events
    ADD CONSTRAINT mini_inventory_movement_events_type_allowed
    CHECK (
        event_type IN (
            'relocated', 'returned_to_warehouse', 'transfer_requested',
            'transfer_approved', 'transfer_rejected', 'transfer_dispatched',
            'transfer_received', 'transfer_cancelled'
        )
    );
