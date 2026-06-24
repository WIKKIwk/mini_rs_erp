CREATE TABLE IF NOT EXISTS mini_orders (
    id TEXT PRIMARY KEY,
    code TEXT NOT NULL,
    order_number TEXT NOT NULL DEFAULT '',
    customer_ref TEXT NOT NULL DEFAULT '',
    customer_name TEXT NOT NULL DEFAULT '',
    product_code TEXT NOT NULL DEFAULT '',
    product_name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    kg NUMERIC(14, 3) NOT NULL DEFAULT 0,
    width_mm NUMERIC(14, 3),
    roll_count NUMERIC(14, 3),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_orders_code_not_blank CHECK (btrim(code) <> ''),
    CONSTRAINT mini_orders_product_name_not_blank CHECK (btrim(product_name) <> ''),
    CONSTRAINT mini_orders_kg_non_negative CHECK (kg >= 0),
    CONSTRAINT mini_orders_width_positive CHECK (width_mm IS NULL OR width_mm > 0),
    CONSTRAINT mini_orders_roll_count_positive CHECK (roll_count IS NULL OR roll_count > 0),
    CONSTRAINT mini_orders_code_unique UNIQUE (code),
    CONSTRAINT mini_orders_order_number_unique UNIQUE (order_number)
);

CREATE TABLE IF NOT EXISTS mini_order_products (
    id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL REFERENCES mini_orders(id) ON DELETE CASCADE,
    item_code TEXT NOT NULL DEFAULT '',
    product_name TEXT NOT NULL,
    material_display TEXT NOT NULL DEFAULT '',
    color TEXT NOT NULL DEFAULT '',
    first_layer_material TEXT NOT NULL DEFAULT '',
    first_layer_micron TEXT NOT NULL DEFAULT '',
    second_layer_material TEXT NOT NULL DEFAULT '',
    second_layer_micron TEXT NOT NULL DEFAULT '',
    third_layer_material TEXT NOT NULL DEFAULT '',
    third_layer_micron TEXT NOT NULL DEFAULT '',
    note TEXT NOT NULL DEFAULT '',
    CONSTRAINT mini_order_products_product_name_not_blank CHECK (btrim(product_name) <> '')
);

CREATE TABLE IF NOT EXISTS mini_quick_order_templates (
    id TEXT PRIMARY KEY,
    owner_key TEXT NOT NULL,
    code TEXT NOT NULL,
    name TEXT NOT NULL,
    item_code TEXT NOT NULL DEFAULT '',
    product_name TEXT NOT NULL,
    customer_ref TEXT NOT NULL DEFAULT '',
    customer_name TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL,
    quick_key TEXT NOT NULL,
    saved_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_quick_order_templates_owner_not_blank CHECK (btrim(owner_key) <> ''),
    CONSTRAINT mini_quick_order_templates_code_not_blank CHECK (btrim(code) <> ''),
    CONSTRAINT mini_quick_order_templates_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT mini_quick_order_templates_product_not_blank CHECK (btrim(product_name) <> ''),
    CONSTRAINT mini_quick_order_templates_owner_code_unique UNIQUE (owner_key, code)
);

CREATE TABLE IF NOT EXISTS mini_quick_order_images (
    owner_key TEXT NOT NULL,
    image_id TEXT NOT NULL,
    image_name TEXT NOT NULL,
    image_mime TEXT NOT NULL,
    image_size_bytes BIGINT NOT NULL,
    body BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (owner_key, image_id),
    CONSTRAINT mini_quick_order_images_owner_not_blank CHECK (btrim(owner_key) <> ''),
    CONSTRAINT mini_quick_order_images_id_not_blank CHECK (btrim(image_id) <> ''),
    CONSTRAINT mini_quick_order_images_name_not_blank CHECK (btrim(image_name) <> ''),
    CONSTRAINT mini_quick_order_images_mime_not_blank CHECK (btrim(image_mime) <> ''),
    CONSTRAINT mini_quick_order_images_size_non_negative CHECK (image_size_bytes >= 0)
);

CREATE TABLE IF NOT EXISTS mini_push_tokens (
    token TEXT PRIMARY KEY,
    owner_key TEXT NOT NULL,
    platform TEXT NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_push_tokens_token_not_blank CHECK (btrim(token) <> ''),
    CONSTRAINT mini_push_tokens_owner_not_blank CHECK (btrim(owner_key) <> '')
);

CREATE TABLE IF NOT EXISTS mini_item_groups (
    name TEXT PRIMARY KEY,
    parent_item_group TEXT NOT NULL DEFAULT '',
    is_group BOOLEAN NOT NULL DEFAULT true,
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_item_groups_name_not_blank CHECK (btrim(name) <> '')
);

CREATE TABLE IF NOT EXISTS mini_items (
    code TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    uom TEXT NOT NULL DEFAULT 'Kg',
    warehouse TEXT NOT NULL DEFAULT '',
    item_group TEXT NOT NULL,
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_items_code_not_blank CHECK (btrim(code) <> ''),
    CONSTRAINT mini_items_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT mini_items_uom_not_blank CHECK (btrim(uom) <> ''),
    CONSTRAINT mini_items_group_not_blank CHECK (btrim(item_group) <> '')
);

CREATE TABLE IF NOT EXISTS mini_production_maps (
    id TEXT PRIMARY KEY,
    order_id TEXT REFERENCES mini_orders(id) ON DELETE SET NULL,
    product_code TEXT NOT NULL,
    title TEXT NOT NULL,
    code TEXT NOT NULL DEFAULT '',
    order_number TEXT NOT NULL DEFAULT '',
    roll_count NUMERIC(14, 3),
    width_mm NUMERIC(14, 3),
    map_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_production_maps_product_code_not_blank CHECK (btrim(product_code) <> ''),
    CONSTRAINT mini_production_maps_title_not_blank CHECK (btrim(title) <> ''),
    CONSTRAINT mini_production_maps_width_positive CHECK (width_mm IS NULL OR width_mm > 0),
    CONSTRAINT mini_production_maps_roll_count_positive CHECK (roll_count IS NULL OR roll_count > 0)
);

