-- One decision shared by the mover's and administrator's chat cards.
ALTER TABLE mini_chat_messages DROP CONSTRAINT mini_chat_messages_type_valid;
ALTER TABLE mini_chat_messages ADD CONSTRAINT mini_chat_messages_type_valid
    CHECK (message_type IN ('text', 'image', 'video', 'audio', 'system', 'reply', 'edit',
        'delete_tombstone', 'order_freeze_request', 'inventory_transfer_request', 'material_link_request'));

CREATE TABLE mini_material_link_requests (
    id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL,
    apparatus_id TEXT NOT NULL REFERENCES mini_apparatus(id),
    requester_ref TEXT NOT NULL,
    mover_ref TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'cancelled', 'stale', 'expired')),
    revision BIGINT NOT NULL DEFAULT 1,
    delivered_revision BIGINT NOT NULL DEFAULT 0,
    payload_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL DEFAULT now() + interval '30 minutes',
    retry_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX mini_material_link_pending_scope
    ON mini_material_link_requests(order_id, apparatus_id, requester_ref, mover_ref)
    WHERE status = 'pending';
CREATE INDEX mini_material_link_delivery ON mini_material_link_requests(retry_at)
    WHERE delivered_revision < revision;
CREATE INDEX mini_material_link_order ON mini_material_link_requests(order_id, apparatus_id);
