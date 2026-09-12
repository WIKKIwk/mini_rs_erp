SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- Tayyorlov masteri formulalari: har bir tayyor mahsulot kodi uchun
-- seriya (homashyo) + foiz ro'yxati. Bitta owner uchun bitta mahsulotga
-- bitta formula; saqlashda alifbo tartibida yoziladi (app tomonidan).
CREATE TABLE mini_preparation_formulas (
    owner_ref TEXT NOT NULL REFERENCES mini_system_users(id) ON DELETE RESTRICT,
    product_code TEXT NOT NULL CHECK (btrim(product_code) <> '' AND char_length(btrim(product_code)) <= 160),
    lines JSONB NOT NULL CHECK (jsonb_typeof(lines) = 'array'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (owner_ref, product_code)
);
CREATE INDEX mini_preparation_formulas_product ON mini_preparation_formulas(product_code);

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        GRANT SELECT, INSERT, UPDATE, DELETE ON mini_preparation_formulas TO mini_rs_erp;
    END IF;
END $$;
