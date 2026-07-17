CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_chat_messages_id_conversation_unique
    ON mini_chat_messages(message_id, conversation_id);

CREATE TABLE IF NOT EXISTS mini_chat_media (
    media_id TEXT PRIMARY KEY,
    upload_id TEXT NOT NULL UNIQUE,
    conversation_id TEXT NOT NULL
        REFERENCES mini_chat_conversations(conversation_id) ON DELETE CASCADE,
    uploader_principal_id TEXT NOT NULL
        REFERENCES mini_chat_principals(principal_id),
    client_upload_id TEXT NOT NULL,
    media_kind TEXT NOT NULL,
    upload_status TEXT NOT NULL DEFAULT 'pending',
    original_filename TEXT NOT NULL,
    declared_content_type TEXT NOT NULL,
    declared_size_bytes BIGINT NOT NULL,
    declared_duration_ms BIGINT,
    source_object_key TEXT NOT NULL UNIQUE,
    actual_size_bytes BIGINT,
    storage_etag TEXT,
    detected_content_type TEXT,
    processed_object_key TEXT,
    thumbnail_object_key TEXT,
    width_pixels INTEGER,
    height_pixels INTEGER,
    duration_ms BIGINT,
    error_code TEXT,
    expires_at TIMESTAMPTZ NOT NULL,
    cleanup_locked_until TIMESTAMPTZ,
    cleaned_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_chat_media_client_upload_unique
        UNIQUE (conversation_id, uploader_principal_id, client_upload_id),
    CONSTRAINT mini_chat_media_id_conversation_unique
        UNIQUE (media_id, conversation_id),
    CONSTRAINT mini_chat_media_kind_valid
        CHECK (media_kind IN ('image', 'video')),
    CONSTRAINT mini_chat_media_status_valid
        CHECK (upload_status IN ('pending', 'uploaded', 'processing', 'ready', 'failed', 'cancelled')),
    CONSTRAINT mini_chat_media_filename_not_blank
        CHECK (btrim(original_filename) <> ''),
    CONSTRAINT mini_chat_media_content_type_not_blank
        CHECK (btrim(declared_content_type) <> ''),
    CONSTRAINT mini_chat_media_declared_size_positive
        CHECK (declared_size_bytes > 0),
    CONSTRAINT mini_chat_media_declared_duration_valid
        CHECK (declared_duration_ms IS NULL OR declared_duration_ms BETWEEN 1 AND 120000),
    CONSTRAINT mini_chat_media_source_key_not_blank
        CHECK (btrim(source_object_key) <> ''),
    CONSTRAINT mini_chat_media_actual_size_positive
        CHECK (actual_size_bytes IS NULL OR actual_size_bytes > 0),
    CONSTRAINT mini_chat_media_dimensions_positive
        CHECK (
            (width_pixels IS NULL AND height_pixels IS NULL)
            OR (width_pixels > 0 AND height_pixels > 0)
        ),
    CONSTRAINT mini_chat_media_duration_positive
        CHECK (duration_ms IS NULL OR duration_ms > 0),
    CONSTRAINT mini_chat_media_expiry_after_creation
        CHECK (expires_at > created_at)
);

CREATE INDEX IF NOT EXISTS idx_mini_chat_media_conversation_created
    ON mini_chat_media(conversation_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_chat_media_orphan_cleanup
    ON mini_chat_media(expires_at, cleanup_locked_until)
    WHERE cleaned_at IS NULL
      AND upload_status IN ('pending', 'uploaded', 'failed', 'cancelled');

CREATE TABLE IF NOT EXISTS mini_chat_message_attachments (
    attachment_id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL UNIQUE,
    conversation_id TEXT NOT NULL,
    media_id TEXT NOT NULL UNIQUE,
    ordinal SMALLINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_chat_message_attachments_message_fkey
        FOREIGN KEY (message_id, conversation_id)
        REFERENCES mini_chat_messages(message_id, conversation_id) ON DELETE CASCADE,
    CONSTRAINT mini_chat_message_attachments_media_fkey
        FOREIGN KEY (media_id, conversation_id)
        REFERENCES mini_chat_media(media_id, conversation_id) ON DELETE RESTRICT,
    CONSTRAINT mini_chat_message_attachments_one_per_message
        CHECK (ordinal = 0)
);

CREATE TABLE IF NOT EXISTS mini_chat_media_jobs (
    job_id TEXT PRIMARY KEY,
    media_id TEXT NOT NULL UNIQUE
        REFERENCES mini_chat_media(media_id) ON DELETE CASCADE,
    job_type TEXT NOT NULL,
    job_status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 5,
    available_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    locked_until TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_chat_media_jobs_type_valid
        CHECK (job_type IN ('canonicalize_image', 'canonicalize_video')),
    CONSTRAINT mini_chat_media_jobs_status_valid
        CHECK (job_status IN ('pending', 'running', 'succeeded', 'failed', 'cancelled')),
    CONSTRAINT mini_chat_media_jobs_attempts_valid
        CHECK (attempts >= 0 AND max_attempts > 0 AND attempts <= max_attempts)
);

CREATE INDEX IF NOT EXISTS idx_mini_chat_media_jobs_available
    ON mini_chat_media_jobs(available_at, created_at)
    WHERE job_status = 'pending';
