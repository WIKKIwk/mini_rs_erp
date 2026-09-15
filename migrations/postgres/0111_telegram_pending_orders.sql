CREATE TABLE mini_pending_orders (
    id TEXT PRIMARY KEY,
    order_number TEXT NOT NULL UNIQUE,
    telegram_user_id TEXT NOT NULL,
    payload_json JSONB NOT NULL,
    image_body BYTEA NOT NULL,
    completion_json JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    CONSTRAINT mini_pending_order_number CHECK (order_number ~ '^[0-9]{4}$'),
    CONSTRAINT mini_pending_order_id CHECK (id = 'zakaz-' || order_number),
    CONSTRAINT mini_pending_order_completion CHECK ((completion_json IS NULL) = (completed_at IS NULL))
);
CREATE INDEX mini_pending_orders_open ON mini_pending_orders (created_at DESC)
    WHERE completion_json IS NULL;
GRANT SELECT, INSERT, UPDATE, DELETE ON mini_pending_orders TO mini_rs_erp;