CREATE TABLE IF NOT EXISTS mini_production_map_nodes (
    map_id TEXT NOT NULL REFERENCES mini_production_maps(id) ON DELETE CASCADE,
    node_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL,
    PRIMARY KEY (map_id, node_id),
    CONSTRAINT mini_production_map_nodes_kind_not_blank CHECK (btrim(kind) <> '')
);

CREATE TABLE IF NOT EXISTS mini_production_map_edges (
    map_id TEXT NOT NULL REFERENCES mini_production_maps(id) ON DELETE CASCADE,
    edge_index INTEGER NOT NULL,
    from_node_id TEXT NOT NULL,
    to_node_id TEXT NOT NULL,
    branch TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL,
    PRIMARY KEY (map_id, edge_index),
    CONSTRAINT mini_production_map_edges_from_not_blank CHECK (btrim(from_node_id) <> ''),
    CONSTRAINT mini_production_map_edges_to_not_blank CHECK (btrim(to_node_id) <> '')
);

CREATE TABLE IF NOT EXISTS mini_apparatus_groups (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    payload_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_apparatus_groups_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT mini_apparatus_groups_name_unique UNIQUE (name)
);

CREATE TABLE IF NOT EXISTS mini_apparatus (
    id TEXT PRIMARY KEY,
    group_id TEXT REFERENCES mini_apparatus_groups(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    base_name TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_apparatus_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT mini_apparatus_name_unique UNIQUE (name)
);

CREATE TABLE IF NOT EXISTS mini_workers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    phone TEXT NOT NULL DEFAULT '',
    level TEXT NOT NULL,
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_workers_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT mini_workers_level_allowed CHECK (
        level IN ('Brigader', 'Master', '1 - darajali', '2 - darajali', '3 - darajali')
    ),
    CONSTRAINT mini_workers_name_unique UNIQUE (name)
);

ALTER TABLE mini_workers
    ADD COLUMN IF NOT EXISTS phone TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS mini_worker_groups (
    apparatus TEXT NOT NULL,
    group_code TEXT NOT NULL,
    shift TEXT NOT NULL,
    start_time TEXT NOT NULL DEFAULT '08:00',
    end_time TEXT NOT NULL DEFAULT '20:00',
    work_days_per_week INTEGER NOT NULL DEFAULT 6,
    start_day TEXT NOT NULL DEFAULT 'monday',
    accounting_enabled BOOLEAN NOT NULL DEFAULT false,
    worker_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (apparatus, group_code),
    CONSTRAINT mini_worker_groups_apparatus_not_blank CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_worker_groups_group_code_not_blank CHECK (btrim(group_code) <> ''),
    CONSTRAINT mini_worker_groups_shift_not_blank CHECK (btrim(shift) <> ''),
    CONSTRAINT mini_worker_groups_start_time_not_blank CHECK (btrim(start_time) <> ''),
    CONSTRAINT mini_worker_groups_end_time_not_blank CHECK (btrim(end_time) <> ''),
    CONSTRAINT mini_worker_groups_work_days_range CHECK (work_days_per_week BETWEEN 1 AND 7),
    CONSTRAINT mini_worker_groups_start_day_not_blank CHECK (btrim(start_day) <> ''),
    CONSTRAINT mini_worker_groups_worker_ids_array CHECK (jsonb_typeof(worker_ids) = 'array')
);

ALTER TABLE mini_worker_groups DROP CONSTRAINT IF EXISTS mini_worker_groups_group_code_allowed;
ALTER TABLE mini_worker_groups DROP CONSTRAINT IF EXISTS mini_worker_groups_shift_allowed;
ALTER TABLE mini_worker_groups ADD COLUMN IF NOT EXISTS start_time TEXT NOT NULL DEFAULT '08:00';
ALTER TABLE mini_worker_groups ADD COLUMN IF NOT EXISTS end_time TEXT NOT NULL DEFAULT '20:00';
ALTER TABLE mini_worker_groups ADD COLUMN IF NOT EXISTS work_days_per_week INTEGER NOT NULL DEFAULT 6;
ALTER TABLE mini_worker_groups ADD COLUMN IF NOT EXISTS start_day TEXT NOT NULL DEFAULT 'monday';
ALTER TABLE mini_worker_groups ADD COLUMN IF NOT EXISTS accounting_enabled BOOLEAN NOT NULL DEFAULT false;

CREATE TABLE IF NOT EXISTS mini_queue_sequences (
    apparatus TEXT PRIMARY KEY,
    order_ids JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_queue_sequences_apparatus_not_blank CHECK (btrim(apparatus) <> '')
);

CREATE TABLE IF NOT EXISTS mini_queue_states (
    apparatus TEXT NOT NULL,
    order_id TEXT NOT NULL,
    state TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (apparatus, order_id),
    CONSTRAINT mini_queue_states_apparatus_not_blank CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_queue_states_order_id_not_blank CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_queue_states_state_allowed CHECK (state IN ('pending', 'in_progress', 'paused', 'completed'))
);

ALTER TABLE mini_queue_states DROP CONSTRAINT IF EXISTS mini_queue_states_state_allowed;
ALTER TABLE mini_queue_states
    ADD CONSTRAINT mini_queue_states_state_allowed CHECK (state IN ('pending', 'in_progress', 'paused', 'completed'));

CREATE TABLE IF NOT EXISTS mini_apparatus_queue_policies (
    apparatus TEXT PRIMARY KEY,
    policy TEXT NOT NULL,
    actor_role TEXT NOT NULL DEFAULT '',
    actor_ref TEXT NOT NULL DEFAULT '',
    actor_display_name TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_apparatus_queue_policies_apparatus_not_blank CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_apparatus_queue_policies_policy_allowed CHECK (policy IN ('strict_sequence', 'free_pick'))
);

