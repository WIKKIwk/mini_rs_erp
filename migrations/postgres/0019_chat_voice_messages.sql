ALTER TABLE mini_chat_media
    DROP CONSTRAINT IF EXISTS mini_chat_media_kind_valid;
ALTER TABLE mini_chat_media
    ADD CONSTRAINT mini_chat_media_kind_valid
        CHECK (media_kind IN ('image', 'video', 'audio'));

ALTER TABLE mini_chat_media
    DROP CONSTRAINT IF EXISTS mini_chat_media_source_size_limit;
ALTER TABLE mini_chat_media
    ADD CONSTRAINT mini_chat_media_source_size_limit CHECK (
        (media_kind = 'image' AND declared_size_bytes <= 15728640)
        OR
        (media_kind = 'video' AND declared_size_bytes <= 2147483648)
        OR
        (media_kind = 'audio' AND declared_size_bytes <= 67108864)
    );

ALTER TABLE mini_chat_media
    DROP CONSTRAINT IF EXISTS mini_chat_media_processed_size_limit;
ALTER TABLE mini_chat_media
    ADD CONSTRAINT mini_chat_media_processed_size_limit CHECK (
        processed_size_bytes IS NULL
        OR (media_kind IN ('image', 'video') AND processed_size_bytes <= 1073741824)
        OR (media_kind = 'audio' AND processed_size_bytes <= 67108864)
    );

ALTER TABLE mini_chat_media
    DROP CONSTRAINT IF EXISTS mini_chat_media_video_metadata_valid;
ALTER TABLE mini_chat_media
    ADD CONSTRAINT mini_chat_media_video_metadata_valid CHECK (
        (
            media_kind = 'image'
            AND frame_rate_milli IS NULL
            AND video_codec IS NULL
            AND audio_codec IS NULL
        )
        OR
        (
            media_kind = 'video'
            AND (frame_rate_milli IS NULL OR frame_rate_milli BETWEEN 1 AND 60000)
        )
        OR
        (
            media_kind = 'audio'
            AND frame_rate_milli IS NULL
            AND video_codec IS NULL
        )
    );

ALTER TABLE mini_chat_media_jobs
    DROP CONSTRAINT IF EXISTS mini_chat_media_jobs_type_valid;
ALTER TABLE mini_chat_media_jobs
    ADD CONSTRAINT mini_chat_media_jobs_type_valid
        CHECK (job_type IN (
            'canonicalize_image', 'canonicalize_video', 'canonicalize_audio'
        ));

ALTER TABLE mini_chat_messages
    DROP CONSTRAINT IF EXISTS mini_chat_messages_type_valid;
ALTER TABLE mini_chat_messages
    ADD CONSTRAINT mini_chat_messages_type_valid
        CHECK (message_type IN (
            'text', 'image', 'video', 'audio', 'system', 'reply', 'edit',
            'delete_tombstone'
        ));

ALTER TABLE mini_chat_messages
    DROP CONSTRAINT IF EXISTS mini_chat_messages_body_size;
ALTER TABLE mini_chat_messages
    ADD CONSTRAINT mini_chat_messages_body_size CHECK (
        (message_type IN ('image', 'video', 'audio') AND char_length(body) BETWEEN 0 AND 4000)
        OR
        (message_type NOT IN ('image', 'video', 'audio') AND char_length(body) BETWEEN 1 AND 4000)
    );
