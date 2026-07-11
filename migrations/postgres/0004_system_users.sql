CREATE TABLE IF NOT EXISTS mini_system_users (
    id TEXT PRIMARY KEY,
    role TEXT NOT NULL,
    name TEXT NOT NULL,
    phone TEXT NOT NULL,
    phone_key TEXT GENERATED ALWAYS AS (regexp_replace(phone, '[^0-9]+', '', 'g')) STORED,
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_system_users_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT mini_system_users_role_allowed CHECK (role IN ('qolipchi')),
    CONSTRAINT mini_system_users_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT mini_system_users_phone_not_blank CHECK (btrim(phone) <> ''),
    CONSTRAINT mini_system_users_phone_key_not_blank CHECK (btrim(phone_key) <> '')
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_system_users_role_phone_key_unique
    ON mini_system_users (role, phone_key);

CREATE INDEX IF NOT EXISTS idx_mini_system_users_role_name
    ON mini_system_users (role, lower(name));
