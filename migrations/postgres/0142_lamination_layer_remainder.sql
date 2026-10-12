-- Residual inventory is not another production output. Its immutable
-- operation links the historical roll to one reusable inventory leaf.
SET LOCAL lock_timeout = '5s';
LOCK TABLE mini_progress_batches IN SHARE ROW EXCLUSIVE MODE;
CREATE TABLE mini_layer_remainder_operations (
    operation_id TEXT PRIMARY KEY,
    apparatus TEXT NOT NULL,
    order_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('progress_batch','raw_material')),
    source_id TEXT NOT NULL,
    recorded_by_ref TEXT NOT NULL,
    request_id TEXT NOT NULL CHECK (length(request_id) BETWEEN 8 AND 128),
    request_json JSONB NOT NULL,
    response_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (recorded_by_ref,request_id),
    UNIQUE (source_kind,source_id),
    CHECK (btrim(operation_id)<>'' AND btrim(apparatus)<>'' AND btrim(order_id)<>'' AND btrim(session_id)<>'' AND btrim(source_id)<>'' AND btrim(recorded_by_ref)<>'')
);
CREATE INDEX mini_layer_remainder_order_history ON mini_layer_remainder_operations(order_id,apparatus,created_at DESC);
CREATE TRIGGER mini_layer_remainder_operations_immutable BEFORE UPDATE OR DELETE ON mini_layer_remainder_operations
FOR EACH ROW EXECUTE FUNCTION mini_raw_material_events_block_mutation();
-- An obsolete label can never be resurrected by correction/recovery/retry.
CREATE FUNCTION mini_layer_remainder_source_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_TABLE_NAME='mini_progress_batches' THEN
        IF EXISTS(SELECT 1 FROM mini_layer_remainder_operations WHERE source_kind='progress_batch' AND source_id=OLD.batch_id)
          AND (NEW.wip_status<>'processed' OR NEW.produced_qty IS DISTINCT FROM OLD.produced_qty OR NEW.qr_payload IS DISTINCT FROM OLD.qr_payload
            OR NEW.finished_goods_kg IS DISTINCT FROM OLD.finished_goods_kg OR NEW.bobina_kg IS DISTINCT FROM OLD.bobina_kg OR NEW.finished_goods_meter IS DISTINCT FROM OLD.finished_goods_meter) THEN
            RAISE EXCEPTION 'layer_remainder_source_used';
        END IF;
    ELSE
        IF EXISTS(SELECT 1 FROM mini_layer_remainder_operations WHERE source_kind='raw_material' AND source_id=OLD.id)
          AND (NEW.status<>'consumed' OR NEW.qty IS DISTINCT FROM OLD.qty OR NEW.barcode IS DISTINCT FROM OLD.barcode) THEN
            RAISE EXCEPTION 'layer_remainder_source_used';
        END IF;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER mini_layer_remainder_progress_source_guard BEFORE UPDATE ON mini_progress_batches FOR EACH ROW EXECUTE FUNCTION mini_layer_remainder_source_guard();
CREATE TRIGGER mini_layer_remainder_raw_source_guard BEFORE UPDATE ON mini_raw_material_stock FOR EACH ROW EXECUTE FUNCTION mini_layer_remainder_source_guard();
ALTER TABLE mini_raw_material_events DROP CONSTRAINT mini_rme_event_type_allowed;
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_event_type_allowed CHECK (event_type IN
('receipt_posted','order_reserved','order_unreserved','usage_started','consumption_posted','adjustment_increase','adjustment_decrease','transfer_in','transfer_out','stock_corrected','stock_deleted','split_consumed','split_produced','delivery_received','layer_remainder_returned'));
ALTER TABLE mini_raw_material_events DROP CONSTRAINT mini_rme_source_type_allowed;
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_source_type_allowed CHECK (source_type IN
('gscale_receipt','order_assignment','consumption','manual_adjustment','warehouse_transfer','system','stock_correction','stock_delete','raw_material_split','material_delivery','layer_remainder'));
ALTER TABLE mini_raw_material_events DROP CONSTRAINT mini_rme_qty_sign_allowed;
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_qty_sign_allowed CHECK (
 CASE WHEN event_type IN ('receipt_posted','adjustment_increase','transfer_in','split_produced','layer_remainder_returned') THEN qty_delta>0
      WHEN event_type IN ('consumption_posted','adjustment_decrease','transfer_out','stock_deleted','split_consumed') THEN qty_delta<0
      WHEN event_type IN ('order_reserved','order_unreserved','usage_started','delivery_received') THEN qty_delta=0
      WHEN event_type='stock_corrected' THEN TRUE ELSE FALSE END);
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_layer_remainder_consistent CHECK (
 (event_type<>'layer_remainder_returned' AND source_type<>'layer_remainder') OR
 (event_type='layer_remainder_returned' AND source_type='layer_remainder' AND order_id IS NOT NULL AND apparatus IS NOT NULL AND stock_status_before IS NULL AND stock_status_after IN ('available','reserved')));
