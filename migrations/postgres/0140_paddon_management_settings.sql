CREATE TABLE mini_paddon_management_settings (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    free_movement_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by_ref TEXT NOT NULL DEFAULT '',
    updated_by_display_name TEXT NOT NULL DEFAULT ''
);

INSERT INTO mini_paddon_management_settings (singleton) VALUES (TRUE);
GRANT SELECT, UPDATE ON mini_paddon_management_settings TO mini_rs_erp;
