SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE mini_raw_material_stock
    DROP CONSTRAINT IF EXISTS mini_raw_material_stock_status_allowed;

ALTER TABLE mini_raw_material_stock
    ADD CONSTRAINT mini_raw_material_stock_status_allowed
    CHECK (status IN ('available', 'reserved', 'in_use', 'consumed', 'deleted'));

ALTER TABLE mini_raw_material_events
    DROP CONSTRAINT IF EXISTS mini_rme_event_type_allowed;
ALTER TABLE mini_raw_material_events
    ADD CONSTRAINT mini_rme_event_type_allowed CHECK (
        event_type IN (
            'receipt_posted', 'order_reserved', 'order_unreserved',
            'usage_started', 'consumption_posted', 'adjustment_increase',
            'adjustment_decrease', 'transfer_in', 'transfer_out',
            'stock_corrected', 'stock_deleted'
        )
    ) NOT VALID;
ALTER TABLE mini_raw_material_events
    VALIDATE CONSTRAINT mini_rme_event_type_allowed;

ALTER TABLE mini_raw_material_events
    DROP CONSTRAINT IF EXISTS mini_rme_source_type_allowed;
ALTER TABLE mini_raw_material_events
    ADD CONSTRAINT mini_rme_source_type_allowed CHECK (
        source_type IN (
            'gscale_receipt', 'order_assignment', 'consumption',
            'manual_adjustment', 'warehouse_transfer', 'system',
            'stock_correction', 'stock_delete'
        )
    ) NOT VALID;
ALTER TABLE mini_raw_material_events
    VALIDATE CONSTRAINT mini_rme_source_type_allowed;

ALTER TABLE mini_raw_material_events
    DROP CONSTRAINT IF EXISTS mini_rme_status_before_allowed;
ALTER TABLE mini_raw_material_events
    ADD CONSTRAINT mini_rme_status_before_allowed CHECK (
        stock_status_before IS NULL OR
        stock_status_before IN ('available', 'reserved', 'in_use', 'consumed', 'deleted')
    ) NOT VALID;
ALTER TABLE mini_raw_material_events
    VALIDATE CONSTRAINT mini_rme_status_before_allowed;

ALTER TABLE mini_raw_material_events
    DROP CONSTRAINT IF EXISTS mini_rme_status_after_allowed;
ALTER TABLE mini_raw_material_events
    ADD CONSTRAINT mini_rme_status_after_allowed CHECK (
        stock_status_after IS NULL OR
        stock_status_after IN ('available', 'reserved', 'in_use', 'consumed', 'deleted')
    ) NOT VALID;
ALTER TABLE mini_raw_material_events
    VALIDATE CONSTRAINT mini_rme_status_after_allowed;

ALTER TABLE mini_raw_material_events
    DROP CONSTRAINT IF EXISTS mini_rme_qty_sign_allowed;
ALTER TABLE mini_raw_material_events
    ADD CONSTRAINT mini_rme_qty_sign_allowed CHECK (
        CASE
            WHEN event_type IN ('receipt_posted', 'adjustment_increase', 'transfer_in')
                THEN qty_delta > 0
            WHEN event_type IN (
                'consumption_posted', 'adjustment_decrease', 'transfer_out',
                'stock_deleted'
            )
                THEN qty_delta < 0
            WHEN event_type IN ('order_reserved', 'order_unreserved', 'usage_started')
                THEN qty_delta = 0
            WHEN event_type = 'stock_corrected'
                THEN TRUE
            ELSE FALSE
        END
    ) NOT VALID;
ALTER TABLE mini_raw_material_events
    VALIDATE CONSTRAINT mini_rme_qty_sign_allowed;

ALTER TABLE mini_raw_material_events
    DROP CONSTRAINT IF EXISTS mini_rme_stock_delete_consistent;
ALTER TABLE mini_raw_material_events
    ADD CONSTRAINT mini_rme_stock_delete_consistent CHECK (
        (
            event_type <> 'stock_deleted'
            AND source_type <> 'stock_delete'
        )
        OR
        (
            event_type = 'stock_deleted'
            AND source_type = 'stock_delete'
            AND stock_status_before = 'available'
            AND stock_status_after = 'deleted'
            AND order_id IS NULL
            AND apparatus IS NULL
            AND qty_delta < 0
        )
    ) NOT VALID;
ALTER TABLE mini_raw_material_events
    VALIDATE CONSTRAINT mini_rme_stock_delete_consistent;