CREATE OR REPLACE FUNCTION mini_progress_batch_adjust_lifecycle_totals(b mini_progress_batches, d BIGINT)
RETURNS void LANGUAGE plpgsql AS $$
BEGIN
    IF COALESCE(b.payload_json->>'origin','') = 'layer_remainder' THEN RETURN; END IF;
    UPDATE mini_progress_batch_lifecycle_totals SET
        free_wip_count = free_wip_count + CASE WHEN b.wip_status = 'waiting' AND COALESCE(b.canonical_next_apparatus_id, '') = '' THEN d ELSE 0 END,
        waiting_next_stage_count = waiting_next_stage_count + CASE WHEN b.wip_status = 'waiting' AND COALESCE(b.canonical_next_apparatus_id, '') <> '' THEN d ELSE 0 END,
        in_use_wip_count = in_use_wip_count + CASE WHEN b.wip_status = 'in_use' THEN d ELSE 0 END,
        accepted_wip_count = accepted_wip_count + CASE WHEN b.wip_status = 'processed' AND lower(COALESCE(b.processed_by_apparatus, '')) LIKE 'warehouse:%' THEN d ELSE 0 END
    WHERE order_id = b.order_id;
    IF NOT FOUND THEN RAISE EXCEPTION 'Missing WIP lifecycle totals'; END IF;
END
$$;

CREATE OR REPLACE FUNCTION mini_progress_batch_refresh_lifecycle_projection()
RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE old_route JSONB; new_route JSONB; order_key TEXT;
BEGIN
    IF TG_OP <> 'INSERT' AND OLD.wip_status IN ('waiting', 'in_use') AND COALESCE(OLD.payload_json->>'origin','') <> 'layer_remainder' THEN
        old_route := mini_progress_batch_work_route(OLD);
    END IF;
    IF TG_OP <> 'DELETE' AND NEW.wip_status IN ('waiting', 'in_use') AND COALESCE(NEW.payload_json->>'origin','') <> 'layer_remainder' THEN
        new_route := mini_progress_batch_work_route(NEW);
    END IF;
    IF TG_OP = 'UPDATE' AND COALESCE(OLD.payload_json->>'origin','') = COALESCE(NEW.payload_json->>'origin','') AND OLD.order_id = NEW.order_id
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


DELETE FROM mini_progress_batch_work_inputs;
DELETE FROM mini_progress_batch_lifecycle_totals;
INSERT INTO mini_progress_batch_work_inputs (order_id, route_key, route_json, batch_count)
SELECT order_id, encode(sha256(convert_to(route_json::text, 'UTF8')), 'hex'), route_json, count(*)
FROM (SELECT b.order_id, mini_progress_batch_work_route(b) AS route_json
      FROM mini_progress_batches b WHERE wip_status IN ('waiting', 'in_use') AND COALESCE(b.payload_json->>'origin','') <> 'layer_remainder') routes
GROUP BY order_id, route_json;

INSERT INTO mini_progress_batch_lifecycle_totals
    (order_id, free_wip_count, waiting_next_stage_count, in_use_wip_count, accepted_wip_count)
SELECT order_id,
    count(*) FILTER (WHERE wip_status = 'waiting' AND COALESCE(canonical_next_apparatus_id, '') = ''),
    count(*) FILTER (WHERE wip_status = 'waiting' AND COALESCE(canonical_next_apparatus_id, '') <> ''),
    count(*) FILTER (WHERE wip_status = 'in_use'),
    count(*) FILTER (WHERE wip_status = 'processed' AND lower(COALESCE(processed_by_apparatus, '')) LIKE 'warehouse:%')
FROM mini_progress_batches WHERE COALESCE(payload_json->>'origin','') <> 'layer_remainder' GROUP BY order_id;


DO $$ BEGIN
 IF EXISTS(SELECT 1 FROM pg_roles WHERE rolname='mini_rs_erp') THEN
    REVOKE UPDATE,DELETE ON mini_layer_remainder_operations FROM mini_rs_erp;
    GRANT SELECT,INSERT ON mini_layer_remainder_operations TO mini_rs_erp;
 END IF;
END $$;
