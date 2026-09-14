SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- Formula endi homashyoga (calculate-material oilasi) bog'lanadi:
-- bitta mahsulotning har bir homashyosi uchun alohida formula cardlari.
-- Eski qatorlar material_id='' bilan legacy qoladi va homashyo
-- tanlovida chiqmaydi.
ALTER TABLE mini_preparation_formulas
    ADD COLUMN material_id TEXT NOT NULL DEFAULT '';
ALTER TABLE mini_preparation_formulas
    ADD COLUMN material_name TEXT NOT NULL DEFAULT '';
ALTER TABLE mini_preparation_formulas
    DROP CONSTRAINT mini_preparation_formulas_pkey;
ALTER TABLE mini_preparation_formulas
    ADD PRIMARY KEY (owner_ref, product_code, material_id, name);
ALTER TABLE mini_preparation_formulas
    ADD CHECK (char_length(btrim(material_id)) <= 128);
DROP INDEX IF EXISTS mini_preparation_formulas_product;
CREATE INDEX mini_preparation_formulas_product ON mini_preparation_formulas(product_code);
CREATE INDEX mini_preparation_formulas_material ON mini_preparation_formulas(material_id);
