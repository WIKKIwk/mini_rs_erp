CREATE SEQUENCE IF NOT EXISTS mini_chat_event_cursor_seq;

ALTER TABLE mini_chat_outbox_events
    ADD COLUMN IF NOT EXISTS event_cursor BIGINT;

UPDATE mini_chat_outbox_events
SET event_cursor = nextval('mini_chat_event_cursor_seq')
WHERE event_cursor IS NULL;

SELECT setval(
    'mini_chat_event_cursor_seq',
    GREATEST(
        COALESCE((SELECT MAX(event_cursor) FROM mini_chat_outbox_events), 0),
        1
    ),
    TRUE
);

ALTER TABLE mini_chat_outbox_events
    ALTER COLUMN event_cursor SET DEFAULT nextval('mini_chat_event_cursor_seq'),
    ALTER COLUMN event_cursor SET NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_chat_outbox_event_cursor
    ON mini_chat_outbox_events(event_cursor);

-- The single-row clock is deliberately updated inside the message transaction.
-- PostgreSQL sequences can be observed out of commit order, which is unsafe for
-- a durable sync cursor: a client could advance past an event that commits late.
CREATE TABLE IF NOT EXISTS mini_chat_event_clock (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    cursor BIGINT NOT NULL CHECK (cursor >= 0)
);

INSERT INTO mini_chat_event_clock (singleton, cursor)
SELECT TRUE, COALESCE(MAX(event_cursor), 0)
FROM mini_chat_outbox_events
ON CONFLICT (singleton) DO UPDATE
SET cursor = GREATEST(mini_chat_event_clock.cursor, excluded.cursor);

ALTER TABLE mini_chat_outbox_events
    ALTER COLUMN event_cursor DROP DEFAULT;

ALTER TABLE mini_chat_outbox_events
    ADD COLUMN IF NOT EXISTS push_recipient_keys JSONB NOT NULL DEFAULT '[]'::JSONB;

UPDATE mini_chat_outbox_events
SET push_recipient_keys = recipient_keys
WHERE push_recipient_keys = '[]'::JSONB
  AND published_at IS NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'mini_chat_outbox_push_recipients_array'
    ) THEN
        ALTER TABLE mini_chat_outbox_events
            ADD CONSTRAINT mini_chat_outbox_push_recipients_array
            CHECK (jsonb_typeof(push_recipient_keys) = 'array');
    END IF;
END
$$;

CREATE TABLE IF NOT EXISTS mini_chat_push_deliveries (
    event_id TEXT NOT NULL
        REFERENCES mini_chat_outbox_events(event_id) ON DELETE CASCADE,
    recipient_key TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    locked_until TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,
    dead_lettered_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (event_id, recipient_key),
    CONSTRAINT mini_chat_push_recipient_not_blank CHECK (btrim(recipient_key) <> ''),
    CONSTRAINT mini_chat_push_attempts_non_negative CHECK (attempts >= 0)
);

INSERT INTO mini_chat_push_deliveries (event_id, recipient_key)
SELECT event.event_id, recipient.value
FROM mini_chat_outbox_events event
CROSS JOIN LATERAL jsonb_array_elements_text(event.push_recipient_keys) recipient(value)
WHERE event.published_at IS NULL
ON CONFLICT (event_id, recipient_key) DO NOTHING;

CREATE INDEX IF NOT EXISTS idx_mini_chat_push_deliveries_pending
    ON mini_chat_push_deliveries(next_attempt_at, created_at)
    WHERE delivered_at IS NULL AND dead_lettered_at IS NULL;

CREATE TABLE IF NOT EXISTS mini_chat_socket_tickets (
    ticket_hash BYTEA PRIMARY KEY,
    principal_role TEXT NOT NULL,
    principal_ref TEXT NOT NULL,
    display_name TEXT NOT NULL,
    legal_name TEXT NOT NULL,
    phone TEXT NOT NULL,
    avatar_url TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_mini_chat_socket_tickets_expiry
    ON mini_chat_socket_tickets(expires_at);

CREATE TABLE IF NOT EXISTS mini_chat_media_access_tickets (
    ticket_hash BYTEA PRIMARY KEY,
    media_id TEXT NOT NULL
        REFERENCES mini_chat_media(media_id) ON DELETE CASCADE,
    principal_id TEXT NOT NULL
        REFERENCES mini_chat_principals(principal_id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_mini_chat_media_access_tickets_expiry
    ON mini_chat_media_access_tickets(expires_at);
