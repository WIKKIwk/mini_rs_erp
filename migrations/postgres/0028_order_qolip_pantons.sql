CREATE TABLE IF NOT EXISTS mini_order_qolip_pantons (
    order_id TEXT NOT NULL,
    qolip_code_key TEXT NOT NULL,
    qolip_code TEXT NOT NULL,
    panton_number SMALLINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_order_qolip_pantons_order_code_pk
        PRIMARY KEY (order_id, qolip_code_key),
    CONSTRAINT mini_order_qolip_pantons_order_number_unique
        UNIQUE (order_id, panton_number),
    CONSTRAINT mini_order_qolip_pantons_order_not_blank
        CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_order_qolip_pantons_code_key_not_blank
        CHECK (btrim(qolip_code_key) <> ''),
    CONSTRAINT mini_order_qolip_pantons_code_not_blank
        CHECK (btrim(qolip_code) <> ''),
    CONSTRAINT mini_order_qolip_pantons_number_allowed
        CHECK (panton_number BETWEEN 1 AND 7)
);

CREATE INDEX IF NOT EXISTS idx_mini_order_qolip_pantons_order
    ON mini_order_qolip_pantons(order_id, panton_number);
