ALTER TABLE mini_waste_bins ADD COLUMN name TEXT;
UPDATE mini_waste_bins SET name = code;
ALTER TABLE mini_waste_bins
    ALTER COLUMN name SET NOT NULL,
    ADD CONSTRAINT mini_waste_bins_name_valid CHECK (char_length(btrim(name)) BETWEEN 1 AND 120);

-- An older cycle still belongs to its original holder at the same apparatus.
-- New reservations record the full operator identity.
ALTER TABLE mini_waste_bin_cycles
    ADD COLUMN opened_by_role TEXT CHECK (btrim(opened_by_role) <> '');

CREATE TABLE mini_waste_bin_apparatuses (
    bin_code TEXT NOT NULL REFERENCES mini_waste_bins(code) ON DELETE CASCADE,
    apparatus_id TEXT NOT NULL REFERENCES mini_apparatus(id) ON DELETE CASCADE,
    PRIMARY KEY (bin_code, apparatus_id)
);

CREATE TABLE mini_waste_bin_operators (
    bin_code TEXT NOT NULL REFERENCES mini_waste_bins(code) ON DELETE CASCADE,
    operator_role TEXT NOT NULL CHECK (operator_role IN (
        'aparatchi', 'rezka_master', 'tayyorlov_masteri', 'metalayzer', 'homashyo_rezkachi'
    )),
    operator_ref TEXT NOT NULL CHECK (btrim(operator_ref) <> ''),
    PRIMARY KEY (bin_code, operator_role, operator_ref)
);

ALTER TABLE mini_waste_bin_apparatuses OWNER TO mini_rs_erp_owner;
ALTER TABLE mini_waste_bin_operators OWNER TO mini_rs_erp_owner;
REVOKE ALL ON mini_waste_bin_apparatuses, mini_waste_bin_operators FROM PUBLIC;
GRANT SELECT, INSERT, DELETE ON mini_waste_bin_apparatuses, mini_waste_bin_operators TO mini_rs_erp;
GRANT UPDATE (name) ON mini_waste_bins TO mini_rs_erp;
