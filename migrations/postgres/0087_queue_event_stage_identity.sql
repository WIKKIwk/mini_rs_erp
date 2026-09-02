ALTER TABLE mini_queue_action_events
    ADD COLUMN IF NOT EXISTS stage_node_id TEXT NOT NULL DEFAULT '';

UPDATE mini_queue_action_events
SET stage_node_id = btrim(COALESCE(payload_json->>'stage_node_id', ''))
WHERE stage_node_id = ''
  AND btrim(COALESCE(payload_json->>'stage_node_id', '')) <> '';

UPDATE mini_queue_action_events
SET payload_json = payload_json - 'stage_node_id'
WHERE payload_json ? 'stage_node_id';

ALTER TABLE mini_queue_action_events
    DROP CONSTRAINT IF EXISTS mini_queue_action_events_stage_node_id_trimmed;
ALTER TABLE mini_queue_action_events
    ADD CONSTRAINT mini_queue_action_events_stage_node_id_trimmed
    CHECK (stage_node_id = btrim(stage_node_id));

ALTER TABLE mini_queue_action_events
    DROP CONSTRAINT IF EXISTS mini_queue_action_events_stage_payload_forbidden;
ALTER TABLE mini_queue_action_events
    ADD CONSTRAINT mini_queue_action_events_stage_payload_forbidden
    CHECK (NOT (payload_json ? 'stage_node_id'));

CREATE INDEX IF NOT EXISTS idx_mini_queue_action_events_order_stage_created
    ON mini_queue_action_events (order_id, stage_node_id, created_at DESC)
    WHERE stage_node_id <> '';
