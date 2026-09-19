CREATE TABLE mini_active_rezka_paddons (
    actor_role TEXT NOT NULL CHECK (btrim(actor_role) <> ''),
    actor_ref TEXT NOT NULL CHECK (btrim(actor_ref) <> ''),
    apparatus_id TEXT NOT NULL CHECK (btrim(apparatus_id) <> ''),
    paddon_code TEXT REFERENCES mini_paddons(code) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (actor_role, actor_ref, apparatus_id)
);
