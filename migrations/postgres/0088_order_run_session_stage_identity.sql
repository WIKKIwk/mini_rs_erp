ALTER TABLE mini_order_run_sessions
    ADD COLUMN IF NOT EXISTS stage_node_id TEXT NOT NULL DEFAULT '';

UPDATE mini_order_run_sessions
SET stage_node_id = btrim(COALESCE(payload_json->>'stage_node_id', ''))
WHERE stage_node_id = ''
  AND btrim(COALESCE(payload_json->>'stage_node_id', '')) <> '';

UPDATE mini_order_run_sessions
SET payload_json = payload_json - 'stage_node_id'
WHERE payload_json ? 'stage_node_id';

ALTER TABLE mini_order_run_sessions
    DROP CONSTRAINT IF EXISTS mini_order_run_sessions_stage_node_id_trimmed;
ALTER TABLE mini_order_run_sessions
    ADD CONSTRAINT mini_order_run_sessions_stage_node_id_trimmed
    CHECK (stage_node_id = btrim(stage_node_id));

ALTER TABLE mini_order_run_sessions
    DROP CONSTRAINT IF EXISTS mini_order_run_sessions_stage_payload_forbidden;
ALTER TABLE mini_order_run_sessions
    ADD CONSTRAINT mini_order_run_sessions_stage_payload_forbidden
    CHECK (NOT (payload_json ? 'stage_node_id'));
