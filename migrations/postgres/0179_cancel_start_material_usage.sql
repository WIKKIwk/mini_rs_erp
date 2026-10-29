SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- A fresh material claim can be undone without changing the physical quantity.
-- Keep ordinary stock corrections and consumption reversals under their
-- existing stricter contracts.
ALTER TABLE mini_raw_material_events DROP CONSTRAINT mini_rme_event_type_allowed;
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_event_type_allowed CHECK (
    event_type IN ('receipt_posted','order_reserved','order_unreserved','usage_started',
        'usage_cancelled','consumption_posted','consumption_reversed','adjustment_increase',
        'adjustment_decrease','transfer_in','transfer_out','stock_corrected','stock_deleted',
        'split_consumed','split_produced','delivery_received','layer_remainder_returned')
);
ALTER TABLE mini_raw_material_events DROP CONSTRAINT mini_rme_qty_sign_allowed;
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_qty_sign_allowed CHECK (
    CASE WHEN event_type IN ('receipt_posted','adjustment_increase','transfer_in','split_produced',
                            'layer_remainder_returned','consumption_reversed') THEN qty_delta > 0
         WHEN event_type IN ('consumption_posted','adjustment_decrease','transfer_out',
                            'stock_deleted','split_consumed') THEN qty_delta < 0
         WHEN event_type IN ('order_reserved','order_unreserved','usage_started',
                            'usage_cancelled','delivery_received') THEN qty_delta = 0
         WHEN event_type = 'stock_corrected' THEN TRUE ELSE FALSE END
);
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_usage_cancellation_consistent CHECK (
    event_type <> 'usage_cancelled' OR (
        source_type = 'consumption' AND stock_status_before = 'in_use'
        AND stock_status_after = 'available' AND order_id IS NOT NULL AND btrim(order_id) <> ''
        AND canonical_apparatus_id IS NOT NULL AND btrim(canonical_apparatus_id) <> ''
        AND COALESCE(payload_json->>'reason_code','') = 'cancel_start'
        AND COALESCE(btrim(payload_json->>'start_event_id'),'') <> ''
    )
);
