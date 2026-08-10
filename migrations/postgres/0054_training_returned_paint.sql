CREATE TABLE IF NOT EXISTS mini_training_returned_paint_reports (
    id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL,
    apparatus TEXT NOT NULL,
    action TEXT NOT NULL,
    items_json JSONB NOT NULL,
    image_id TEXT NOT NULL DEFAULT '',
    return_ink_kg DOUBLE PRECISION,
    calculation_json JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_mini_training_returned_paint_order
    ON mini_training_returned_paint_reports
        (order_id, lower(apparatus), created_at DESC);
