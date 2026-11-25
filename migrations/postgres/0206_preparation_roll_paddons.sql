ALTER TABLE mini_raw_material_paddon_units
    ADD COLUMN prepared_roll BOOLEAN NOT NULL DEFAULT false,
    ADD CONSTRAINT mini_raw_material_paddon_prepared_roll_shape CHECK (
        NOT prepared_roll OR (kind = 'roll' AND stock_id IS NOT NULL AND unit_kg IS NOT NULL AND NOT opened)
    ),
    DROP CONSTRAINT mini_raw_material_paddon_units_stock_id_key;

-- A stock roll can be packed again after its previous entry is issued.
CREATE UNIQUE INDEX mini_raw_material_paddon_active_stock
    ON mini_raw_material_paddon_units (stock_id)
    WHERE issue_id IS NULL AND stock_id IS NOT NULL;

CREATE TABLE mini_raw_material_paddon_attachments (
    owner_ref TEXT NOT NULL,
    request_id TEXT NOT NULL,
    paddon_id TEXT NOT NULL REFERENCES mini_raw_material_paddons(id),
    receipt_names TEXT[] NOT NULL,
    PRIMARY KEY (owner_ref, request_id)
);

GRANT SELECT, INSERT ON mini_raw_material_paddon_attachments TO mini_rs_erp;
