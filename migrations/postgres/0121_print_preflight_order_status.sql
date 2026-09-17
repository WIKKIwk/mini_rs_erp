-- Colour matching is a durable queue/order status, not an expiring UI hold.
ALTER TABLE mini_queue_states DROP CONSTRAINT mini_queue_states_state_allowed;
ALTER TABLE mini_queue_states ADD CONSTRAINT mini_queue_states_state_allowed
    CHECK (state IN ('pending', 'print_preflight', 'in_progress', 'paused', 'frozen', 'completed'));

ALTER TABLE mini_queue_action_events DROP CONSTRAINT mini_queue_action_events_from_state_allowed;
ALTER TABLE mini_queue_action_events ADD CONSTRAINT mini_queue_action_events_from_state_allowed
    CHECK (from_state IN ('pending', 'print_preflight', 'in_progress', 'paused', 'frozen', 'completed'));
ALTER TABLE mini_queue_action_events DROP CONSTRAINT mini_queue_action_events_to_state_allowed;
ALTER TABLE mini_queue_action_events ADD CONSTRAINT mini_queue_action_events_to_state_allowed
    CHECK (to_state IN ('pending', 'print_preflight', 'in_progress', 'paused', 'frozen', 'completed'));

ALTER TABLE mini_production_maps DROP CONSTRAINT mini_production_maps_operational_status_allowed;
ALTER TABLE mini_production_maps ADD CONSTRAINT mini_production_maps_operational_status_allowed
    CHECK (operational_status IN ('not_started', 'ready', 'print_preflight', 'in_progress',
        'paused', 'frozen', 'waiting_next_stage', 'partially_completed', 'completed', 'completed_with_issue'));
ALTER TABLE mini_production_maps DROP CONSTRAINT mini_production_maps_flow_status_allowed;
ALTER TABLE mini_production_maps ADD CONSTRAINT mini_production_maps_flow_status_allowed
    CHECK (flow_status IN ('not_started', 'ready', 'print_preflight', 'in_progress', 'paused',
        'frozen', 'waiting_next_stage', 'partially_completed', 'completed', 'completed_with_issue',
        'free_wip', 'accepted_to_stock'));

ALTER TABLE mini_print_preflight_holds ADD COLUMN previous_queue_state TEXT;
-- Do not resurrect trials already expired under the previous implementation.
UPDATE mini_print_preflight_holds SET status = 'cancelled'
WHERE status IN ('held', 'running', 'passed')
  AND expires_at_unix <= EXTRACT(EPOCH FROM now())::BIGINT;
UPDATE mini_print_preflight_holds SET status = 'running' WHERE status = 'held';

UPDATE mini_print_preflight_holds h
SET previous_queue_state = q.state
FROM mini_queue_states q
WHERE h.status IN ('held', 'running', 'passed')
  AND q.canonical_apparatus_id = h.canonical_apparatus_id AND q.order_id = h.order_id;

INSERT INTO mini_queue_states (apparatus, canonical_apparatus_id, order_id, state, updated_at)
SELECT COALESCE(a.name, h.canonical_apparatus_id), h.canonical_apparatus_id,
       h.order_id, 'print_preflight', now()
FROM mini_print_preflight_holds h
JOIN mini_production_maps m ON m.id = h.order_id
LEFT JOIN mini_apparatus a ON a.id = h.canonical_apparatus_id
WHERE h.status IN ('held', 'running', 'passed')
ON CONFLICT (canonical_apparatus_id, order_id)
DO UPDATE SET state = EXCLUDED.state, updated_at = now();

UPDATE mini_production_maps m
SET operational_status = 'print_preflight', flow_status = 'print_preflight',
    operational_status_changed_at = now()
WHERE lifecycle_status IN ('released', 'in_progress')
  AND EXISTS (SELECT 1 FROM mini_queue_states q WHERE q.order_id = m.id AND q.state = 'print_preflight')
  AND NOT EXISTS (SELECT 1 FROM mini_queue_states q WHERE q.order_id = m.id AND q.state = 'frozen');