CREATE TABLE IF NOT EXISTS mini_queue_action_events (
    id BIGSERIAL PRIMARY KEY,
    event_id TEXT NOT NULL,
    apparatus TEXT NOT NULL,
    order_id TEXT NOT NULL,
    action TEXT NOT NULL,
    from_state TEXT NOT NULL,
    to_state TEXT NOT NULL,
    policy TEXT NOT NULL,
    actor_role TEXT NOT NULL DEFAULT '',
    actor_ref TEXT NOT NULL DEFAULT '',
    actor_display_name TEXT NOT NULL DEFAULT '',
    assigned_apparatus JSONB NOT NULL DEFAULT '[]'::jsonb,
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_queue_action_events_event_id_not_blank CHECK (btrim(event_id) <> ''),
    CONSTRAINT mini_queue_action_events_apparatus_not_blank CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_queue_action_events_order_id_not_blank CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_queue_action_events_action_allowed CHECK (action IN ('start', 'pause', 'resume', 'complete')),
    CONSTRAINT mini_queue_action_events_from_state_allowed CHECK (from_state IN ('pending', 'in_progress', 'paused', 'completed')),
    CONSTRAINT mini_queue_action_events_to_state_allowed CHECK (to_state IN ('pending', 'in_progress', 'paused', 'completed')),
    CONSTRAINT mini_queue_action_events_policy_allowed CHECK (policy IN ('strict_sequence', 'free_pick')),
    CONSTRAINT mini_queue_action_events_assigned_array CHECK (jsonb_typeof(assigned_apparatus) = 'array'),
    CONSTRAINT mini_queue_action_events_event_id_unique UNIQUE (event_id)
);

ALTER TABLE mini_queue_action_events DROP CONSTRAINT IF EXISTS mini_queue_action_events_action_allowed;
ALTER TABLE mini_queue_action_events DROP CONSTRAINT IF EXISTS mini_queue_action_events_from_state_allowed;
ALTER TABLE mini_queue_action_events DROP CONSTRAINT IF EXISTS mini_queue_action_events_to_state_allowed;
ALTER TABLE mini_queue_action_events
    ADD CONSTRAINT mini_queue_action_events_action_allowed CHECK (action IN ('start', 'pause', 'resume', 'complete'));
ALTER TABLE mini_queue_action_events
    ADD CONSTRAINT mini_queue_action_events_from_state_allowed CHECK (from_state IN ('pending', 'in_progress', 'paused', 'completed'));
ALTER TABLE mini_queue_action_events
    ADD CONSTRAINT mini_queue_action_events_to_state_allowed CHECK (to_state IN ('pending', 'in_progress', 'paused', 'completed'));

CREATE TABLE IF NOT EXISTS mini_order_run_sessions (
    session_id TEXT PRIMARY KEY,
    apparatus TEXT NOT NULL,
    order_id TEXT NOT NULL,
    status TEXT NOT NULL,
    worker_role TEXT NOT NULL DEFAULT '',
    worker_ref TEXT NOT NULL DEFAULT '',
    worker_display_name TEXT NOT NULL DEFAULT '',
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    CONSTRAINT mini_order_run_sessions_session_id_not_blank CHECK (btrim(session_id) <> ''),
    CONSTRAINT mini_order_run_sessions_apparatus_not_blank CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_order_run_sessions_order_id_not_blank CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_order_run_sessions_status_allowed CHECK (status IN ('active', 'paused', 'completed'))
);

CREATE TABLE IF NOT EXISTS mini_order_progress_events (
    id BIGSERIAL PRIMARY KEY,
    event_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    batch_id TEXT NOT NULL DEFAULT '',
    apparatus TEXT NOT NULL,
    order_id TEXT NOT NULL,
    action TEXT NOT NULL,
    produced_qty NUMERIC NOT NULL DEFAULT 0,
    uom TEXT NOT NULL DEFAULT '',
    worker_role TEXT NOT NULL DEFAULT '',
    worker_ref TEXT NOT NULL DEFAULT '',
    worker_display_name TEXT NOT NULL DEFAULT '',
    qr_payload TEXT NOT NULL DEFAULT '',
    return_ink_kg NUMERIC,
    lamination_print_leftover_rolls NUMERIC,
    lamination_film_leftover_rolls NUMERIC,
    rezka_bosma_waste NUMERIC,
    rezka_lamination_waste NUMERIC,
    rezka_edge_waste NUMERIC,
    total_waste NUMERIC,
    finished_goods_kg NUMERIC,
    finished_goods_meter NUMERIC,
    description TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_order_progress_events_event_id_not_blank CHECK (btrim(event_id) <> ''),
    CONSTRAINT mini_order_progress_events_session_id_not_blank CHECK (btrim(session_id) <> ''),
    CONSTRAINT mini_order_progress_events_apparatus_not_blank CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_order_progress_events_order_id_not_blank CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_order_progress_events_action_allowed CHECK (action IN ('start', 'pause', 'resume', 'complete')),
    CONSTRAINT mini_order_progress_events_qty_non_negative CHECK (produced_qty >= 0),
    CONSTRAINT mini_order_progress_events_event_id_unique UNIQUE (event_id)
);

