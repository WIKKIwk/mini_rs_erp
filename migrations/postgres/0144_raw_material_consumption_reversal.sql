SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE mini_raw_material_events DROP CONSTRAINT mini_rme_event_type_allowed;
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_event_type_allowed CHECK (
    event_type IN ('receipt_posted','order_reserved','order_unreserved','usage_started',
        'consumption_posted','consumption_reversed','adjustment_increase','adjustment_decrease',
        'transfer_in','transfer_out','stock_corrected','stock_deleted','split_consumed',
        'split_produced','delivery_received','layer_remainder_returned')
);
ALTER TABLE mini_raw_material_events DROP CONSTRAINT mini_rme_qty_sign_allowed;
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_qty_sign_allowed CHECK (
    CASE WHEN event_type IN ('receipt_posted','adjustment_increase','transfer_in','split_produced',
                            'layer_remainder_returned','consumption_reversed') THEN qty_delta > 0
         WHEN event_type IN ('consumption_posted','adjustment_decrease','transfer_out',
                            'stock_deleted','split_consumed') THEN qty_delta < 0
         WHEN event_type IN ('order_reserved','order_unreserved','usage_started','delivery_received') THEN qty_delta = 0
         WHEN event_type = 'stock_corrected' THEN TRUE ELSE FALSE END
);
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_consumption_reversal_consistent CHECK (
    event_type <> 'consumption_reversed' OR (
        source_type = 'consumption' AND order_id IS NOT NULL AND btrim(order_id) <> ''
        AND canonical_apparatus_id IS NOT NULL AND btrim(canonical_apparatus_id) <> ''
        AND stock_status_before IS NOT DISTINCT FROM 'consumed'
        AND stock_status_after IS NOT DISTINCT FROM 'in_use'
        AND COALESCE(btrim(payload_json->>'reversed_event_id'), '') <> ''
        AND COALESCE(payload_json->>'reason_code', '') = 'premature_wip_completion'
    )
);
CREATE UNIQUE INDEX mini_rme_consumption_reversal_once
    ON mini_raw_material_events ((payload_json->>'reversed_event_id'))
    WHERE event_type = 'consumption_reversed';
