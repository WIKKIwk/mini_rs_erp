CREATE TABLE IF NOT EXISTS mini_laminatsiya_astatka_reports (
    id BIGSERIAL PRIMARY KEY,
    report_id TEXT NOT NULL UNIQUE,
    order_id TEXT NOT NULL,
    apparatus TEXT NOT NULL,
    from_at TIMESTAMPTZ NOT NULL,
    to_at TIMESTAMPTZ NOT NULL,
    lamination_print_leftover_rolls NUMERIC NOT NULL,
    lamination_film_leftover_rolls NUMERIC NOT NULL,
    total_waste NUMERIC NOT NULL,
    worker_role TEXT NOT NULL DEFAULT '',
    worker_ref TEXT NOT NULL DEFAULT '',
    worker_display_name TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_laminatsiya_astatka_reports_order_required
        CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_laminatsiya_astatka_reports_apparatus_required
        CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_laminatsiya_astatka_reports_interval_valid
        CHECK (to_at >= from_at),
    CONSTRAINT mini_laminatsiya_astatka_reports_print_rolls_valid
        CHECK (lamination_print_leftover_rolls >= 0),
    CONSTRAINT mini_laminatsiya_astatka_reports_film_rolls_valid
        CHECK (lamination_film_leftover_rolls >= 0),
    CONSTRAINT mini_laminatsiya_astatka_reports_waste_valid
        CHECK (total_waste >= 0)
);

CREATE INDEX IF NOT EXISTS idx_mini_laminatsiya_astatka_order_to
    ON mini_laminatsiya_astatka_reports (order_id, to_at DESC, created_at DESC);

GRANT SELECT, INSERT, UPDATE, DELETE
    ON TABLE mini_laminatsiya_astatka_reports TO mini_rs_erp;
GRANT USAGE, SELECT
    ON SEQUENCE mini_laminatsiya_astatka_reports_id_seq TO mini_rs_erp;
