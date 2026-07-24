CREATE TABLE IF NOT EXISTS mini_order_control_states (
    order_id TEXT PRIMARY KEY
        REFERENCES mini_production_maps(id) ON DELETE CASCADE,
    state TEXT NOT NULL,
    actor_role TEXT NOT NULL DEFAULT '',
    actor_ref TEXT NOT NULL DEFAULT '',
    actor_display_name TEXT NOT NULL DEFAULT '',
    requested_at_unix BIGINT NOT NULL DEFAULT 0,
    frozen_at_unix BIGINT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_order_control_states_state_allowed
        CHECK (state IN ('active', 'freeze_requested', 'frozen')),
    CONSTRAINT mini_order_control_states_frozen_timestamp CHECK (
        (state = 'frozen' AND frozen_at_unix IS NOT NULL)
        OR (state <> 'frozen' AND frozen_at_unix IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_mini_order_control_states_state
    ON mini_order_control_states(state);
