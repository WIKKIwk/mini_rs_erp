ALTER TABLE mini_chat_media
    DROP CONSTRAINT IF EXISTS mini_chat_media_declared_duration_valid;
ALTER TABLE mini_chat_media
    ADD CONSTRAINT mini_chat_media_declared_duration_valid
        CHECK (declared_duration_ms IS NULL OR declared_duration_ms BETWEEN 1 AND 600000);

ALTER TABLE mini_chat_media
    ADD COLUMN IF NOT EXISTS upload_mode TEXT NOT NULL DEFAULT 'single',
    ADD COLUMN IF NOT EXISTS chunk_size_bytes BIGINT,
    ADD COLUMN IF NOT EXISTS total_chunks INTEGER,
    ADD COLUMN IF NOT EXISTS storage_multipart_upload_id TEXT,
    ADD COLUMN IF NOT EXISTS frame_rate_milli INTEGER,
    ADD COLUMN IF NOT EXISTS video_codec TEXT,
    ADD COLUMN IF NOT EXISTS audio_codec TEXT;

ALTER TABLE mini_chat_media
    DROP CONSTRAINT IF EXISTS mini_chat_media_source_size_limit;
ALTER TABLE mini_chat_media
    ADD CONSTRAINT mini_chat_media_source_size_limit CHECK (
        (media_kind = 'image' AND declared_size_bytes <= 15728640)
        OR
        (media_kind = 'video' AND declared_size_bytes <= 2147483648)
    );

ALTER TABLE mini_chat_media
    DROP CONSTRAINT IF EXISTS mini_chat_media_processed_size_limit;
ALTER TABLE mini_chat_media
    ADD CONSTRAINT mini_chat_media_processed_size_limit
        CHECK (processed_size_bytes IS NULL OR processed_size_bytes <= 1073741824);

ALTER TABLE mini_chat_media
    DROP CONSTRAINT IF EXISTS mini_chat_media_upload_mode_valid;
ALTER TABLE mini_chat_media
    ADD CONSTRAINT mini_chat_media_upload_mode_valid
        CHECK (upload_mode IN ('single', 'chunked'));

ALTER TABLE mini_chat_media
    DROP CONSTRAINT IF EXISTS mini_chat_media_chunk_configuration_valid;
ALTER TABLE mini_chat_media
    ADD CONSTRAINT mini_chat_media_chunk_configuration_valid CHECK (
        (upload_mode = 'single' AND chunk_size_bytes IS NULL AND total_chunks IS NULL)
        OR
        (
            upload_mode = 'chunked'
            AND media_kind = 'video'
            AND chunk_size_bytes BETWEEN 5242880 AND 67108864
            AND total_chunks > 0
        )
    );

ALTER TABLE mini_chat_media
    DROP CONSTRAINT IF EXISTS mini_chat_media_video_metadata_valid;
ALTER TABLE mini_chat_media
    ADD CONSTRAINT mini_chat_media_video_metadata_valid CHECK (
        (media_kind = 'image' AND frame_rate_milli IS NULL AND video_codec IS NULL AND audio_codec IS NULL)
        OR
        (
            media_kind = 'video'
            AND (frame_rate_milli IS NULL OR frame_rate_milli BETWEEN 1 AND 60000)
        )
    );

CREATE TABLE IF NOT EXISTS mini_chat_media_upload_chunks (
    media_id TEXT NOT NULL
        REFERENCES mini_chat_media(media_id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    offset_bytes BIGINT NOT NULL,
    size_bytes BIGINT NOT NULL,
    storage_part_etag TEXT NOT NULL,
    uploaded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (media_id, chunk_index),
    CONSTRAINT mini_chat_media_chunk_index_valid CHECK (chunk_index >= 0),
    CONSTRAINT mini_chat_media_chunk_offset_valid CHECK (offset_bytes >= 0),
    CONSTRAINT mini_chat_media_chunk_size_valid CHECK (size_bytes > 0),
    CONSTRAINT mini_chat_media_chunk_etag_not_blank CHECK (btrim(storage_part_etag) <> '')
);

CREATE INDEX IF NOT EXISTS idx_mini_chat_media_chunks_uploaded
    ON mini_chat_media_upload_chunks(media_id, uploaded_at);
