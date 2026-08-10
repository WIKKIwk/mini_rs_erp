CREATE SEQUENCE IF NOT EXISTS mini_training_order_number_seq
    START WITH 1
    INCREMENT BY 1;

CREATE TABLE IF NOT EXISTS mini_training_production_maps (
    id TEXT PRIMARY KEY,
    order_number TEXT NOT NULL,
    map_json JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_training_production_maps_order_number
    ON mini_training_production_maps (order_number);

CREATE TABLE IF NOT EXISTS mini_training_quick_order_templates (
    id TEXT PRIMARY KEY,
    owner_key TEXT NOT NULL,
    code TEXT NOT NULL,
    payload_json JSONB NOT NULL,
    saved_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_mini_training_quick_order_templates_owner_saved
    ON mini_training_quick_order_templates (owner_key, saved_at DESC);

CREATE TABLE IF NOT EXISTS mini_training_raw_material_assignments (
    id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL,
    apparatus TEXT NOT NULL,
    barcode TEXT NOT NULL,
    payload_json JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_training_raw_assignments_identity
    ON mini_training_raw_material_assignments
        (order_id, lower(apparatus), lower(barcode));

CREATE INDEX IF NOT EXISTS idx_mini_training_raw_assignments_order
    ON mini_training_raw_material_assignments (order_id, apparatus);

CREATE TABLE IF NOT EXISTS mini_training_apparatus_modes (
    apparatus TEXT PRIMARY KEY,
    enabled BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS mini_training_order_images (
    owner_key TEXT NOT NULL,
    image_id TEXT NOT NULL,
    image_name TEXT NOT NULL,
    image_mime TEXT NOT NULL,
    image_size_bytes BIGINT NOT NULL,
    body BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (owner_key, image_id)
);
