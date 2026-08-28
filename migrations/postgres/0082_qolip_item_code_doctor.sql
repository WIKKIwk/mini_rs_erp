CREATE TABLE IF NOT EXISTS mini_qolip_item_code_repairs (
    id BIGSERIAL PRIMARY KEY,
    source_item_code TEXT NOT NULL,
    canonical_item_code TEXT NOT NULL,
    item_name TEXT NOT NULL,
    item_group TEXT NOT NULL,
    product_specs_updated BIGINT NOT NULL DEFAULT 0,
    locations_updated BIGINT NOT NULL DEFAULT 0,
    open_checkouts_updated BIGINT NOT NULL DEFAULT 0,
    order_notes_updated BIGINT NOT NULL DEFAULT 0,
    reason TEXT NOT NULL DEFAULT 'qolip_item_code_mismatch',
    repaired_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_qolip_item_code_repairs_source_not_blank
        CHECK (btrim(source_item_code) <> ''),
    CONSTRAINT mini_qolip_item_code_repairs_canonical_not_blank
        CHECK (btrim(canonical_item_code) <> ''),
    CONSTRAINT mini_qolip_item_code_repairs_codes_differ
        CHECK (lower(btrim(source_item_code)) <> lower(btrim(canonical_item_code))),
    CONSTRAINT mini_qolip_item_code_repairs_item_name_not_blank
        CHECK (btrim(item_name) <> ''),
    CONSTRAINT mini_qolip_item_code_repairs_item_group_not_blank
        CHECK (btrim(item_group) <> ''),
    CONSTRAINT mini_qolip_item_code_repairs_counts_non_negative
        CHECK (
            product_specs_updated >= 0
            AND locations_updated >= 0
            AND open_checkouts_updated >= 0
            AND order_notes_updated >= 0
        ),
    CONSTRAINT mini_qolip_item_code_repairs_reason_not_blank
        CHECK (btrim(reason) <> '')
);

CREATE INDEX IF NOT EXISTS idx_mini_qolip_item_code_repairs_repaired_at
    ON mini_qolip_item_code_repairs (repaired_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_mini_qolip_item_code_repairs_codes
    ON mini_qolip_item_code_repairs (
        lower(source_item_code),
        lower(canonical_item_code),
        repaired_at DESC
    );