CREATE TABLE IF NOT EXISTS mini_progress_batches (
    batch_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    apparatus TEXT NOT NULL,
    order_id TEXT NOT NULL,
    action TEXT NOT NULL,
    status TEXT NOT NULL,
    produced_qty NUMERIC NOT NULL,
    uom TEXT NOT NULL,
    qr_payload TEXT NOT NULL,
    label_item_code TEXT NOT NULL,
    label_item_name TEXT NOT NULL,
    executor_name TEXT NOT NULL DEFAULT '',
    worker_role TEXT NOT NULL DEFAULT '',
    worker_ref TEXT NOT NULL DEFAULT '',
    worker_display_name TEXT NOT NULL DEFAULT '',
    wip_status TEXT NOT NULL DEFAULT 'waiting',
    current_apparatus TEXT NOT NULL DEFAULT '',
    current_apparatus_key TEXT NOT NULL DEFAULT '',
    current_location TEXT NOT NULL DEFAULT '',
    next_apparatus TEXT NOT NULL DEFAULT '',
    parent_batch_id TEXT NOT NULL DEFAULT '',
    used_by_session_id TEXT NOT NULL DEFAULT '',
    used_by_apparatus TEXT NOT NULL DEFAULT '',
    processed_by_session_id TEXT NOT NULL DEFAULT '',
    processed_by_apparatus TEXT NOT NULL DEFAULT '',
    return_ink_kg NUMERIC,
    lamination_print_leftover_rolls NUMERIC,
    lamination_film_leftover_rolls NUMERIC,
    rezka_bosma_waste NUMERIC,
    rezka_lamination_waste NUMERIC,
    rezka_edge_waste NUMERIC,
    total_waste NUMERIC,
    finished_goods_kg NUMERIC,
    finished_goods_meter NUMERIC,
    description TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_progress_batches_batch_id_not_blank CHECK (btrim(batch_id) <> ''),
    CONSTRAINT mini_progress_batches_session_id_not_blank CHECK (btrim(session_id) <> ''),
    CONSTRAINT mini_progress_batches_apparatus_not_blank CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_progress_batches_order_id_not_blank CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_progress_batches_action_allowed CHECK (action IN ('pause', 'complete')),
    CONSTRAINT mini_progress_batches_status_allowed CHECK (status IN ('paused', 'completed', 'resumed')),
    CONSTRAINT mini_progress_batches_wip_status_allowed CHECK (wip_status IN ('waiting', 'in_use', 'processed')),
    CONSTRAINT mini_progress_batches_qty_positive CHECK (produced_qty > 0),
    CONSTRAINT mini_progress_batches_uom_not_blank CHECK (btrim(uom) <> ''),
    CONSTRAINT mini_progress_batches_qr_payload_not_blank CHECK (btrim(qr_payload) <> ''),
    CONSTRAINT mini_progress_batches_label_item_code_not_blank CHECK (btrim(label_item_code) <> ''),
    CONSTRAINT mini_progress_batches_label_item_name_not_blank CHECK (btrim(label_item_name) <> ''),
    CONSTRAINT mini_progress_batches_qr_payload_unique UNIQUE (qr_payload)
);

ALTER TABLE mini_order_progress_events ADD COLUMN IF NOT EXISTS return_ink_kg NUMERIC;
ALTER TABLE mini_order_progress_events ADD COLUMN IF NOT EXISTS lamination_print_leftover_rolls NUMERIC;
ALTER TABLE mini_order_progress_events ADD COLUMN IF NOT EXISTS lamination_film_leftover_rolls NUMERIC;
ALTER TABLE mini_order_progress_events ADD COLUMN IF NOT EXISTS rezka_bosma_waste NUMERIC;
ALTER TABLE mini_order_progress_events ADD COLUMN IF NOT EXISTS rezka_lamination_waste NUMERIC;
ALTER TABLE mini_order_progress_events ADD COLUMN IF NOT EXISTS rezka_edge_waste NUMERIC;
ALTER TABLE mini_order_progress_events ADD COLUMN IF NOT EXISTS total_waste NUMERIC;
ALTER TABLE mini_order_progress_events ADD COLUMN IF NOT EXISTS finished_goods_kg NUMERIC;
ALTER TABLE mini_order_progress_events ADD COLUMN IF NOT EXISTS finished_goods_meter NUMERIC;
ALTER TABLE mini_order_progress_events ADD COLUMN IF NOT EXISTS description TEXT NOT NULL DEFAULT '';

ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS return_ink_kg NUMERIC;
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS lamination_print_leftover_rolls NUMERIC;
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS lamination_film_leftover_rolls NUMERIC;
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS rezka_bosma_waste NUMERIC;
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS rezka_lamination_waste NUMERIC;
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS rezka_edge_waste NUMERIC;
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS total_waste NUMERIC;
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS finished_goods_kg NUMERIC;
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS finished_goods_meter NUMERIC;
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS description TEXT NOT NULL DEFAULT '';
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS wip_status TEXT NOT NULL DEFAULT 'waiting';
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS current_apparatus TEXT NOT NULL DEFAULT '';
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS current_apparatus_key TEXT NOT NULL DEFAULT '';
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS current_location TEXT NOT NULL DEFAULT '';
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS next_apparatus TEXT NOT NULL DEFAULT '';
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS parent_batch_id TEXT NOT NULL DEFAULT '';
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS used_by_session_id TEXT NOT NULL DEFAULT '';
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS used_by_apparatus TEXT NOT NULL DEFAULT '';
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS processed_by_session_id TEXT NOT NULL DEFAULT '';
ALTER TABLE mini_progress_batches ADD COLUMN IF NOT EXISTS processed_by_apparatus TEXT NOT NULL DEFAULT '';
ALTER TABLE mini_progress_batches DROP CONSTRAINT IF EXISTS mini_progress_batches_wip_status_allowed;
ALTER TABLE mini_progress_batches ADD CONSTRAINT mini_progress_batches_wip_status_allowed CHECK (wip_status IN ('waiting', 'in_use', 'processed'));
UPDATE mini_progress_batches
SET current_apparatus_key = CASE
    WHEN btrim(current_apparatus) = '' THEN ''
    WHEN current_apparatus ~* '(^|[^[:alnum:]])7[[:space:]]*(ta[[:space:]]*)?rangli' THEN 'pechat:7'
    WHEN current_apparatus ~* '(^|[^[:alnum:]])8[[:space:]]*(ta[[:space:]]*)?rangli' THEN 'pechat:8'
    WHEN current_apparatus ~* '(^|[^[:alnum:]])9[[:space:]]*(ta[[:space:]]*)?rangli' THEN 'pechat:9'
    ELSE lower(regexp_replace(btrim(regexp_replace(current_apparatus, '[[:space:]]+', ' ', 'g')), '[[:space:]]+-[[:space:]]+[[:alnum:]_-]{1,16}$', ''))
END
WHERE btrim(current_apparatus_key) = '';

