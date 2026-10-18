-- Production timestamps are facts about a physical output. Neither the
-- parent session's updated_at nor a later batch edit is a production event.
ALTER TABLE mini_progress_batches
    ADD COLUMN IF NOT EXISTS production_started_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS production_completed_at TIMESTAMPTZ;

-- Replay immutable events in insertion order (several can share a second).
-- A freeze/handoff/merge continues the same roll. A real output closes it;
-- resume starts the next one. Individual Rezka frame recordings do not close
-- the other frames. Never use today's mutable session payload as past history.
DO $$
DECLARE
    work RECORD;
    event RECORD;
    cycle_start TIMESTAMPTZ;
    cycle_source TEXT;
    waiting BOOLEAN;
    before_resume_start TIMESTAMPTZ;
    before_resume_source TEXT;
    before_resume_waiting BOOLEAN;
    output_count INTEGER;
    derived_count INTEGER;
    last_action TEXT;
BEGIN
    FOR work IN SELECT * FROM mini_order_run_sessions ORDER BY session_id LOOP
        cycle_start := date_trunc('second', work.started_at);
        cycle_source := 'session_start';
        waiting := work.payload_json ? '_reactivation_of_session_id';
        before_resume_start := cycle_start;
        before_resume_source := cycle_source;
        before_resume_waiting := waiting;
        last_action := '';
        FOR event IN
            SELECT * FROM mini_order_progress_events
            WHERE session_id = work.session_id ORDER BY id
        LOOP
            last_action := event.action;
            IF event.action = 'start' THEN
                cycle_start := date_trunc('second', event.created_at);
                cycle_source := 'start_event';
                waiting := false;
            ELSIF event.action = 'resume' THEN
                before_resume_start := cycle_start;
                before_resume_source := cycle_source;
                before_resume_waiting := waiting;
                IF waiting THEN
                    cycle_start := date_trunc('second', event.created_at);
                    cycle_source := 'resume_event';
                    waiting := false;
                END IF;
            ELSIF event.payload_json->>'event' = 're_pause' THEN
                cycle_start := before_resume_start;
                cycle_source := before_resume_source;
                waiting := before_resume_waiting;
            END IF;

            IF event.action IN ('pause', 'detach_roll', 'roll_complete', 'complete') THEN
                UPDATE mini_progress_batches AS batch SET
                    production_started_at = CASE
                        WHEN cycle_start <= event.created_at THEN cycle_start END,
                    production_completed_at = date_trunc('second', event.created_at),
                    payload_json = batch.payload_json || jsonb_build_object(
                        'production_timing_source', jsonb_build_object(
                            'start', CASE WHEN cycle_start <= event.created_at
                                THEN cycle_source ELSE 'unknown' END,
                            'completion', 'output_event', 'event_id', event.event_id))
                WHERE batch.session_id = work.session_id
                    AND batch.production_completed_at IS NULL
                    AND COALESCE(batch.payload_json->>'origin', '') <> 'layer_remainder'
                    AND (batch.batch_id = event.batch_id OR EXISTS (
                        SELECT 1 FROM jsonb_array_elements(CASE
                            WHEN jsonb_typeof(event.payload_json->'rezka_output_batches') = 'array'
                            THEN event.payload_json->'rezka_output_batches' ELSE '[]'::jsonb END) item
                        WHERE item->>'batch_id' = batch.batch_id));
                GET DIAGNOSTICS output_count = ROW_COUNT;

                IF event.payload_json->>'rezka_record_frame_index' IS NULL THEN
                    IF event.action = 'roll_complete' THEN
                        cycle_start := date_trunc('second', event.created_at);
                        cycle_source := 'previous_roll_complete_event';
                        waiting := false;
                    ELSIF event.action IN ('pause', 'detach_roll') AND (
                        event.produced_qty > 0 OR output_count > 0 OR jsonb_array_length(CASE
                            WHEN jsonb_typeof(event.payload_json->'rezka_previously_recorded_batches') = 'array'
                            THEN event.payload_json->'rezka_previously_recorded_batches' ELSE '[]'::jsonb END) > 0
                        OR jsonb_array_length(CASE
                            WHEN jsonb_typeof(event.payload_json->'rezka_frame_issues') = 'array'
                            THEN event.payload_json->'rezka_frame_issues' ELSE '[]'::jsonb END) > 0
                    ) THEN
                        waiting := true;
                    END IF;
                END IF;
            END IF;
        END LOOP;

        -- Existing reversible-resume checkpoints hash their whole payload.
        -- Keep them intact; the application seeds their clock on the next output.
        UPDATE mini_order_run_sessions SET payload_json = payload_json || jsonb_build_object(
            'wip_started_at_unix', EXTRACT(EPOCH FROM cycle_start)::bigint,
            'wip_waiting_for_resume', waiting)
        WHERE session_id = work.session_id AND status <> 'completed'
            AND NOT (payload_json ? '_resume_pause' AND last_action IN ('', 'resume'))
            AND NOT (payload_json ? 'wip_started_at_unix');
    END LOOP;

    UPDATE mini_progress_batches SET
        production_completed_at = date_trunc('second', created_at),
        payload_json = payload_json || jsonb_build_object('production_timing_source',
            jsonb_build_object('start', 'unknown', 'completion', 'batch_created_at'))
    WHERE production_completed_at IS NULL
        AND COALESCE(payload_json->>'origin', '') <> 'layer_remainder';

    -- A split remainder retains the upstream roll's production history. Its
    -- own creation time describes the split, not a second production operation.
    LOOP
        UPDATE mini_progress_batches AS child SET
            production_started_at = parent.production_started_at,
            production_completed_at = parent.production_completed_at,
            payload_json = child.payload_json || jsonb_build_object(
                'production_timing_source', jsonb_build_object('start', 'parent_batch',
                    'completion', 'parent_batch', 'batch_id', parent.batch_id))
        FROM mini_progress_batches AS parent
        WHERE child.parent_batch_id = parent.batch_id
            AND child.payload_json->>'origin' = 'layer_remainder'
            AND child.production_completed_at IS NULL
            AND parent.production_completed_at IS NOT NULL;
        GET DIAGNOSTICS derived_count = ROW_COUNT;
        EXIT WHEN derived_count = 0;
    END LOOP;
END $$;

-- Missing historical evidence must not manufacture a session-wide interval.
-- Keep the known creation instant, expose an unknown start as API zero/blank,
-- and retain provenance for review instead of silently claiming an exact time.
UPDATE mini_progress_batches SET
    production_completed_at = date_trunc('second', created_at),
    payload_json = payload_json || jsonb_build_object('production_timing_source',
        jsonb_build_object('start', 'unknown', 'completion', 'batch_created_at'))
WHERE production_completed_at IS NULL;

ALTER TABLE mini_progress_batches
    ALTER COLUMN production_completed_at SET NOT NULL,
    ALTER COLUMN production_completed_at SET DEFAULT now(),
    ADD CONSTRAINT mini_progress_batches_production_interval_valid
        CHECK (production_started_at IS NULL OR production_started_at <= production_completed_at);

CREATE FUNCTION mini_keep_wip_production_times() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.production_started_at IS DISTINCT FROM OLD.production_started_at
        OR NEW.production_completed_at IS DISTINCT FROM OLD.production_completed_at THEN
        RAISE EXCEPTION 'WIP production timestamps are immutable for batch %', OLD.batch_id
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END $$;

CREATE TRIGGER mini_progress_batches_keep_production_times
    BEFORE UPDATE ON mini_progress_batches
    FOR EACH ROW EXECUTE FUNCTION mini_keep_wip_production_times();
