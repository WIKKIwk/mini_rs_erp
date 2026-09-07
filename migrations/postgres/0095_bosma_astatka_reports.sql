CREATE TABLE IF NOT EXISTS mini_bosma_astatka_reports (
    report_id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL REFERENCES mini_production_maps(id),
    apparatus TEXT NOT NULL REFERENCES mini_apparatus(id),
    from_at_unix BIGINT NOT NULL,
    to_at_unix BIGINT NOT NULL CHECK (to_at_unix >= from_at_unix),
    report_json JSONB NOT NULL CHECK (jsonb_typeof(report_json) = 'object'),
    CHECK (report_json->>'report_id' = report_id),
    CHECK (report_json->>'order_id' = order_id),
    CHECK (report_json->>'apparatus' = apparatus)
);
CREATE INDEX IF NOT EXISTS idx_mini_bosma_astatka_order_to
    ON mini_bosma_astatka_reports (order_id, apparatus, to_at_unix);
GRANT SELECT, INSERT ON mini_bosma_astatka_reports TO mini_rs_erp;
