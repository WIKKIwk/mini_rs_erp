-- Stock corrections are append-only audit events. Extend the existing event
-- vocabulary without rewriting historical rows or changing applied migration
-- checksums. A bounded lock/statement timeout makes the whole migration fail
-- and roll back instead of waiting indefinitely on a busy warehouse ledger.

SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

ALTER TABLE mini_raw_material_events
    DROP CONSTRAINT IF EXISTS mini_rme_event_type_allowed;
ALTER TABLE mini_raw_material_events
    ADD CONSTRAINT mini_rme_event_type_allowed CHECK (
        event_type IN (
            'receipt_posted',
            'order_reserved',
            'order_unreserved',
            'usage_started',
            'consumption_posted',
            'adjustment_increase',
            'adjustment_decrease',
            'transfer_in',
            'transfer_out',
            'stock_corrected'
        )
    ) NOT VALID;
ALTER TABLE mini_raw_material_events
    VALIDATE CONSTRAINT mini_rme_event_type_allowed;

ALTER TABLE mini_raw_material_events
    DROP CONSTRAINT IF EXISTS mini_rme_source_type_allowed;
ALTER TABLE mini_raw_material_events
    ADD CONSTRAINT mini_rme_source_type_allowed CHECK (
        source_type IN (
            'gscale_receipt',
            'order_assignment',
            'consumption',
            'manual_adjustment',
            'warehouse_transfer',
            'system',
            'stock_correction'
        )
    ) NOT VALID;
ALTER TABLE mini_raw_material_events
    VALIDATE CONSTRAINT mini_rme_source_type_allowed;

ALTER TABLE mini_raw_material_events
    DROP CONSTRAINT IF EXISTS mini_rme_qty_sign_allowed;
ALTER TABLE mini_raw_material_events
    ADD CONSTRAINT mini_rme_qty_sign_allowed CHECK (
        CASE
            WHEN event_type IN ('receipt_posted', 'adjustment_increase', 'transfer_in')
                THEN qty_delta > 0
            WHEN event_type IN ('consumption_posted', 'adjustment_decrease', 'transfer_out')
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
    DROP CONSTRAINT IF EXISTS mini_rme_stock_correction_consistent;
ALTER TABLE mini_raw_material_events
    ADD CONSTRAINT mini_rme_stock_correction_consistent CHECK (
        (
            event_type <> 'stock_corrected'
            AND source_type <> 'stock_correction'
        )
        OR
        (
            event_type = 'stock_corrected'
            AND source_type = 'stock_correction'
            AND stock_status_before = 'available'
            AND stock_status_after = 'available'
            AND order_id IS NULL
            AND apparatus IS NULL
        )
    ) NOT VALID;
ALTER TABLE mini_raw_material_events
    VALIDATE CONSTRAINT mini_rme_stock_correction_consistent;
