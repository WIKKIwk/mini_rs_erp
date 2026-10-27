CREATE TABLE mini_qolip_start_settings (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    automatic_confirmation_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    revision BIGINT NOT NULL DEFAULT 0 CHECK (revision >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by_ref TEXT NOT NULL DEFAULT '',
    updated_by_display_name TEXT NOT NULL DEFAULT ''
);
INSERT INTO mini_qolip_start_settings (singleton) VALUES (TRUE);
GRANT SELECT, UPDATE ON mini_qolip_start_settings TO mini_rs_erp;
CREATE TRIGGER mini_production_live_changed
AFTER INSERT OR UPDATE OR DELETE ON mini_qolip_start_settings
FOR EACH ROW EXECUTE FUNCTION mini_notify_production_live();