CREATE TABLE IF NOT EXISTS mini_apparatus_material_rules (
    apparatus TEXT PRIMARY KEY,
    item_groups JSONB NOT NULL DEFAULT '[]'::jsonb,
    requires_material BOOLEAN NOT NULL DEFAULT false,
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_apparatus_material_rules_apparatus_not_blank CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_apparatus_material_rules_groups_array CHECK (jsonb_typeof(item_groups) = 'array')
);

ALTER TABLE mini_apparatus_material_rules
    ADD COLUMN IF NOT EXISTS requires_material BOOLEAN NOT NULL DEFAULT false;

CREATE TABLE IF NOT EXISTS mini_raw_material_assignments (
    barcode TEXT PRIMARY KEY,
    order_id TEXT NOT NULL,
    apparatus TEXT NOT NULL,
    item_code TEXT NOT NULL,
    item_group TEXT NOT NULL,
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_raw_material_assignments_barcode_not_blank CHECK (btrim(barcode) <> ''),
    CONSTRAINT mini_raw_material_assignments_order_not_blank CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_raw_material_assignments_apparatus_not_blank CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_raw_material_assignments_item_code_not_blank CHECK (btrim(item_code) <> ''),
    CONSTRAINT mini_raw_material_assignments_item_group_not_blank CHECK (btrim(item_group) <> '')
);

ALTER TABLE mini_raw_material_assignments
    DROP CONSTRAINT IF EXISTS mini_raw_material_assignments_order_apparatus_unique;

CREATE TABLE IF NOT EXISTS mini_warehouses (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    company TEXT NOT NULL DEFAULT '',
    is_group BOOLEAN NOT NULL DEFAULT false,
    parent_warehouse TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_warehouses_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT mini_warehouses_name_unique UNIQUE (name)
);

CREATE TABLE IF NOT EXISTS mini_warehouse_assignments (
    warehouse TEXT NOT NULL,
    principal_role TEXT NOT NULL,
    principal_ref TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (warehouse, principal_role, principal_ref),
    CONSTRAINT mini_warehouse_assignments_warehouse_not_blank CHECK (btrim(warehouse) <> ''),
    CONSTRAINT mini_warehouse_assignments_role_not_blank CHECK (btrim(principal_role) <> ''),
    CONSTRAINT mini_warehouse_assignments_ref_not_blank CHECK (btrim(principal_ref) <> '')
);

CREATE TABLE IF NOT EXISTS mini_qolip_locations (
    id TEXT PRIMARY KEY,
    block TEXT NOT NULL,
    warehouse TEXT NOT NULL DEFAULT '',
    item_code TEXT NOT NULL,
    item_name TEXT NOT NULL,
    qolip_code TEXT NOT NULL,
    size INTEGER NOT NULL,
    quantity INTEGER NOT NULL,
    row_letter TEXT NOT NULL DEFAULT '',
    column_number INTEGER,
    location_label TEXT NOT NULL DEFAULT '',
    created_by_role TEXT NOT NULL DEFAULT '',
    created_by_ref TEXT NOT NULL DEFAULT '',
    created_by_name TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_qolip_locations_block_not_blank CHECK (btrim(block) <> ''),
    CONSTRAINT mini_qolip_locations_item_code_not_blank CHECK (btrim(item_code) <> ''),
    CONSTRAINT mini_qolip_locations_item_name_not_blank CHECK (btrim(item_name) <> ''),
    CONSTRAINT mini_qolip_locations_qolip_code_not_blank CHECK (btrim(qolip_code) <> ''),
    CONSTRAINT mini_qolip_locations_size_positive CHECK (size > 0),
    CONSTRAINT mini_qolip_locations_quantity_positive CHECK (quantity > 0),
    CONSTRAINT mini_qolip_locations_column_range CHECK (column_number IS NULL OR column_number BETWEEN 1 AND 9)
);

CREATE TABLE IF NOT EXISTS mini_qolip_product_specs (
    item_code TEXT PRIMARY KEY,
    item_name TEXT NOT NULL,
    item_group TEXT NOT NULL DEFAULT '',
    qolip_code TEXT NOT NULL,
    size INTEGER NOT NULL,
    created_by_role TEXT NOT NULL DEFAULT '',
    created_by_ref TEXT NOT NULL DEFAULT '',
    created_by_name TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_qolip_product_specs_item_code_not_blank CHECK (btrim(item_code) <> ''),
    CONSTRAINT mini_qolip_product_specs_item_name_not_blank CHECK (btrim(item_name) <> ''),
    CONSTRAINT mini_qolip_product_specs_qolip_code_not_blank CHECK (btrim(qolip_code) <> ''),
    CONSTRAINT mini_qolip_product_specs_size_positive CHECK (size > 0)
);

CREATE TABLE IF NOT EXISTS mini_qolip_cell_qrs (
    id TEXT PRIMARY KEY,
    block TEXT NOT NULL,
    warehouse TEXT NOT NULL DEFAULT '',
    row_letter TEXT NOT NULL,
    column_number INTEGER NOT NULL,
    location_label TEXT NOT NULL,
    qr_payload TEXT NOT NULL,
    created_by_role TEXT NOT NULL DEFAULT '',
    created_by_ref TEXT NOT NULL DEFAULT '',
    created_by_name TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_qolip_cell_qrs_block_not_blank CHECK (btrim(block) <> ''),
    CONSTRAINT mini_qolip_cell_qrs_row_not_blank CHECK (btrim(row_letter) <> ''),
    CONSTRAINT mini_qolip_cell_qrs_column_range CHECK (column_number BETWEEN 1 AND 9),
    CONSTRAINT mini_qolip_cell_qrs_label_not_blank CHECK (btrim(location_label) <> ''),
    CONSTRAINT mini_qolip_cell_qrs_qr_not_blank CHECK (btrim(qr_payload) <> ''),
    CONSTRAINT mini_qolip_cell_qrs_cell_unique UNIQUE (warehouse, block, row_letter, column_number),
    CONSTRAINT mini_qolip_cell_qrs_qr_unique UNIQUE (qr_payload)
);

