-- Invalidate an unused Resume at the same durable mutation boundary as work.
-- Checkpoints live in the run payload; output/WIP identities are never rewound.
CREATE FUNCTION mini_resume_pause_session_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.payload_json->'_resume_pause' IS NOT DISTINCT FROM OLD.payload_json->'_resume_pause' THEN
        NEW.payload_json := NEW.payload_json - '_resume_pause';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER mini_resume_pause_session_guard BEFORE UPDATE ON mini_order_run_sessions
FOR EACH ROW EXECUTE FUNCTION mini_resume_pause_session_guard();

CREATE FUNCTION mini_resume_pause_invalidate() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE row_data JSONB; order_key TEXT; apparatus_key TEXT;
BEGIN
    -- Resume writes its zero-quantity event after installing its checkpoint.
    IF TG_TABLE_NAME = 'mini_order_progress_events' AND to_jsonb(NEW)->>'action' = 'resume' THEN
        RETURN NULL;
    END IF;
    FOR row_data IN SELECT value FROM jsonb_array_elements(
        CASE WHEN TG_OP = 'DELETE' THEN jsonb_build_array(to_jsonb(OLD))
             WHEN TG_OP = 'INSERT' THEN jsonb_build_array(to_jsonb(NEW))
             ELSE jsonb_build_array(to_jsonb(OLD), to_jsonb(NEW)) END) LOOP
        order_key := CASE WHEN TG_TABLE_NAME = 'mini_production_maps' THEN row_data->>'id'
                          ELSE row_data->>'order_id' END;
        apparatus_key := COALESCE(NULLIF(row_data->>'canonical_apparatus_id', ''), row_data->>'apparatus', '');
        UPDATE mini_order_run_sessions SET payload_json = payload_json - '_resume_pause'
        WHERE order_id = order_key AND payload_json ? '_resume_pause'
          AND (apparatus_key = '' OR canonical_apparatus_id = apparatus_key OR apparatus = apparatus_key);
    END LOOP;
    RETURN NULL;
END $$;

CREATE TRIGGER mini_resume_pause_queue_action AFTER INSERT ON mini_queue_action_events
FOR EACH ROW EXECUTE FUNCTION mini_resume_pause_invalidate();
CREATE TRIGGER mini_resume_pause_progress AFTER INSERT ON mini_order_progress_events
FOR EACH ROW EXECUTE FUNCTION mini_resume_pause_invalidate();
CREATE TRIGGER mini_resume_pause_queue_state AFTER INSERT OR UPDATE OR DELETE ON mini_queue_states
FOR EACH ROW EXECUTE FUNCTION mini_resume_pause_invalidate();
CREATE TRIGGER mini_resume_pause_material_events AFTER INSERT ON mini_raw_material_events
FOR EACH ROW EXECUTE FUNCTION mini_resume_pause_invalidate();
CREATE TRIGGER mini_resume_pause_material_assignment AFTER INSERT OR UPDATE OR DELETE ON mini_raw_material_assignments
FOR EACH ROW EXECUTE FUNCTION mini_resume_pause_invalidate();
CREATE TRIGGER mini_resume_pause_bosma_report AFTER INSERT ON mini_bosma_astatka_reports
FOR EACH ROW EXECUTE FUNCTION mini_resume_pause_invalidate();
CREATE TRIGGER mini_resume_pause_lamination_report AFTER INSERT ON mini_laminatsiya_astatka_reports
FOR EACH ROW EXECUTE FUNCTION mini_resume_pause_invalidate();
CREATE TRIGGER mini_resume_pause_rezka_report AFTER INSERT ON mini_rezka_astatka_reports
FOR EACH ROW EXECUTE FUNCTION mini_resume_pause_invalidate();
CREATE TRIGGER mini_resume_pause_layer_remainder AFTER INSERT ON mini_layer_remainder_operations
FOR EACH ROW EXECUTE FUNCTION mini_resume_pause_invalidate();
CREATE TRIGGER mini_resume_pause_control AFTER INSERT OR UPDATE OR DELETE ON mini_order_control_states
FOR EACH ROW EXECUTE FUNCTION mini_resume_pause_invalidate();
CREATE TRIGGER mini_resume_pause_map AFTER UPDATE OF map_json ON mini_production_maps
FOR EACH ROW WHEN (OLD.map_json IS DISTINCT FROM NEW.map_json)
EXECUTE FUNCTION mini_resume_pause_invalidate();

CREATE INDEX mini_resume_pause_latest_queue_action
ON mini_queue_action_events(order_id, canonical_apparatus_id, id DESC);
