ALTER TABLE mini_paddon_management_settings
    ADD COLUMN worker_visibility_enabled BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX mini_paddons_creator_updated_idx
    ON mini_paddons (created_by_ref, updated_at DESC, code);
