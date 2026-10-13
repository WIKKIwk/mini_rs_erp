-- One-time repair for the verified partial-roll consumption in local order 0054.
-- Keeps the original event and supplier ownership; replay never restores twice.
BEGIN;
DO $$
DECLARE
    target_order CONSTANT text := 'zakaz-0054';
    target_apparatus CONSTANT text := 'apparatus:default:asset-007';
    target_barcode CONSTANT text := '30DDE6C27B4A9230F0A13CA9';
    original_event CONSTANT text := 'rme_47f878e2c7b58a2accf76db6c208329c';
    failed_session CONSTANT text := 'order-run:1791844578597507000:apparatus-default-asset-007:zakaz-0054:worker-d5f3bafec0949a7e26073f5d';
    original mini_raw_material_events%ROWTYPE;
    stock mini_raw_material_stock%ROWTYPE;
    previous_run mini_order_run_sessions%ROWTYPE;
    current_run mini_order_run_sessions%ROWTYPE;
    repair_key text;
    repair_event text;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended('mini-rs-erp:production-map:order:' || target_order, 0));
    PERFORM pg_advisory_xact_lock(hashtextextended('mini-rs-erp:production-map:apparatus:' || target_apparatus, 0));
    repair_key := 'raw-consumption-recovery:' || original_event;
    repair_event := 'rme_recovery_' || md5(original_event);
    IF EXISTS (SELECT 1 FROM mini_raw_material_events WHERE idempotency_key = repair_key) THEN
        RETURN;
    END IF;

    SELECT * INTO STRICT original FROM mini_raw_material_events WHERE event_id = original_event;
    SELECT * INTO STRICT stock FROM mini_raw_material_stock WHERE lower(barcode) = lower(target_barcode) FOR UPDATE;
    SELECT * INTO STRICT previous_run FROM mini_order_run_sessions WHERE session_id = failed_session FOR UPDATE;
    SELECT * INTO STRICT current_run FROM mini_order_run_sessions
      WHERE order_id = target_order AND canonical_apparatus_id = target_apparatus
      ORDER BY started_at DESC, session_id DESC LIMIT 1 FOR UPDATE;
    IF original.event_type <> 'consumption_posted' OR original.source_type <> 'consumption'
       OR original.order_id IS DISTINCT FROM target_order
       OR original.canonical_apparatus_id IS DISTINCT FROM target_apparatus
       OR original.idempotency_key <> 'consumption_posted:' || target_barcode || ':' || target_order || ':' || target_apparatus
       OR upper(original.barcode) <> target_barcode OR original.qty_delta <> -stock.qty
       OR stock.status <> 'consumed' OR stock.reserved_order_id <> target_order OR stock.uom <> 'kg'
       OR previous_run.order_id <> target_order OR previous_run.canonical_apparatus_id <> target_apparatus
       OR previous_run.status <> 'completed' OR previous_run.payload_json->>'last_action' IS DISTINCT FROM 'complete'
       OR previous_run.payload_json ? 'stage_work_report'
       OR original.occurred_at < previous_run.updated_at
       OR original.occurred_at >= previous_run.updated_at + interval '1 second'
       OR current_run.session_id = failed_session
       OR current_run.status NOT IN ('active', 'paused', 'roll_detached')
       OR EXISTS (SELECT 1 FROM mini_layer_remainder_operations WHERE source_kind = 'raw_material' AND source_id = stock.id)
       OR COALESCE(stock.payload_json->>'inventory_transfer_id', '') <> ''
       OR COALESCE(stock.payload_json->>'raw_material_split_id', '') <> '' THEN
        RAISE EXCEPTION 'Raw consumption recovery witnesses changed; no data was modified';
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM mini_raw_material_assignments r
        JOIN mini_inventory_placements p ON p.asset_kind = 'raw_material' AND p.asset_ref = stock.id
        JOIN mini_inventory_locations l ON l.id = p.physical_location_id
        JOIN mini_factory_location_apparatus_links a ON a.location_id = l.factory_location_id
        WHERE lower(r.barcode) = lower(target_barcode) AND r.order_id = target_order
          AND r.canonical_apparatus_id = target_apparatus
          AND l.kind = 'state' AND l.active AND a.apparatus_id = target_apparatus
    ) THEN
        RAISE EXCEPTION 'The raw roll is no longer mounted for this order and apparatus';
    END IF;

    INSERT INTO mini_raw_material_events (
        event_id, idempotency_key, event_type, warehouse, barcode, item_code, item_name,
        qty_delta, uom, stock_status_before, stock_status_after, order_id, apparatus, canonical_apparatus_id,
        actor_role, actor_ref, actor_display_name, owner_role, owner_ref, owner_display_name,
        source_type, source_id, correlation_id, payload_json
    ) VALUES (
        repair_event, repair_key, 'consumption_reversed', original.warehouse, original.barcode,
        original.item_code, original.item_name, -original.qty_delta, original.uom, 'consumed', 'in_use',
        target_order, target_apparatus, target_apparatus, 'system', 'maintenance:raw-consumption-recovery',
        'Wikki raw consumption recovery', original.owner_role, original.owner_ref, original.owner_display_name,
        'consumption', stock.id, original.event_id,
        jsonb_build_object('reason_code', 'premature_wip_completion', 'reversed_event_id', original.event_id,
            'failed_session_id', failed_session, 'current_session_id', current_run.session_id,
            'source_qty', stock.qty, 'measurement_pending', true,
            'reason', 'Preserve mounted lamination material across WIP rolls; user-authorized recovery')
    );
    UPDATE mini_raw_material_stock SET status = 'in_use',
        payload_json = (payload_json - 'consumed_order_id') || jsonb_build_object(
            'in_use_order_id', target_order, 'consumption_recovery_event_id', repair_event),
        updated_at = now()
      WHERE id = stock.id;
END $$;
COMMIT;
