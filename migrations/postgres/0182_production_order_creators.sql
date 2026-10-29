-- The authenticated creator is a creation-time snapshot, never a template owner.
CREATE TABLE mini_production_order_creators (
    map_id TEXT PRIMARY KEY REFERENCES mini_production_maps(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (btrim(role) <> ''),
    principal_ref TEXT NOT NULL CHECK (btrim(principal_ref) <> ''),
    display_name TEXT NOT NULL CHECK (btrim(display_name) <> '')
);

GRANT SELECT, INSERT, DELETE ON mini_production_order_creators TO mini_rs_erp;
REVOKE UPDATE ON mini_production_order_creators FROM mini_rs_erp;