CREATE TABLE IF NOT EXISTS mini_qolip_checkouts (
    id TEXT PRIMARY KEY,
    location_id TEXT NOT NULL,
    block TEXT NOT NULL,
    warehouse TEXT NOT NULL DEFAULT '',
    item_code TEXT NOT NULL,
    item_name TEXT NOT NULL,
    qolip_code TEXT NOT NULL,
    size INTEGER NOT NULL,
    quantity INTEGER NOT NULL,
    row_letter TEXT NOT NULL DEFAULT '',
    column_number INTEGER,
    location_label TEXT NOT NULL DEFAULT '',
    issued_to_ref TEXT NOT NULL,
    issued_to_name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    issued_by_role TEXT NOT NULL DEFAULT '',
    issued_by_ref TEXT NOT NULL DEFAULT '',
    issued_by_name TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    issued_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_qolip_checkouts_location_not_blank CHECK (btrim(location_id) <> ''),
    CONSTRAINT mini_qolip_checkouts_block_not_blank CHECK (btrim(block) <> ''),
    CONSTRAINT mini_qolip_checkouts_item_code_not_blank CHECK (btrim(item_code) <> ''),
    CONSTRAINT mini_qolip_checkouts_item_name_not_blank CHECK (btrim(item_name) <> ''),
    CONSTRAINT mini_qolip_checkouts_qolip_code_not_blank CHECK (btrim(qolip_code) <> ''),
    CONSTRAINT mini_qolip_checkouts_size_positive CHECK (size > 0),
    CONSTRAINT mini_qolip_checkouts_quantity_positive CHECK (quantity > 0),
    CONSTRAINT mini_qolip_checkouts_issued_to_ref_not_blank CHECK (btrim(issued_to_ref) <> ''),
    CONSTRAINT mini_qolip_checkouts_issued_to_name_not_blank CHECK (btrim(issued_to_name) <> ''),
    CONSTRAINT mini_qolip_checkouts_status_allowed CHECK (status IN ('open', 'returned', 'cancelled'))
);

CREATE TABLE IF NOT EXISTS mini_gscale_receipts (
    name TEXT PRIMARY KEY,
    status TEXT NOT NULL DEFAULT 'draft',
    item_code TEXT NOT NULL,
    warehouse TEXT NOT NULL,
    qty DOUBLE PRECISION NOT NULL,
    uom TEXT NOT NULL DEFAULT 'kg',
    barcode TEXT NOT NULL,
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    submitted_at TIMESTAMPTZ,
    CONSTRAINT mini_gscale_receipts_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT mini_gscale_receipts_item_code_not_blank CHECK (btrim(item_code) <> ''),
    CONSTRAINT mini_gscale_receipts_warehouse_not_blank CHECK (btrim(warehouse) <> ''),
    CONSTRAINT mini_gscale_receipts_barcode_not_blank CHECK (btrim(barcode) <> ''),
    CONSTRAINT mini_gscale_receipts_qty_positive CHECK (qty > 0),
    CONSTRAINT mini_gscale_receipts_status_allowed CHECK (status IN ('draft', 'submitted')),
    CONSTRAINT mini_gscale_receipts_barcode_unique UNIQUE (barcode)
);

CREATE TABLE IF NOT EXISTS mini_raw_material_stock (
    id TEXT PRIMARY KEY,
    warehouse TEXT NOT NULL,
    item_code TEXT NOT NULL,
    item_name TEXT NOT NULL DEFAULT '',
    barcode TEXT NOT NULL,
    qty DOUBLE PRECISION NOT NULL,
    uom TEXT NOT NULL DEFAULT 'kg',
    status TEXT NOT NULL DEFAULT 'available',
    reserved_order_id TEXT NOT NULL DEFAULT '',
    source_receipt_id TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_raw_material_stock_warehouse_not_blank CHECK (btrim(warehouse) <> ''),
    CONSTRAINT mini_raw_material_stock_item_code_not_blank CHECK (btrim(item_code) <> ''),
    CONSTRAINT mini_raw_material_stock_barcode_not_blank CHECK (btrim(barcode) <> ''),
    CONSTRAINT mini_raw_material_stock_qty_positive CHECK (qty > 0),
    CONSTRAINT mini_raw_material_stock_status_allowed CHECK (status IN ('available', 'reserved', 'in_use', 'consumed')),
    CONSTRAINT mini_raw_material_stock_barcode_unique UNIQUE (barcode)
);

ALTER TABLE mini_raw_material_stock DROP CONSTRAINT IF EXISTS mini_raw_material_stock_status_allowed;
ALTER TABLE mini_raw_material_stock
    ADD CONSTRAINT mini_raw_material_stock_status_allowed CHECK (status IN ('available', 'reserved', 'in_use', 'consumed'));

INSERT INTO mini_raw_material_stock (
    id, warehouse, item_code, item_name, barcode, qty, uom, status,
    source_receipt_id, payload_json, created_at, updated_at
)
SELECT
    'raw:' || lower(barcode),
    warehouse,
    item_code,
    item_code,
    barcode,
    qty,
    uom,
    'available',
    name,
    jsonb_build_object(
        'source_receipt_id', name,
        'source', 'mini_gscale_receipts_backfill'
    ),
    created_at,
    updated_at
FROM mini_gscale_receipts
WHERE status = 'submitted'
  AND btrim(warehouse) <> ''
  AND btrim(item_code) <> ''
  AND btrim(barcode) <> ''
ON CONFLICT (barcode) DO NOTHING;

