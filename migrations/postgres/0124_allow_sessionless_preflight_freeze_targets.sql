-- Colour preflight freezes have a real apparatus and worker target before a
-- production session exists.  Keep the target complete, but allow the
-- session id to be empty for this preflight-only state.
ALTER TABLE mini_order_freeze_requests
    DROP CONSTRAINT IF EXISTS mini_order_freeze_requests_target_complete;

ALTER TABLE mini_order_freeze_requests
    ADD CONSTRAINT mini_order_freeze_requests_target_complete CHECK (
        (target_session_id = '' AND target_apparatus = ''
            AND target_worker_role = '' AND target_worker_ref = ''
            AND target_worker_display_name = '')
        OR
        (target_apparatus <> '' AND target_worker_role <> ''
            AND target_worker_ref <> '')
    );
