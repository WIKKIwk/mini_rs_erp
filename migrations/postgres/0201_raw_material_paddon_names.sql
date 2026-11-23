ALTER TABLE mini_raw_material_paddons
    ADD COLUMN name TEXT NOT NULL DEFAULT '' CHECK (char_length(name) <= 80);
