-- Compatibility-first inventory movement layer.
--
-- Existing raw-material, finished-goods and qolip tables remain the only
-- quantity source of truth. These tables add stable physical locations,
-- bilateral transfer documents and append-only movement audit records.

-- Historical stock can predate mini_warehouses. Normalize every custody name
-- into the warehouse catalog before creating stable location references.
WITH stock_warehouses AS (
    SELECT warehouse FROM mini_raw_material_stock
    UNION
    SELECT warehouse FROM mini_finished_goods_stock
    UNION
    SELECT warehouse FROM mini_qolip_locations
),
normalized AS (
    SELECT DISTINCT ON (lower(btrim(warehouse)))
        btrim(warehouse) AS warehouse
    FROM stock_warehouses
    WHERE btrim(COALESCE(warehouse, '')) <> ''
    ORDER BY lower(btrim(warehouse)), btrim(warehouse)
)
INSERT INTO mini_warehouses (
    id, name, company, is_group, parent_warehouse, payload_json
)
SELECT
    'warehouse:' || lower(warehouse),
    warehouse,
    '',
    false,
    '',
    jsonb_build_object('source', 'inventory_movement_backfill')
FROM normalized
ON CONFLICT ((lower(name))) DO NOTHING;

CREATE TABLE IF NOT EXISTS mini_inventory_locations (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    warehouse_id TEXT
        REFERENCES mini_warehouses(id) ON DELETE CASCADE,
    factory_location_id TEXT
        REFERENCES mini_factory_locations(id) ON DELETE CASCADE,
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_inventory_locations_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT mini_inventory_locations_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT mini_inventory_locations_kind_allowed
        CHECK (kind IN ('warehouse', 'state', 'transit')),
    CONSTRAINT mini_inventory_locations_reference_shape CHECK (
        (kind = 'warehouse' AND warehouse_id IS NOT NULL AND factory_location_id IS NULL)
        OR
        (kind = 'state' AND warehouse_id IS NULL AND factory_location_id IS NOT NULL)
        OR
        (kind = 'transit' AND warehouse_id IS NULL AND factory_location_id IS NULL)
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_inventory_locations_warehouse
    ON mini_inventory_locations (warehouse_id)
    WHERE warehouse_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_inventory_locations_factory
    ON mini_inventory_locations (factory_location_id)
    WHERE factory_location_id IS NOT NULL;

CREATE OR REPLACE FUNCTION mini_inventory_sync_warehouse_location()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO mini_inventory_locations (
        id, kind, name, warehouse_id, active, created_at, updated_at
    )
    VALUES (
        'inventory_location:warehouse:' || NEW.id,
        'warehouse',
        NEW.name,
        NEW.id,
        true,
        COALESCE(NEW.updated_at, now()),
        now()
    )
    ON CONFLICT (warehouse_id) WHERE warehouse_id IS NOT NULL DO UPDATE SET
        name = excluded.name,
        active = true,
        updated_at = now();
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS mini_inventory_sync_warehouse_location_trg ON mini_warehouses;
CREATE TRIGGER mini_inventory_sync_warehouse_location_trg
AFTER INSERT OR UPDATE OF id, name ON mini_warehouses
FOR EACH ROW EXECUTE FUNCTION mini_inventory_sync_warehouse_location();

CREATE OR REPLACE FUNCTION mini_inventory_sync_factory_location()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO mini_inventory_locations (
        id, kind, name, factory_location_id, active, created_at, updated_at
    )
    VALUES (
        'inventory_location:state:' || NEW.id,
        'state',
        NEW.name,
        NEW.id,
        NEW.active,
        NEW.created_at,
        now()
    )
    ON CONFLICT (factory_location_id) WHERE factory_location_id IS NOT NULL DO UPDATE SET
        name = excluded.name,
        active = excluded.active,
        updated_at = now();
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS mini_inventory_sync_factory_location_trg ON mini_factory_locations;
CREATE TRIGGER mini_inventory_sync_factory_location_trg
AFTER INSERT OR UPDATE OF id, name, active ON mini_factory_locations
FOR EACH ROW EXECUTE FUNCTION mini_inventory_sync_factory_location();

INSERT INTO mini_inventory_locations (
    id, kind, name, warehouse_id, active, created_at, updated_at
)
SELECT
    'inventory_location:warehouse:' || id,
    'warehouse',
    name,
    id,
    true,
    updated_at,
    now()
FROM mini_warehouses
ON CONFLICT (warehouse_id) WHERE warehouse_id IS NOT NULL DO UPDATE SET
    name = excluded.name,
    active = true,
    updated_at = now();

INSERT INTO mini_inventory_locations (
    id, kind, name, factory_location_id, active, created_at, updated_at
)
SELECT
    'inventory_location:state:' || id,
    'state',
    name,
    id,
    active,
    created_at,
    now()
FROM mini_factory_locations
ON CONFLICT (factory_location_id) WHERE factory_location_id IS NOT NULL DO UPDATE SET
    name = excluded.name,
    active = excluded.active,
    updated_at = now();

CREATE TABLE IF NOT EXISTS mini_inventory_placements (
    asset_kind TEXT NOT NULL,
    asset_ref TEXT NOT NULL,
    physical_location_id TEXT NOT NULL
        REFERENCES mini_inventory_locations(id) ON DELETE CASCADE,
    version BIGINT NOT NULL DEFAULT 1,
    updated_by_role TEXT NOT NULL DEFAULT '',
    updated_by_ref TEXT NOT NULL DEFAULT '',
    updated_by_name TEXT NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (asset_kind, asset_ref),
    CONSTRAINT mini_inventory_placements_kind_allowed
        CHECK (asset_kind IN ('raw_material', 'finished_goods', 'qolip')),
    CONSTRAINT mini_inventory_placements_ref_not_blank CHECK (btrim(asset_ref) <> ''),
    CONSTRAINT mini_inventory_placements_version_positive CHECK (version > 0)
);

INSERT INTO mini_inventory_placements (asset_kind, asset_ref, physical_location_id)
SELECT 'raw_material', stock.id, location.id
FROM mini_raw_material_stock stock
JOIN mini_warehouses warehouse ON lower(warehouse.name) = lower(stock.warehouse)
JOIN mini_inventory_locations location ON location.warehouse_id = warehouse.id
ON CONFLICT (asset_kind, asset_ref) DO NOTHING;

INSERT INTO mini_inventory_placements (asset_kind, asset_ref, physical_location_id)
SELECT 'finished_goods', stock.id, location.id
FROM mini_finished_goods_stock stock
JOIN mini_warehouses warehouse ON lower(warehouse.name) = lower(stock.warehouse)
JOIN mini_inventory_locations location ON location.warehouse_id = warehouse.id
ON CONFLICT (asset_kind, asset_ref) DO NOTHING;

INSERT INTO mini_inventory_placements (asset_kind, asset_ref, physical_location_id)
SELECT 'qolip', stock.id, location.id
FROM mini_qolip_locations stock
JOIN mini_warehouses warehouse ON lower(warehouse.name) = lower(stock.warehouse)
JOIN mini_inventory_locations location ON location.warehouse_id = warehouse.id
ON CONFLICT (asset_kind, asset_ref) DO NOTHING;

CREATE TABLE IF NOT EXISTS mini_inventory_transfers (
    id TEXT PRIMARY KEY,
    idempotency_key TEXT NOT NULL,
    source_warehouse_id TEXT NOT NULL,
    source_warehouse TEXT NOT NULL,
    destination_warehouse_id TEXT NOT NULL,
    destination_warehouse TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'requested',
    note TEXT NOT NULL DEFAULT '',
    requested_by_role TEXT NOT NULL,
    requested_by_ref TEXT NOT NULL,
    requested_by_name TEXT NOT NULL DEFAULT '',
    approved_by_role TEXT NOT NULL DEFAULT '',
    approved_by_ref TEXT NOT NULL DEFAULT '',
    approved_by_name TEXT NOT NULL DEFAULT '',
    dispatched_by_role TEXT NOT NULL DEFAULT '',
    dispatched_by_ref TEXT NOT NULL DEFAULT '',
    dispatched_by_name TEXT NOT NULL DEFAULT '',
    received_by_role TEXT NOT NULL DEFAULT '',
    received_by_ref TEXT NOT NULL DEFAULT '',
    received_by_name TEXT NOT NULL DEFAULT '',
    rejected_by_role TEXT NOT NULL DEFAULT '',
    rejected_by_ref TEXT NOT NULL DEFAULT '',
    rejected_by_name TEXT NOT NULL DEFAULT '',
    cancelled_by_role TEXT NOT NULL DEFAULT '',
    cancelled_by_ref TEXT NOT NULL DEFAULT '',
    cancelled_by_name TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    approved_at TIMESTAMPTZ,
    dispatched_at TIMESTAMPTZ,
    received_at TIMESTAMPTZ,
    rejected_at TIMESTAMPTZ,
    cancelled_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_inventory_transfers_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT mini_inventory_transfers_idempotency_unique UNIQUE (idempotency_key),
    CONSTRAINT mini_inventory_transfers_source_id_not_blank
        CHECK (btrim(source_warehouse_id) <> ''),
    CONSTRAINT mini_inventory_transfers_source_not_blank
        CHECK (btrim(source_warehouse) <> ''),
    CONSTRAINT mini_inventory_transfers_destination_id_not_blank
        CHECK (btrim(destination_warehouse_id) <> ''),
    CONSTRAINT mini_inventory_transfers_destination_not_blank
        CHECK (btrim(destination_warehouse) <> ''),
    CONSTRAINT mini_inventory_transfers_warehouses_different
        CHECK (source_warehouse_id <> destination_warehouse_id),
    CONSTRAINT mini_inventory_transfers_status_allowed CHECK (
        status IN (
            'requested', 'approved', 'in_transit', 'received',
            'rejected', 'cancelled'
        )
    ),
    CONSTRAINT mini_inventory_transfers_requester_not_blank CHECK (
        btrim(requested_by_role) <> '' AND btrim(requested_by_ref) <> ''
    )
);

CREATE INDEX IF NOT EXISTS idx_mini_inventory_transfers_source_status
    ON mini_inventory_transfers (source_warehouse_id, status, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_mini_inventory_transfers_destination_status
    ON mini_inventory_transfers (destination_warehouse_id, status, created_at DESC);

CREATE TABLE IF NOT EXISTS mini_inventory_transfer_actions (
    idempotency_key TEXT PRIMARY KEY,
    transfer_id TEXT NOT NULL
        REFERENCES mini_inventory_transfers(id) ON DELETE CASCADE,
    action TEXT NOT NULL,
    actor_role TEXT NOT NULL,
    actor_ref TEXT NOT NULL,
    actor_name TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_inventory_transfer_actions_key_not_blank
        CHECK (btrim(idempotency_key) <> ''),
    CONSTRAINT mini_inventory_transfer_actions_action_allowed
        CHECK (action IN ('approve', 'reject', 'dispatch', 'receive', 'cancel')),
    CONSTRAINT mini_inventory_transfer_actions_actor_not_blank
        CHECK (btrim(actor_role) <> '' AND btrim(actor_ref) <> '')
);

CREATE INDEX IF NOT EXISTS idx_mini_inventory_transfer_actions_transfer
    ON mini_inventory_transfer_actions (transfer_id, created_at);

CREATE TABLE IF NOT EXISTS mini_inventory_transfer_lines (
    transfer_id TEXT NOT NULL
        REFERENCES mini_inventory_transfers(id) ON DELETE CASCADE,
    asset_kind TEXT NOT NULL,
    asset_ref TEXT NOT NULL,
    item_code TEXT NOT NULL DEFAULT '',
    item_name TEXT NOT NULL DEFAULT '',
    identifier TEXT NOT NULL DEFAULT '',
    qty NUMERIC(18,3) NOT NULL,
    uom TEXT NOT NULL,
    source_physical_location_id TEXT NOT NULL,
    PRIMARY KEY (transfer_id, asset_kind, asset_ref),
    CONSTRAINT mini_inventory_transfer_lines_kind_allowed
        CHECK (asset_kind IN ('raw_material', 'finished_goods', 'qolip')),
    CONSTRAINT mini_inventory_transfer_lines_asset_ref_not_blank CHECK (btrim(asset_ref) <> ''),
    CONSTRAINT mini_inventory_transfer_lines_qty_positive CHECK (qty > 0),
    CONSTRAINT mini_inventory_transfer_lines_uom_not_blank CHECK (btrim(uom) <> '')
);

CREATE INDEX IF NOT EXISTS idx_mini_inventory_transfer_lines_asset
    ON mini_inventory_transfer_lines (asset_kind, asset_ref);

CREATE TABLE IF NOT EXISTS mini_inventory_movement_events (
    id TEXT PRIMARY KEY,
    idempotency_key TEXT NOT NULL UNIQUE,
    event_type TEXT NOT NULL,
    transfer_id TEXT,
    asset_kind TEXT NOT NULL,
    asset_ref TEXT NOT NULL,
    from_warehouse_id TEXT NOT NULL DEFAULT '',
    to_warehouse_id TEXT NOT NULL DEFAULT '',
    from_location_id TEXT NOT NULL DEFAULT '',
    to_location_id TEXT NOT NULL DEFAULT '',
    qty NUMERIC(18,3) NOT NULL,
    uom TEXT NOT NULL,
    actor_role TEXT NOT NULL,
    actor_ref TEXT NOT NULL,
    actor_name TEXT NOT NULL DEFAULT '',
    note TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_inventory_movement_events_type_allowed CHECK (
        event_type IN (
            'relocated', 'transfer_requested', 'transfer_approved',
            'transfer_rejected', 'transfer_dispatched', 'transfer_received',
            'transfer_cancelled'
        )
    ),
    CONSTRAINT mini_inventory_movement_events_kind_allowed
        CHECK (asset_kind IN ('raw_material', 'finished_goods', 'qolip')),
    CONSTRAINT mini_inventory_movement_events_ref_not_blank CHECK (btrim(asset_ref) <> ''),
    CONSTRAINT mini_inventory_movement_events_qty_positive CHECK (qty > 0),
    CONSTRAINT mini_inventory_movement_events_uom_not_blank CHECK (btrim(uom) <> ''),
    CONSTRAINT mini_inventory_movement_events_actor_not_blank CHECK (
        btrim(actor_role) <> '' AND btrim(actor_ref) <> ''
    )
);

CREATE INDEX IF NOT EXISTS idx_mini_inventory_movement_events_asset
    ON mini_inventory_movement_events (asset_kind, asset_ref, occurred_at DESC);

CREATE INDEX IF NOT EXISTS idx_mini_inventory_movement_events_transfer
    ON mini_inventory_movement_events (transfer_id, occurred_at);

CREATE OR REPLACE FUNCTION mini_inventory_movement_events_block_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'mini_inventory_movement_events is append-only';
END;
$$;

DROP TRIGGER IF EXISTS mini_inventory_movement_events_no_mutation_trg
    ON mini_inventory_movement_events;
CREATE TRIGGER mini_inventory_movement_events_no_mutation_trg
BEFORE UPDATE OR DELETE ON mini_inventory_movement_events
FOR EACH ROW EXECUTE FUNCTION mini_inventory_movement_events_block_mutation();

-- Raw material already has a reserved state. Finished goods needs explicit
-- transfer states so every existing "status = available" query fails closed.
ALTER TABLE mini_finished_goods_stock
    DROP CONSTRAINT IF EXISTS mini_finished_goods_stock_status_allowed;
ALTER TABLE mini_finished_goods_stock
    ADD CONSTRAINT mini_finished_goods_stock_status_allowed CHECK (
        status IN ('available', 'dispatched', 'transfer_reserved', 'in_transit')
    );

-- Qolip has no stock status column. A transfer lock hides it from operational
-- qolip flows while it is awaiting acceptance or in transit.
ALTER TABLE mini_qolip_locations
    ADD COLUMN IF NOT EXISTS inventory_transfer_id TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_mini_qolip_locations_inventory_transfer
    ON mini_qolip_locations (inventory_transfer_id)
    WHERE btrim(inventory_transfer_id) <> '';

CREATE OR REPLACE FUNCTION mini_qolip_transfer_lock_guard()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF btrim(OLD.inventory_transfer_id) = '' THEN
        IF TG_OP = 'DELETE' THEN
            RETURN OLD;
        END IF;
        RETURN NEW;
    END IF;
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'qolip location is locked by inventory transfer %',
            OLD.inventory_transfer_id;
    END IF;
    IF btrim(NEW.inventory_transfer_id) = btrim(OLD.inventory_transfer_id) THEN
        RAISE EXCEPTION 'qolip location is locked by inventory transfer %',
            OLD.inventory_transfer_id;
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS mini_qolip_transfer_lock_guard_trg ON mini_qolip_locations;
CREATE TRIGGER mini_qolip_transfer_lock_guard_trg
BEFORE UPDATE OR DELETE ON mini_qolip_locations
FOR EACH ROW EXECUTE FUNCTION mini_qolip_transfer_lock_guard();

-- Migrations can run under a database owner while the application uses the
-- restricted mini_rs_erp runtime role. Fail closed if grants are impossible.
DO $$
DECLARE
    table_name TEXT;
    table_owner TEXT;
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        RAISE EXCEPTION 'required runtime role mini_rs_erp does not exist';
    END IF;

    FOREACH table_name IN ARRAY ARRAY[
        'mini_inventory_locations',
        'mini_inventory_placements',
        'mini_inventory_transfers',
        'mini_inventory_transfer_actions',
        'mini_inventory_transfer_lines',
        'mini_inventory_movement_events'
    ]
    LOOP
        SELECT tableowner
        INTO table_owner
        FROM pg_tables
        WHERE schemaname = 'public' AND tablename = table_name;

        IF table_owner = current_user
            OR pg_has_role(current_user, table_owner, 'MEMBER')
        THEN
            EXECUTE format(
                'GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.%I TO mini_rs_erp',
                table_name
            );
        ELSIF NOT has_table_privilege(
            'mini_rs_erp',
            format('public.%I', table_name),
            'SELECT,INSERT,UPDATE,DELETE'
        ) THEN
            RAISE EXCEPTION
                'migration user % cannot grant inventory movement privileges on public.% to mini_rs_erp',
                current_user,
                table_name;
        END IF;
    END LOOP;
END;
$$;
