-- Replace inherited 0062 display-key indexes after the canonical clean
-- cutover. The original 0062 migration remains immutable; these definitions
-- preserve its concurrency invariants using canonical runtime identity.

DROP INDEX IF EXISTS idx_mini_apparatus_factory_map_object_id_unique;
CREATE UNIQUE INDEX idx_mini_apparatus_factory_map_object_id_unique
    ON mini_apparatus (
        btrim(payload_json #>> '{placement,factory_map_object_id}')
    )
    WHERE btrim(COALESCE(
        payload_json #>> '{placement,factory_map_object_id}',
        ''
    )) <> '';

DROP INDEX IF EXISTS idx_mini_queue_action_events_pending_completion;
CREATE UNIQUE INDEX idx_mini_queue_action_events_pending_completion
    ON mini_queue_action_events (canonical_apparatus_id, order_id)
    WHERE action = 'complete'
      AND payload_json->>'completion_request' = 'true'
      AND COALESCE(payload_json->>'completion_request_status', 'pending') = 'pending';
