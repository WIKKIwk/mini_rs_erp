CREATE TABLE IF NOT EXISTS mini_training_queue_events (
    event_id TEXT PRIMARY KEY,
    apparatus TEXT NOT NULL,
    order_id TEXT NOT NULL,
    action TEXT NOT NULL,
    from_state TEXT NOT NULL,
    to_state TEXT NOT NULL,
    actor_ref TEXT NOT NULL DEFAULT '',
    actor_display_name TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_mini_training_queue_events_order
    ON mini_training_queue_events (order_id, apparatus, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_mini_training_queue_events_actor
    ON mini_training_queue_events (actor_ref, created_at DESC);
