SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- Bitta mahsulotga bir nechta nomli formula cardlari.
-- Eski qatorlar 'Asosiy' nomini oladi.
ALTER TABLE mini_preparation_formulas
    ADD COLUMN name TEXT NOT NULL DEFAULT 'Asosiy';
ALTER TABLE mini_preparation_formulas
    DROP CONSTRAINT mini_preparation_formulas_pkey;
ALTER TABLE mini_preparation_formulas
    ADD PRIMARY KEY (owner_ref, product_code, name);
ALTER TABLE mini_preparation_formulas
    ADD CHECK (btrim(name) <> '' AND char_length(btrim(name)) <= 80);
DROP INDEX IF EXISTS mini_preparation_formulas_product;
CREATE INDEX mini_preparation_formulas_product ON mini_preparation_formulas(product_code);
