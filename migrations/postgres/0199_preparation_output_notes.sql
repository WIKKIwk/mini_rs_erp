CREATE TABLE mini_preparation_output_notes (
    owner_ref TEXT NOT NULL CHECK (btrim(owner_ref) <> ''),
    target_kind TEXT NOT NULL CHECK (target_kind IN ('batch', 'roll')),
    target_id TEXT NOT NULL CHECK (btrim(target_id) <> ''),
    note TEXT NOT NULL CHECK (char_length(note) <= 2000),
    revision BIGINT NOT NULL CHECK (revision > 0),
    updated_by_name TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (owner_ref, target_kind, target_id)
);

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON mini_preparation_output_notes TO mini_rs_erp;
    END IF;
END $$;
