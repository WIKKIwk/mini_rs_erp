ALTER TABLE mini_raw_material_paddon_attachments
    ADD COLUMN owner_role TEXT NOT NULL DEFAULT 'tayyorlov_masteri'
        CHECK (owner_role IN ('tayyorlov_masteri', 'material_taminotchi')),
    ADD COLUMN stock_ids TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN created_at TIMESTAMPTZ,
    DROP CONSTRAINT mini_raw_material_paddon_attachments_pkey,
    ADD PRIMARY KEY (owner_role, owner_ref, request_id);

ALTER TABLE mini_raw_material_paddon_attachments
    ALTER COLUMN created_at SET DEFAULT now();
