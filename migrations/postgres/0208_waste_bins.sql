CREATE TABLE mini_waste_bins (
    code TEXT PRIMARY KEY CHECK (code ~ '^[A-Z0-9][A-Z0-9_-]{0,39}$'),
    tare_units BIGINT NOT NULL CHECK (tare_units >= 0),
    tolerance_units BIGINT NOT NULL CHECK (tolerance_units >= 0),
    registered_by_ref TEXT NOT NULL,
    registered_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE mini_waste_bin_cycles (
    id TEXT PRIMARY KEY,
    bin_code TEXT NOT NULL REFERENCES mini_waste_bins(code),
    apparatus_id TEXT NOT NULL CHECK (btrim(apparatus_id) <> ''),
    opened_by_ref TEXT NOT NULL,
    opened_by_display_name TEXT NOT NULL,
    opened_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expected_units BIGINT CHECK (expected_units >= 0),
    sealed_by_ref TEXT,
    sealed_by_display_name TEXT,
    sealed_at TIMESTAMPTZ,
    replacement_cycle_id TEXT REFERENCES mini_waste_bin_cycles(id),
    reset_by_ref TEXT,
    reset_by_display_name TEXT,
    reset_at TIMESTAMPTZ,
    CHECK ((sealed_at IS NULL AND sealed_by_ref IS NULL AND sealed_by_display_name IS NULL AND expected_units IS NULL)
        OR (sealed_at IS NOT NULL AND sealed_by_ref IS NOT NULL AND sealed_by_display_name IS NOT NULL AND expected_units IS NOT NULL)),
    CHECK ((reset_at IS NULL AND reset_by_ref IS NULL AND reset_by_display_name IS NULL)
        OR (reset_at IS NOT NULL AND reset_by_ref IS NOT NULL AND reset_by_display_name IS NOT NULL AND sealed_at IS NOT NULL))
);

CREATE UNIQUE INDEX mini_waste_bins_active_cycle
    ON mini_waste_bin_cycles (bin_code) WHERE reset_at IS NULL;
CREATE UNIQUE INDEX mini_waste_bins_open_apparatus
    ON mini_waste_bin_cycles (apparatus_id) WHERE sealed_at IS NULL;
CREATE INDEX mini_waste_bins_cycle_history ON mini_waste_bin_cycles (bin_code, opened_at DESC, id);

CREATE TABLE mini_waste_bin_entries (
    id TEXT PRIMARY KEY,
    origin TEXT NOT NULL CHECK (origin IN ('manual', 'production')),
    request_id TEXT NOT NULL,
    apparatus_id TEXT NOT NULL CHECK (btrim(apparatus_id) <> ''),
    expected_cycle_id TEXT,
    cycle_id TEXT REFERENCES mini_waste_bin_cycles(id),
    order_id TEXT,
    session_id TEXT,
    reported_units BIGINT NOT NULL CHECK (reported_units > 0),
    units BIGINT NOT NULL CHECK (units BETWEEN 0 AND reported_units),
    reconcile_manual BOOLEAN NOT NULL,
    reason TEXT NOT NULL CHECK (btrim(reason) <> ''),
    actor_role TEXT NOT NULL,
    actor_ref TEXT NOT NULL,
    actor_display_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    assigned_at TIMESTAMPTZ,
    assigned_by_ref TEXT,
    CHECK ((cycle_id IS NULL AND assigned_at IS NULL AND assigned_by_ref IS NULL)
        OR (cycle_id IS NOT NULL AND assigned_at IS NOT NULL AND assigned_by_ref IS NOT NULL AND units > 0)),
    UNIQUE (origin, actor_role, actor_ref, request_id)
);

CREATE INDEX mini_waste_bins_entries_by_cycle ON mini_waste_bin_entries (cycle_id, created_at, id);
CREATE INDEX mini_waste_bins_pending ON mini_waste_bin_entries (apparatus_id, created_at, id)
    WHERE cycle_id IS NULL AND units > 0;
CREATE INDEX mini_waste_bins_manual_scope
    ON mini_waste_bin_entries (apparatus_id, actor_role, actor_ref, order_id, created_at, id) WHERE origin = 'manual';

CREATE TABLE mini_waste_bin_manual_coverage (
    report_id TEXT NOT NULL REFERENCES mini_waste_bin_entries(id),
    manual_entry_id TEXT NOT NULL REFERENCES mini_waste_bin_entries(id),
    units BIGINT NOT NULL CHECK (units > 0),
    PRIMARY KEY (report_id, manual_entry_id)
);
CREATE INDEX mini_waste_bins_coverage_by_entry ON mini_waste_bin_manual_coverage (manual_entry_id);

CREATE TABLE mini_waste_bin_receipts (
    cycle_id TEXT PRIMARY KEY REFERENCES mini_waste_bin_cycles(id),
    gross_units BIGINT NOT NULL,
    tare_units BIGINT NOT NULL,
    expected_units BIGINT NOT NULL CHECK (expected_units >= 0),
    tolerance_units BIGINT NOT NULL CHECK (tolerance_units >= 0),
    measurement_source TEXT NOT NULL CHECK (measurement_source IN ('gscale', 'manual')),
    note TEXT NOT NULL,
    received_by_ref TEXT NOT NULL,
    received_by_display_name TEXT NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (gross_units >= tare_units AND tare_units >= 0),
    CHECK (abs(gross_units - tare_units - expected_units) <= tolerance_units OR btrim(note) <> '')
);

ALTER TABLE mini_waste_bins OWNER TO mini_rs_erp_owner;
ALTER TABLE mini_waste_bin_cycles OWNER TO mini_rs_erp_owner;
ALTER TABLE mini_waste_bin_entries OWNER TO mini_rs_erp_owner;
ALTER TABLE mini_waste_bin_manual_coverage OWNER TO mini_rs_erp_owner;
ALTER TABLE mini_waste_bin_receipts OWNER TO mini_rs_erp_owner;

REVOKE ALL ON mini_waste_bins, mini_waste_bin_cycles, mini_waste_bin_entries,
    mini_waste_bin_manual_coverage, mini_waste_bin_receipts FROM PUBLIC;
GRANT SELECT, INSERT ON mini_waste_bins, mini_waste_bin_cycles, mini_waste_bin_entries,
    mini_waste_bin_manual_coverage, mini_waste_bin_receipts TO mini_rs_erp;
GRANT UPDATE (expected_units, sealed_at, sealed_by_ref, sealed_by_display_name, replacement_cycle_id,
    reset_at, reset_by_ref, reset_by_display_name) ON mini_waste_bin_cycles TO mini_rs_erp;
GRANT UPDATE (units, cycle_id, assigned_at, assigned_by_ref) ON mini_waste_bin_entries TO mini_rs_erp;