CREATE TABLE IF NOT EXISTS mini_finished_goods_stock (
    id TEXT PRIMARY KEY,
    warehouse TEXT NOT NULL,
    order_id TEXT NOT NULL DEFAULT '',
    item_code TEXT NOT NULL,
    item_name TEXT NOT NULL DEFAULT '',
    qty DOUBLE PRECISION NOT NULL,
    uom TEXT NOT NULL DEFAULT 'dona',
    status TEXT NOT NULL DEFAULT 'available',
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_finished_goods_stock_warehouse_not_blank CHECK (btrim(warehouse) <> ''),
    CONSTRAINT mini_finished_goods_stock_item_code_not_blank CHECK (btrim(item_code) <> ''),
    CONSTRAINT mini_finished_goods_stock_qty_positive CHECK (qty > 0),
    CONSTRAINT mini_finished_goods_stock_status_allowed CHECK (status IN ('available', 'dispatched'))
);

CREATE TABLE IF NOT EXISTS mini_rps_batches (
    owner_key TEXT PRIMARY KEY,
    batch_id TEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT false,
    owner_role TEXT NOT NULL,
    owner_ref TEXT NOT NULL,
    item_code TEXT NOT NULL DEFAULT '',
    warehouse TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_rps_batches_owner_not_blank CHECK (btrim(owner_key) <> ''),
    CONSTRAINT mini_rps_batches_batch_not_blank CHECK (btrim(batch_id) <> ''),
    CONSTRAINT mini_rps_batches_owner_role_not_blank CHECK (btrim(owner_role) <> ''),
    CONSTRAINT mini_rps_batches_owner_ref_not_blank CHECK (btrim(owner_ref) <> '')
);

CREATE TABLE IF NOT EXISTS mini_engine_events (
    id BIGSERIAL PRIMARY KEY,
    event_id TEXT NOT NULL,
    domain TEXT NOT NULL,
    action TEXT NOT NULL,
    entity_id TEXT NOT NULL DEFAULT '',
    actor_key TEXT NOT NULL DEFAULT '',
    idempotency_key TEXT NOT NULL DEFAULT '',
    payload_json JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_engine_events_event_id_not_blank CHECK (btrim(event_id) <> ''),
    CONSTRAINT mini_engine_events_domain_not_blank CHECK (btrim(domain) <> ''),
    CONSTRAINT mini_engine_events_action_not_blank CHECK (btrim(action) <> ''),
    CONSTRAINT mini_engine_events_event_id_unique UNIQUE (event_id)
);

CREATE TABLE IF NOT EXISTS mini_idempotency_keys (
    key TEXT PRIMARY KEY,
    domain TEXT NOT NULL,
    action TEXT NOT NULL,
    entity_id TEXT NOT NULL DEFAULT '',
    response_json JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    CONSTRAINT mini_idempotency_keys_key_not_blank CHECK (btrim(key) <> ''),
    CONSTRAINT mini_idempotency_keys_domain_not_blank CHECK (btrim(domain) <> ''),
    CONSTRAINT mini_idempotency_keys_action_not_blank CHECK (btrim(action) <> '')
);

