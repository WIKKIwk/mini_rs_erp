ALTER TABLE mini_apparatus_schedule_reservations
    DROP CONSTRAINT IF EXISTS mini_apparatus_schedule_reservations_status_allowed;

ALTER TABLE mini_apparatus_schedule_reservations
    ADD CONSTRAINT mini_apparatus_schedule_reservations_status_allowed
    CHECK (status IN ('planned', 'active', 'paused', 'completed', 'cancelled'));
