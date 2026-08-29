ALTER TABLE mini_opening_wip_batches
    ADD COLUMN IF NOT EXISTS voided_at TIMESTAMPTZ;

ALTER TABLE mini_opening_wip_batches
    ADD COLUMN IF NOT EXISTS voided_by_role TEXT NOT NULL DEFAULT '';

ALTER TABLE mini_opening_wip_batches
    ADD COLUMN IF NOT EXISTS voided_by_ref TEXT NOT NULL DEFAULT '';

ALTER TABLE mini_opening_wip_batches
    ADD COLUMN IF NOT EXISTS voided_by_display_name TEXT NOT NULL DEFAULT '';

UPDATE mini_opening_wip_batches
SET voided_at = updated_at
WHERE wip_status = 'void' AND voided_at IS NULL;

ALTER TABLE mini_opening_wip_batches
    DROP CONSTRAINT IF EXISTS mini_opening_wip_batches_void_audit_consistent;

ALTER TABLE mini_opening_wip_batches
    ADD CONSTRAINT mini_opening_wip_batches_void_audit_consistent CHECK (
        (wip_status = 'void' AND voided_at IS NOT NULL)
        OR (
            wip_status <> 'void'
            AND voided_at IS NULL
            AND btrim(voided_by_role) = ''
            AND btrim(voided_by_ref) = ''
            AND btrim(voided_by_display_name) = ''
        )
    );
