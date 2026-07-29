CREATE TABLE IF NOT EXISTS mini_inventory_transfer_chat_outbox (
    event_sequence BIGSERIAL PRIMARY KEY,
    event_id TEXT NOT NULL UNIQUE,
    transfer_id TEXT NOT NULL
        REFERENCES mini_inventory_transfers(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    target_role TEXT NOT NULL,
    target_ref TEXT NOT NULL,
    target_display_name TEXT NOT NULL DEFAULT '',
    attempts INTEGER NOT NULL DEFAULT 0,
    locked_until TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,
    last_error TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_inventory_transfer_chat_outbox_status_allowed
        CHECK (status IN (
            'requested', 'approved', 'in_transit', 'received', 'rejected', 'cancelled'
        )),
    CONSTRAINT mini_inventory_transfer_chat_outbox_target_not_blank
        CHECK (btrim(target_role) <> '' AND btrim(target_ref) <> ''),
    CONSTRAINT mini_inventory_transfer_chat_outbox_transfer_target_status_unique
        UNIQUE (transfer_id, target_role, target_ref, status)
);

CREATE INDEX IF NOT EXISTS idx_mini_inventory_transfer_chat_outbox_pending
    ON mini_inventory_transfer_chat_outbox(event_sequence)
    WHERE delivered_at IS NULL;

ALTER TABLE mini_chat_messages
    DROP CONSTRAINT IF EXISTS mini_chat_messages_type_valid;
ALTER TABLE mini_chat_messages
    ADD CONSTRAINT mini_chat_messages_type_valid
        CHECK (message_type IN (
            'text', 'image', 'video', 'audio', 'system', 'reply', 'edit',
            'delete_tombstone', 'order_freeze_request', 'inventory_transfer_request'
        ));
