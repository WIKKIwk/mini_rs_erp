ALTER TABLE mini_paddons
    ADD COLUMN locked_at TIMESTAMPTZ,
    ADD COLUMN locked_by_ref TEXT NOT NULL DEFAULT '',
    ADD COLUMN locked_by_display_name TEXT NOT NULL DEFAULT '';

-- A retry of the new-paddon confirmation reuses the same physical package.
CREATE TABLE mini_paddon_print_successors (
    source_code TEXT NOT NULL REFERENCES mini_paddons(code),
    actor_role TEXT NOT NULL,
    actor_ref TEXT NOT NULL,
    apparatus_id TEXT NOT NULL,
    paddon_code TEXT NOT NULL REFERENCES mini_paddons(code),
    PRIMARY KEY (source_code, actor_role, actor_ref, apparatus_id)
);
