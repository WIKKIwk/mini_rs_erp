ALTER TABLE mini_production_maps
    ADD COLUMN IF NOT EXISTS lifecycle_status TEXT NOT NULL DEFAULT 'released',
    ADD COLUMN IF NOT EXISTS completion_outcome TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS lifecycle_changed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS production_completed_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS closed_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS lifecycle_version BIGINT NOT NULL DEFAULT 0;

ALTER TABLE mini_production_maps
    DROP CONSTRAINT IF EXISTS mini_production_maps_lifecycle_status_allowed,
    DROP CONSTRAINT IF EXISTS mini_production_maps_completion_outcome_allowed,
    DROP CONSTRAINT IF EXISTS mini_production_maps_lifecycle_version_non_negative,
    DROP CONSTRAINT IF EXISTS mini_production_maps_completed_timestamp_consistent,
    DROP CONSTRAINT IF EXISTS mini_production_maps_closed_timestamp_consistent;

ALTER TABLE mini_production_maps
    ADD CONSTRAINT mini_production_maps_lifecycle_status_allowed CHECK (
        lifecycle_status IN (
            'released',
            'in_progress',
            'production_completed',
            'closed',
            'cancelled'
        )
    ),
    ADD CONSTRAINT mini_production_maps_completion_outcome_allowed CHECK (
        completion_outcome IN ('', 'normal', 'with_issue')
    ),
    ADD CONSTRAINT mini_production_maps_lifecycle_version_non_negative CHECK (
        lifecycle_version >= 0
    ),
    ADD CONSTRAINT mini_production_maps_completed_timestamp_consistent CHECK (
        lifecycle_status NOT IN ('production_completed', 'closed')
        OR production_completed_at IS NOT NULL
    ),
    ADD CONSTRAINT mini_production_maps_closed_timestamp_consistent CHECK (
        lifecycle_status <> 'closed' OR closed_at IS NOT NULL
    );

CREATE TABLE IF NOT EXISTS mini_production_order_lifecycle_events (
    event_id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL REFERENCES mini_production_maps(id) ON DELETE CASCADE,
    from_status TEXT NOT NULL DEFAULT '',
    to_status TEXT NOT NULL,
    completion_outcome TEXT NOT NULL DEFAULT '',
    actor_role TEXT NOT NULL DEFAULT '',
    actor_ref TEXT NOT NULL DEFAULT '',
    actor_display_name TEXT NOT NULL DEFAULT '',
    source_event_id TEXT NOT NULL DEFAULT '',
    reason TEXT NOT NULL DEFAULT '',
    lifecycle_version BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_production_order_lifecycle_events_order_not_blank
        CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_production_order_lifecycle_events_from_status_allowed CHECK (
        from_status IN (
            '',
            'released',
            'in_progress',
            'production_completed',
            'closed',
            'cancelled'
        )
    ),
    CONSTRAINT mini_production_order_lifecycle_events_to_status_allowed CHECK (
        to_status IN (
            'released',
            'in_progress',
            'production_completed',
            'closed',
            'cancelled'
        )
    ),
    CONSTRAINT mini_production_order_lifecycle_events_outcome_allowed CHECK (
        completion_outcome IN ('', 'normal', 'with_issue')
    ),
    CONSTRAINT mini_production_order_lifecycle_events_version_positive CHECK (
        lifecycle_version > 0
    )
);

CREATE INDEX IF NOT EXISTS idx_mini_production_maps_active_lifecycle
    ON mini_production_maps (updated_at DESC, id)
    WHERE lifecycle_status IN ('released', 'in_progress');

CREATE INDEX IF NOT EXISTS idx_mini_production_maps_lifecycle_status_updated
    ON mini_production_maps (lifecycle_status, updated_at DESC, id);

CREATE INDEX IF NOT EXISTS idx_mini_production_maps_completed_lifecycle
    ON mini_production_maps (production_completed_at DESC, id)
    WHERE lifecycle_status IN ('production_completed', 'closed');

CREATE INDEX IF NOT EXISTS idx_mini_production_order_lifecycle_events_order_created
    ON mini_production_order_lifecycle_events (order_id, created_at DESC);

WITH lifecycle_projection AS (
    SELECT
        maps.id AS order_id,
        CASE
            WHEN EXISTS (
                SELECT 1
                FROM mini_production_map_nodes required
                WHERE required.map_id = maps.id
                  AND required.kind = 'apparatus'
            )
            AND NOT EXISTS (
                SELECT 1
                FROM mini_production_map_nodes required
                WHERE required.map_id = maps.id
                  AND required.kind = 'apparatus'
                  AND NOT EXISTS (
                      SELECT 1
                      FROM mini_queue_states states
                      WHERE states.order_id = maps.id
                        AND states.state = 'completed'
                        AND states.canonical_apparatus_id = COALESCE(
                            required.canonical_alternative_apparatus_id,
                            required.canonical_apparatus_id
                        )
                  )
            ) THEN 'production_completed'
            WHEN EXISTS (
                SELECT 1
                FROM mini_queue_states states
                WHERE states.order_id = maps.id
                  AND states.state <> 'pending'
            ) THEN 'in_progress'
            ELSE 'released'
        END AS lifecycle_status,
        COALESCE(
            (
                SELECT max(states.updated_at)
                FROM mini_queue_states states
                WHERE states.order_id = maps.id
                  AND states.state <> 'pending'
            ),
            maps.updated_at
        ) AS lifecycle_changed_at,
        (
            SELECT max(states.updated_at)
            FROM mini_queue_states states
            WHERE states.order_id = maps.id
              AND states.state = 'completed'
        ) AS production_completed_at,
        EXISTS (
            SELECT 1
            FROM mini_queue_action_events events
            WHERE events.order_id = maps.id
              AND events.action = 'complete'
              AND COALESCE(events.payload_json->>'completed_with_issue', 'false') = 'true'
        ) AS completed_with_issue
    FROM mini_production_maps maps
)
UPDATE mini_production_maps maps
SET lifecycle_status = projection.lifecycle_status,
    completion_outcome = CASE
        WHEN projection.lifecycle_status = 'production_completed'
            THEN CASE WHEN projection.completed_with_issue THEN 'with_issue' ELSE 'normal' END
        ELSE ''
    END,
    lifecycle_changed_at = projection.lifecycle_changed_at,
    production_completed_at = CASE
        WHEN projection.lifecycle_status = 'production_completed'
            THEN COALESCE(projection.production_completed_at, projection.lifecycle_changed_at)
        ELSE NULL
    END,
    closed_at = NULL,
    lifecycle_version = CASE
        WHEN projection.lifecycle_status = 'released' THEN 0
        ELSE 1
    END
FROM lifecycle_projection projection
WHERE maps.id = projection.order_id;

INSERT INTO mini_production_order_lifecycle_events (
    event_id,
    order_id,
    from_status,
    to_status,
    completion_outcome,
    actor_role,
    actor_ref,
    actor_display_name,
    source_event_id,
    reason,
    lifecycle_version,
    created_at
)
SELECT
    'lifecycle-backfill:' || maps.id,
    maps.id,
    'released',
    maps.lifecycle_status,
    maps.completion_outcome,
    'system',
    'migration:0077',
    'Migration 0077',
    '',
    'one_time_backfill',
    maps.lifecycle_version,
    maps.lifecycle_changed_at
FROM mini_production_maps maps
WHERE maps.lifecycle_status <> 'released'
ON CONFLICT (event_id) DO NOTHING;
