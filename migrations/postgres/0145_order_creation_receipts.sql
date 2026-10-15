-- Order, its design fork, quick template and retry receipt commit together.
CREATE TABLE mini_order_creation_receipts (
    owner_key TEXT NOT NULL,
    request_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    map_id TEXT NOT NULL REFERENCES mini_production_maps(id) ON DELETE CASCADE,
    response_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (owner_key, request_key)
);
GRANT SELECT, INSERT, DELETE ON mini_order_creation_receipts TO mini_rs_erp;
