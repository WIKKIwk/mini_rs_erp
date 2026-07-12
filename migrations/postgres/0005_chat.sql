CREATE TABLE IF NOT EXISTS mini_chat_principals (
    principal_id TEXT PRIMARY KEY,
    principal_role TEXT NOT NULL,
    principal_ref TEXT NOT NULL,
    display_name TEXT NOT NULL,
    avatar_url TEXT NOT NULL DEFAULT '',
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_chat_principals_identity_unique
        UNIQUE (principal_role, principal_ref),
    CONSTRAINT mini_chat_principals_role_not_blank
        CHECK (btrim(principal_role) <> ''),
    CONSTRAINT mini_chat_principals_ref_not_blank
        CHECK (btrim(principal_ref) <> ''),
    CONSTRAINT mini_chat_principals_name_not_blank
        CHECK (btrim(display_name) <> '')
);

CREATE TABLE IF NOT EXISTS mini_chat_conversations (
    conversation_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    dm_key TEXT,
    created_by_principal_id TEXT NOT NULL
        REFERENCES mini_chat_principals(principal_id),
    last_message_sequence BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_chat_conversations_kind_valid
        CHECK (kind IN ('dm', 'group')),
    CONSTRAINT mini_chat_conversations_sequence_non_negative
        CHECK (last_message_sequence >= 0),
    CONSTRAINT mini_chat_conversations_dm_key_shape
        CHECK ((kind = 'dm' AND btrim(COALESCE(dm_key, '')) <> '') OR kind = 'group')
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_chat_conversations_dm_key_unique
    ON mini_chat_conversations(dm_key)
    WHERE dm_key IS NOT NULL;

CREATE TABLE IF NOT EXISTS mini_chat_conversation_members (
    conversation_id TEXT NOT NULL
        REFERENCES mini_chat_conversations(conversation_id) ON DELETE CASCADE,
    principal_id TEXT NOT NULL
        REFERENCES mini_chat_principals(principal_id),
    member_role TEXT NOT NULL DEFAULT 'member',
    can_post BOOLEAN NOT NULL DEFAULT TRUE,
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    left_at TIMESTAMPTZ,
    last_read_sequence BIGINT NOT NULL DEFAULT 0,
    muted_until TIMESTAMPTZ,
    PRIMARY KEY (conversation_id, principal_id),
    CONSTRAINT mini_chat_members_role_valid
        CHECK (member_role IN ('owner', 'admin', 'member')),
    CONSTRAINT mini_chat_members_read_non_negative
        CHECK (last_read_sequence >= 0)
);

CREATE TABLE IF NOT EXISTS mini_chat_messages (
    message_id TEXT NOT NULL UNIQUE,
    conversation_id TEXT NOT NULL
        REFERENCES mini_chat_conversations(conversation_id) ON DELETE CASCADE,
    sender_principal_id TEXT NOT NULL
        REFERENCES mini_chat_principals(principal_id),
    client_message_id TEXT NOT NULL,
    message_sequence BIGINT NOT NULL,
    message_type TEXT NOT NULL DEFAULT 'text',
    body TEXT NOT NULL,
    reply_to_message_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    edited_at TIMESTAMPTZ,
    deleted_at TIMESTAMPTZ,
    metadata_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    PRIMARY KEY (conversation_id, message_sequence),
    CONSTRAINT mini_chat_messages_client_id_unique
        UNIQUE (conversation_id, sender_principal_id, client_message_id),
    CONSTRAINT mini_chat_messages_sequence_positive
        CHECK (message_sequence > 0),
    CONSTRAINT mini_chat_messages_type_valid
        CHECK (message_type IN ('text', 'system', 'reply', 'edit', 'delete_tombstone')),
    CONSTRAINT mini_chat_messages_body_size
        CHECK (char_length(body) BETWEEN 1 AND 4000)
);

CREATE INDEX IF NOT EXISTS idx_mini_chat_messages_created
    ON mini_chat_messages(conversation_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_chat_members_principal
    ON mini_chat_conversation_members(principal_id, conversation_id)
    WHERE left_at IS NULL;

CREATE TABLE IF NOT EXISTS mini_chat_device_cursors (
    principal_id TEXT NOT NULL
        REFERENCES mini_chat_principals(principal_id) ON DELETE CASCADE,
    device_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL
        REFERENCES mini_chat_conversations(conversation_id) ON DELETE CASCADE,
    last_delivered_sequence BIGINT NOT NULL DEFAULT 0,
    last_read_sequence BIGINT NOT NULL DEFAULT 0,
    last_sync_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (principal_id, device_id, conversation_id),
    CONSTRAINT mini_chat_device_delivered_non_negative
        CHECK (last_delivered_sequence >= 0),
    CONSTRAINT mini_chat_device_read_non_negative
        CHECK (last_read_sequence >= 0)
);

CREATE TABLE IF NOT EXISTS mini_chat_outbox_events (
    event_id TEXT PRIMARY KEY,
    topic TEXT NOT NULL,
    conversation_id TEXT NOT NULL
        REFERENCES mini_chat_conversations(conversation_id) ON DELETE CASCADE,
    message_sequence BIGINT NOT NULL,
    recipient_keys JSONB NOT NULL,
    payload_json JSONB NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    locked_until TIMESTAMPTZ,
    published_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_chat_outbox_topic_not_blank CHECK (btrim(topic) <> ''),
    CONSTRAINT mini_chat_outbox_recipients_array
        CHECK (jsonb_typeof(recipient_keys) = 'array')
);

CREATE INDEX IF NOT EXISTS idx_mini_chat_outbox_pending
    ON mini_chat_outbox_events(created_at)
    WHERE published_at IS NULL;
