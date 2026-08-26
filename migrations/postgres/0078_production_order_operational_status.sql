ALTER TABLE mini_production_maps
    ADD COLUMN IF NOT EXISTS operational_status TEXT NOT NULL DEFAULT 'not_started',
    ADD COLUMN IF NOT EXISTS operational_status_changed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS completed_with_issue_count BIGINT NOT NULL DEFAULT 0;

ALTER TABLE mini_production_maps
    DROP CONSTRAINT IF EXISTS mini_production_maps_operational_status_allowed,
    DROP CONSTRAINT IF EXISTS mini_production_maps_completed_issue_count_non_negative;

ALTER TABLE mini_production_maps
    ADD CONSTRAINT mini_production_maps_operational_status_allowed CHECK (
        operational_status IN (
            'not_started',
            'ready',
            'in_progress',
            'paused',
            'frozen',
            'waiting_next_stage',
            'partially_completed',
            'completed',
            'completed_with_issue'
        )
    ),
    ADD CONSTRAINT mini_production_maps_completed_issue_count_non_negative CHECK (
        completed_with_issue_count >= 0
    );

WITH event_summary AS (
    SELECT
        order_id,
        count(*) FILTER (
            WHERE COALESCE(payload_json->>'completed_with_issue', 'false') = 'true'
        )::BIGINT AS completed_with_issue_count,
        max(created_at) AS last_event_at
    FROM mini_queue_action_events
    GROUP BY order_id
),
queue_summary AS (
    SELECT
        order_id,
        bool_or(state = 'frozen') AS has_frozen,
        bool_or(state = 'in_progress') AS has_in_progress,
        bool_or(state = 'paused') AS has_paused,
        bool_or(state = 'completed') AS has_completed,
        bool_or(state = 'pending') AS has_pending,
        max(updated_at) AS last_queue_at
    FROM mini_queue_states
    GROUP BY order_id
)
UPDATE mini_production_maps maps
SET operational_status = CASE
        WHEN maps.lifecycle_status IN ('production_completed', 'closed')
            AND COALESCE(events.completed_with_issue_count, 0) > 0
            THEN 'completed_with_issue'
        WHEN maps.lifecycle_status IN ('production_completed', 'closed')
            THEN 'completed'
        WHEN COALESCE(queue.has_frozen, false) THEN 'frozen'
        WHEN COALESCE(queue.has_in_progress, false) THEN 'in_progress'
        WHEN COALESCE(queue.has_paused, false) THEN 'paused'
        WHEN COALESCE(queue.has_completed, false) THEN 'partially_completed'
        WHEN COALESCE(queue.has_pending, false) THEN 'ready'
        ELSE 'not_started'
    END,
    operational_status_changed_at = GREATEST(
        maps.lifecycle_changed_at,
        COALESCE(queue.last_queue_at, maps.lifecycle_changed_at),
        COALESCE(events.last_event_at, maps.lifecycle_changed_at)
    ),
    completed_with_issue_count = COALESCE(events.completed_with_issue_count, 0)
FROM queue_summary queue
FULL JOIN event_summary events ON events.order_id = queue.order_id
WHERE maps.id = COALESCE(queue.order_id, events.order_id);
