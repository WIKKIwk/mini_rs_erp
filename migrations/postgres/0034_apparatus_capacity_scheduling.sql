CREATE TABLE IF NOT EXISTS mini_apparatus_capacity_profiles (
    apparatus_id TEXT PRIMARY KEY,
    apparatus TEXT NOT NULL,
    capacity_slots INTEGER NOT NULL DEFAULT 1,
    setup_minutes INTEGER NOT NULL DEFAULT 0,
    cleanup_minutes INTEGER NOT NULL DEFAULT 0,
    efficiency_percent INTEGER NOT NULL DEFAULT 100,
    finite_capacity BOOLEAN NOT NULL DEFAULT TRUE,
    working_windows JSONB NOT NULL DEFAULT '[]'::jsonb,
    capabilities JSONB NOT NULL DEFAULT '[]'::jsonb,
    capability_levels JSONB NOT NULL DEFAULT '{}'::jsonb,
    notes TEXT NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_apparatus_capacity_profiles_id_not_blank CHECK (btrim(apparatus_id) <> ''),
    CONSTRAINT mini_apparatus_capacity_profiles_apparatus_not_blank CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_apparatus_capacity_profiles_slots_positive CHECK (capacity_slots BETWEEN 1 AND 64),
    CONSTRAINT mini_apparatus_capacity_profiles_efficiency_valid CHECK (efficiency_percent BETWEEN 1 AND 200),
    CONSTRAINT mini_apparatus_capacity_profiles_windows_array CHECK (jsonb_typeof(working_windows) = 'array'),
    CONSTRAINT mini_apparatus_capacity_profiles_capabilities_array CHECK (jsonb_typeof(capabilities) = 'array'),
    CONSTRAINT mini_apparatus_capacity_profiles_levels_object CHECK (jsonb_typeof(capability_levels) = 'object')
);

CREATE TABLE IF NOT EXISTS mini_apparatus_downtimes (
    id TEXT PRIMARY KEY,
    apparatus_id TEXT NOT NULL,
    apparatus TEXT NOT NULL,
    starts_at TIMESTAMPTZ NOT NULL,
    ends_at TIMESTAMPTZ NOT NULL,
    reason TEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    actor_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_apparatus_downtimes_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT mini_apparatus_downtimes_apparatus_not_blank CHECK (btrim(apparatus_id) <> ''),
    CONSTRAINT mini_apparatus_downtimes_reason_not_blank CHECK (btrim(reason) <> ''),
    CONSTRAINT mini_apparatus_downtimes_interval_valid CHECK (ends_at > starts_at)
);

CREATE TABLE IF NOT EXISTS mini_apparatus_schedule_reservations (
    reservation_id TEXT PRIMARY KEY,
    idempotency_key TEXT NOT NULL UNIQUE,
    order_id TEXT NOT NULL REFERENCES mini_production_maps(id) ON DELETE RESTRICT,
    apparatus_id TEXT NOT NULL,
    apparatus TEXT NOT NULL,
    starts_at TIMESTAMPTZ NOT NULL,
    ends_at TIMESTAMPTZ NOT NULL,
    requested_duration_minutes INTEGER NOT NULL,
    reserved_duration_minutes INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'planned',
    priority INTEGER NOT NULL DEFAULT 0,
    source TEXT NOT NULL DEFAULT '',
    reason TEXT NOT NULL DEFAULT '',
    capability_requirements JSONB NOT NULL DEFAULT '[]'::jsonb,
    actor_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_apparatus_schedule_reservations_id_not_blank CHECK (btrim(reservation_id) <> ''),
    CONSTRAINT mini_apparatus_schedule_reservations_idempotency_not_blank CHECK (btrim(idempotency_key) <> ''),
    CONSTRAINT mini_apparatus_schedule_reservations_order_not_blank CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_apparatus_schedule_reservations_apparatus_not_blank CHECK (btrim(apparatus_id) <> ''),
    CONSTRAINT mini_apparatus_schedule_reservations_interval_valid CHECK (ends_at > starts_at),
    CONSTRAINT mini_apparatus_schedule_reservations_duration_positive CHECK (requested_duration_minutes > 0 AND reserved_duration_minutes > 0),
    CONSTRAINT mini_apparatus_schedule_reservations_status_allowed CHECK (status IN ('planned', 'active', 'completed', 'cancelled')),
    CONSTRAINT mini_apparatus_schedule_reservations_requirements_array CHECK (jsonb_typeof(capability_requirements) = 'array')
);

CREATE INDEX IF NOT EXISTS idx_mini_apparatus_capacity_profiles_apparatus
    ON mini_apparatus_capacity_profiles(lower(apparatus));
CREATE INDEX IF NOT EXISTS idx_mini_apparatus_downtimes_apparatus_time
    ON mini_apparatus_downtimes(apparatus_id, starts_at, ends_at)
    WHERE active;
CREATE INDEX IF NOT EXISTS idx_mini_apparatus_schedule_reservations_apparatus_time
    ON mini_apparatus_schedule_reservations(apparatus_id, starts_at, ends_at)
    WHERE status IN ('planned', 'active');
CREATE INDEX IF NOT EXISTS idx_mini_apparatus_schedule_reservations_order
    ON mini_apparatus_schedule_reservations(order_id, starts_at DESC);
