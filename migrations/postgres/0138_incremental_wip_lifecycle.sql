-- Maintain lifecycle input classes and counts with each roll transaction.
-- A print must not fetch or recount the order's entire roll inventory.
LOCK TABLE mini_progress_batches IN SHARE ROW EXCLUSIVE MODE;

CREATE TABLE mini_progress_batch_work_inputs (
    order_id TEXT NOT NULL,
    route_key TEXT NOT NULL,
    route_json JSONB NOT NULL,
    batch_count BIGINT NOT NULL CHECK (batch_count >= 0),
    PRIMARY KEY (order_id, route_key)
);
CREATE TABLE mini_progress_batch_lifecycle_totals (
    order_id TEXT PRIMARY KEY,
    free_wip_count BIGINT NOT NULL DEFAULT 0 CHECK (free_wip_count >= 0),
    waiting_next_stage_count BIGINT NOT NULL DEFAULT 0 CHECK (waiting_next_stage_count >= 0),
    in_use_wip_count BIGINT NOT NULL DEFAULT 0 CHECK (in_use_wip_count >= 0),
    accepted_wip_count BIGINT NOT NULL DEFAULT 0 CHECK (accepted_wip_count >= 0)
);

CREATE FUNCTION mini_progress_batch_work_route(b mini_progress_batches)
RETURNS JSONB LANGUAGE sql IMMUTABLE AS $$
    SELECT jsonb_build_object(
        'apparatus', b.canonical_apparatus_id,
        'action', b.action, 'wip_status', b.wip_status,
        'current_apparatus', COALESCE(b.canonical_current_apparatus_id, ''),
        'next_apparatus', COALESCE(b.canonical_next_apparatus_id, ''),
        'used_by_apparatus', COALESCE(b.canonical_used_by_apparatus_id, ''),
        'used_by_session_id', CASE WHEN btrim(COALESCE(b.used_by_session_id, '')) = '' THEN '' ELSE 'owned' END,
        'processed_by_apparatus', COALESCE(b.canonical_processed_by_apparatus_id, ''),
        'processed_by_session_id', CASE WHEN btrim(COALESCE(b.processed_by_session_id, '')) = '' THEN '' ELSE 'owned' END,
        'payload_json', jsonb_strip_nulls(jsonb_build_object(
            'stage_node_id', b.payload_json->'stage_node_id',
            'next_stage_node_id', b.payload_json->'next_stage_node_id',
            'wip_route_binding', CASE WHEN b.payload_json ? 'wip_route_binding' THEN
                CASE WHEN jsonb_typeof(b.payload_json->'wip_route_binding') = 'object' THEN
                    jsonb_build_object(
                        'source_stage_node_id', b.payload_json->'wip_route_binding'->'source_stage_node_id',
                        'stage_node_id', b.payload_json->'wip_route_binding'->'stage_node_id',
                        'consumer_apparatus_ids', b.payload_json->'wip_route_binding'->'consumer_apparatus_ids',
                        'map_fingerprint', b.payload_json->'wip_route_binding'->'map_fingerprint',
                        'remapped', b.payload_json->'wip_route_binding'->'remapped')
                ELSE 'false'::jsonb END
            ELSE NULL END)))
$$;

CREATE FUNCTION mini_progress_batch_adjust_work_input(p_order TEXT, p_route JSONB, p_delta BIGINT)
RETURNS void LANGUAGE plpgsql AS $$
DECLARE affected BIGINT;
BEGIN
    IF p_delta > 0 THEN
        INSERT INTO mini_progress_batch_work_inputs (order_id, route_key, route_json, batch_count)
        VALUES (p_order, encode(sha256(convert_to(p_route::text, 'UTF8')), 'hex'), p_route, p_delta)
        ON CONFLICT (order_id, route_key) DO UPDATE
        SET batch_count = mini_progress_batch_work_inputs.batch_count + EXCLUDED.batch_count
        WHERE mini_progress_batch_work_inputs.route_json = EXCLUDED.route_json;
        GET DIAGNOSTICS affected = ROW_COUNT;
        IF affected <> 1 THEN
            RAISE EXCEPTION 'WIP route projection key collision';
        END IF;
    ELSE
        UPDATE mini_progress_batch_work_inputs SET batch_count = batch_count + p_delta
        WHERE order_id = p_order
          AND route_key = encode(sha256(convert_to(p_route::text, 'UTF8')), 'hex')
          AND route_json = p_route;
        GET DIAGNOSTICS affected = ROW_COUNT;
        IF affected <> 1 THEN
            RAISE EXCEPTION 'Missing WIP route projection';
        END IF;
        DELETE FROM mini_progress_batch_work_inputs
        WHERE order_id = p_order
          AND route_key = encode(sha256(convert_to(p_route::text, 'UTF8')), 'hex')
          AND batch_count = 0;
    END IF;
END
$$;

