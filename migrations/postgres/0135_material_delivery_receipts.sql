-- Delivery receipt records placement without changing stock quantity or status.
ALTER TABLE mini_raw_material_events DROP CONSTRAINT mini_rme_event_type_allowed;
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_event_type_allowed CHECK (
    event_type IN ('receipt_posted', 'order_reserved', 'order_unreserved', 'usage_started',
        'consumption_posted', 'adjustment_increase', 'adjustment_decrease', 'transfer_in',
        'transfer_out', 'stock_corrected', 'stock_deleted', 'split_consumed', 'split_produced',
        'delivery_received'));

ALTER TABLE mini_raw_material_events DROP CONSTRAINT mini_rme_source_type_allowed;
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_source_type_allowed CHECK (
    source_type IN ('gscale_receipt', 'order_assignment', 'consumption', 'manual_adjustment',
        'warehouse_transfer', 'system', 'stock_correction', 'stock_delete', 'raw_material_split',
        'material_delivery'));

ALTER TABLE mini_raw_material_events DROP CONSTRAINT mini_rme_qty_sign_allowed;
ALTER TABLE mini_raw_material_events ADD CONSTRAINT mini_rme_qty_sign_allowed CHECK (
    CASE
        WHEN event_type IN ('receipt_posted', 'adjustment_increase', 'transfer_in', 'split_produced')
            THEN qty_delta > 0
        WHEN event_type IN ('consumption_posted', 'adjustment_decrease', 'transfer_out',
            'stock_deleted', 'split_consumed') THEN qty_delta < 0
        WHEN event_type IN ('order_reserved', 'order_unreserved', 'usage_started', 'delivery_received')
            THEN qty_delta = 0
        WHEN event_type = 'stock_corrected' THEN TRUE
        ELSE FALSE
    END);
