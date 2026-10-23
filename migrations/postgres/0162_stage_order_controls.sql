-- Legacy order controls retain their global meaning. Local stops have their
-- own key so releasing a global stop cannot erase a stage stop.
-- Old queue-only stops predate explicit controls; preserve their global meaning.
INSERT INTO mini_order_control_states (
    order_id, state, actor_role, actor_ref, actor_display_name,
    requested_at_unix, frozen_at_unix
)
SELECT queue.order_id, 'frozen', 'system', 'legacy-global-freeze',
       'Legacy global freeze', EXTRACT(EPOCH FROM MAX(queue.updated_at))::BIGINT,
       EXTRACT(EPOCH FROM MAX(queue.updated_at))::BIGINT
FROM mini_queue_states queue
JOIN mini_production_maps orders ON orders.id = queue.order_id
WHERE queue.state = 'frozen'
GROUP BY queue.order_id
ON CONFLICT (order_id) DO NOTHING;

CREATE TABLE mini_stage_order_controls (
    order_id TEXT NOT NULL REFERENCES mini_production_maps(id) ON DELETE CASCADE,
    canonical_apparatus_id TEXT NOT NULL REFERENCES mini_apparatus(id),
    stage_node_id TEXT NOT NULL CHECK (btrim(stage_node_id) <> ''),
    record_json JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (order_id, canonical_apparatus_id, stage_node_id),
    CHECK (record_json ?& ARRAY['order_id','stage_node_id','state','freeze_request']),
    CHECK (record_json#>>'{freeze_request,target_apparatus}' = canonical_apparatus_id),
    CHECK (record_json->>'order_id' = order_id),
    CHECK (record_json->>'stage_node_id' = stage_node_id),
    CHECK (record_json->>'state' IN ('active', 'freeze_requested', 'frozen'))
);

ALTER TABLE mini_order_freeze_requests ADD COLUMN stage_node_id TEXT NOT NULL DEFAULT '';
ALTER TABLE mini_order_freeze_requests DROP CONSTRAINT mini_order_freeze_requests_target_complete;
ALTER TABLE mini_order_freeze_requests ADD CONSTRAINT mini_order_freeze_requests_target_complete CHECK (
    (stage_node_id <> '' AND canonical_target_apparatus_id IS NOT NULL)
    OR (stage_node_id = '' AND (
        (target_session_id = '' AND target_apparatus = '' AND target_worker_role IN ('', 'aparatchi')
         AND target_worker_ref = '' AND target_worker_display_name = '')
        OR (target_apparatus <> '' AND target_worker_role <> '' AND target_worker_ref <> '')
    ))
);

CREATE TRIGGER mini_production_live_changed AFTER INSERT OR UPDATE OR DELETE
    ON mini_stage_order_controls FOR EACH ROW EXECUTE FUNCTION mini_notify_production_live();
CREATE TRIGGER mini_production_live_truncated AFTER TRUNCATE
    ON mini_stage_order_controls FOR EACH STATEMENT EXECUTE FUNCTION mini_notify_production_live();