CREATE INDEX IF NOT EXISTS idx_mini_orders_customer_ref ON mini_orders(customer_ref);
CREATE INDEX IF NOT EXISTS idx_mini_orders_status ON mini_orders(status);
CREATE INDEX IF NOT EXISTS idx_mini_quick_order_templates_owner_saved ON mini_quick_order_templates(owner_key, saved_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_quick_order_templates_owner_quick_key ON mini_quick_order_templates(owner_key, quick_key);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_quick_order_templates_owner_lower_code ON mini_quick_order_templates (owner_key, lower(code));

WITH quick_template_dimensions AS (
    SELECT
        id,
        (payload_json ->> 'width_mm')::numeric AS width_mm,
        CASE
            WHEN payload_json ->> 'edge_allowance_mm' ~ '^-?[0-9]+(\.[0-9]+)?$'
                THEN (payload_json ->> 'edge_allowance_mm')::numeric
            ELSE 15
        END AS edge_allowance_mm,
        CASE
            WHEN payload_json ->> 'frame_product_size_mm' ~ '^-?[0-9]+(\.[0-9]+)?$'
                THEN (payload_json ->> 'frame_product_size_mm')::numeric
            ELSE 0
        END AS frame_product_size_mm,
        CASE
            WHEN payload_json ->> 'frame_count' ~ '^-?[0-9]+(\.[0-9]+)?$'
                THEN (payload_json ->> 'frame_count')::numeric
            ELSE 0
        END AS frame_count
    FROM mini_quick_order_templates
    WHERE payload_json ->> 'width_mm' ~ '^-?[0-9]+(\.[0-9]+)?$'
)
UPDATE mini_quick_order_templates templates
SET payload_json = jsonb_set(
        jsonb_set(templates.payload_json, '{frame_count}', '1'::jsonb, true),
        '{frame_product_size_mm}',
        to_jsonb(quick_template_dimensions.width_mm - quick_template_dimensions.edge_allowance_mm),
        true
    )
FROM quick_template_dimensions
WHERE templates.id = quick_template_dimensions.id
  AND quick_template_dimensions.width_mm > quick_template_dimensions.edge_allowance_mm
  AND (
      quick_template_dimensions.frame_product_size_mm <= 0
      OR quick_template_dimensions.frame_count <= 0
  );

CREATE INDEX IF NOT EXISTS idx_mini_push_tokens_owner ON mini_push_tokens(owner_key);
CREATE INDEX IF NOT EXISTS idx_mini_push_tokens_updated ON mini_push_tokens(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_items_lower_code ON mini_items(lower(code));
CREATE INDEX IF NOT EXISTS idx_mini_items_lower_name ON mini_items(lower(name));
CREATE INDEX IF NOT EXISTS idx_mini_items_lower_group ON mini_items(lower(item_group));
CREATE INDEX IF NOT EXISTS idx_mini_item_groups_lower_name ON mini_item_groups(lower(name));
CREATE INDEX IF NOT EXISTS idx_mini_item_groups_parent ON mini_item_groups(lower(parent_item_group));
CREATE INDEX IF NOT EXISTS idx_mini_production_maps_order_id ON mini_production_maps(order_id);
CREATE INDEX IF NOT EXISTS idx_mini_production_maps_order_number ON mini_production_maps(order_number) WHERE btrim(order_number) <> '';
CREATE INDEX IF NOT EXISTS idx_mini_production_map_nodes_kind_title ON mini_production_map_nodes(kind, lower(title));
CREATE INDEX IF NOT EXISTS idx_mini_production_map_nodes_title ON mini_production_map_nodes(lower(title));
CREATE INDEX IF NOT EXISTS idx_mini_production_map_edges_from ON mini_production_map_edges(from_node_id);
CREATE INDEX IF NOT EXISTS idx_mini_production_map_edges_to ON mini_production_map_edges(to_node_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_apparatus_groups_lower_name ON mini_apparatus_groups (lower(name));
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_apparatus_lower_name ON mini_apparatus (lower(name));
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_workers_lower_name ON mini_workers (lower(name));
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_warehouses_lower_name ON mini_warehouses (lower(name));
CREATE INDEX IF NOT EXISTS idx_mini_qolip_locations_block ON mini_qolip_locations (lower(block), row_letter, column_number);
CREATE INDEX IF NOT EXISTS idx_mini_qolip_locations_item ON mini_qolip_locations (lower(item_code), lower(item_name));
CREATE INDEX IF NOT EXISTS idx_mini_qolip_cell_qrs_cell ON mini_qolip_cell_qrs (lower(block), row_letter, column_number);
CREATE INDEX IF NOT EXISTS idx_mini_qolip_product_specs_item ON mini_qolip_product_specs (lower(item_code), lower(item_name), lower(qolip_code));
CREATE INDEX IF NOT EXISTS idx_mini_qolip_checkouts_status_issued ON mini_qolip_checkouts (status, issued_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_qolip_checkouts_block ON mini_qolip_checkouts (lower(block), issued_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_qolip_checkouts_worker ON mini_qolip_checkouts (lower(issued_to_ref), status, issued_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_gscale_receipts_status_updated ON mini_gscale_receipts (status, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_gscale_receipts_item_updated ON mini_gscale_receipts (lower(item_code), updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_workers_level ON mini_workers(level);
CREATE INDEX IF NOT EXISTS idx_mini_worker_groups_apparatus ON mini_worker_groups (lower(apparatus));
CREATE INDEX IF NOT EXISTS idx_mini_worker_groups_shift ON mini_worker_groups (shift);
CREATE INDEX IF NOT EXISTS idx_mini_queue_states_order_id ON mini_queue_states(order_id);
CREATE INDEX IF NOT EXISTS idx_mini_queue_action_events_apparatus_created ON mini_queue_action_events(apparatus, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_queue_action_events_order_created ON mini_queue_action_events(order_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_queue_action_events_actor_created ON mini_queue_action_events(actor_role, actor_ref, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_order_run_sessions_order_status ON mini_order_run_sessions(order_id, status, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_order_run_sessions_apparatus_order ON mini_order_run_sessions(lower(apparatus), order_id, updated_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_order_run_sessions_one_open
    ON mini_order_run_sessions(lower(apparatus), order_id)
    WHERE status IN ('active', 'paused');
CREATE INDEX IF NOT EXISTS idx_mini_order_progress_events_order_created ON mini_order_progress_events(order_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_progress_batches_order_created ON mini_progress_batches(order_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_progress_batches_qr ON mini_progress_batches(lower(qr_payload));
CREATE INDEX IF NOT EXISTS idx_mini_progress_batches_wip_status_apparatus
    ON mini_progress_batches(wip_status, lower(current_apparatus), updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_progress_batches_wip_status_apparatus_key
    ON mini_progress_batches(wip_status, current_apparatus_key, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_raw_material_assignments_order ON mini_raw_material_assignments(order_id);
CREATE INDEX IF NOT EXISTS idx_mini_raw_material_assignments_apparatus ON mini_raw_material_assignments(lower(apparatus));
CREATE INDEX IF NOT EXISTS idx_mini_raw_material_assignments_item_group ON mini_raw_material_assignments(lower(item_group));
CREATE INDEX IF NOT EXISTS idx_mini_rps_batches_active ON mini_rps_batches(active) WHERE active;
CREATE INDEX IF NOT EXISTS idx_mini_engine_events_entity ON mini_engine_events(domain, entity_id, created_at DESC);

INSERT INTO mini_production_map_nodes (map_id, node_id, kind, title, payload_json)
SELECT
    maps.id,
    btrim(node.payload ->> 'id'),
    btrim(node.payload ->> 'kind'),
    COALESCE(btrim(node.payload ->> 'title'), ''),
    node.payload
FROM mini_production_maps maps
CROSS JOIN LATERAL jsonb_array_elements(COALESCE(maps.map_json -> 'nodes', '[]'::jsonb)) AS node(payload)
WHERE btrim(COALESCE(node.payload ->> 'id', '')) <> ''
  AND btrim(COALESCE(node.payload ->> 'kind', '')) <> ''
ON CONFLICT (map_id, node_id) DO UPDATE SET
    kind = excluded.kind,
    title = excluded.title,
    payload_json = excluded.payload_json;

INSERT INTO mini_production_map_edges (map_id, edge_index, from_node_id, to_node_id, branch, payload_json)
SELECT
    maps.id,
    (edge.ordinality - 1)::integer,
    btrim(edge.payload ->> 'from'),
    btrim(edge.payload ->> 'to'),
    COALESCE(btrim(edge.payload ->> 'branch'), ''),
    edge.payload
FROM mini_production_maps maps
CROSS JOIN LATERAL jsonb_array_elements(COALESCE(maps.map_json -> 'edges', '[]'::jsonb))
    WITH ORDINALITY AS edge(payload, ordinality)
WHERE btrim(COALESCE(edge.payload ->> 'from', '')) <> ''
  AND btrim(COALESCE(edge.payload ->> 'to', '')) <> ''
ON CONFLICT (map_id, edge_index) DO UPDATE SET
    from_node_id = excluded.from_node_id,
    to_node_id = excluded.to_node_id,
    branch = excluded.branch,
    payload_json = excluded.payload_json;
