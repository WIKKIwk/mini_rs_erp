CREATE TABLE IF NOT EXISTS mini_qolip_order_notes (
    order_id TEXT NOT NULL,
    principal_role TEXT NOT NULL,
    principal_ref TEXT NOT NULL,
    principal_name TEXT NOT NULL DEFAULT '',
    item_code TEXT NOT NULL,
    item_name TEXT NOT NULL,
    qolip_codes TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    status TEXT NOT NULL DEFAULT 'given',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (order_id, principal_role, principal_ref),
    CONSTRAINT mini_qolip_order_notes_order_id_not_blank CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_qolip_order_notes_principal_role_not_blank CHECK (btrim(principal_role) <> ''),
    CONSTRAINT mini_qolip_order_notes_principal_ref_not_blank CHECK (btrim(principal_ref) <> ''),
    CONSTRAINT mini_qolip_order_notes_item_code_not_blank CHECK (btrim(item_code) <> ''),
    CONSTRAINT mini_qolip_order_notes_item_name_not_blank CHECK (btrim(item_name) <> ''),
    CONSTRAINT mini_qolip_order_notes_status_allowed CHECK (status IN ('given', 'returned'))
);

CREATE INDEX IF NOT EXISTS mini_qolip_order_notes_principal_idx
    ON mini_qolip_order_notes (principal_role, principal_ref, updated_at DESC);
