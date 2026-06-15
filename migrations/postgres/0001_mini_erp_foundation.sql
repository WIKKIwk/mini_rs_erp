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

CREATE TABLE IF NOT EXISTS mini_queue_sequences (
    apparatus TEXT PRIMARY KEY,
    order_ids JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_queue_sequences_apparatus_not_blank CHECK (btrim(apparatus) <> '')
);

CREATE TABLE IF NOT EXISTS mini_daily_work_sequences (
    work_date TEXT PRIMARY KEY,
    order_ids JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_daily_work_sequences_date_format CHECK (
        work_date ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'
    )
);

CREATE TABLE IF NOT EXISTS mini_queue_states (
    apparatus TEXT NOT NULL,
    order_id TEXT NOT NULL,
    state TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (apparatus, order_id),
    CONSTRAINT mini_queue_states_apparatus_not_blank CHECK (btrim(apparatus) <> ''),
    CONSTRAINT mini_queue_states_order_id_not_blank CHECK (btrim(order_id) <> ''),
    CONSTRAINT mini_queue_states_state_allowed CHECK (state IN ('pending', 'in_progress', 'completed'))
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
CREATE INDEX IF NOT EXISTS idx_mini_production_maps_order_id ON mini_production_maps(order_id);
CREATE INDEX IF NOT EXISTS idx_mini_production_maps_order_number ON mini_production_maps(order_number) WHERE btrim(order_number) <> '';
CREATE INDEX IF NOT EXISTS idx_mini_production_map_nodes_kind_title ON mini_production_map_nodes(kind, lower(title));
CREATE INDEX IF NOT EXISTS idx_mini_production_map_nodes_title ON mini_production_map_nodes(lower(title));
CREATE INDEX IF NOT EXISTS idx_mini_production_map_edges_from ON mini_production_map_edges(from_node_id);
CREATE INDEX IF NOT EXISTS idx_mini_production_map_edges_to ON mini_production_map_edges(to_node_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_apparatus_groups_lower_name ON mini_apparatus_groups (lower(name));
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_apparatus_lower_name ON mini_apparatus (lower(name));
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_workers_lower_name ON mini_workers (lower(name));
CREATE INDEX IF NOT EXISTS idx_mini_workers_level ON mini_workers(level);
CREATE INDEX IF NOT EXISTS idx_mini_queue_states_order_id ON mini_queue_states(order_id);
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
