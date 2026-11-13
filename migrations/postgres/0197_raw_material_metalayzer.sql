-- Processing metadata is independent of stock availability and quantity.
ALTER TABLE mini_raw_material_stock
    ADD COLUMN is_metalayzer BOOLEAN NOT NULL DEFAULT false;