CREATE FUNCTION mini_progress_batch_adjust_lifecycle_totals(b mini_progress_batches, d BIGINT)
RETURNS void LANGUAGE plpgsql AS $$
BEGIN
    UPDATE mini_progress_batch_lifecycle_totals SET
        free_wip_count = free_wip_count + CASE WHEN b.wip_status = 'waiting' AND COALESCE(b.canonical_next_apparatus_id, '') = '' THEN d ELSE 0 END,
        waiting_next_stage_count = waiting_next_stage_count + CASE WHEN b.wip_status = 'waiting' AND COALESCE(b.canonical_next_apparatus_id, '') <> '' THEN d ELSE 0 END,
        in_use_wip_count = in_use_wip_count + CASE WHEN b.wip_status = 'in_use' THEN d ELSE 0 END,
        accepted_wip_count = accepted_wip_count + CASE WHEN b.wip_status = 'processed' AND lower(COALESCE(b.processed_by_apparatus, '')) LIKE 'warehouse:%' THEN d ELSE 0 END
    WHERE order_id = b.order_id;
    IF NOT FOUND THEN RAISE EXCEPTION 'Missing WIP lifecycle totals'; END IF;
END
$$;

CREATE FUNCTION mini_progress_batch_refresh_lifecycle_projection()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE old_route JSONB; new_route JSONB; order_key TEXT;
BEGIN
    IF TG_OP <> 'INSERT' AND OLD.wip_status IN ('waiting', 'in_use') THEN
        old_route := mini_progress_batch_work_route(OLD);
    END IF;
    IF TG_OP <> 'DELETE' AND NEW.wip_status IN ('waiting', 'in_use') THEN
        new_route := mini_progress_batch_work_route(NEW);
    END IF;
    IF TG_OP = 'UPDATE' AND OLD.order_id = NEW.order_id
        AND old_route IS NOT DISTINCT FROM new_route
        AND OLD.wip_status = NEW.wip_status
        AND COALESCE(OLD.canonical_next_apparatus_id, '') = COALESCE(NEW.canonical_next_apparatus_id, '')
        AND (lower(COALESCE(OLD.processed_by_apparatus, '')) LIKE 'warehouse:%')
            = (lower(COALESCE(NEW.processed_by_apparatus, '')) LIKE 'warehouse:%') THEN
        RETURN NULL;
    END IF;
    -- Every path locks the one totals row before touching route-class rows.
    -- Keep cross-order moves ordered too; existing queue transactions retain
    -- their order/apparatus advisory locks and roll ownership row locks.
    FOR order_key IN SELECT DISTINCT id FROM unnest(ARRAY[
        CASE WHEN TG_OP <> 'INSERT' THEN OLD.order_id END,
        CASE WHEN TG_OP <> 'DELETE' THEN NEW.order_id END]) id
        WHERE id IS NOT NULL ORDER BY id LOOP
        INSERT INTO mini_progress_batch_lifecycle_totals (order_id) VALUES (order_key)
        ON CONFLICT (order_id) DO NOTHING;
        PERFORM 1 FROM mini_progress_batch_lifecycle_totals WHERE order_id = order_key FOR UPDATE;
    END LOOP;
    IF TG_OP <> 'INSERT' THEN PERFORM mini_progress_batch_adjust_lifecycle_totals(OLD, -1); END IF;
    IF TG_OP <> 'DELETE' THEN PERFORM mini_progress_batch_adjust_lifecycle_totals(NEW, 1); END IF;
    IF old_route IS NOT NULL THEN
        PERFORM mini_progress_batch_adjust_work_input(OLD.order_id, old_route, -1);
    END IF;
    IF new_route IS NOT NULL THEN
        PERFORM mini_progress_batch_adjust_work_input(NEW.order_id, new_route, 1);
    END IF;
    RETURN NULL;
END
$$;

INSERT INTO mini_progress_batch_work_inputs (order_id, route_key, route_json, batch_count)
SELECT order_id, encode(sha256(convert_to(route_json::text, 'UTF8')), 'hex'), route_json, count(*)
FROM (SELECT b.order_id, mini_progress_batch_work_route(b) AS route_json
      FROM mini_progress_batches b WHERE wip_status IN ('waiting', 'in_use')) routes
GROUP BY order_id, route_json;

INSERT INTO mini_progress_batch_lifecycle_totals
    (order_id, free_wip_count, waiting_next_stage_count, in_use_wip_count, accepted_wip_count)
SELECT order_id,
    count(*) FILTER (WHERE wip_status = 'waiting' AND COALESCE(canonical_next_apparatus_id, '') = ''),
    count(*) FILTER (WHERE wip_status = 'waiting' AND COALESCE(canonical_next_apparatus_id, '') <> ''),
    count(*) FILTER (WHERE wip_status = 'in_use'),
    count(*) FILTER (WHERE wip_status = 'processed' AND lower(COALESCE(processed_by_apparatus, '')) LIKE 'warehouse:%')
FROM mini_progress_batches GROUP BY order_id;

CREATE TRIGGER mini_progress_batch_lifecycle_projection
AFTER INSERT OR UPDATE OR DELETE ON mini_progress_batches
FOR EACH ROW EXECUTE FUNCTION mini_progress_batch_refresh_lifecycle_projection();
