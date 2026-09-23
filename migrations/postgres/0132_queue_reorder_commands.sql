-- Durable retry receipts; no production order or existing queue is rewritten.
CREATE TABLE mini_queue_reorder_commands (
    canonical_apparatus_id TEXT NOT NULL REFERENCES mini_apparatus(id),
    actor_role TEXT NOT NULL,
    actor_ref TEXT NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 200),
    request_json JSONB NOT NULL,
    result_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (canonical_apparatus_id, actor_role, actor_ref, idempotency_key)
);
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        GRANT SELECT, INSERT ON mini_queue_reorder_commands TO mini_rs_erp;
    END IF;
END $$;
