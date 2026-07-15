CREATE TABLE IF NOT EXISTS mini_returned_paint_images (
    image_id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL,
    apparatus TEXT NOT NULL,
    owner_ref TEXT NOT NULL,
    image_name TEXT NOT NULL,
    image_mime TEXT NOT NULL,
    image_size_bytes BIGINT NOT NULL,
    body BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_returned_paint_images_order_id_not_blank
        CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_returned_paint_images_apparatus_not_blank
        CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_returned_paint_images_owner_ref_not_blank
        CHECK (btrim(owner_ref) <> ''),
    CONSTRAINT mini_returned_paint_images_name_not_blank
        CHECK (btrim(image_name) <> ''),
    CONSTRAINT mini_returned_paint_images_mime_allowed
        CHECK (image_mime IN (
            'image/jpeg',
            'image/png',
            'image/webp',
            'image/heic',
            'image/heif'
        )),
    CONSTRAINT mini_returned_paint_images_size_valid
        CHECK (image_size_bytes > 0 AND image_size_bytes <= 6291456),
    CONSTRAINT mini_returned_paint_images_body_not_empty
        CHECK (octet_length(body) > 0 AND octet_length(body) <= 6291456),
    CONSTRAINT mini_returned_paint_images_size_matches_body
        CHECK (image_size_bytes = octet_length(body))
);

CREATE INDEX IF NOT EXISTS idx_mini_returned_paint_images_owner_order
    ON mini_returned_paint_images (owner_ref, order_id, apparatus, created_at DESC);

ALTER TABLE mini_returned_paint_requests
    ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'completed',
    ADD COLUMN IF NOT EXISTS image_id TEXT;

ALTER TABLE mini_returned_paint_requests
    DROP CONSTRAINT IF EXISTS mini_returned_paint_requests_items_not_empty,
    DROP CONSTRAINT IF EXISTS mini_returned_paint_requests_status_allowed,
    ADD CONSTRAINT mini_returned_paint_requests_status_allowed CHECK (
        status IN ('waiting_for_boyoqchi_input', 'completed')
    ),
    DROP CONSTRAINT IF EXISTS mini_returned_paint_requests_image_fk,
    ADD CONSTRAINT mini_returned_paint_requests_image_fk
        FOREIGN KEY (image_id)
        REFERENCES mini_returned_paint_images (image_id)
        ON DELETE RESTRICT,
    DROP CONSTRAINT IF EXISTS mini_returned_paint_requests_workflow_consistent,
    ADD CONSTRAINT mini_returned_paint_requests_workflow_consistent CHECK (
        (
            status = 'waiting_for_boyoqchi_input'
            AND image_id IS NOT NULL
            AND jsonb_array_length(items_json) = 0
            AND rasxot_mix_total IS NULL
            AND astatka_mix_total IS NULL
            AND rasxot_alcohol IS NULL
            AND astatka_alcohol IS NULL
            AND final_used_alcohol IS NULL
            AND rasxot_pure_paint IS NULL
            AND astatka_pure_paint IS NULL
            AND final_used_paint IS NULL
        )
        OR
        (
            status = 'completed'
            AND jsonb_array_length(items_json) > 0
            AND rasxot_mix_total IS NOT NULL
            AND astatka_mix_total IS NOT NULL
            AND rasxot_alcohol IS NOT NULL
            AND astatka_alcohol IS NOT NULL
            AND final_used_alcohol IS NOT NULL
            AND rasxot_pure_paint IS NOT NULL
            AND astatka_pure_paint IS NOT NULL
            AND final_used_paint IS NOT NULL
        )
    ) NOT VALID;

CREATE UNIQUE INDEX IF NOT EXISTS uq_mini_returned_paint_requests_image
    ON mini_returned_paint_requests (image_id)
    WHERE image_id IS NOT NULL;
