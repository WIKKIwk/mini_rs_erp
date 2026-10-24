CREATE TABLE mini_lamination_settings (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    layer_remainders_required BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by_ref TEXT NOT NULL DEFAULT '',
    updated_by_display_name TEXT NOT NULL DEFAULT ''
);

INSERT INTO mini_lamination_settings (singleton) VALUES (TRUE);
GRANT SELECT, UPDATE ON mini_lamination_settings TO mini_rs_erp;
