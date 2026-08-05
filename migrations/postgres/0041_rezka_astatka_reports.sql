CREATE TABLE IF NOT EXISTS mini_rezka_astatka_reports (
    id BIGSERIAL PRIMARY KEY,
    report_id TEXT NOT NULL UNIQUE,
    order_id TEXT NOT NULL,
    apparatus TEXT NOT NULL,
    from_at TIMESTAMPTZ NOT NULL,
    to_at TIMESTAMPTZ NOT NULL,
    total_waste NUMERIC NOT NULL,
    rezka_bosma_waste NUMERIC NOT NULL,
    rezka_lamination_waste NUMERIC NOT NULL,
    rezka_edge_waste NUMERIC NOT NULL,
    worker_role TEXT NOT NULL DEFAULT '',
    worker_ref TEXT NOT NULL DEFAULT '',
    worker_display_name TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_rezka_astatka_reports_order_required
        CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_rezka_astatka_reports_apparatus_required
        CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_rezka_astatka_reports_interval_valid
        CHECK (to_at >= from_at),
    CONSTRAINT mini_rezka_astatka_reports_total_waste_valid
        CHECK (total_waste >= 0),
    CONSTRAINT mini_rezka_astatka_reports_bosma_waste_valid
        CHECK (rezka_bosma_waste >= 0),
    CONSTRAINT mini_rezka_astatka_reports_lamination_waste_valid
        CHECK (rezka_lamination_waste >= 0),
    CONSTRAINT mini_rezka_astatka_reports_edge_waste_valid
        CHECK (rezka_edge_waste >= 0)
);

CREATE INDEX IF NOT EXISTS idx_mini_rezka_astatka_order_to
    ON mini_rezka_astatka_reports (order_id, to_at DESC, created_at DESC);

GRANT SELECT, INSERT, UPDATE, DELETE
    ON TABLE mini_rezka_astatka_reports TO mini_rs_erp;
GRANT USAGE, SELECT
    ON SEQUENCE mini_rezka_astatka_reports_id_seq TO mini_rs_erp;
