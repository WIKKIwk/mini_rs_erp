CREATE OR REPLACE FUNCTION mini_chat_assign_event_cursor()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.event_cursor IS NULL THEN
        UPDATE mini_chat_event_clock
        SET cursor = cursor + 1
        WHERE singleton = TRUE
        RETURNING cursor INTO NEW.event_cursor;
    END IF;
    RETURN NEW;
END
$$;

DROP TRIGGER IF EXISTS trg_mini_chat_assign_event_cursor
    ON mini_chat_outbox_events;
CREATE TRIGGER trg_mini_chat_assign_event_cursor
BEFORE INSERT ON mini_chat_outbox_events
FOR EACH ROW
EXECUTE FUNCTION mini_chat_assign_event_cursor();

UPDATE mini_chat_outbox_events event
SET push_recipient_keys = COALESCE(
    (
        SELECT jsonb_agg(recipient.value)
        FROM jsonb_array_elements_text(event.recipient_keys) recipient(value)
        WHERE recipient.value <> concat(
            event.payload_json #>> '{message,sender_role}',
            ':',
            event.payload_json #>> '{message,sender_ref}'
        )
    ),
    '[]'::JSONB
)
WHERE event.published_at IS NULL;

DELETE FROM mini_chat_push_deliveries delivery
USING mini_chat_outbox_events event
WHERE delivery.event_id = event.event_id
  AND event.published_at IS NULL
  AND NOT (
      event.push_recipient_keys @> to_jsonb(ARRAY[delivery.recipient_key]::TEXT[])
  );

INSERT INTO mini_chat_push_deliveries (event_id, recipient_key)
SELECT event.event_id, recipient.value
FROM mini_chat_outbox_events event
CROSS JOIN LATERAL jsonb_array_elements_text(event.push_recipient_keys) recipient(value)
WHERE event.published_at IS NULL
ON CONFLICT (event_id, recipient_key) DO NOTHING;

UPDATE mini_chat_outbox_events event
SET published_at = now(), locked_until = NULL
WHERE event.published_at IS NULL
  AND NOT EXISTS (
      SELECT 1
      FROM mini_chat_push_deliveries delivery
      WHERE delivery.event_id = event.event_id
        AND delivery.delivered_at IS NULL
        AND delivery.dead_lettered_at IS NULL
  );
