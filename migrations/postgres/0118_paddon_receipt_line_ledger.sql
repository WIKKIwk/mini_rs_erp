-- Normalize a received paddon into one durable document-to-roll ledger.
-- The JSON receipt remains a compatibility snapshot; this table is the
-- relational source for audit and warehouse history.
ALTER TABLE mini_inventory_movement_events
    ADD COLUMN IF NOT EXISTS source_document_type TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS source_document_id TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS source_line_id TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_mini_inventory_movement_events_source_document
    ON mini_inventory_movement_events (
        source_document_type,
        source_document_id,
        occurred_at DESC
    )
    WHERE btrim(source_document_id) <> '';

CREATE TABLE IF NOT EXISTS mini_paddon_receipt_lines (
    id TEXT PRIMARY KEY,
    paddon_id TEXT NOT NULL REFERENCES mini_paddons(id) ON DELETE CASCADE,
    paddon_code TEXT NOT NULL,
    progress_batch_id TEXT NOT NULL REFERENCES mini_progress_batches(batch_id) ON DELETE CASCADE,
    stock_id TEXT NOT NULL REFERENCES mini_finished_goods_stock(id) ON DELETE CASCADE,
    warehouse TEXT NOT NULL,
    item_code TEXT NOT NULL,
    item_name TEXT NOT NULL DEFAULT '',
    qty NUMERIC(18,6) NOT NULL,
    uom TEXT NOT NULL,
    accepted_by_role TEXT NOT NULL,
    accepted_by_ref TEXT NOT NULL,
    accepted_by_display_name TEXT NOT NULL DEFAULT '',
    accepted_at TIMESTAMPTZ NOT NULL,
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    CONSTRAINT mini_paddon_receipt_lines_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT mini_paddon_receipt_lines_code_not_blank CHECK (btrim(paddon_code) <> ''),
    CONSTRAINT mini_paddon_receipt_lines_batch_not_blank CHECK (btrim(progress_batch_id) <> ''),
    CONSTRAINT mini_paddon_receipt_lines_stock_not_blank CHECK (btrim(stock_id) <> ''),
    CONSTRAINT mini_paddon_receipt_lines_warehouse_not_blank CHECK (btrim(warehouse) <> ''),
    CONSTRAINT mini_paddon_receipt_lines_item_code_not_blank CHECK (btrim(item_code) <> ''),
    CONSTRAINT mini_paddon_receipt_lines_qty_positive CHECK (qty > 0),
    CONSTRAINT mini_paddon_receipt_lines_uom_not_blank CHECK (btrim(uom) <> ''),
    CONSTRAINT mini_paddon_receipt_lines_actor_not_blank CHECK (
        btrim(accepted_by_role) <> '' AND btrim(accepted_by_ref) <> ''
    ),
    CONSTRAINT mini_paddon_receipt_lines_paddon_batch_unique
        UNIQUE (paddon_id, progress_batch_id),
    CONSTRAINT mini_paddon_receipt_lines_stock_unique UNIQUE (stock_id)
);

CREATE INDEX IF NOT EXISTS idx_mini_paddon_receipt_lines_paddon
    ON mini_paddon_receipt_lines (paddon_id, accepted_at DESC, progress_batch_id);

CREATE INDEX IF NOT EXISTS idx_mini_paddon_receipt_lines_warehouse
    ON mini_paddon_receipt_lines (warehouse, accepted_at DESC);

CREATE INDEX IF NOT EXISTS idx_mini_paddon_receipt_lines_batch
    ON mini_paddon_receipt_lines (progress_batch_id);

-- Historical movement events are append-only and retain their source links in
-- payload_json; source_document_* is populated for new events by the runtime.
-- Backfill the relational line ledger from the retained receipt snapshot.
INSERT INTO mini_paddon_receipt_lines (
    id, paddon_id, paddon_code, progress_batch_id, stock_id,
    warehouse, item_code, item_name, qty, uom,
    accepted_by_role, accepted_by_ref, accepted_by_display_name,
    accepted_at, payload_json
)
SELECT
    'paddon-receipt-line:' || paddon.id || ':' ||
        (stock->>'source_progress_batch_id'),
    paddon.id,
    paddon.code,
    stock->>'source_progress_batch_id',
    stock->>'id',
    COALESCE(
        NULLIF(btrim(stock->>'warehouse'), ''),
        NULLIF(btrim(paddon.receipt_json->>'warehouse'), '')
    ),
    NULLIF(btrim(stock->>'item_code'), ''),
    COALESCE(stock->>'item_name', ''),
    (stock->>'qty')::numeric,
    NULLIF(btrim(stock->>'uom'), ''),
    COALESCE(NULLIF(btrim(stock->>'accepted_by_role'), ''), 'werka'),
    COALESCE(
        NULLIF(btrim(stock->>'accepted_by_ref'), ''),
        NULLIF(btrim(paddon.receipt_json->>'accepted_by_ref'), ''),
        'werka'
    ),
    COALESCE(
        NULLIF(btrim(stock->>'accepted_by_display_name'), ''),
        NULLIF(btrim(paddon.receipt_json->>'accepted_by_display_name'), ''),
        ''
    ),
    to_timestamp(COALESCE(
        NULLIF(stock->>'accepted_at_unix', '')::bigint,
        NULLIF(paddon.receipt_json->>'accepted_at_unix', '')::bigint,
        EXTRACT(EPOCH FROM now())::bigint
    )),
    jsonb_build_object(
        'source', 'paddon_receipt_snapshot_backfill',
        'paddon_id', paddon.id,
        'paddon_code', paddon.code,
        'stock', stock
    )
FROM mini_paddons AS paddon
CROSS JOIN LATERAL jsonb_array_elements(
    COALESCE(paddon.receipt_json->'stocks', '[]'::jsonb)
) AS stock
JOIN mini_progress_batches AS batch
  ON batch.batch_id = stock->>'source_progress_batch_id'
JOIN mini_finished_goods_stock AS finished_stock
  ON finished_stock.id = stock->>'id'
WHERE paddon.receipt_json IS NOT NULL
  AND btrim(COALESCE(stock->>'id', '')) <> ''
  AND btrim(COALESCE(stock->>'source_progress_batch_id', '')) <> ''
  AND btrim(COALESCE(stock->>'warehouse', paddon.receipt_json->>'warehouse', '')) <> ''
  AND btrim(COALESCE(stock->>'item_code', '')) <> ''
  AND COALESCE(NULLIF(stock->>'qty', '')::numeric, 0) > 0
  AND btrim(COALESCE(stock->>'uom', '')) <> ''
ON CONFLICT (paddon_id, progress_batch_id) DO NOTHING;
