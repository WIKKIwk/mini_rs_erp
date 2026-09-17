ALTER TABLE mini_inventory_movement_events
    DROP CONSTRAINT IF EXISTS mini_inventory_movement_events_type_allowed;

ALTER TABLE mini_inventory_movement_events
    ADD CONSTRAINT mini_inventory_movement_events_type_allowed
    CHECK (
        event_type IN (
            'relocated', 'returned_to_warehouse', 'paddon_received',
            'transfer_requested', 'transfer_approved', 'transfer_rejected',
            'transfer_dispatched', 'transfer_received', 'transfer_cancelled'
        )
    );

WITH receipt_stocks AS (
    SELECT
        paddon.code AS paddon_code,
        paddon.receipt_json,
        stock.value AS stock
    FROM mini_paddons AS paddon
    CROSS JOIN LATERAL jsonb_array_elements(
        COALESCE(paddon.receipt_json->'stocks', '[]'::jsonb)
    ) AS stock(value)
    WHERE paddon.receipt_json IS NOT NULL
), normalized AS (
    SELECT
        'paddon_received:' || paddon_code || ':' || (stock->>'id') AS event_key,
        paddon_code,
        receipt_json,
        stock,
        COALESCE(
            NULLIF(btrim(stock->>'warehouse'), ''),
            NULLIF(btrim(receipt_json->>'warehouse'), ''),
            ''
        ) AS warehouse,
        COALESCE(
            NULLIF(btrim(stock->>'accepted_by_role'), ''),
            'werka'
        ) AS actor_role,
        COALESCE(
            NULLIF(btrim(stock->>'accepted_by_ref'), ''),
            NULLIF(btrim(receipt_json->>'accepted_by_ref'), ''),
            'werka'
        ) AS actor_ref,
        COALESCE(
            NULLIF(btrim(stock->>'accepted_by_display_name'), ''),
            NULLIF(btrim(receipt_json->>'accepted_by_display_name'), ''),
            ''
        ) AS actor_name,
        COALESCE(
            NULLIF(stock->>'accepted_at_unix', '')::bigint,
            NULLIF(receipt_json->>'accepted_at_unix', '')::bigint,
            EXTRACT(EPOCH FROM now())::bigint
        ) AS accepted_at_unix
    FROM receipt_stocks
)
INSERT INTO mini_inventory_movement_events (
    id, idempotency_key, event_type, transfer_id,
    asset_kind, asset_ref,
    from_warehouse_id, to_warehouse_id,
    from_location_id, to_location_id,
    qty, uom,
    actor_role, actor_ref, actor_name,
    note, payload_json, occurred_at
)
SELECT
    event_key,
    event_key,
    'paddon_received',
    NULL,
    'finished_goods',
    stock->>'id',
    '',
    'warehouse:' || lower(warehouse),
    '',
    'inventory_location:warehouse:warehouse:' || lower(warehouse),
    ((stock->>'qty')::double precision)::numeric(18,6),
    COALESCE(NULLIF(btrim(stock->>'uom'), ''), 'dona'),
    actor_role,
    actor_ref,
    actor_name,
    'Paddon ' || paddon_code || ' qabul qilindi',
    jsonb_build_object(
        'source', 'paddon_receipt',
        'paddon_code', paddon_code,
        'stock', stock
    ),
    to_timestamp(accepted_at_unix)
FROM normalized
WHERE btrim(COALESCE(stock->>'id', '')) <> ''
  AND btrim(warehouse) <> ''
  AND COALESCE(NULLIF(stock->>'qty', '')::double precision, 0) > 0
ON CONFLICT (idempotency_key) DO NOTHING;
