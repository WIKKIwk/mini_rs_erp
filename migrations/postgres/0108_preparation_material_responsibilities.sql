SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- Tayyorlov masteri qaysi calculate-material (homashyo oilasi) uchun javobgar.
-- material_id = mini_calculate_materials.id (masalan builtin-pet), micron'siz.
-- Faqat tayyorlov_masteri uchun, ombor biriktirishdan mustaqil.
CREATE TABLE IF NOT EXISTS mini_preparation_material_responsibilities (
    principal_role TEXT NOT NULL CHECK (principal_role = 'tayyorlov_masteri'),
    principal_ref TEXT NOT NULL CHECK (btrim(principal_ref) <> ''),
    material_id TEXT NOT NULL CHECK (btrim(material_id) <> '' AND char_length(btrim(material_id)) <= 128),
    material_name TEXT NOT NULL CHECK (btrim(material_name) <> ''),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (principal_role, principal_ref, material_id)
);
CREATE INDEX IF NOT EXISTS idx_prep_mat_resp_ref
    ON mini_preparation_material_responsibilities (principal_ref);

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        GRANT SELECT, INSERT, DELETE ON mini_preparation_material_responsibilities TO mini_rs_erp;
    END IF;
END $$;
