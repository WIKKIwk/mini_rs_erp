SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- GScale/Tarozi sodda kirimi admin biriktirgan ERP Rulon itemini ham
-- preparation receipt sifatida qayd etadi. Eski Seriyo itemlari esa shu
-- FK orqali mini_items katalogida qoladi.
ALTER TABLE mini_preparation_receipts
    DROP CONSTRAINT IF EXISTS mini_preparation_receipts_item_code_fkey;

ALTER TABLE mini_preparation_receipts
    ADD CONSTRAINT mini_preparation_receipts_item_code_fkey
    FOREIGN KEY (item_code) REFERENCES mini_items(code)
    ON UPDATE CASCADE ON DELETE RESTRICT;
