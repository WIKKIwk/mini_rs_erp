CREATE TABLE IF NOT EXISTS mini_opening_wip_intakes (
    intake_id TEXT PRIMARY KEY,
    idempotency_key TEXT NOT NULL UNIQUE,
    request_fingerprint TEXT NOT NULL,
    order_id TEXT NOT NULL REFERENCES mini_production_maps(id) ON DELETE RESTRICT,
    entry_apparatus TEXT NOT NULL,
    source_operation TEXT NOT NULL,
    source_apparatus TEXT NOT NULL DEFAULT '',
    current_location TEXT NOT NULL,
    history_status TEXT NOT NULL DEFAULT 'unavailable_before_cutover',
    status TEXT NOT NULL DEFAULT 'confirmed',
    note TEXT NOT NULL DEFAULT '',
    actor_role TEXT NOT NULL DEFAULT '',
    actor_ref TEXT NOT NULL DEFAULT '',
    actor_display_name TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_opening_wip_intakes_id_not_blank CHECK (btrim(intake_id) <> ''),
    CONSTRAINT mini_opening_wip_intakes_idempotency_not_blank CHECK (btrim(idempotency_key) <> ''),
    CONSTRAINT mini_opening_wip_intakes_fingerprint_not_blank CHECK (btrim(request_fingerprint) <> ''),
    CONSTRAINT mini_opening_wip_intakes_order_not_blank CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_opening_wip_intakes_entry_not_blank CHECK (btrim(entry_apparatus) <> ''),
    CONSTRAINT mini_opening_wip_intakes_source_operation_not_blank CHECK (btrim(source_operation) <> ''),
    CONSTRAINT mini_opening_wip_intakes_location_not_blank CHECK (btrim(current_location) <> ''),
    CONSTRAINT mini_opening_wip_intakes_history_allowed CHECK (
        history_status IN ('unavailable_before_cutover')
    ),
    CONSTRAINT mini_opening_wip_intakes_status_allowed CHECK (
        status IN ('confirmed', 'cancelled')
    )
);

CREATE TABLE IF NOT EXISTS mini_opening_wip_batches (
    batch_id TEXT PRIMARY KEY,
    intake_id TEXT NOT NULL REFERENCES mini_opening_wip_intakes(intake_id) ON DELETE RESTRICT,
    order_id TEXT NOT NULL REFERENCES mini_production_maps(id) ON DELETE RESTRICT,
    sequence_no INTEGER NOT NULL,
    qr_payload TEXT NOT NULL UNIQUE,
    quantity NUMERIC,
    uom TEXT NOT NULL DEFAULT '',
    quantity_basis TEXT NOT NULL,
    wip_status TEXT NOT NULL DEFAULT 'waiting',
    used_by_session_id TEXT NOT NULL DEFAULT '',
    used_by_apparatus TEXT NOT NULL DEFAULT '',
    processed_by_session_id TEXT NOT NULL DEFAULT '',
    processed_by_apparatus TEXT NOT NULL DEFAULT '',
    label_item_code TEXT NOT NULL,
    label_item_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_opening_wip_batches_id_not_blank CHECK (btrim(batch_id) <> ''),
    CONSTRAINT mini_opening_wip_batches_intake_not_blank CHECK (btrim(intake_id) <> ''),
    CONSTRAINT mini_opening_wip_batches_order_not_blank CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_opening_wip_batches_sequence_positive CHECK (sequence_no > 0),
    CONSTRAINT mini_opening_wip_batches_qr_not_blank CHECK (btrim(qr_payload) <> ''),
    CONSTRAINT mini_opening_wip_batches_quantity_positive CHECK (quantity IS NULL OR quantity > 0),
    CONSTRAINT mini_opening_wip_batches_quantity_basis_allowed CHECK (
        quantity_basis IN ('measured', 'estimated', 'unknown')
    ),
    CONSTRAINT mini_opening_wip_batches_quantity_consistent CHECK (
        (quantity_basis = 'unknown' AND quantity IS NULL AND btrim(uom) = '')
        OR (quantity_basis IN ('measured', 'estimated') AND quantity IS NOT NULL AND btrim(uom) <> '')
    ),
    CONSTRAINT mini_opening_wip_batches_wip_status_allowed CHECK (
        wip_status IN ('waiting', 'in_use', 'processed', 'void')
    ),
    CONSTRAINT mini_opening_wip_batches_label_code_not_blank CHECK (btrim(label_item_code) <> ''),
    CONSTRAINT mini_opening_wip_batches_label_name_not_blank CHECK (btrim(label_item_name) <> ''),
    CONSTRAINT mini_opening_wip_batches_intake_sequence_unique UNIQUE (intake_id, sequence_no)
);

CREATE INDEX IF NOT EXISTS idx_mini_opening_wip_intakes_order
    ON mini_opening_wip_intakes (order_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_mini_opening_wip_batches_order_status
    ON mini_opening_wip_batches (order_id, wip_status, sequence_no);

CREATE INDEX IF NOT EXISTS idx_mini_opening_wip_batches_intake
    ON mini_opening_wip_batches (intake_id, sequence_no);
