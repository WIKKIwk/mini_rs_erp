SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- This is infrastructure state, recovered from durable events, not a new
-- business message. Never rewind a surviving clock after an import/restore.
INSERT INTO mini_chat_event_clock (singleton, cursor)
SELECT TRUE, COALESCE(MAX(event_cursor), 0) FROM mini_chat_outbox_events
ON CONFLICT (singleton) DO UPDATE
SET cursor = GREATEST(mini_chat_event_clock.cursor, EXCLUDED.cursor);

-- Older writers omit event_cursor. Keep them on the same transactional clock
-- as current writers, including when the singleton was removed by a restore.
CREATE OR REPLACE FUNCTION mini_chat_assign_event_cursor()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.event_cursor IS NULL THEN
        INSERT INTO mini_chat_event_clock (singleton, cursor)
        SELECT TRUE, COALESCE(MAX(event_cursor), 0) + 1 FROM mini_chat_outbox_events
        ON CONFLICT (singleton) DO UPDATE
        SET cursor = GREATEST(mini_chat_event_clock.cursor + 1, EXCLUDED.cursor)
        RETURNING cursor INTO NEW.event_cursor;
    END IF;
    RETURN NEW;
END;
$$;

-- A worker's own freeze transition needs no DM to themselves. Preserve the
-- original audit event and distinguish a deliberate skip from actual delivery.
ALTER TABLE mini_order_freeze_chat_outbox
    ADD COLUMN IF NOT EXISTS skipped_at TIMESTAMPTZ;
