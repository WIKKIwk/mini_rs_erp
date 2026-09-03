ALTER TABLE mini_production_maps
    ADD COLUMN IF NOT EXISTS flow_status TEXT NOT NULL DEFAULT 'not_started',
    ADD COLUMN IF NOT EXISTS stock_status TEXT NOT NULL DEFAULT '';

ALTER TABLE mini_production_maps
    DROP CONSTRAINT IF EXISTS mini_production_maps_flow_status_allowed,
    DROP CONSTRAINT IF EXISTS mini_production_maps_stock_status_allowed;

ALTER TABLE mini_production_maps
    ADD CONSTRAINT mini_production_maps_flow_status_allowed CHECK (
        flow_status IN (
            'not_started',
            'ready',
            'in_progress',
            'paused',
            'frozen',
            'waiting_next_stage',
            'partially_completed',
            'completed',
            'completed_with_issue',
            'free_wip',
            'accepted_to_stock'
        )
    ),
    ADD CONSTRAINT mini_production_maps_stock_status_allowed CHECK (
        stock_status IN ('', 'accepted')
    );

WITH wip_summary AS (
    SELECT
        order_id,
        count(*) FILTER (
            WHERE wip_status = 'waiting'
              AND (COALESCE(canonical_next_apparatus_id, '') = '' OR canonical_next_apparatus_id IS NULL)
        )::BIGINT AS free_wip_count,
        count(*) FILTER (
            WHERE wip_status = 'waiting'
              AND COALESCE(canonical_next_apparatus_id, '') <> ''
        )::BIGINT AS waiting_next_stage_count,
        count(*) FILTER (
            WHERE wip_status = 'in_use'
        )::BIGINT AS in_use_wip_count,
        count(*) FILTER (
            WHERE wip_status = 'processed'
              AND lower(COALESCE(processed_by_apparatus, '')) LIKE 'warehouse:%'
        )::BIGINT AS accepted_wip_count
    FROM mini_progress_batches
    GROUP BY order_id
)
UPDATE mini_production_maps maps
SET flow_status = CASE
        WHEN COALESCE(wip.free_wip_count, 0) > 0 AND COALESCE(wip.waiting_next_stage_count, 0) = 0
            THEN 'free_wip'
        WHEN COALESCE(wip.accepted_wip_count, 0) > 0
            AND COALESCE(wip.free_wip_count, 0) = 0
            AND COALESCE(wip.waiting_next_stage_count, 0) = 0
            AND COALESCE(wip.in_use_wip_count, 0) = 0
            THEN 'accepted_to_stock'
        WHEN maps.operational_status IN (
            'not_started', 'ready', 'in_progress', 'paused', 'frozen',
            'waiting_next_stage', 'partially_completed', 'completed', 'completed_with_issue'
        ) THEN maps.operational_status
        ELSE 'not_started'
    END,
    stock_status = CASE
        WHEN COALESCE(wip.accepted_wip_count, 0) > 0
            AND COALESCE(wip.free_wip_count, 0) = 0
            AND COALESCE(wip.waiting_next_stage_count, 0) = 0
            AND COALESCE(wip.in_use_wip_count, 0) = 0
            THEN 'accepted'
        ELSE ''
    END
FROM wip_summary wip
WHERE maps.id = wip.order_id;

UPDATE mini_production_maps
SET flow_status = CASE
        WHEN operational_status IN (
            'not_started', 'ready', 'in_progress', 'paused', 'frozen',
            'waiting_next_stage', 'partially_completed', 'completed', 'completed_with_issue'
        ) THEN operational_status
        ELSE 'not_started'
    END,
    stock_status = ''
WHERE flow_status = 'not_started' AND NOT EXISTS (
    SELECT 1 FROM mini_progress_batches WHERE mini_progress_batches.order_id = mini_production_maps.id
);
