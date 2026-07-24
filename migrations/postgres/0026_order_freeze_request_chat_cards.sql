CREATE TABLE IF NOT EXISTS mini_order_freeze_requests (
    request_id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL
        REFERENCES mini_production_maps(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    requester_role TEXT NOT NULL,
    requester_ref TEXT NOT NULL,
    requester_display_name TEXT NOT NULL,
    target_session_id TEXT NOT NULL DEFAULT '',
    target_apparatus TEXT NOT NULL DEFAULT '',
    target_worker_role TEXT NOT NULL DEFAULT '',
    target_worker_ref TEXT NOT NULL DEFAULT '',
    target_worker_display_name TEXT NOT NULL DEFAULT '',
    requested_at_unix BIGINT NOT NULL,
    transitioned_at_unix BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_order_freeze_requests_status_allowed
        CHECK (status IN ('pending', 'frozen', 'cancelled', 'unfrozen')),
    CONSTRAINT mini_order_freeze_requests_target_complete CHECK (
        (target_session_id = '' AND target_apparatus = ''
            AND target_worker_role = '' AND target_worker_ref = ''
            AND target_worker_display_name = '')
        OR
        (target_session_id <> '' AND target_apparatus <> ''
            AND target_worker_role <> '' AND target_worker_ref <> '')
    )
);

ALTER TABLE mini_order_control_states
    ADD COLUMN IF NOT EXISTS freeze_request_id TEXT
        REFERENCES mini_order_freeze_requests(request_id);

CREATE INDEX IF NOT EXISTS idx_mini_order_freeze_requests_order
    ON mini_order_freeze_requests(order_id, requested_at_unix DESC);

CREATE TABLE IF NOT EXISTS mini_order_freeze_chat_outbox (
    event_sequence BIGSERIAL PRIMARY KEY,
    event_id TEXT NOT NULL UNIQUE,
    request_id TEXT NOT NULL
        REFERENCES mini_order_freeze_requests(request_id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    locked_until TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,
    last_error TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_order_freeze_chat_outbox_status_allowed
        CHECK (status IN ('pending', 'frozen', 'cancelled', 'unfrozen'))
);

CREATE INDEX IF NOT EXISTS idx_mini_order_freeze_chat_outbox_pending
    ON mini_order_freeze_chat_outbox(event_sequence)
    WHERE delivered_at IS NULL;

ALTER TABLE mini_chat_messages
    DROP CONSTRAINT IF EXISTS mini_chat_messages_type_valid;
ALTER TABLE mini_chat_messages
    ADD CONSTRAINT mini_chat_messages_type_valid
        CHECK (message_type IN (
            'text', 'image', 'video', 'audio', 'system', 'reply', 'edit',
            'delete_tombstone', 'order_freeze_request'
        ));
