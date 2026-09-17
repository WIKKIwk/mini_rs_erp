CREATE TABLE IF NOT EXISTS mini_print_preflight_holds (
    hold_id TEXT PRIMARY KEY,
    idempotency_key TEXT NOT NULL UNIQUE,
    order_id TEXT NOT NULL,
    canonical_apparatus_id TEXT NOT NULL,
    stage_node_id TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    actor_role TEXT NOT NULL,
    actor_ref TEXT NOT NULL,
    actor_display_name TEXT NOT NULL DEFAULT '',
    created_at_unix BIGINT NOT NULL,
    updated_at_unix BIGINT NOT NULL,
    expires_at_unix BIGINT NOT NULL,
    CONSTRAINT mini_print_preflight_holds_id_not_blank CHECK (btrim(hold_id) <> ''),
    CONSTRAINT mini_print_preflight_holds_idempotency_not_blank CHECK (btrim(idempotency_key) <> ''),
    CONSTRAINT mini_print_preflight_holds_order_not_blank CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_print_preflight_holds_apparatus_not_blank CHECK (btrim(canonical_apparatus_id) <> ''),
    CONSTRAINT mini_print_preflight_holds_actor_not_blank CHECK (
        btrim(actor_role) <> '' AND btrim(actor_ref) <> ''
    ),
    CONSTRAINT mini_print_preflight_holds_status CHECK (
        status IN ('held', 'running', 'passed', 'failed', 'cancelled', 'consumed')
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_print_preflight_holds_live_apparatus
    ON mini_print_preflight_holds (canonical_apparatus_id)
    WHERE status IN ('held', 'running', 'passed');

CREATE INDEX IF NOT EXISTS idx_mini_print_preflight_holds_order
    ON mini_print_preflight_holds (order_id, updated_at_unix DESC);
