-- Qolip ombori UI 13 qatorni ko‘rsatadi, eski schema faqat 1..9 ni qabul qilgan.
ALTER TABLE mini_qolip_locations
    DROP CONSTRAINT IF EXISTS mini_qolip_locations_column_range;

ALTER TABLE mini_qolip_locations
    ADD CONSTRAINT mini_qolip_locations_column_range
    CHECK (column_number IS NULL OR column_number BETWEEN 1 AND 13);

ALTER TABLE mini_qolip_cell_qrs
    DROP CONSTRAINT IF EXISTS mini_qolip_cell_qrs_column_range;

ALTER TABLE mini_qolip_cell_qrs
    ADD CONSTRAINT mini_qolip_cell_qrs_column_range
    CHECK (column_number BETWEEN 1 AND 13);
