ALTER TABLE mini_system_users
    DROP CONSTRAINT IF EXISTS mini_system_users_role_allowed;

ALTER TABLE mini_system_users
    ADD CONSTRAINT mini_system_users_role_allowed
    CHECK (role IN ('qolipchi', 'boyoqchi'));

CREATE TABLE IF NOT EXISTS mini_returned_paint_requests (
    id TEXT PRIMARY KEY,
    target_role TEXT NOT NULL DEFAULT 'boyoqchi',
    order_id TEXT NOT NULL,
    order_code TEXT NOT NULL DEFAULT '',
    order_name TEXT NOT NULL DEFAULT '',
    apparatus TEXT NOT NULL,
    sender_role TEXT NOT NULL,
    sender_ref TEXT NOT NULL,
    sender_display_name TEXT NOT NULL,
    items_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_returned_paint_requests_target_role_allowed
        CHECK (target_role = 'boyoqchi'),
    CONSTRAINT mini_returned_paint_requests_order_id_not_blank
        CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_returned_paint_requests_apparatus_not_blank
        CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_returned_paint_requests_sender_role_not_blank
        CHECK (btrim(sender_role) <> ''),
    CONSTRAINT mini_returned_paint_requests_sender_ref_not_blank
        CHECK (btrim(sender_ref) <> ''),
    CONSTRAINT mini_returned_paint_requests_sender_name_not_blank
        CHECK (btrim(sender_display_name) <> ''),
    CONSTRAINT mini_returned_paint_requests_items_array
        CHECK (jsonb_typeof(items_json) = 'array'),
    CONSTRAINT mini_returned_paint_requests_items_not_empty
        CHECK (jsonb_array_length(items_json) > 0)
);

CREATE INDEX IF NOT EXISTS idx_mini_returned_paint_requests_target_created
    ON mini_returned_paint_requests (target_role, created_at DESC, id DESC);
