ALTER TABLE mini_chat_media
    ADD COLUMN IF NOT EXISTS processed_content_type TEXT,
    ADD COLUMN IF NOT EXISTS processed_size_bytes BIGINT,
    ADD COLUMN IF NOT EXISTS processed_etag TEXT;

ALTER TABLE mini_chat_media
    DROP CONSTRAINT IF EXISTS mini_chat_media_processed_size_positive;
ALTER TABLE mini_chat_media
    ADD CONSTRAINT mini_chat_media_processed_size_positive
        CHECK (processed_size_bytes IS NULL OR processed_size_bytes > 0);

ALTER TABLE mini_chat_messages
    DROP CONSTRAINT IF EXISTS mini_chat_messages_type_valid;
ALTER TABLE mini_chat_messages
    ADD CONSTRAINT mini_chat_messages_type_valid
        CHECK (message_type IN (
            'text', 'image', 'video', 'system', 'reply', 'edit', 'delete_tombstone'
        ));

ALTER TABLE mini_chat_messages
    DROP CONSTRAINT IF EXISTS mini_chat_messages_body_size;
ALTER TABLE mini_chat_messages
    ADD CONSTRAINT mini_chat_messages_body_size CHECK (
        (message_type IN ('image', 'video') AND char_length(body) BETWEEN 0 AND 4000)
        OR
        (message_type NOT IN ('image', 'video') AND char_length(body) BETWEEN 1 AND 4000)
    );

CREATE INDEX IF NOT EXISTS idx_mini_chat_media_jobs_claim
    ON mini_chat_media_jobs(available_at, created_at)
    WHERE job_status = 'pending' AND attempts < max_attempts;

CREATE INDEX IF NOT EXISTS idx_mini_chat_attachments_media_conversation
    ON mini_chat_message_attachments(media_id, conversation_id);
