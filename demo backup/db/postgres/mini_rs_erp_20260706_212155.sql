--
-- PostgreSQL database dump
--

\restrict AHI20H1RKU00CoIswOW2dtGi9oeK2EJRUf1woPepG35H5jRVogZN6TXEPNEvmZG

-- Dumped from database version 16.13 (Homebrew)
-- Dumped by pg_dump version 16.13 (Homebrew)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: mini_apparatus; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_apparatus (
    id text NOT NULL,
    group_id text,
    name text NOT NULL,
    base_name text DEFAULT ''::text NOT NULL,
    kind text DEFAULT ''::text NOT NULL,
    payload_json jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_apparatus_name_not_blank CHECK ((btrim(name) <> ''::text))
);


--
-- Name: mini_apparatus_groups; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_apparatus_groups (
    id text NOT NULL,
    name text NOT NULL,
    payload_json jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_apparatus_groups_name_not_blank CHECK ((btrim(name) <> ''::text))
);


--
-- Name: mini_apparatus_material_rules; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_apparatus_material_rules (
    apparatus text NOT NULL,
    item_groups jsonb DEFAULT '[]'::jsonb NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    requires_material boolean DEFAULT false NOT NULL,
    requirement_groups jsonb DEFAULT '[]'::jsonb NOT NULL,
    CONSTRAINT mini_apparatus_material_rules_apparatus_not_blank CHECK ((btrim(apparatus) <> ''::text)),
    CONSTRAINT mini_apparatus_material_rules_groups_array CHECK ((jsonb_typeof(item_groups) = 'array'::text))
);


--
-- Name: mini_apparatus_queue_policies; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_apparatus_queue_policies (
    apparatus text NOT NULL,
    policy text NOT NULL,
    actor_role text DEFAULT ''::text NOT NULL,
    actor_ref text DEFAULT ''::text NOT NULL,
    actor_display_name text DEFAULT ''::text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_apparatus_queue_policies_apparatus_not_blank CHECK ((btrim(apparatus) <> ''::text)),
    CONSTRAINT mini_apparatus_queue_policies_policy_allowed CHECK ((policy = ANY (ARRAY['strict_sequence'::text, 'free_pick'::text])))
);


--
-- Name: mini_daily_apparatus_sequences; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_daily_apparatus_sequences (
    work_date text NOT NULL,
    apparatus text NOT NULL,
    order_ids jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_daily_apparatus_sequences_apparatus_not_blank CHECK ((btrim(apparatus) <> ''::text)),
    CONSTRAINT mini_daily_apparatus_sequences_date_format CHECK ((work_date ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'::text))
);


--
-- Name: mini_daily_work_sequences; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_daily_work_sequences (
    work_date text NOT NULL,
    order_ids jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_daily_work_sequences_date_format CHECK ((work_date ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}$'::text))
);


--
-- Name: mini_engine_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_engine_events (
    id bigint NOT NULL,
    event_id text NOT NULL,
    domain text NOT NULL,
    action text NOT NULL,
    entity_id text DEFAULT ''::text NOT NULL,
    actor_key text DEFAULT ''::text NOT NULL,
    idempotency_key text DEFAULT ''::text NOT NULL,
    payload_json jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_engine_events_action_not_blank CHECK ((btrim(action) <> ''::text)),
    CONSTRAINT mini_engine_events_domain_not_blank CHECK ((btrim(domain) <> ''::text)),
    CONSTRAINT mini_engine_events_event_id_not_blank CHECK ((btrim(event_id) <> ''::text))
);


--
-- Name: mini_engine_events_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.mini_engine_events_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: mini_engine_events_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.mini_engine_events_id_seq OWNED BY public.mini_engine_events.id;


--
-- Name: mini_finished_goods_stock; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_finished_goods_stock (
    id text NOT NULL,
    warehouse text NOT NULL,
    order_id text DEFAULT ''::text NOT NULL,
    item_code text NOT NULL,
    item_name text DEFAULT ''::text NOT NULL,
    qty double precision NOT NULL,
    uom text DEFAULT 'dona'::text NOT NULL,
    status text DEFAULT 'available'::text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_finished_goods_stock_item_code_not_blank CHECK ((btrim(item_code) <> ''::text)),
    CONSTRAINT mini_finished_goods_stock_qty_positive CHECK ((qty > (0)::double precision)),
    CONSTRAINT mini_finished_goods_stock_status_allowed CHECK ((status = ANY (ARRAY['available'::text, 'dispatched'::text]))),
    CONSTRAINT mini_finished_goods_stock_warehouse_not_blank CHECK ((btrim(warehouse) <> ''::text))
);


--
-- Name: mini_gscale_receipts; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_gscale_receipts (
    name text NOT NULL,
    status text DEFAULT 'draft'::text NOT NULL,
    item_code text NOT NULL,
    warehouse text NOT NULL,
    qty double precision NOT NULL,
    uom text DEFAULT 'kg'::text NOT NULL,
    barcode text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    submitted_at timestamp with time zone,
    CONSTRAINT mini_gscale_receipts_barcode_not_blank CHECK ((btrim(barcode) <> ''::text)),
    CONSTRAINT mini_gscale_receipts_item_code_not_blank CHECK ((btrim(item_code) <> ''::text)),
    CONSTRAINT mini_gscale_receipts_name_not_blank CHECK ((btrim(name) <> ''::text)),
    CONSTRAINT mini_gscale_receipts_qty_positive CHECK ((qty > (0)::double precision)),
    CONSTRAINT mini_gscale_receipts_status_allowed CHECK ((status = ANY (ARRAY['draft'::text, 'submitted'::text]))),
    CONSTRAINT mini_gscale_receipts_warehouse_not_blank CHECK ((btrim(warehouse) <> ''::text))
);


--
-- Name: mini_idempotency_keys; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_idempotency_keys (
    key text NOT NULL,
    domain text NOT NULL,
    action text NOT NULL,
    entity_id text DEFAULT ''::text NOT NULL,
    response_json jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    completed_at timestamp with time zone,
    CONSTRAINT mini_idempotency_keys_action_not_blank CHECK ((btrim(action) <> ''::text)),
    CONSTRAINT mini_idempotency_keys_domain_not_blank CHECK ((btrim(domain) <> ''::text)),
    CONSTRAINT mini_idempotency_keys_key_not_blank CHECK ((btrim(key) <> ''::text))
);


--
-- Name: mini_item_groups; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_item_groups (
    name text NOT NULL,
    parent_item_group text DEFAULT ''::text NOT NULL,
    is_group boolean DEFAULT true NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_item_groups_name_not_blank CHECK ((btrim(name) <> ''::text))
);


--
-- Name: mini_items; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_items (
    code text NOT NULL,
    name text NOT NULL,
    uom text DEFAULT 'Kg'::text NOT NULL,
    warehouse text DEFAULT ''::text NOT NULL,
    item_group text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_items_code_not_blank CHECK ((btrim(code) <> ''::text)),
    CONSTRAINT mini_items_group_not_blank CHECK ((btrim(item_group) <> ''::text)),
    CONSTRAINT mini_items_name_not_blank CHECK ((btrim(name) <> ''::text)),
    CONSTRAINT mini_items_uom_not_blank CHECK ((btrim(uom) <> ''::text))
);


--
-- Name: mini_order_products; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_order_products (
    id text NOT NULL,
    order_id text NOT NULL,
    item_code text DEFAULT ''::text NOT NULL,
    product_name text NOT NULL,
    material_display text DEFAULT ''::text NOT NULL,
    color text DEFAULT ''::text NOT NULL,
    first_layer_material text DEFAULT ''::text NOT NULL,
    first_layer_micron text DEFAULT ''::text NOT NULL,
    second_layer_material text DEFAULT ''::text NOT NULL,
    second_layer_micron text DEFAULT ''::text NOT NULL,
    third_layer_material text DEFAULT ''::text NOT NULL,
    third_layer_micron text DEFAULT ''::text NOT NULL,
    note text DEFAULT ''::text NOT NULL,
    CONSTRAINT mini_order_products_product_name_not_blank CHECK ((btrim(product_name) <> ''::text))
);


--
-- Name: mini_order_progress_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_order_progress_events (
    id bigint NOT NULL,
    event_id text NOT NULL,
    session_id text NOT NULL,
    batch_id text DEFAULT ''::text NOT NULL,
    apparatus text NOT NULL,
    order_id text NOT NULL,
    action text NOT NULL,
    produced_qty numeric DEFAULT 0 NOT NULL,
    uom text DEFAULT ''::text NOT NULL,
    worker_role text DEFAULT ''::text NOT NULL,
    worker_ref text DEFAULT ''::text NOT NULL,
    worker_display_name text DEFAULT ''::text NOT NULL,
    qr_payload text DEFAULT ''::text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    return_ink_kg numeric,
    lamination_print_leftover_rolls numeric,
    lamination_film_leftover_rolls numeric,
    rezka_bosma_waste numeric,
    rezka_lamination_waste numeric,
    rezka_edge_waste numeric,
    total_waste numeric,
    finished_goods_kg numeric,
    finished_goods_meter numeric,
    description text DEFAULT ''::text NOT NULL,
    CONSTRAINT mini_order_progress_events_action_allowed CHECK ((action = ANY (ARRAY['start'::text, 'pause'::text, 'resume'::text, 'complete'::text]))),
    CONSTRAINT mini_order_progress_events_apparatus_not_blank CHECK ((btrim(apparatus) <> ''::text)),
    CONSTRAINT mini_order_progress_events_event_id_not_blank CHECK ((btrim(event_id) <> ''::text)),
    CONSTRAINT mini_order_progress_events_order_id_not_blank CHECK ((btrim(order_id) <> ''::text)),
    CONSTRAINT mini_order_progress_events_qty_non_negative CHECK ((produced_qty >= (0)::numeric)),
    CONSTRAINT mini_order_progress_events_session_id_not_blank CHECK ((btrim(session_id) <> ''::text))
);


--
-- Name: mini_order_progress_events_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.mini_order_progress_events_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: mini_order_progress_events_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.mini_order_progress_events_id_seq OWNED BY public.mini_order_progress_events.id;


--
-- Name: mini_order_run_sessions; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_order_run_sessions (
    session_id text NOT NULL,
    apparatus text NOT NULL,
    order_id text NOT NULL,
    status text NOT NULL,
    worker_role text DEFAULT ''::text NOT NULL,
    worker_ref text DEFAULT ''::text NOT NULL,
    worker_display_name text DEFAULT ''::text NOT NULL,
    started_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    CONSTRAINT mini_order_run_sessions_apparatus_not_blank CHECK ((btrim(apparatus) <> ''::text)),
    CONSTRAINT mini_order_run_sessions_order_id_not_blank CHECK ((btrim(order_id) <> ''::text)),
    CONSTRAINT mini_order_run_sessions_session_id_not_blank CHECK ((btrim(session_id) <> ''::text)),
    CONSTRAINT mini_order_run_sessions_status_allowed CHECK ((status = ANY (ARRAY['active'::text, 'paused'::text, 'completed'::text])))
);


--
-- Name: mini_orders; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_orders (
    id text NOT NULL,
    code text NOT NULL,
    order_number text DEFAULT ''::text NOT NULL,
    customer_ref text DEFAULT ''::text NOT NULL,
    customer_name text DEFAULT ''::text NOT NULL,
    product_code text DEFAULT ''::text NOT NULL,
    product_name text NOT NULL,
    status text DEFAULT 'draft'::text NOT NULL,
    kg numeric(14,3) DEFAULT 0 NOT NULL,
    width_mm numeric(14,3),
    roll_count numeric(14,3),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_orders_code_not_blank CHECK ((btrim(code) <> ''::text)),
    CONSTRAINT mini_orders_kg_non_negative CHECK ((kg >= (0)::numeric)),
    CONSTRAINT mini_orders_product_name_not_blank CHECK ((btrim(product_name) <> ''::text)),
    CONSTRAINT mini_orders_roll_count_positive CHECK (((roll_count IS NULL) OR (roll_count > (0)::numeric))),
    CONSTRAINT mini_orders_width_positive CHECK (((width_mm IS NULL) OR (width_mm > (0)::numeric)))
);


--
-- Name: mini_production_map_edges; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_production_map_edges (
    map_id text NOT NULL,
    edge_index integer NOT NULL,
    from_node_id text NOT NULL,
    to_node_id text NOT NULL,
    branch text DEFAULT ''::text NOT NULL,
    payload_json jsonb NOT NULL,
    CONSTRAINT mini_production_map_edges_from_not_blank CHECK ((btrim(from_node_id) <> ''::text)),
    CONSTRAINT mini_production_map_edges_to_not_blank CHECK ((btrim(to_node_id) <> ''::text))
);


--
-- Name: mini_production_map_nodes; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_production_map_nodes (
    map_id text NOT NULL,
    node_id text NOT NULL,
    kind text NOT NULL,
    title text DEFAULT ''::text NOT NULL,
    payload_json jsonb NOT NULL,
    CONSTRAINT mini_production_map_nodes_kind_not_blank CHECK ((btrim(kind) <> ''::text))
);


--
-- Name: mini_production_maps; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_production_maps (
    id text NOT NULL,
    order_id text,
    product_code text NOT NULL,
    title text NOT NULL,
    code text DEFAULT ''::text NOT NULL,
    order_number text DEFAULT ''::text NOT NULL,
    roll_count numeric(14,3),
    width_mm numeric(14,3),
    map_json jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_production_maps_product_code_not_blank CHECK ((btrim(product_code) <> ''::text)),
    CONSTRAINT mini_production_maps_roll_count_positive CHECK (((roll_count IS NULL) OR (roll_count > (0)::numeric))),
    CONSTRAINT mini_production_maps_title_not_blank CHECK ((btrim(title) <> ''::text)),
    CONSTRAINT mini_production_maps_width_positive CHECK (((width_mm IS NULL) OR (width_mm > (0)::numeric)))
);


--
-- Name: mini_progress_batches; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_progress_batches (
    batch_id text NOT NULL,
    session_id text NOT NULL,
    apparatus text NOT NULL,
    order_id text NOT NULL,
    action text NOT NULL,
    status text NOT NULL,
    produced_qty numeric NOT NULL,
    uom text NOT NULL,
    qr_payload text NOT NULL,
    label_item_code text NOT NULL,
    label_item_name text NOT NULL,
    executor_name text DEFAULT ''::text NOT NULL,
    worker_role text DEFAULT ''::text NOT NULL,
    worker_ref text DEFAULT ''::text NOT NULL,
    worker_display_name text DEFAULT ''::text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    return_ink_kg numeric,
    lamination_print_leftover_rolls numeric,
    lamination_film_leftover_rolls numeric,
    rezka_bosma_waste numeric,
    rezka_lamination_waste numeric,
    rezka_edge_waste numeric,
    total_waste numeric,
    finished_goods_kg numeric,
    finished_goods_meter numeric,
    description text DEFAULT ''::text NOT NULL,
    wip_status text DEFAULT 'waiting'::text NOT NULL,
    current_apparatus text DEFAULT ''::text NOT NULL,
    current_apparatus_key text DEFAULT ''::text NOT NULL,
    current_location text DEFAULT ''::text NOT NULL,
    next_apparatus text DEFAULT ''::text NOT NULL,
    parent_batch_id text DEFAULT ''::text NOT NULL,
    used_by_session_id text DEFAULT ''::text NOT NULL,
    used_by_apparatus text DEFAULT ''::text NOT NULL,
    processed_by_session_id text DEFAULT ''::text NOT NULL,
    processed_by_apparatus text DEFAULT ''::text NOT NULL,
    CONSTRAINT mini_progress_batches_action_allowed CHECK ((action = ANY (ARRAY['pause'::text, 'complete'::text]))),
    CONSTRAINT mini_progress_batches_apparatus_not_blank CHECK ((btrim(apparatus) <> ''::text)),
    CONSTRAINT mini_progress_batches_batch_id_not_blank CHECK ((btrim(batch_id) <> ''::text)),
    CONSTRAINT mini_progress_batches_label_item_code_not_blank CHECK ((btrim(label_item_code) <> ''::text)),
    CONSTRAINT mini_progress_batches_label_item_name_not_blank CHECK ((btrim(label_item_name) <> ''::text)),
    CONSTRAINT mini_progress_batches_order_id_not_blank CHECK ((btrim(order_id) <> ''::text)),
    CONSTRAINT mini_progress_batches_qr_payload_not_blank CHECK ((btrim(qr_payload) <> ''::text)),
    CONSTRAINT mini_progress_batches_qty_positive CHECK ((produced_qty > (0)::numeric)),
    CONSTRAINT mini_progress_batches_session_id_not_blank CHECK ((btrim(session_id) <> ''::text)),
    CONSTRAINT mini_progress_batches_status_allowed CHECK ((status = ANY (ARRAY['paused'::text, 'completed'::text, 'resumed'::text]))),
    CONSTRAINT mini_progress_batches_uom_not_blank CHECK ((btrim(uom) <> ''::text)),
    CONSTRAINT mini_progress_batches_wip_status_allowed CHECK ((wip_status = ANY (ARRAY['waiting'::text, 'in_use'::text, 'processed'::text])))
);


--
-- Name: mini_push_tokens; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_push_tokens (
    token text NOT NULL,
    owner_key text NOT NULL,
    platform text DEFAULT ''::text NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_push_tokens_owner_not_blank CHECK ((btrim(owner_key) <> ''::text)),
    CONSTRAINT mini_push_tokens_token_not_blank CHECK ((btrim(token) <> ''::text))
);


--
-- Name: mini_qolip_cell_qrs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_qolip_cell_qrs (
    id text NOT NULL,
    block text NOT NULL,
    warehouse text DEFAULT ''::text NOT NULL,
    row_letter text NOT NULL,
    column_number integer NOT NULL,
    location_label text NOT NULL,
    qr_payload text NOT NULL,
    created_by_role text DEFAULT ''::text NOT NULL,
    created_by_ref text DEFAULT ''::text NOT NULL,
    created_by_name text DEFAULT ''::text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_qolip_cell_qrs_block_not_blank CHECK ((btrim(block) <> ''::text)),
    CONSTRAINT mini_qolip_cell_qrs_column_range CHECK (((column_number >= 1) AND (column_number <= 9))),
    CONSTRAINT mini_qolip_cell_qrs_label_not_blank CHECK ((btrim(location_label) <> ''::text)),
    CONSTRAINT mini_qolip_cell_qrs_qr_not_blank CHECK ((btrim(qr_payload) <> ''::text)),
    CONSTRAINT mini_qolip_cell_qrs_row_not_blank CHECK ((btrim(row_letter) <> ''::text))
);


--
-- Name: mini_qolip_checkouts; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_qolip_checkouts (
    id text NOT NULL,
    location_id text NOT NULL,
    block text NOT NULL,
    warehouse text DEFAULT ''::text NOT NULL,
    item_code text NOT NULL,
    item_name text NOT NULL,
    qolip_code text NOT NULL,
    size integer NOT NULL,
    quantity integer NOT NULL,
    row_letter text DEFAULT ''::text NOT NULL,
    column_number integer,
    location_label text DEFAULT ''::text NOT NULL,
    issued_to_ref text NOT NULL,
    issued_to_name text NOT NULL,
    status text DEFAULT 'open'::text NOT NULL,
    issued_by_role text DEFAULT ''::text NOT NULL,
    issued_by_ref text DEFAULT ''::text NOT NULL,
    issued_by_name text DEFAULT ''::text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    issued_at timestamp with time zone DEFAULT now() NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_qolip_checkouts_block_not_blank CHECK ((btrim(block) <> ''::text)),
    CONSTRAINT mini_qolip_checkouts_issued_to_name_not_blank CHECK ((btrim(issued_to_name) <> ''::text)),
    CONSTRAINT mini_qolip_checkouts_issued_to_ref_not_blank CHECK ((btrim(issued_to_ref) <> ''::text)),
    CONSTRAINT mini_qolip_checkouts_item_code_not_blank CHECK ((btrim(item_code) <> ''::text)),
    CONSTRAINT mini_qolip_checkouts_item_name_not_blank CHECK ((btrim(item_name) <> ''::text)),
    CONSTRAINT mini_qolip_checkouts_location_not_blank CHECK ((btrim(location_id) <> ''::text)),
    CONSTRAINT mini_qolip_checkouts_qolip_code_not_blank CHECK ((btrim(qolip_code) <> ''::text)),
    CONSTRAINT mini_qolip_checkouts_quantity_positive CHECK ((quantity > 0)),
    CONSTRAINT mini_qolip_checkouts_size_positive CHECK ((size > 0)),
    CONSTRAINT mini_qolip_checkouts_status_allowed CHECK ((status = ANY (ARRAY['open'::text, 'returned'::text, 'cancelled'::text])))
);


--
-- Name: mini_qolip_locations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_qolip_locations (
    id text NOT NULL,
    block text NOT NULL,
    warehouse text DEFAULT ''::text NOT NULL,
    item_code text NOT NULL,
    item_name text NOT NULL,
    qolip_code text NOT NULL,
    size integer NOT NULL,
    quantity integer NOT NULL,
    row_letter text DEFAULT ''::text NOT NULL,
    column_number integer,
    location_label text DEFAULT ''::text NOT NULL,
    created_by_role text DEFAULT ''::text NOT NULL,
    created_by_ref text DEFAULT ''::text NOT NULL,
    created_by_name text DEFAULT ''::text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_qolip_locations_block_not_blank CHECK ((btrim(block) <> ''::text)),
    CONSTRAINT mini_qolip_locations_column_range CHECK (((column_number IS NULL) OR ((column_number >= 1) AND (column_number <= 9)))),
    CONSTRAINT mini_qolip_locations_item_code_not_blank CHECK ((btrim(item_code) <> ''::text)),
    CONSTRAINT mini_qolip_locations_item_name_not_blank CHECK ((btrim(item_name) <> ''::text)),
    CONSTRAINT mini_qolip_locations_qolip_code_not_blank CHECK ((btrim(qolip_code) <> ''::text)),
    CONSTRAINT mini_qolip_locations_quantity_positive CHECK ((quantity > 0)),
    CONSTRAINT mini_qolip_locations_size_positive CHECK ((size > 0))
);


--
-- Name: mini_qolip_product_specs; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_qolip_product_specs (
    item_code text NOT NULL,
    item_name text NOT NULL,
    item_group text DEFAULT ''::text NOT NULL,
    qolip_code text NOT NULL,
    size integer NOT NULL,
    created_by_role text DEFAULT ''::text NOT NULL,
    created_by_ref text DEFAULT ''::text NOT NULL,
    created_by_name text DEFAULT ''::text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_qolip_product_specs_item_code_not_blank CHECK ((btrim(item_code) <> ''::text)),
    CONSTRAINT mini_qolip_product_specs_item_name_not_blank CHECK ((btrim(item_name) <> ''::text)),
    CONSTRAINT mini_qolip_product_specs_qolip_code_not_blank CHECK ((btrim(qolip_code) <> ''::text)),
    CONSTRAINT mini_qolip_product_specs_size_positive CHECK ((size > 0))
);


--
-- Name: mini_queue_action_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_queue_action_events (
    id bigint NOT NULL,
    event_id text NOT NULL,
    apparatus text NOT NULL,
    order_id text NOT NULL,
    action text NOT NULL,
    from_state text NOT NULL,
    to_state text NOT NULL,
    policy text NOT NULL,
    actor_role text DEFAULT ''::text NOT NULL,
    actor_ref text DEFAULT ''::text NOT NULL,
    actor_display_name text DEFAULT ''::text NOT NULL,
    assigned_apparatus jsonb DEFAULT '[]'::jsonb NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_queue_action_events_action_allowed CHECK ((action = ANY (ARRAY['start'::text, 'pause'::text, 'resume'::text, 'complete'::text]))),
    CONSTRAINT mini_queue_action_events_apparatus_not_blank CHECK ((btrim(apparatus) <> ''::text)),
    CONSTRAINT mini_queue_action_events_assigned_array CHECK ((jsonb_typeof(assigned_apparatus) = 'array'::text)),
    CONSTRAINT mini_queue_action_events_event_id_not_blank CHECK ((btrim(event_id) <> ''::text)),
    CONSTRAINT mini_queue_action_events_from_state_allowed CHECK ((from_state = ANY (ARRAY['pending'::text, 'in_progress'::text, 'paused'::text, 'completed'::text]))),
    CONSTRAINT mini_queue_action_events_order_id_not_blank CHECK ((btrim(order_id) <> ''::text)),
    CONSTRAINT mini_queue_action_events_policy_allowed CHECK ((policy = ANY (ARRAY['strict_sequence'::text, 'free_pick'::text]))),
    CONSTRAINT mini_queue_action_events_to_state_allowed CHECK ((to_state = ANY (ARRAY['pending'::text, 'in_progress'::text, 'paused'::text, 'completed'::text])))
);


--
-- Name: mini_queue_action_events_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

CREATE SEQUENCE public.mini_queue_action_events_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


--
-- Name: mini_queue_action_events_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: -
--

ALTER SEQUENCE public.mini_queue_action_events_id_seq OWNED BY public.mini_queue_action_events.id;


--
-- Name: mini_queue_sequences; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_queue_sequences (
    apparatus text NOT NULL,
    order_ids jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_queue_sequences_apparatus_not_blank CHECK ((btrim(apparatus) <> ''::text))
);


--
-- Name: mini_queue_states; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_queue_states (
    apparatus text NOT NULL,
    order_id text NOT NULL,
    state text NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_queue_states_apparatus_not_blank CHECK ((btrim(apparatus) <> ''::text)),
    CONSTRAINT mini_queue_states_order_id_not_blank CHECK ((btrim(order_id) <> ''::text)),
    CONSTRAINT mini_queue_states_state_allowed CHECK ((state = ANY (ARRAY['pending'::text, 'in_progress'::text, 'paused'::text, 'completed'::text])))
);


--
-- Name: mini_quick_order_images; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_quick_order_images (
    owner_key text NOT NULL,
    image_id text NOT NULL,
    image_name text NOT NULL,
    image_mime text NOT NULL,
    image_size_bytes bigint NOT NULL,
    body bytea NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_quick_order_images_id_not_blank CHECK ((btrim(image_id) <> ''::text)),
    CONSTRAINT mini_quick_order_images_mime_not_blank CHECK ((btrim(image_mime) <> ''::text)),
    CONSTRAINT mini_quick_order_images_name_not_blank CHECK ((btrim(image_name) <> ''::text)),
    CONSTRAINT mini_quick_order_images_owner_not_blank CHECK ((btrim(owner_key) <> ''::text)),
    CONSTRAINT mini_quick_order_images_size_non_negative CHECK ((image_size_bytes >= 0))
);


--
-- Name: mini_quick_order_templates; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_quick_order_templates (
    id text NOT NULL,
    owner_key text NOT NULL,
    code text NOT NULL,
    name text NOT NULL,
    item_code text DEFAULT ''::text NOT NULL,
    product_name text NOT NULL,
    customer_ref text DEFAULT ''::text NOT NULL,
    customer_name text DEFAULT ''::text NOT NULL,
    payload_json jsonb NOT NULL,
    quick_key text NOT NULL,
    saved_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_quick_order_templates_code_not_blank CHECK ((btrim(code) <> ''::text)),
    CONSTRAINT mini_quick_order_templates_name_not_blank CHECK ((btrim(name) <> ''::text)),
    CONSTRAINT mini_quick_order_templates_owner_not_blank CHECK ((btrim(owner_key) <> ''::text)),
    CONSTRAINT mini_quick_order_templates_product_not_blank CHECK ((btrim(product_name) <> ''::text))
);


--
-- Name: mini_quick_order_templates_backup_frame_fields_20260620_131158; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_quick_order_templates_backup_frame_fields_20260620_131158 (
    id text,
    owner_key text,
    code text,
    name text,
    item_code text,
    product_name text,
    customer_ref text,
    customer_name text,
    payload_json jsonb,
    quick_key text,
    saved_at timestamp with time zone
);


--
-- Name: mini_raw_material_assignments; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_raw_material_assignments (
    barcode text NOT NULL,
    order_id text NOT NULL,
    apparatus text NOT NULL,
    item_code text NOT NULL,
    item_group text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_raw_material_assignments_apparatus_not_blank CHECK ((btrim(apparatus) <> ''::text)),
    CONSTRAINT mini_raw_material_assignments_barcode_not_blank CHECK ((btrim(barcode) <> ''::text)),
    CONSTRAINT mini_raw_material_assignments_item_code_not_blank CHECK ((btrim(item_code) <> ''::text)),
    CONSTRAINT mini_raw_material_assignments_item_group_not_blank CHECK ((btrim(item_group) <> ''::text)),
    CONSTRAINT mini_raw_material_assignments_order_not_blank CHECK ((btrim(order_id) <> ''::text))
);


--
-- Name: mini_raw_material_stock; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_raw_material_stock (
    id text NOT NULL,
    warehouse text NOT NULL,
    item_code text NOT NULL,
    item_name text DEFAULT ''::text NOT NULL,
    barcode text NOT NULL,
    qty double precision NOT NULL,
    uom text DEFAULT 'kg'::text NOT NULL,
    status text DEFAULT 'available'::text NOT NULL,
    reserved_order_id text DEFAULT ''::text NOT NULL,
    source_receipt_id text DEFAULT ''::text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_raw_material_stock_barcode_not_blank CHECK ((btrim(barcode) <> ''::text)),
    CONSTRAINT mini_raw_material_stock_item_code_not_blank CHECK ((btrim(item_code) <> ''::text)),
    CONSTRAINT mini_raw_material_stock_qty_positive CHECK ((qty > (0)::double precision)),
    CONSTRAINT mini_raw_material_stock_status_allowed CHECK ((status = ANY (ARRAY['available'::text, 'reserved'::text, 'in_use'::text, 'consumed'::text]))),
    CONSTRAINT mini_raw_material_stock_warehouse_not_blank CHECK ((btrim(warehouse) <> ''::text))
);


--
-- Name: mini_rps_batches; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_rps_batches (
    owner_key text NOT NULL,
    batch_id text NOT NULL,
    active boolean DEFAULT false NOT NULL,
    owner_role text NOT NULL,
    owner_ref text NOT NULL,
    item_code text DEFAULT ''::text NOT NULL,
    warehouse text DEFAULT ''::text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_rps_batches_batch_not_blank CHECK ((btrim(batch_id) <> ''::text)),
    CONSTRAINT mini_rps_batches_owner_not_blank CHECK ((btrim(owner_key) <> ''::text)),
    CONSTRAINT mini_rps_batches_owner_ref_not_blank CHECK ((btrim(owner_ref) <> ''::text)),
    CONSTRAINT mini_rps_batches_owner_role_not_blank CHECK ((btrim(owner_role) <> ''::text))
);


--
-- Name: mini_warehouse_assignments; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_warehouse_assignments (
    warehouse text NOT NULL,
    principal_role text NOT NULL,
    principal_ref text NOT NULL,
    display_name text DEFAULT ''::text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_warehouse_assignments_ref_not_blank CHECK ((btrim(principal_ref) <> ''::text)),
    CONSTRAINT mini_warehouse_assignments_role_not_blank CHECK ((btrim(principal_role) <> ''::text)),
    CONSTRAINT mini_warehouse_assignments_warehouse_not_blank CHECK ((btrim(warehouse) <> ''::text))
);


--
-- Name: mini_warehouses; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_warehouses (
    id text NOT NULL,
    name text NOT NULL,
    company text DEFAULT ''::text NOT NULL,
    is_group boolean DEFAULT false NOT NULL,
    parent_warehouse text DEFAULT ''::text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT mini_warehouses_name_not_blank CHECK ((btrim(name) <> ''::text))
);


--
-- Name: mini_worker_groups; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_worker_groups (
    apparatus text NOT NULL,
    group_code text NOT NULL,
    shift text NOT NULL,
    worker_ids jsonb DEFAULT '[]'::jsonb NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    start_time text DEFAULT '08:00'::text NOT NULL,
    end_time text DEFAULT '20:00'::text NOT NULL,
    work_days_per_week integer DEFAULT 6 NOT NULL,
    start_day text DEFAULT 'monday'::text NOT NULL,
    accounting_enabled boolean DEFAULT false NOT NULL,
    CONSTRAINT mini_worker_groups_apparatus_not_blank CHECK ((btrim(apparatus) <> ''::text)),
    CONSTRAINT mini_worker_groups_worker_ids_array CHECK ((jsonb_typeof(worker_ids) = 'array'::text))
);


--
-- Name: mini_workers; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.mini_workers (
    id text NOT NULL,
    name text NOT NULL,
    level text NOT NULL,
    payload_json jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    phone text DEFAULT ''::text NOT NULL,
    CONSTRAINT mini_workers_level_allowed CHECK ((level = ANY (ARRAY['Brigader'::text, 'Master'::text, '1 - darajali'::text, '2 - darajali'::text, '3 - darajali'::text]))),
    CONSTRAINT mini_workers_name_not_blank CHECK ((btrim(name) <> ''::text))
);


--
-- Name: mini_engine_events id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_engine_events ALTER COLUMN id SET DEFAULT nextval('public.mini_engine_events_id_seq'::regclass);


--
-- Name: mini_order_progress_events id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_order_progress_events ALTER COLUMN id SET DEFAULT nextval('public.mini_order_progress_events_id_seq'::regclass);


--
-- Name: mini_queue_action_events id; Type: DEFAULT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_queue_action_events ALTER COLUMN id SET DEFAULT nextval('public.mini_queue_action_events_id_seq'::regclass);


--
-- Data for Name: mini_apparatus; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_apparatus (id, group_id, name, base_name, kind, payload_json, created_at, updated_at) FROM stdin;
apparatus:rezka	\N	Rezka			{"warehouse": "Rezka"}	2026-06-15 17:03:00.818439+05	2026-06-15 17:03:00.818439+05
apparatus:extruder laminatsiya	\N	Extruder laminatsiya			{"warehouse": "Extruder laminatsiya"}	2026-06-15 17:03:43.421878+05	2026-06-15 17:03:43.421878+05
apparatus:flexo pechat	\N	Flexo pechat			{"warehouse": "Flexo pechat"}	2026-06-15 17:03:53.768766+05	2026-06-15 17:03:53.768766+05
apparatus:holodniy kley aparat	\N	Holodniy kley aparat			{"warehouse": "Holodniy kley aparat"}	2026-06-15 17:04:07.73667+05	2026-06-15 17:04:07.73667+05
apparatus:paket aparat	\N	Paket aparat			{"warehouse": "Paket aparat"}	2026-06-15 17:04:16.049922+05	2026-06-15 17:04:16.049922+05
\.


--
-- Data for Name: mini_apparatus_groups; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_apparatus_groups (id, name, payload_json, created_at, updated_at) FROM stdin;
apparatus_group:laminatsiya	Laminatsiya	{"name": "Laminatsiya", "apparatus": ["Laminatsiya 1", "Laminatsiya 2"]}	2026-06-15 09:12:13.84928+05	2026-06-15 09:12:13.84928+05
apparatus_group:rezka	Rezka	{"name": "Rezka", "apparatus": ["Rezka"]}	2026-06-15 17:05:19.897441+05	2026-06-15 17:05:19.897441+05
apparatus_group:flexo pechat	Flexo pechat	{"name": "Flexo pechat", "apparatus": ["Flexo pechat"]}	2026-06-15 17:05:50.820723+05	2026-06-15 17:05:50.820723+05
apparatus_group:holodniy kley aparat	Holodniy kley aparat	{"name": "Holodniy kley aparat", "apparatus": ["Holodniy kley aparat"]}	2026-06-15 17:06:13.027406+05	2026-06-15 17:06:13.027406+05
apparatus_group:bosma aparat	Bosma aparat	{"name": "Bosma aparat", "apparatus": ["7 ta rangli bosma aparat", "8 ta rangli bosma aparat", "9 ta rangli bosma aparat"]}	2026-06-15 09:18:37.211256+05	2026-07-06 13:29:41.175681+05
\.


--
-- Data for Name: mini_apparatus_material_rules; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_apparatus_material_rules (apparatus, item_groups, payload_json, updated_at, requires_material, requirement_groups) FROM stdin;
Laminatsiya 1	["laminatka"]	{"apparatus": "Laminatsiya 1", "item_groups": ["laminatka"], "requires_material": true}	2026-06-18 14:51:31.439023+05	t	[]
7 ta rangli pechat	["kraska", "rulon"]	{"apparatus": "7 ta rangli pechat", "item_groups": ["kraska", "rulon"], "requires_material": true}	2026-06-23 08:52:03.089591+05	t	[]
8 ta rangli pechat	["kraska", "rulon"]	{"apparatus": "8 ta rangli pechat", "item_groups": ["kraska", "rulon"], "requires_material": true}	2026-06-23 08:52:17.948795+05	t	[]
9 ta rangli pechat	["kraska", "rulon"]	{"apparatus": "9 ta rangli pechat", "item_groups": ["kraska", "rulon"], "requires_material": true}	2026-06-23 08:52:30.233153+05	t	[]
\.


--
-- Data for Name: mini_apparatus_queue_policies; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_apparatus_queue_policies (apparatus, policy, actor_role, actor_ref, actor_display_name, payload_json, updated_at) FROM stdin;
Flexo pechat	strict_sequence	admin	admin	Admin	{"actor": {"ref_": "admin", "role": "admin", "display_name": "Admin"}, "policy": "strict_sequence"}	2026-06-17 20:18:25.808093+05
Extruder laminatsiya	strict_sequence	admin	admin	Admin	{"actor": {"ref_": "admin", "role": "admin", "display_name": "Admin"}, "policy": "strict_sequence"}	2026-06-23 08:51:11.111302+05
\.


--
-- Data for Name: mini_daily_apparatus_sequences; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_daily_apparatus_sequences (work_date, apparatus, order_ids, updated_at) FROM stdin;
\.


--
-- Data for Name: mini_daily_work_sequences; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_daily_work_sequences (work_date, order_ids, updated_at) FROM stdin;
\.


--
-- Data for Name: mini_engine_events; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_engine_events (id, event_id, domain, action, entity_id, actor_key, idempotency_key, payload_json, created_at) FROM stdin;
\.


--
-- Data for Name: mini_finished_goods_stock; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_finished_goods_stock (id, warehouse, order_id, item_code, item_name, qty, uom, status, payload_json, created_at, updated_at) FROM stdin;
\.


--
-- Data for Name: mini_gscale_receipts; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_gscale_receipts (name, status, item_code, warehouse, qty, uom, barcode, payload_json, created_at, updated_at, submitted_at) FROM stdin;
\.


--
-- Data for Name: mini_idempotency_keys; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_idempotency_keys (key, domain, action, entity_id, response_json, created_at, completed_at) FROM stdin;
\.


--
-- Data for Name: mini_item_groups; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_item_groups (name, parent_item_group, is_group, payload_json, created_at, updated_at) FROM stdin;
rulon	homashyo	t	{}	2026-06-19 14:45:57.841507+05	2026-06-19 14:45:57.841507+05
Kley	homashyo	t	{"name": "Kley", "is_group": true, "item_group_name": "Kley", "parent_item_group": "homashyo"}	2026-06-17 17:57:38.586707+05	2026-06-17 17:57:38.586707+05
laminatka	homashyo	t	{"name": "laminatka", "is_group": true, "item_group_name": "laminatka", "parent_item_group": "homashyo"}	2026-06-18 14:50:07.113186+05	2026-06-18 14:50:07.113186+05
Pechat	All Item Groups	f	{"name": "Pechat", "is_group": false, "item_group_name": "Pechat", "parent_item_group": "All Item Groups"}	2026-06-23 09:04:47.092986+05	2026-06-23 09:04:47.092986+05
All Item Groups		t	{"name": "All Item Groups", "is_group": true, "item_group_name": "All Item Groups", "parent_item_group": ""}	2026-06-17 09:15:12.103503+05	2026-06-25 17:48:55.430603+05
homashyo	All Item Groups	t	{"name": "homashyo", "is_group": true, "item_group_name": "homashyo", "parent_item_group": "All Item Groups"}	2026-06-17 09:15:12.103503+05	2026-06-25 17:48:55.43596+05
kraska	homashyo	t	{"name": "kraska", "is_group": true, "item_group_name": "kraska", "parent_item_group": "homashyo"}	2026-06-17 09:15:12.103503+05	2026-06-25 17:48:55.436526+05
tayyor mahsulot	All Item Groups	t	{"name": "tayyor mahsulot", "is_group": true, "item_group_name": "tayyor mahsulot", "parent_item_group": "All Item Groups"}	2026-06-17 09:15:12.103503+05	2026-06-25 17:48:55.436783+05
\.


--
-- Data for Name: mini_items; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_items (code, name, uom, warehouse, item_group, payload_json, created_at, updated_at) FROM stdin;
Bagetto rozviy	Bagetto rozviy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bagetto rozviy", "name": "Bagetto rozviy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.4948+05
Bismak	Bismak	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bismak", "name": "Bismak", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.504531+05
Bonitto sergili 740mm paket	Bonitto sergili 740mm paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bonitto sergili 740mm paket", "name": "Bonitto sergili 740mm paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.508622+05
Cho’p teshikli 7.5-8sm	Cho’p teshikli 7.5-8sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Cho’p teshikli 7.5-8sm", "name": "Cho’p teshikli 7.5-8sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.519566+05
Kif-kif Keks yangi	Kif-kif Keks yangi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kif-kif Keks yangi", "name": "Kif-kif Keks yangi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.550422+05
Milano Shokolad	Milano Shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Milano Shokolad", "name": "Milano Shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.579668+05
Oltin biday uni paket	Oltin biday uni paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Oltin biday uni paket", "name": "Oltin biday uni paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.592109+05
Pitos trubichka kolbaska	Pitos trubichka kolbaska	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pitos trubichka kolbaska", "name": "Pitos trubichka kolbaska", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.602341+05
RE 630/80	RE 630/80	Kg		tayyor mahsulot	{"uom": "Kg", "code": "RE 630/80", "name": "RE 630/80", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.609858+05
Silver asartiy paket 50gr	Silver asartiy paket 50gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Silver asartiy paket 50gr", "name": "Silver asartiy paket 50gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.618956+05
Sladkaya aladin paket	Sladkaya aladin paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sladkaya aladin paket", "name": "Sladkaya aladin paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.623368+05
Toretto Turbo	Toretto Turbo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto Turbo", "name": "Toretto Turbo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.633658+05
Xrustik plus kotta 80gr paket	Xrustik plus kotta 80gr paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustik plus kotta 80gr paket", "name": "Xrustik plus kotta 80gr paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.649867+05
day sakfetka 120 sht qizil kok	Day Sakfetka 120 Sht Qizil Kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "day sakfetka 120 sht qizil kok", "name": "Day Sakfetka 120 Sht Qizil Kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.706696+05
dilmuratov sinkers 100 gr	Dilmuratov Sinkers 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dilmuratov sinkers 100 gr", "name": "Dilmuratov Sinkers 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.711988+05
doni sendvich nukus paket	Doni Sendvich Nukus Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "doni sendvich nukus paket", "name": "Doni Sendvich Nukus Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.716636+05
ezo parashok 3ka paket	Ezo Parashok 3ka Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ezo parashok 3ka paket", "name": "Ezo Parashok 3ka Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.726504+05
frustick arbuz dinya	Frustick Arbuz Dinya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "frustick arbuz dinya", "name": "Frustick Arbuz Dinya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.731534+05
iz korzinka sendvich kofe 100 gr	Iz Korzinka Sendvich Kofe 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "iz korzinka sendvich kofe 100 gr", "name": "Iz Korzinka Sendvich Kofe 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.75431+05
kendiy asartiy paket	Kendiy Asartiy Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kendiy asartiy paket", "name": "Kendiy Asartiy Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.776253+05
kristal tuz rossiya 2,5 kg paket	Kristal Tuz Rossiya 2,5 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kristal tuz rossiya 2,5 kg paket", "name": "Kristal Tuz Rossiya 2,5 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.785313+05
mavis suﬂe asarty 28 gr	Mavis Suﬂe Asarty 28 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mavis suﬂe asarty 28 gr", "name": "Mavis Suﬂe Asarty 28 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.813724+05
musaﬀo sirok qora qarag'atli	Musaﬀo Sirok Qora Qarag'atli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musaﬀo sirok qora qarag'atli", "name": "Musaﬀo Sirok Qora Qarag'atli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.843593+05
prayniki 600gr plambir	Prayniki 600gr Plambir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prayniki 600gr plambir", "name": "Prayniki 600gr Plambir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.957861+05
stiks suxariki sir 20gr	Stiks Suxariki Sir 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "stiks suxariki sir 20gr", "name": "Stiks Suxariki Sir 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.009889+05
venskiye vaﬂi chempion paket	Venskiye Vaﬂi Chempion Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "venskiye vaﬂi chempion paket", "name": "Venskiye Vaﬂi Chempion Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.034822+05
1190/30 pe pr toza	PE PR 1190/30	Kg		homashyo	{"uom": "Kg", "code": "1190/30 pe pr toza", "name": "PE PR 1190/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.445013+05
475/60 pe oq	PE OQ 475/60	Kg		homashyo	{"uom": "Kg", "code": "475/60 pe oq", "name": "PE OQ 475/60", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.446131+05
ABCD Family	ABCD Family	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ABCD Family", "name": "ABCD Family", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.471279+05
Abrazesla	Abrazesla	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Abrazesla", "name": "Abrazesla", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.471637+05
BaBom molochniy morojenniy	BaBom molochniy morojenniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "BaBom molochniy morojenniy", "name": "BaBom molochniy morojenniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.493795+05
610/45 pe pr toza	PE PR 610/45	Kg		homashyo	{"uom": "Kg", "code": "610/45 pe pr toza", "name": "PE PR 610/45", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.454646+05
Aladin semechka kok	Aladin semechka kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Aladin semechka kok", "name": "Aladin semechka kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.478457+05
Aladin semechka qolipi	Aladin semechka qolipi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Aladin semechka qolipi", "name": "Aladin semechka qolipi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.478795+05
Agra bravo olcha	Agra bravo olcha	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Agra bravo olcha", "name": "Agra bravo olcha", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.47475+05
Ali 7D	Ali 7D	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ali 7D", "name": "Ali 7D", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.479224+05
Ali bobo Frutti	Ali bobo Frutti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ali bobo Frutti", "name": "Ali bobo Frutti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.479577+05
Ali chips	Ali chips	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ali chips", "name": "Ali chips", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.480135+05
Alibobo marojniy 80gr	Alibobo marojniy 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Alibobo marojniy 80gr", "name": "Alibobo marojniy 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.480439+05
Allora pasta paket	Allora pasta paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Allora pasta paket", "name": "Allora pasta paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.480727+05
Alotem 12sht	Alotem 12sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Alotem 12sht", "name": "Alotem 12sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.482277+05
Amir boss shef 65 gr	Amir boss shef 65 gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Amir boss shef 65 gr", "name": "Amir boss shef 65 gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.482547+05
And gold tekistil	And gold tekistil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "And gold tekistil", "name": "And gold tekistil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.482798+05
Antiqa	Antiqa	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Antiqa", "name": "Antiqa", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.483093+05
Apatito sasiska	Apatito sasiska	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Apatito sasiska", "name": "Apatito sasiska", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.483386+05
Arbuz	Arbuz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Arbuz", "name": "Arbuz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.484435+05
Aroma parashok 250gr	Aroma parashok 250gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Aroma parashok 250gr", "name": "Aroma parashok 250gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.485228+05
Aroma tea 1L	Aroma tea 1L	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Aroma tea 1L", "name": "Aroma tea 1L", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.486017+05
Asil Fruiti 4sm	Asil Fruiti 4sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Asil Fruiti 4sm", "name": "Asil Fruiti 4sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.486534+05
Asil shirin kukruz	Asil shirin kukruz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Asil shirin kukruz", "name": "Asil shirin kukruz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.487551+05
Askarbinka vitemin S	Askarbinka vitemin S	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Askarbinka vitemin S", "name": "Askarbinka vitemin S", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.490705+05
Avalon	Avalon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Avalon", "name": "Avalon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.491844+05
Axmad Olmos Jo’ja	Axmad Olmos Jo’ja	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Axmad Olmos Jo’ja", "name": "Axmad Olmos Jo’ja", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.492384+05
A’lo Ta’m Kanada	A’lo Ta’m Kanada	Kg		tayyor mahsulot	{"uom": "Kg", "code": "A’lo Ta’m Kanada", "name": "A’lo Ta’m Kanada", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.492944+05
Banana	Banana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Banana", "name": "Banana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.49561+05
Bravo pista 5kg paket	Bravo pista 5kg paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bravo pista 5kg paket", "name": "Bravo pista 5kg paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.51119+05
Chevrolet	Chevrolet	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Chevrolet", "name": "Chevrolet", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.513274+05
Jem  545/25	JEM 545/25	Kg		homashyo	{"uom": "Kg", "code": "Jem  545/25", "name": "JEM 545/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.536822+05
Jem 675/20	JEM 675/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 675/20", "name": "JEM 675/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.54401+05
Jobir shashlik	Jobir shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Jobir shashlik", "name": "Jobir shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.546471+05
575/80 pe pr toza	PE PR 575/80	Kg		homashyo	{"uom": "Kg", "code": "575/80 pe pr toza", "name": "PE PR 575/80", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.452651+05
Al safo hoddog kotta	Al safo hoddog kotta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Al safo hoddog kotta", "name": "Al safo hoddog kotta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.477791+05
Aladin semechka	Aladin semechka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Aladin semechka", "name": "Aladin semechka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.478123+05
Musqaymoq-365kun lyod 80g	Musqaymoq-365kun lyod 80g	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Musqaymoq-365kun lyod 80g", "name": "Musqaymoq-365kun lyod 80g", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.584122+05
480/70 pe oq	PE OQ 480/70	Kg		homashyo	{"uom": "Kg", "code": "480/70 pe oq", "name": "PE OQ 480/70", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.446943+05
Aiva tarvuz-qovun-shaftoli-multifrut	Aiva tarvuz-qovun-shaftoli-multifrut	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Aiva tarvuz-qovun-shaftoli-multifrut", "name": "Aiva tarvuz-qovun-shaftoli-multifrut", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.475869+05
Al safo hod dog kichik	Al safo hoddog kichik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Al safo hod dog kichik", "name": "Al safo hoddog kichik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.477434+05
Aroma tea 0.5	Aroma tea 0.5	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Aroma tea 0.5", "name": "Aroma tea 0.5", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.485762+05
Asal	Asal	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Asal", "name": "Asal", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.486269+05
Asil Fruiti 70gr	Asil Fruiti 70gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Asil Fruiti 70gr", "name": "Asil Fruiti 70gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.48678+05
Asil Fruiti jele dumaloq	Asil Fruiti jele dumaloq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Asil Fruiti jele dumaloq", "name": "Asil Fruiti jele dumaloq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.487045+05
Asil samarqand qurt paket	Asil samarqand qurt paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Asil samarqand qurt paket", "name": "Asil samarqand qurt paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.487305+05
Askarbinka masha	Askarbinka masha	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Askarbinka masha", "name": "Askarbinka masha", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.488934+05
Askarbinka minyons	Askarbinka minyons	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Askarbinka minyons", "name": "Askarbinka minyons", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.489265+05
Askarbinka red	Askarbinka red	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Askarbinka red", "name": "Askarbinka red", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.489547+05
Askarbinka vitamis s kichik	Askarbinka vitamis s kichik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Askarbinka vitamis s kichik", "name": "Askarbinka vitamis s kichik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.49043+05
Asl Sifat Hot Dog	Asl Sifat Hot Dog	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Asl Sifat Hot Dog", "name": "Asl Sifat Hot Dog", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.49101+05
Atabekov sasiska	Atabekov sasiska	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Atabekov sasiska", "name": "Atabekov sasiska", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.491563+05
Avalon Paket	Avalon Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Avalon Paket", "name": "Avalon Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.492117+05
Ayron	Ayron	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ayron", "name": "Ayron", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.49268+05
BRAVO PAYuShIY 26,8 SM	BRAVO PAYuShIY 26,8 SM	Kg		tayyor mahsulot	{"uom": "Kg", "code": "BRAVO PAYuShIY 26,8 SM", "name": "BRAVO PAYuShIY 26,8 SM", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.493235+05
Barakali chuchvala 300g paket	Barakali chuchvala 300g paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Barakali chuchvala 300g paket", "name": "Barakali chuchvala 300g paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.496345+05
Barbaris	Barbaris	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Barbaris", "name": "Barbaris", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.496946+05
Barbol vaﬂiy	Barbol vaﬂiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Barbol vaﬂiy", "name": "Barbol vaﬂiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.497213+05
Bebikar gulli	Bebikar gulli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bebikar gulli", "name": "Bebikar gulli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.498351+05
Bek qora 160gr	Bek qora 160gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bek qora 160gr", "name": "Bek qora 160gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.499228+05
Bek qora 30gr	Bek qora 30gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bek qora 30gr", "name": "Bek qora 30gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.499943+05
Bek qora 80gr	Bek qora 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bek qora 80gr", "name": "Bek qora 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.500269+05
Bella classic	Bella classic	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bella classic", "name": "Bella classic", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.501248+05
Bibo paket	Bibo paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bibo paket", "name": "Bibo paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.503519+05
Choco Granule	Choco Granule	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Choco Granule", "name": "Choco Granule", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.51468+05
Jem 485/20	JEM 485/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 485/20", "name": "JEM 485/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.541345+05
Maksi	Maksi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Maksi", "name": "Maksi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.571893+05
Xrumstik Paket Kichkina	Xrumstik Paket Kichkina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrumstik Paket Kichkina", "name": "Xrumstik Paket Kichkina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.645324+05
740/45 pe pr toza	PE PR 740/45	Kg		homashyo	{"uom": "Kg", "code": "740/45 pe pr toza", "name": "PE PR 740/45", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.462665+05
Akis mega 2-3 kg paket	Akis mega 2-3 kg paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Akis mega 2-3 kg paket", "name": "Akis mega 2-3 kg paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.476135+05
Akis mega 3-5 kg paket	Akis mega 3-5 kg paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Akis mega 3-5 kg paket", "name": "Akis mega 3-5 kg paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.476405+05
opp 495/18	OPP 495/18	Kg		homashyo	{"uom": "Kg", "code": "opp 495/18", "name": "OPP 495/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.87119+05
795/60 pe pr oq	PE PR 795/60	Kg		homashyo	{"uom": "Kg", "code": "795/60 pe pr oq", "name": "PE PR 795/60", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.465959+05
Bagetto yashil	Bagetto yashil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bagetto yashil", "name": "Bagetto yashil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.495058+05
Bal bala paket	Bal bala paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bal bala paket", "name": "Bal bala paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.495342+05
Baxtli oila salfetka	Baxtli oila salfetka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Baxtli oila salfetka", "name": "Baxtli oila salfetka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.498052+05
Bek oq 100gr	Bek oq 100gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bek oq 100gr", "name": "Bek oq 100gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.498896+05
laminatka test	laminatka test	Kg		laminatka	{"uom": "Kg", "code": "laminatka test", "name": "laminatka test", "warehouse": "", "item_group": "laminatka"}	2026-06-18 14:50:52.928205+05	2026-06-18 14:50:52.928205+05
Bek qora 25gr	Bek qora 25gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bek qora 25gr", "name": "Bek qora 25gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.499604+05
Bek xan choy zip paket	Bek xan choy zip paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bek xan choy zip paket", "name": "Bek xan choy zip paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.500606+05
Bekluks aboy paket	Bekluks aboy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bekluks aboy paket", "name": "Bekluks aboy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.500961+05
Beneo jitkiy aboy paket	Beneo jitkiy aboy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Beneo jitkiy aboy paket", "name": "Beneo jitkiy aboy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.501536+05
Bextarin 5kg paket	Bextarin 5kg paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bextarin 5kg paket", "name": "Bextarin 5kg paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.503239+05
Big max 75gr	Big max 75gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Big max 75gr", "name": "Big max 75gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.503773+05
Biggo preprava	Biggo preprava	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Biggo preprava", "name": "Biggo preprava", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.50403+05
Bisrvit asartiy	Bisrvit asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bisrvit asartiy", "name": "Bisrvit asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.504792+05
Bomba marojni	Bomba marojni	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bomba marojni", "name": "Bomba marojni", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.50609+05
Bonitto 990mm paket	Bonitto 990mm paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bonitto 990mm paket", "name": "Bonitto 990mm paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.507868+05
Bontiy	Bontiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bontiy", "name": "Bontiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.508874+05
Braslet masha medved	Braslet masha medved	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Braslet masha medved", "name": "Braslet masha medved", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.509227+05
Bravo keks asartiy	Bravo keks asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bravo keks asartiy", "name": "Bravo keks asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.510489+05
Bravo paket	Bravo paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bravo paket", "name": "Bravo paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.510814+05
Bravo pista 2+3	Bravo pista 2+3	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bravo pista 2+3", "name": "Bravo pista 2+3", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.511027+05
Briland paket	Briland paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Briland paket", "name": "Briland paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.511364+05
Bruno	Bruno	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bruno", "name": "Bruno", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.511525+05
Buba paket	Buba paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Buba paket", "name": "Buba paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.511696+05
Buyuk pista paket	Buyuk pista paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Buyuk pista paket", "name": "Buyuk pista paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.512041+05
CPP / 20 mikron / Navruz prazrachniy	CPP 20 Navruz prazrachniy	Kg		homashyo	{"uom": "Kg", "code": "CPP / 20 mikron / Navruz prazrachniy", "name": "CPP 20 Navruz prazrachniy", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.512704+05
Charli	Charli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Charli", "name": "Charli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.512998+05
Chempions league shashlik	Chempions league shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Chempions league shashlik", "name": "Chempions league shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.513137+05
Jem 620/20	JEM 620/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 620/20", "name": "JEM 620/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.543394+05
Osiyo sasiska karalevskiy	Osiyo sasiska karalevskiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Osiyo sasiska karalevskiy", "name": "Osiyo sasiska karalevskiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.593278+05
fayz m sulton paket	Fayz M Sulton Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fayz m sulton paket", "name": "Fayz M Sulton Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.726771+05
880/65 pe pr toza	PE PR 880/65	Kg		homashyo	{"uom": "Kg", "code": "880/65 pe pr toza", "name": "PE PR 880/65", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.469342+05
Banoﬀy	Banoﬀy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Banoﬀy", "name": "Banoﬀy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.495843+05
945/80 pe pr oq	PE PR 945/80	Kg		homashyo	{"uom": "Kg", "code": "945/80 pe pr oq", "name": "PE PR 945/80", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.469914+05
opp 665/18 pol	OPP 665/18	Kg		homashyo	{"uom": "Kg", "code": "opp 665/18 pol", "name": "OPP 665/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.887956+05
Bonitto 540mm	Bonitto 540mm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bonitto 540mm", "name": "Bonitto 540mm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.506858+05
Bonitto 840mm	Bonitto 840mm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bonitto 840mm", "name": "Bonitto 840mm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.50736+05
Bonitto 930mm paket	Bonitto 930mm paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bonitto 930mm paket", "name": "Bonitto 930mm paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.507622+05
Bonitto keles zip paket	Bonitto keles zip paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bonitto keles zip paket", "name": "Bonitto keles zip paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.508125+05
Bonitto kichik	Bonitto kichik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bonitto kichik", "name": "Bonitto kichik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.508376+05
Braun pista 135gr	Braun pista 135gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Braun pista 135gr", "name": "Braun pista 135gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.50954+05
Bravo kalbaski	Bravo kalbaski	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bravo kalbaski", "name": "Bravo kalbaski", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.509813+05
Bravo keks	Bravo keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bravo keks", "name": "Bravo keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.510081+05
Bumerang	Bumerang	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bumerang", "name": "Bumerang", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.511863+05
Chao	Chao	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Chao", "name": "Chao", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.512851+05
Chikko kòk paket	Chikko kòk paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Chikko kòk paket", "name": "Chikko kòk paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.513553+05
Chikko qizil rulon	Chikko qizil rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Chikko qizil rulon", "name": "Chikko qizil rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.513988+05
Chikko salyami	Chikko salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Chikko salyami", "name": "Chikko salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.514131+05
Chinor aboy paket	Chinor aboy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Chinor aboy paket", "name": "Chinor aboy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.514265+05
Choco Air	Choco Air	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Choco Air", "name": "Choco Air", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.514541+05
Choco Mix paket	Choco Mix paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Choco Mix paket", "name": "Choco Mix paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.514818+05
Choko rols malina	Choko rols malina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Choko rols malina", "name": "Choko rols malina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.51546+05
Cho’p 20sm	Cho’p 20sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Cho’p 20sm", "name": "Cho’p 20sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.518981+05
Cho’p 8 sm	Cho’p 8 sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Cho’p 8 sm", "name": "Cho’p 8 sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.519421+05
Chup 8 sm teshili Dey	Chup 8 sm teshili Dey	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Chup 8 sm teshili Dey", "name": "Chup 8 sm teshili Dey", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.519845+05
Crown	Crown	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Crown", "name": "Crown", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.519969+05
Delice	Delice	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Delice", "name": "Delice", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.520102+05
Delis	Delis	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Delis", "name": "Delis", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.520246+05
Diet 24 salfetkala	Diet 24 salfetkala	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Diet 24 salfetkala", "name": "Diet 24 salfetkala", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.52038+05
Dilﬁn parasho usluga	Dilﬁn parasho usluga	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Dilﬁn parasho usluga", "name": "Dilﬁn parasho usluga", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.52052+05
Dinya	Dinya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Dinya", "name": "Dinya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.520656+05
Diona	Diona	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Diona", "name": "Diona", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.520799+05
Diyorbek marojniy	Diyorbek marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Diyorbek marojniy", "name": "Diyorbek marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.520947+05
Dizayin dekor yashil	Dizayin dekor yashil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Dizayin dekor yashil", "name": "Dizayin dekor yashil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.521233+05
Dizzy	Dizzy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Dizzy", "name": "Dizzy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.521364+05
atlantis bountli energy 60 gr	Atlantis Bountli Energy 60 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "atlantis bountli energy 60 gr", "name": "Atlantis Bountli Energy 60 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.671686+05
pet 580/12 kar	PET 580/12	Kg		homashyo	{"uom": "Kg", "code": "pet 580/12 kar", "name": "PET 580/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.943025+05
Adras aboy 4kg paket	Adras aboy 4kg paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Adras aboy 4kg paket", "name": "Adras aboy 4kg paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.473191+05
Bolajon kukruz	Bolajon kukruz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bolajon kukruz", "name": "Bolajon kukruz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.505815+05
Cho’p 12sm	Cho’p 12sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Cho’p 12sm", "name": "Cho’p 12sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.518653+05
Choko rols moloko	Choko rols moloko	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Choko rols moloko", "name": "Choko rols moloko", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.515591+05
Cho’p 19 sm	Cho’p 19 sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Cho’p 19 sm", "name": "Cho’p 19 sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.518837+05
Cho’p 7.5sm	Cho’p 7.5sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Cho’p 7.5sm", "name": "Cho’p 7.5sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.519254+05
Dolce konfet	Dolce konfet	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Dolce konfet", "name": "Dolce konfet", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.521795+05
Dona nasir food	Dona nasir food	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Dona nasir food", "name": "Dona nasir food", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.521938+05
Doni qurt	Doni qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Doni qurt", "name": "Doni qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.522099+05
Dunyo salfetka	Dunyo salfetka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Dunyo salfetka", "name": "Dunyo salfetka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.522396+05
Eco milano aboy paket	Eco milano aboy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Eco milano aboy paket", "name": "Eco milano aboy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.522534+05
Efendim	Efendim	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Efendim", "name": "Efendim", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.522683+05
Elegant aboy paket	Elegant aboy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Elegant aboy paket", "name": "Elegant aboy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.523689+05
Eskimo fayz	Eskimo fayz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Eskimo fayz", "name": "Eskimo fayz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.5245+05
Evro dekor aboy paket FYALETVIY	Evro dekor aboy paket FYALETVIY	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Evro dekor aboy paket FYALETVIY", "name": "Evro dekor aboy paket FYALETVIY", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.524786+05
F1 king semechka 2-3 kg	F1 king semechka 2-3 kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "F1 king semechka 2-3 kg", "name": "F1 king semechka 2-3 kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.52506+05
Facho	Facho	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Facho", "name": "Facho", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.525181+05
Fan keks	Fan keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Fan keks", "name": "Fan keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.525323+05
Fanat 3D salyami	Fanat 3D salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Fanat 3D salyami", "name": "Fanat 3D salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.525448+05
Fanat 3D shashlik	Fanat 3D shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Fanat 3D shashlik", "name": "Fanat 3D shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.525578+05
Farina suxoy malako	Farina suxoy malako	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Farina suxoy malako", "name": "Farina suxoy malako", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.525839+05
Fayz keks	Fayz keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Fayz keks", "name": "Fayz keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.52597+05
Fayz m tekistil skoch paket	Fayz m tekistil skoch paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Fayz m tekistil skoch paket", "name": "Fayz m tekistil skoch paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.526128+05
Fayz millennum	Fayz millennum	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Fayz millennum", "name": "Fayz millennum", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.526266+05
Fayz super rajok	Fayz super rajok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Fayz super rajok", "name": "Fayz super rajok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.526412+05
Fayz tvist marojniy	Fayz tvist marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Fayz tvist marojniy", "name": "Fayz tvist marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.526552+05
Fayz-m mayka paket	Fayz-m mayka paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Fayz-m mayka paket", "name": "Fayz-m mayka paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.526714+05
Fayz-m shortik paket	Fayz-m shortik paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Fayz-m shortik paket", "name": "Fayz-m shortik paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.52701+05
Finchuza paket 300gr	Finchuza paket 300gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Finchuza paket 300gr", "name": "Finchuza paket 300gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.527177+05
Finchuza paket 400gr	Finchuza paket 400gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Finchuza paket 400gr", "name": "Finchuza paket 400gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.527339+05
Finler Vaﬂi	Finler Vaﬂi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Finler Vaﬂi", "name": "Finler Vaﬂi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.527535+05
For baby salfetka 72ta	For baby salfetka 72ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "For baby salfetka 72ta", "name": "For baby salfetka 72ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.527708+05
divino xolodnaya payka	Divino Xolodnaya Payka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "divino xolodnaya payka", "name": "Divino Xolodnaya Payka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.712542+05
Aisha twist	Aisha twist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Aisha twist", "name": "Aisha twist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.475273+05
Choko bom	Choko bom	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Choko bom", "name": "Choko bom", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.514976+05
Choko rols yagodi	Choko rols yagodi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Choko rols yagodi", "name": "Choko rols yagodi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.515726+05
Else 250gr	Else 250gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Else 250gr", "name": "Else 250gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.524229+05
Elma sochiq qizil	Elma sochiq qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Elma sochiq qizil", "name": "Elma sochiq qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.523958+05
Emilya	Emilya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Emilya", "name": "Emilya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.52437+05
Evos salat	Evos salat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Evos salat", "name": "Evos salat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.524636+05
Excellent 65g	Excellent 65g	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Excellent 65g", "name": "Excellent 65g", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.524915+05
Frendo 3D chili	Frendo 3D chili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frendo 3D chili", "name": "Frendo 3D chili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.527881+05
Frendo 3D shashlik	Frendo 3D shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frendo 3D shashlik", "name": "Frendo 3D shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.528043+05
Frendo 3D smetana	Frendo 3D smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frendo 3D smetana", "name": "Frendo 3D smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.528187+05
Frendo 5D tamat	Frendo 5D tamat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frendo 5D tamat", "name": "Frendo 5D tamat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.52835+05
Frendo 7D shashlik	Frendo 7D shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frendo 7D shashlik", "name": "Frendo 7D shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.528888+05
Frendo 7D sir	Frendo 7D sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frendo 7D sir", "name": "Frendo 7D sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.529031+05
Frendo 9D steyk	Frendo 9D steyk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frendo 9D steyk", "name": "Frendo 9D steyk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.529509+05
Frendo asarti	Frendo asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frendo asarti", "name": "Frendo asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.52967+05
Fritel	Fritel	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Fritel", "name": "Fritel", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.529829+05
Frittos	Frittos	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frittos", "name": "Frittos", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.529973+05
Frutella prazrachniy pechat	Frutella prazrachniy pechat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frutella prazrachniy pechat", "name": "Frutella prazrachniy pechat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.530617+05
Frutti 340gr max paket	Frutti 340gr max paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frutti 340gr max paket", "name": "Frutti 340gr max paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.530803+05
Ganga marojniy	Ganga marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ganga marojniy", "name": "Ganga marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.530957+05
Goldmag lapsha 1kg LVG103	Goldmag lapsha 1kg LVG103	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Goldmag lapsha 1kg LVG103", "name": "Goldmag lapsha 1kg LVG103", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.532045+05
Grand kumush salfetka	Grand kumush salfetka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Grand kumush salfetka", "name": "Grand kumush salfetka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.532854+05
Grat pista tuzli	Grat pista tuzli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Grat pista tuzli", "name": "Grat pista tuzli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.533396+05
Hagelnuts	Hagelnuts	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Hagelnuts", "name": "Hagelnuts", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.533549+05
Hasons karamel	Hasons karamel	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Hasons karamel", "name": "Hasons karamel", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.533747+05
Hero zero	Hero zero	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Hero zero", "name": "Hero zero", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.534069+05
Holli	Holli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Holli", "name": "Holli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.534233+05
Ice cream Mers 80gr	Ice cream Mers 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ice cream Mers 80gr", "name": "Ice cream Mers 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.534381+05
Imkon plaster aboy paket	Imkon plaster aboy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Imkon plaster aboy paket", "name": "Imkon plaster aboy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.53469+05
Imperator salyami	Imperator salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Imperator salyami", "name": "Imperator salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.534847+05
Jobir smetana	Jobir smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Jobir smetana", "name": "Jobir smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.546708+05
Rols line	Rols line	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rols line", "name": "Rols line", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.612451+05
hamroh semechka 80gr tuzli	Hamroh Semechka 80gr Tuzli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "hamroh semechka 80gr tuzli", "name": "Hamroh Semechka 80gr Tuzli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.743326+05
Almond ﬁstashka paket	Almond ﬁstashka paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Almond ﬁstashka paket", "name": "Almond ﬁstashka paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.481811+05
Element mix	Element mix	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Element mix", "name": "Element mix", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.523825+05
Elma sochiq yashil	Elma sochiq yashil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Elma sochiq yashil", "name": "Elma sochiq yashil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.524094+05
Goldmag 1gr LVG102	Goldmag 1gr LVG102	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Goldmag 1gr LVG102", "name": "Goldmag 1gr LVG102", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.531864+05
Govyajya	Govyajya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Govyajya", "name": "Govyajya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.532549+05
Grafkofe	Grafkofe	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Grafkofe", "name": "Grafkofe", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.532704+05
Grant pista tuzsiz	Grant pista tuzsiz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Grant pista tuzsiz", "name": "Grant pista tuzsiz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.533188+05
Imperator shashlik	Imperator shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Imperator shashlik", "name": "Imperator shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.535022+05
Imperator smetana	Imperator smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Imperator smetana", "name": "Imperator smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.535164+05
Impirt aboy 3kg paket	Impirt aboy 3kg paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Impirt aboy 3kg paket", "name": "Impirt aboy 3kg paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.535488+05
Iris Assorti	Iris Assorti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Iris Assorti", "name": "Iris Assorti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.535634+05
Iris asarti	Iris asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Iris asarti", "name": "Iris asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.535811+05
Isko kichik	Isko kichik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Isko kichik", "name": "Isko kichik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.535969+05
Izakaya	Izakaya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Izakaya", "name": "Izakaya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.536157+05
Jem 655/20	JEM 655/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 655/20", "name": "JEM 655/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.543897+05
Kafort aboy	Kafort aboy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kafort aboy", "name": "Kafort aboy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.546982+05
Kak ran’she	Kak ran’she	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kak ran’she", "name": "Kak ran’she", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.547103+05
Karamello	Karamello	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Karamello", "name": "Karamello", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.547868+05
Karofka paket 500gr	Karofka paket 500gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Karofka paket 500gr", "name": "Karofka paket 500gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.547981+05
Karvon	Karvon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Karvon", "name": "Karvon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.548214+05
Kazaxistan	Kazaxistan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kazaxistan", "name": "Kazaxistan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.548394+05
Kaﬀeino bardoviy	Kaﬀeino bardoviy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kaﬀeino bardoviy", "name": "Kaﬀeino bardoviy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.548687+05
Kaﬀeino jigarren	Kaﬀeino jigarren	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kaﬀeino jigarren", "name": "Kaﬀeino jigarren", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.548854+05
Mikki maus	Mikki maus	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mikki maus", "name": "Mikki maus", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.57932+05
OPP 945/25 pol	OPP 945/25	Kg		homashyo	{"uom": "Kg", "code": "OPP 945/25 pol", "name": "OPP 945/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.591564+05
Oltin Don Paket	Oltin Don Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Oltin Don Paket", "name": "Oltin Don Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.591887+05
Panjoy shokolat	Panjoy shokolat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Panjoy shokolat", "name": "Panjoy shokolat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.596119+05
Rara nusa paket	Rara nusa paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rara nusa paket", "name": "Rara nusa paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.610628+05
Sladkiy kichik rulon KZ	Sladkiy kichik rulon KZ	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sladkiy kichik rulon KZ", "name": "Sladkiy kichik rulon KZ", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.62347+05
hamroh semechka 80gr tuzsiz	Hamroh Semechka 80gr Tuzsiz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "hamroh semechka 80gr tuzsiz", "name": "Hamroh Semechka 80gr Tuzsiz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.743477+05
happy tvist	Happy Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "happy tvist", "name": "Happy Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.743935+05
maxi boom kukruz paket	Maxi Boom Kukruz Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "maxi boom kukruz paket", "name": "Maxi Boom Kukruz Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.814021+05
Askarbinka	Askarbinka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Askarbinka", "name": "Askarbinka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.48807+05
Askarbinka fkusnyashka	Askarbinka fkusnyashka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Askarbinka fkusnyashka", "name": "Askarbinka fkusnyashka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.488628+05
Good 999.9 kichik	Good 999.9 kichik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Good 999.9 kichik", "name": "Good 999.9 kichik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.532191+05
Gosht	Gosht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Gosht", "name": "Gosht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.532379+05
Kanadskie sasiski kuriniy	Kanadskie sasiski kuriniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kanadskie sasiski kuriniy", "name": "Kanadskie sasiski kuriniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.54758+05
Pz 505/25	Pz 505/25	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pz 505/25", "name": "Pz 505/25", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.60645+05
Kaﬀeino qizil	Kaﬀeino qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kaﬀeino qizil", "name": "Kaﬀeino qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.549023+05
Jip-Jip	Jip-Jip	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Jip-Jip", "name": "Jip-Jip", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.546234+05
Kaﬀeino qora	Kaﬀeino qora	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kaﬀeino qora", "name": "Kaﬀeino qora", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.549186+05
Kendi gold kichik mevali	Kendi gold kichik mevali	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kendi gold kichik mevali", "name": "Kendi gold kichik mevali", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.549677+05
Kendi gold kichik pamadka	Kendi gold kichik pamadka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kendi gold kichik pamadka", "name": "Kendi gold kichik pamadka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.54983+05
Kendi gold kotta mevali	Kendi gold kotta mevali	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kendi gold kotta mevali", "name": "Kendi gold kotta mevali", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.549973+05
Klassik mayka mens	Klassik mayka mens	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Klassik mayka mens", "name": "Klassik mayka mens", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.551841+05
Koker Aiva pecheniy	Koker Aiva pecheniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Koker Aiva pecheniy", "name": "Koker Aiva pecheniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.552434+05
Konfetto	Konfetto	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Konfetto", "name": "Konfetto", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.552533+05
Konus	Konus	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Konus", "name": "Konus", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.552635+05
Korofka	Korofka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Korofka", "name": "Korofka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.552732+05
Korofka kotta	Korofka kotta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Korofka kotta", "name": "Korofka kotta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.552838+05
Korovka Lili	Korovka Lili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Korovka Lili", "name": "Korovka Lili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.552941+05
Korovka Yangi	Korovka Yangi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Korovka Yangi", "name": "Korovka Yangi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.55304+05
Kosmis 3D steyk	Kosmis 3D steyk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmis 3D steyk", "name": "Kosmis 3D steyk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.553205+05
Kosmis 3D telatena	Kosmis 3D telatena	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmis 3D telatena", "name": "Kosmis 3D telatena", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.553305+05
Kosmis 5D chili	Kosmis 5D chili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmis 5D chili", "name": "Kosmis 5D chili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.553558+05
Kosmis 5D sir	Kosmis 5D sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmis 5D sir", "name": "Kosmis 5D sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.553668+05
Kosmis 7D shashlik	Kosmis 7D shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmis 7D shashlik", "name": "Kosmis 7D shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.553985+05
Kosmis 7D suluguni	Kosmis 7D suluguni	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmis 7D suluguni", "name": "Kosmis 7D suluguni", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.554122+05
Kosmos 3D smetana	Kosmos 3D smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmos 3D smetana", "name": "Kosmos 3D smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.55422+05
Kosmos 5D smetana	Kosmos 5D smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmos 5D smetana", "name": "Kosmos 5D smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.554326+05
Kosmos 5D sol	Kosmos 5D sol	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmos 5D sol", "name": "Kosmos 5D sol", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.554443+05
VANTED MSR 220/20	VANTED MSR 220/20	Kg		tayyor mahsulot	{"uom": "Kg", "code": "VANTED MSR 220/20", "name": "VANTED MSR 220/20", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.641222+05
Zizi chupa chups prazrachniy	Zizi chupa chups prazrachniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi chupa chups prazrachniy", "name": "Zizi chupa chups prazrachniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.660165+05
baxt lola paket	Baxt Lola Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "baxt lola paket", "name": "Baxt Lola Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.676869+05
chastle kids 37/30	Chastle Kids 37/30	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chastle kids 37/30", "name": "Chastle Kids 37/30", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.687869+05
hamroh semechka bez sheluxi	Hamroh Semechka Bez Sheluxi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "hamroh semechka bez sheluxi", "name": "Hamroh Semechka Bez Sheluxi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.743637+05
jem 695/25 kar	JEM 695/25	Kg		homashyo	{"uom": "Kg", "code": "jem 695/25 kar", "name": "JEM 695/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.766498+05
kreko kuritsa 50 gr	Kreko Kuritsa 50 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kreko kuritsa 50 gr", "name": "Kreko Kuritsa 50 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.781823+05
kristal tuz rossiya 1 kg paket	Kristal Tuz Rossiya 1 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kristal tuz rossiya 1 kg paket", "name": "Kristal Tuz Rossiya 1 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.784714+05
Blum kchik	Blum kchik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Blum kchik", "name": "Blum kchik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.505326+05
Jiada	Jiada	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Jiada", "name": "Jiada", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.546099+05
Jobir kolbasa	Jobir kolbasa	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Jobir kolbasa", "name": "Jobir kolbasa", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.546347+05
Klassik plombir brule	Klassik plombir brule	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Klassik plombir brule", "name": "Klassik plombir brule", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.551979+05
Kofe efendim paket	Kofe efendim paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kofe efendim paket", "name": "Kofe efendim paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.552117+05
Klassik Fud	Klassik Fud	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Klassik Fud", "name": "Klassik Fud", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.551124+05
Kofeino	Kofeino	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kofeino", "name": "Kofeino", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.552302+05
Kosmos 9D kazi	Kosmos 9D kazi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmos 9D kazi", "name": "Kosmos 9D kazi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.554543+05
Kosmos 9D shashlik	Kosmos 9D shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmos 9D shashlik", "name": "Kosmos 9D shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.554641+05
Kosmos 9D smetana	Kosmos 9D smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmos 9D smetana", "name": "Kosmos 9D smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.554744+05
Kosmos Stix chili	Kosmos Stix chili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmos Stix chili", "name": "Kosmos Stix chili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.554847+05
Kubishki Rubishki	Kubishki Rubishki	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kubishki Rubishki", "name": "Kubishki Rubishki", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.557043+05
Kuk	Kuk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kuk", "name": "Kuk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.557191+05
Kukruz 333	Kukruz 333	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kukruz 333", "name": "Kukruz 333", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.55731+05
Kukruz 555	Kukruz 555	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kukruz 555", "name": "Kukruz 555", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.557556+05
Kukruz 777 shkalat rus rulon	Kukruz 777 shkalat rus rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kukruz 777 shkalat rus rulon", "name": "Kukruz 777 shkalat rus rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.55766+05
Kukruz Zebest paket	Kukruz Zebest paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kukruz Zebest paket", "name": "Kukruz Zebest paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.557844+05
Kuksi	Kuksi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kuksi", "name": "Kuksi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.55849+05
Kuku kukruz paket	Kuku kukruz paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kuku kukruz paket", "name": "Kuku kukruz paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.558687+05
Kukuruz 777	Kukuruz 777	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kukuruz 777", "name": "Kukuruz 777", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.558962+05
Kukuruz 999	Kukuruz 999	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kukuruz 999", "name": "Kukuruz 999", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.559188+05
Kukuruz Grand	Kukuruz Grand	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kukuruz Grand", "name": "Kukuruz Grand", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.55932+05
Kuru kukruz paket	Kuru kukruz paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kuru kukruz paket", "name": "Kuru kukruz paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.55945+05
Kvadrat aboy paket	Kvadrat aboy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kvadrat aboy paket", "name": "Kvadrat aboy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.559555+05
Lagmon pket	Lagmon pket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lagmon pket", "name": "Lagmon pket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.55966+05
Laktobiotik baby	Laktobiotik baby	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Laktobiotik baby", "name": "Laktobiotik baby", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.559766+05
Layf 300gr	Layf 300gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Layf 300gr", "name": "Layf 300gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.559885+05
Layf 900gr paket	Layf 900gr paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Layf 900gr paket", "name": "Layf 900gr paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.560037+05
Layf paket 900gr	Layf paket 900gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Layf paket 900gr", "name": "Layf paket 900gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.560172+05
Layko	Layko	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Layko", "name": "Layko", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.560291+05
Lazzat karamel	Lazzat karamel	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lazzat karamel", "name": "Lazzat karamel", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.560457+05
Lazzax paket	Lazzax paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lazzax paket", "name": "Lazzax paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.560595+05
Makiz #106	Makiz #106	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz #106", "name": "Makiz #106", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.566076+05
hamroh yer yongoq 500 gr paket	Hamroh Yer Yongoq 500 Gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "hamroh yer yongoq 500 gr paket", "name": "Hamroh Yer Yongoq 500 Gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.743781+05
opp 665/20	OPP 665/20	Kg		homashyo	{"uom": "Kg", "code": "opp 665/20", "name": "OPP 665/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.888185+05
Chittos	Chittos	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Chittos", "name": "Chittos", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.514399+05
Klassik Paket	Klassik Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Klassik Paket", "name": "Klassik Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.551385+05
Kivi	Kivi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kivi", "name": "Kivi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.551007+05
Krekir kalbasa	Krekir kalbasa	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Krekir kalbasa", "name": "Krekir kalbasa", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.555647+05
Krekir salyami	Krekir salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Krekir salyami", "name": "Krekir salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.555834+05
Krekir shashlik	Krekir shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Krekir shashlik", "name": "Krekir shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.555953+05
Kruds paket eski	Kruds paket eski	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kruds paket eski", "name": "Kruds paket eski", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.55646+05
Kruds paket yengi	Kruds paket yengi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kruds paket yengi", "name": "Kruds paket yengi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.556641+05
Locin aboy paket	Locin aboy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Locin aboy paket", "name": "Locin aboy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.561444+05
Lovee	Lovee	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lovee", "name": "Lovee", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.561969+05
Lovis qulupnay	Lovis qulupnay	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lovis qulupnay", "name": "Lovis qulupnay", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.562264+05
Lovis shaftoli	Lovis shaftoli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lovis shaftoli", "name": "Lovis shaftoli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.56236+05
Lovis shokolad	Lovis shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lovis shokolad", "name": "Lovis shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.562463+05
Lubimiy	Lubimiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lubimiy", "name": "Lubimiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.562566+05
Luna 11D	Luna 11D	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Luna 11D", "name": "Luna 11D", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.562668+05
Luna 8D-5D	Luna 8D-5D	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Luna 8D-5D", "name": "Luna 8D-5D", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.563213+05
MAT+MCP plyonka	MAT+MCP plyonka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "MAT+MCP plyonka", "name": "MAT+MCP plyonka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.564246+05
MIG aboy paket	MIG aboy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "MIG aboy paket", "name": "MIG aboy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.565466+05
Magnat pista kok	Magnat pista kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Magnat pista kok", "name": "Magnat pista kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.565669+05
Magnat pista qizil	Magnat pista qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Magnat pista qizil", "name": "Magnat pista qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.565763+05
Mago paket	Mago paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mago paket", "name": "Mago paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.565862+05
Makiz #100	Makiz #100	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz #100", "name": "Makiz #100", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.56597+05
Makiz #107	Makiz #107	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz #107", "name": "Makiz #107", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.566194+05
Makiz 400gr L402	Makiz 400gr L402	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz 400gr L402", "name": "Makiz 400gr L402", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.56681+05
Makiz C542 500gr	Makiz C542 500gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz C542 500gr", "name": "Makiz C542 500gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.567313+05
Makiz CU700	Makiz CU700	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz CU700", "name": "Makiz CU700", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.567533+05
Makiz N320	Makiz N320	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz N320", "name": "Makiz N320", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.570498+05
Makiz pasta N174	Makiz pasta N174	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz pasta N174", "name": "Makiz pasta N174", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.570759+05
Makiz pasta N175	Makiz pasta N175	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz pasta N175", "name": "Makiz pasta N175", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.570897+05
Makiz pasta N179	Makiz pasta N179	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz pasta N179", "name": "Makiz pasta N179", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.571008+05
Manako 500gr paket	Manako 500gr paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Manako 500gr paket", "name": "Manako 500gr paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.572553+05
Maroqand qurt paket	Maroqand qurt paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Maroqand qurt paket", "name": "Maroqand qurt paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.573078+05
Marshmallow heart	Marshmallow heart	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Marshmallow heart", "name": "Marshmallow heart", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.574222+05
Toretto golden dak chili	Toretto golden dak chili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto golden dak chili", "name": "Toretto golden dak chili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.63387+05
har kuni paket	Har Kuni Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "har kuni paket", "name": "Har Kuni Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.744243+05
Eko sfat aboy paket 4kg	Eko sfat aboy paket 4kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Eko sfat aboy paket 4kg", "name": "Eko sfat aboy paket 4kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.522978+05
Krafers belgium	Krafers belgium	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Krafers belgium", "name": "Krafers belgium", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.555481+05
Lotus 70gr tulon	Lotus 70gr tulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lotus 70gr tulon", "name": "Lotus 70gr tulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.561656+05
Lider super	Lider super	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lider super", "name": "Lider super", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.561119+05
Lotus msg	Lotus msg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lotus msg", "name": "Lotus msg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.561764+05
Lotus paket	Lotus paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lotus paket", "name": "Lotus paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.561866+05
Lovis banan	Lovis banan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lovis banan", "name": "Lovis banan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.562067+05
MCP / 20 mikron / 700	MCP 20/700	Kg		homashyo	{"uom": "Kg", "code": "MCP / 20 mikron / 700", "name": "MCP 20/700", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.564842+05
Makiz 2kg CU2	Makiz 2kg CU2	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz 2kg CU2", "name": "Makiz 2kg CU2", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.56671+05
Makiz CU5 5kg	Makiz CU5 5kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz CU5 5kg", "name": "Makiz CU5 5kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.567433+05
Makiz paket ($161)	Makiz paket ($161)	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz paket ($161)", "name": "Makiz paket ($161)", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.570628+05
Makiz pasta N295	Makiz pasta N295	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz pasta N295", "name": "Makiz pasta N295", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.57117+05
Makiz pasta N320 paket	Makiz pasta N320 paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz pasta N320 paket", "name": "Makiz pasta N320 paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.571319+05
Makiz vermishel LV102	Makiz vermishel LV102	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz vermishel LV102", "name": "Makiz vermishel LV102", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.571422+05
Maks kofe	Maks kofe	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Maks kofe", "name": "Maks kofe", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.571664+05
Maksi shashlik yashil	Maksi shashlik yashil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Maksi shashlik yashil", "name": "Maksi shashlik yashil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.57212+05
Mana Chips 35sm	Mana Chips 35sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mana Chips 35sm", "name": "Mana Chips 35sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.572237+05
Mana Chips 37,5sm	Mana Chips 37,5sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mana Chips 37,5sm", "name": "Mana Chips 37,5sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.572355+05
Manaka premium	Manaka premium	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Manaka premium", "name": "Manaka premium", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.572454+05
Manako koko	Manako koko	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Manako koko", "name": "Manako koko", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.572655+05
Manbo	Manbo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Manbo", "name": "Manbo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.572766+05
Marmelad assorti	Marmelad assorti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Marmelad assorti", "name": "Marmelad assorti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.572866+05
Marojniy ays grand	Marojniy ays grand	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Marojniy ays grand", "name": "Marojniy ays grand", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.572976+05
Marvel superman	Marvel superman	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Marvel superman", "name": "Marvel superman", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.574535+05
Maryachkiy keks	Maryachkiy keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Maryachkiy keks", "name": "Maryachkiy keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.574629+05
Maxi 30g	Maxi 30g	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Maxi 30g", "name": "Maxi 30g", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.575247+05
Mcpp 26sm	Mcpp 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mcpp 26sm", "name": "Mcpp 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.575644+05
Mega crackr shashlik	Mega crackr shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mega crackr shashlik", "name": "Mega crackr shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.577074+05
chempion toshkent chikkien longer	Chempion Toshkent Chikkien Longer	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion toshkent chikkien longer", "name": "Chempion Toshkent Chikkien Longer", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.695929+05
havas 38cm 18+30mk	Havas 38cm 18+30mk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "havas 38cm 18+30mk", "name": "Havas 38cm 18+30mk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.744411+05
Gold star shkalad	Gold star shkalad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Gold star shkalad", "name": "Gold star shkalad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.531482+05
Kosta 0.5kg	Kosta 0.5kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosta 0.5kg", "name": "Kosta 0.5kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.555149+05
Lili	Lili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lili", "name": "Lili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.56134+05
Lotus 170gr rulon	Lotus 170gr rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lotus 170gr rulon", "name": "Lotus 170gr rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.561556+05
Makiz N176	Makiz N176	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz N176", "name": "Makiz N176", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.570237+05
kuk suxarik kuritsa	Kuk Suxarik Kuritsa	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kuk suxarik kuritsa", "name": "Kuk Suxarik Kuritsa", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.786747+05
Masha Medved Askarbinka oq tvist	Masha Medved Askarbinka oq tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Masha Medved Askarbinka oq tvist", "name": "Masha Medved Askarbinka oq tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.57484+05
Maslo	Maslo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Maslo", "name": "Maslo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.575033+05
Mat 160/50	Mat 160/50	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mat 160/50", "name": "Mat 160/50", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.575147+05
May chipso	May chipso	Kg		tayyor mahsulot	{"uom": "Kg", "code": "May chipso", "name": "May chipso", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.575346+05
Maya semya paket	Maya semya paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Maya semya paket", "name": "Maya semya paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.575452+05
Mazzali konfet	Mazzali konfet	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mazzali konfet", "name": "Mazzali konfet", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.575548+05
Meat hause sasiska	Meat hause sasiska	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Meat hause sasiska", "name": "Meat hause sasiska", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.575742+05
Mega 20shtuk Tuzsiz paket	Mega 20shtuk Tuzsiz paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mega 20shtuk Tuzsiz paket", "name": "Mega 20shtuk Tuzsiz paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.576069+05
Mega Plombir paket	Mega Plombir paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mega Plombir paket", "name": "Mega Plombir paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.576178+05
Mega Tuzsiz	Mega Tuzsiz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mega Tuzsiz", "name": "Mega Tuzsiz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.576977+05
Metal zip paket 20/25	Metal zip paket 20/25	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Metal zip paket 20/25", "name": "Metal zip paket 20/25", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.578821+05
Mikki kukruz paket	Mikki kukruz paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mikki kukruz paket", "name": "Mikki kukruz paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.579219+05
Musa moko choko	Musa moko choko	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Musa moko choko", "name": "Musa moko choko", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.583646+05
PE 1700/80	PE 1700/80	Kg		tayyor mahsulot	{"uom": "Kg", "code": "PE 1700/80", "name": "PE 1700/80", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.593522+05
PF 19sm	PF 19sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "PF 19sm", "name": "PF 19sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.593981+05
PF-2sm	PF-2sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "PF-2sm", "name": "PF-2sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.594742+05
PRAZ/NIY ZIP PAKET 48/42	PRAZ/NIY ZIP PAKET 48/42	Kg		tayyor mahsulot	{"uom": "Kg", "code": "PRAZ/NIY ZIP PAKET 48/42", "name": "PRAZ/NIY ZIP PAKET 48/42", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.595246+05
Rose sovun	Rose sovun	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rose sovun", "name": "Rose sovun", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.612666+05
Torento asarti 3 5 7 D	Torento asarti 3 5 7 D	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Torento asarti 3 5 7 D", "name": "Torento asarti 3 5 7 D", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.631116+05
komalina krupa 3 kg paket	Komalina Krupa 3 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "komalina krupa 3 kg paket", "name": "Komalina Krupa 3 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.780074+05
metronex paket 42/10	Metronex Paket 42/10	Kg		tayyor mahsulot	{"uom": "Kg", "code": "metronex paket 42/10", "name": "Metronex Paket 42/10", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.82347+05
opp 460/20 pol	OPP 460/20	Kg		homashyo	{"uom": "Kg", "code": "opp 460/20 pol", "name": "OPP 460/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.867831+05
pistello tuzli kok 02	Pistello Tuzli Kok 02	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pistello tuzli kok 02", "name": "Pistello Tuzli Kok 02", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.953069+05
realniy plombir cjornaya smorodina	Realniy Plombir Cjornaya Smorodina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "realniy plombir cjornaya smorodina", "name": "Realniy Plombir Cjornaya Smorodina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.968385+05
saleftka XNS Wet wipes 20ta	saleftka XNS Wet wipes 20ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "saleftka XNS Wet wipes 20ta", "name": "saleftka XNS Wet wipes 20ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.974386+05
salfetka buny for baby 72 sht	Salfetka Buny For Baby 72 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka buny for baby 72 sht", "name": "Salfetka Buny For Baby 72 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.977836+05
shirin soz pryaniki 300gr paket	Shirin Soz Pryaniki 300gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shirin soz pryaniki 300gr paket", "name": "Shirin Soz Pryaniki 300gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.993672+05
usluga xeppi foks 28/25 paket zamok	Usluga Xeppi Foks 28/25 Paket Zamok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga xeppi foks 28/25 paket zamok", "name": "Usluga Xeppi Foks 28/25 Paket Zamok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.030387+05
Jem 985/20	JEM 985/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 985/20", "name": "JEM 985/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.54556+05
Maryachkiy keks paket	Maryachkiy keks paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Maryachkiy keks paket", "name": "Maryachkiy keks paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.574735+05
Masha Medved prazrachniy	Masha Medved prazrachniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Masha Medved prazrachniy", "name": "Masha Medved prazrachniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.574935+05
Jesko	Jesko	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Jesko", "name": "Jesko", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.545772+05
Kosmis 7D mangan	Kosmis 7D mangan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmis 7D mangan", "name": "Kosmis 7D mangan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.553791+05
Djelli Djoy jele 4sm	Djelli Djoy jele 4sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Djelli Djoy jele 4sm", "name": "Djelli Djoy jele 4sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.521651+05
Kukruz jojo paket	Kukruz jojo paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kukruz jojo paket", "name": "Kukruz jojo paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.558361+05
OPP 645/25 pol	OPP 645/25	Kg		homashyo	{"uom": "Kg", "code": "OPP 645/25 pol", "name": "OPP 645/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.589508+05
OPP 650/18	OPP 650/18	Kg		homashyo	{"uom": "Kg", "code": "OPP 650/18", "name": "OPP 650/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.589776+05
PF-20sm	PF-20sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "PF-20sm", "name": "PF-20sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.594463+05
PF-25sm	PF-25sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "PF-25sm", "name": "PF-25sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.594613+05
PRAZ/NIY ZIP PAKET 25/35	PRAZ/NIY ZIP PAKET 25/35	Kg		tayyor mahsulot	{"uom": "Kg", "code": "PRAZ/NIY ZIP PAKET 25/35", "name": "PRAZ/NIY ZIP PAKET 25/35", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.594851+05
PRAZ/NIY ZIP PAKET 28/35	PRAZ/NIY ZIP PAKET 28/35	Kg		tayyor mahsulot	{"uom": "Kg", "code": "PRAZ/NIY ZIP PAKET 28/35", "name": "PRAZ/NIY ZIP PAKET 28/35", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.594962+05
Panda	Panda	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Panda", "name": "Panda", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.595805+05
Panjoy banan	Panjoy banan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Panjoy banan", "name": "Panjoy banan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.595909+05
Patato	Patato	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Patato", "name": "Patato", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.596326+05
Paynet	Paynet	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Paynet", "name": "Paynet", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.596787+05
Payushiy 28sm 18x18	Payushiy 28sm 18x18	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Payushiy 28sm 18x18", "name": "Payushiy 28sm 18x18", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.59691+05
Pedro salyami	Pedro salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pedro salyami", "name": "Pedro salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.59703+05
Pedro shashlik	Pedro shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pedro shashlik", "name": "Pedro shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.597142+05
Pekarushka	Pekarushka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pekarushka", "name": "Pekarushka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.597356+05
Pelmen 300g paket	Pelmen 300g paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pelmen 300g paket", "name": "Pelmen 300g paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.59746+05
Pelmeni paket 200gr	Pelmeni paket 200gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pelmeni paket 200gr", "name": "Pelmeni paket 200gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.598743+05
Pf 23sm	Pf 23sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pf 23sm", "name": "Pf 23sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.600306+05
Pf 2slo'y 26sm	Pf 2slo'y 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pf 2slo'y 26sm", "name": "Pf 2slo'y 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.600454+05
Pf 2slo'y 38sm	Pf 2slo'y 38sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pf 2slo'y 38sm", "name": "Pf 2slo'y 38sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.600595+05
Pf 9.8sm	Pf 9.8sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pf 9.8sm", "name": "Pf 9.8sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.600777+05
Piknik salami	Piknik salami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Piknik salami", "name": "Piknik salami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.601123+05
Piknik smetana	Piknik smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Piknik smetana", "name": "Piknik smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.601277+05
Rameo	Rameo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rameo", "name": "Rameo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.610433+05
Toti kukruz	Toti kukruz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toti kukruz", "name": "Toti kukruz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.635822+05
king kvadrashki suxarik asarty	King Kvadrashki Suxarik Asarty	Kg		tayyor mahsulot	{"uom": "Kg", "code": "king kvadrashki suxarik asarty", "name": "King Kvadrashki Suxarik Asarty", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.776682+05
seven days chokolat 1 kg paket	Seven Days Chokolat 1 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "seven days chokolat 1 kg paket", "name": "Seven Days Chokolat 1 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.991253+05
Bagetto qizil	Bagetto qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bagetto qizil", "name": "Bagetto qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.494037+05
Best vakumniy 45gr	Best vakumniy 45gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Best vakumniy 45gr", "name": "Best vakumniy 45gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.50235+05
MAT 580/20	MAT 580/20	Kg		homashyo	{"uom": "Kg", "code": "MAT 580/20", "name": "MAT 580/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.564038+05
PF 18 sm	PF 18 sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "PF 18 sm", "name": "PF 18 sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.593639+05
PF 22	PF 22	Kg		tayyor mahsulot	{"uom": "Kg", "code": "PF 22", "name": "PF 22", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.594161+05
PF 9sm	PF 9sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "PF 9sm", "name": "PF 9sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.594273+05
Parvoz pista	Parvoz pista	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Parvoz pista", "name": "Parvoz pista", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.596226+05
Pedro sir	Pedro sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pedro sir", "name": "Pedro sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.597257+05
Pet+cpp 33sm	Pet+cpp 33sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pet+cpp 33sm", "name": "Pet+cpp 33sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.599989+05
Piknik barbiku	Piknik barbiku	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Piknik barbiku", "name": "Piknik barbiku", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.600972+05
Pistachi qurt	Pistachi qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pistachi qurt", "name": "Pistachi qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.601432+05
Pitos 7D shashlik	Pitos 7D shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pitos 7D shashlik", "name": "Pitos 7D shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.601551+05
Pitos 7D smetana	Pitos 7D smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pitos 7D smetana", "name": "Pitos 7D smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.601702+05
Pitos 7D tamat	Pitos 7D tamat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pitos 7D tamat", "name": "Pitos 7D tamat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.601847+05
Pitos eski metal	Pitos eski metal	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pitos eski metal", "name": "Pitos eski metal", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.601992+05
Pitos trubichka kuritsa	Pitos trubichka kuritsa	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pitos trubichka kuritsa", "name": "Pitos trubichka kuritsa", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.602457+05
Pitos trubichka sir	Pitos trubichka sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pitos trubichka sir", "name": "Pitos trubichka sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.602687+05
Pittos trubichka shashlik	Pittos trubichka shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pittos trubichka shashlik", "name": "Pittos trubichka shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.602808+05
Pokiza klass	Pokiza klass	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pokiza klass", "name": "Pokiza klass", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.603061+05
Polichudes	Polichudes	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Polichudes", "name": "Polichudes", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.603188+05
Pompi chompi qora	Pompi chompi qora	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pompi chompi qora", "name": "Pompi chompi qora", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.603402+05
Pompi chompi sariq	Pompi chompi sariq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pompi chompi sariq", "name": "Pompi chompi sariq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.6035+05
Pompik korn stik rulonda N110	Pompik korn stik rulonda N110	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pompik korn stik rulonda N110", "name": "Pompik korn stik rulonda N110", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.603601+05
Pompik mega pack kukruz kotta paket	Pompik mega pack kukruz kotta paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pompik mega pack kukruz kotta paket", "name": "Pompik mega pack kukruz kotta paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.604287+05
Prazrachniy 42mm	Prazrachniy 42mm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Prazrachniy 42mm", "name": "Prazrachniy 42mm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.604387+05
Prazrachniy paket	Prazrachniy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Prazrachniy paket", "name": "Prazrachniy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.604487+05
Prazrachniy paket 60sm	Prazrachniy paket 60sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Prazrachniy paket 60sm", "name": "Prazrachniy paket 60sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.604585+05
Premium	Premium	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Premium", "name": "Premium", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.604803+05
Preprava AGF	Preprava AGF	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Preprava AGF", "name": "Preprava AGF", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.604903+05
Present marojniy	Present marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Present marojniy", "name": "Present marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.605003+05
Prinses	Prinses	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Prinses", "name": "Prinses", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.605096+05
Priprava	Priprava	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Priprava", "name": "Priprava", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.605194+05
Probar	Probar	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Probar", "name": "Probar", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.6053+05
Prozrachniy 10/11	Prozrachniy 10/11	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Prozrachniy 10/11", "name": "Prozrachniy 10/11", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.605404+05
Prozrachniy 10sm	Prozrachniy 10sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Prozrachniy 10sm", "name": "Prozrachniy 10sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.605503+05
Prozrachniy 505	Prozrachniy 505	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Prozrachniy 505", "name": "Prozrachniy 505", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.605607+05
Pure layf 400gr	Pure layf 400gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pure layf 400gr", "name": "Pure layf 400gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.605706+05
Best vakumniy 60gr	Best vakumniy 60gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Best vakumniy 60gr", "name": "Best vakumniy 60gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.502634+05
Paket 3kg	Paket 3kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Paket 3kg", "name": "Paket 3kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.595692+05
Panjoy olcha	Panjoy olcha	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Panjoy olcha", "name": "Panjoy olcha", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.596008+05
Mega Tuzli	Mega Tuzli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mega Tuzli", "name": "Mega Tuzli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.576874+05
Patitos chips sir 20gr	Patitos chips sir 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Patitos chips sir 20gr", "name": "Patitos chips sir 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.596525+05
Pitos trubichka salyami	Pitos trubichka salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pitos trubichka salyami", "name": "Pitos trubichka salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.602581+05
Pomer marmalad	Pomer marmalad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pomer marmalad", "name": "Pomer marmalad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.603291+05
Premer paket	Premer paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Premer paket", "name": "Premer paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.604703+05
Pure layf paket 1kg	Pure layf paket 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pure layf paket 1kg", "name": "Pure layf paket 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.605816+05
Pure life 1kg paket	Pure life 1kg paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pure life 1kg paket", "name": "Pure life 1kg paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.605918+05
Qars qur maksimum 50gr xalol palichka	Qars qur maksimum 50gr xalol palichka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qars qur maksimum 50gr xalol palichka", "name": "Qars qur maksimum 50gr xalol palichka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.606843+05
Qars qurs smetana	Qars qurs smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qars qurs smetana", "name": "Qars qurs smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.607719+05
Qars qurs xalol 15gr	Qars qurs xalol 15gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qars qurs xalol 15gr", "name": "Qars qurs xalol 15gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.607818+05
Qarsqur tamat	Qarsqur tamat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qarsqur tamat", "name": "Qarsqur tamat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.608165+05
Qasir qusur	Qasir qusur	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qasir qusur", "name": "Qasir qusur", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.608285+05
Qizil 50gr	Qizil 50gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qizil 50gr", "name": "Qizil 50gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.608392+05
Qizil 50gr xalol	Qizil 50gr xalol	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qizil 50gr xalol", "name": "Qizil 50gr xalol", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.608503+05
Qolip puli fayz marojniy	Qolip puli fayz marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qolip puli fayz marojniy", "name": "Qolip puli fayz marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.60864+05
Qora Pelmeni Paket	Qora Pelmeni Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qora Pelmeni Paket", "name": "Qora Pelmeni Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.608746+05
Qozoxiston marojniy	Qozoxiston marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qozoxiston marojniy", "name": "Qozoxiston marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.608842+05
Qum top	Qum top	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qum top", "name": "Qum top", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.608938+05
Qum-top 999.9	Qum-top 999.9	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qum-top 999.9", "name": "Qum-top 999.9", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.609039+05
Quritilgan shakarli sut	Quritilgan shakarli sut	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Quritilgan shakarli sut", "name": "Quritilgan shakarli sut", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.609458+05
Qurt	Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qurt", "name": "Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.60965+05
Rachkiy	Rachkiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rachkiy", "name": "Rachkiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.610228+05
Raduga lyod	Raduga lyod	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Raduga lyod", "name": "Raduga lyod", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.610329+05
Redbul	Redbul	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Redbul", "name": "Redbul", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.611217+05
Remiks	Remiks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Remiks", "name": "Remiks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.611326+05
Ringo chernika	Ringo chernika	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ringo chernika", "name": "Ringo chernika", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.611637+05
Ringo klubnika	Ringo klubnika	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ringo klubnika", "name": "Ringo klubnika", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.611742+05
Ringo persik	Ringo persik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ringo persik", "name": "Ringo persik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.611839+05
Ringo shokolad	Ringo shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ringo shokolad", "name": "Ringo shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.611935+05
Ringo varyonoye moloko	Ringo varyonoye moloko	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ringo varyonoye moloko", "name": "Ringo varyonoye moloko", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.612038+05
Rols konus	Rols konus	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rols konus", "name": "Rols konus", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.612355+05
Afsona xoddog yangisi	Afsona xoddog yangisi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Afsona xoddog yangisi", "name": "Afsona xoddog yangisi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.474187+05
Blessing paket	Blessing paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Blessing paket", "name": "Blessing paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.505066+05
Bombik marojniy	Bombik marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bombik marojniy", "name": "Bombik marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.506361+05
Qars-qurs suxarik	Qars-qurs suxarik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qars-qurs suxarik", "name": "Qars-qurs suxarik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.607962+05
Qars qurs pishloq	Qars qurs pishloq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qars qurs pishloq", "name": "Qars qurs pishloq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.607498+05
Qarsildoq	Qarsildoq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qarsildoq", "name": "Qarsildoq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.608067+05
Rosli Banan	Rosli Banan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rosli Banan", "name": "Rosli Banan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.612777+05
Rosli Olma	Rosli Olma	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rosli Olma", "name": "Rosli Olma", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.612997+05
Rosli apelsin	Rosli apelsin	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rosli apelsin", "name": "Rosli apelsin", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.613095+05
Rosli paket	Rosli paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rosli paket", "name": "Rosli paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.613321+05
Rosli qulupnay	Rosli qulupnay	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rosli qulupnay", "name": "Rosli qulupnay", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.613438+05
Roxat lazat paket 50ta	Roxat lazat paket 50ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Roxat lazat paket 50ta", "name": "Roxat lazat paket 50ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.613666+05
Roxat pista	Roxat pista	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Roxat pista", "name": "Roxat pista", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.613783+05
Roxat suxariki	Roxat suxariki	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Roxat suxariki", "name": "Roxat suxariki", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.613878+05
Royal paket	Royal paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Royal paket", "name": "Royal paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.613983+05
SAFINA 1 KG ASARTI PAKET	SAFINA 1 KG ASARTI PAKET	Kg		tayyor mahsulot	{"uom": "Kg", "code": "SAFINA 1 KG ASARTI PAKET", "name": "SAFINA 1 KG ASARTI PAKET", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.614093+05
SAFINA 500 GR ASARTI PAKET	SAFINA 500 GR ASARTI PAKET	Kg		tayyor mahsulot	{"uom": "Kg", "code": "SAFINA 500 GR ASARTI PAKET", "name": "SAFINA 500 GR ASARTI PAKET", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.614191+05
Sadaf 5kg aboy paket	Sadaf 5kg aboy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sadaf 5kg aboy paket", "name": "Sadaf 5kg aboy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.6143+05
Sadaf parashok	Sadaf parashok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sadaf parashok", "name": "Sadaf parashok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.614406+05
Sagbon Kanada	Sagbon Kanada	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sagbon Kanada", "name": "Sagbon Kanada", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.614522+05
Salfetka comfort 15dona	Salfetka comfort 15dona	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Salfetka comfort 15dona", "name": "Salfetka comfort 15dona", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.614617+05
Sarafroz sasiska	Sarafroz sasiska	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sarafroz sasiska", "name": "Sarafroz sasiska", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.615229+05
Sarbon pista paket 5kg	Sarbon pista paket 5kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sarbon pista paket 5kg", "name": "Sarbon pista paket 5kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.615333+05
Sardaor simba pista kok	Sardaor simba pista kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sardaor simba pista kok", "name": "Sardaor simba pista kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.615437+05
Semechka yadra	Semechka yadra	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Semechka yadra", "name": "Semechka yadra", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.615737+05
Sensatsia shashlik	Sensatsia shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sensatsia shashlik", "name": "Sensatsia shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.615835+05
Sensyatsya kuritsa	Sensyatsya kuritsa	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sensyatsya kuritsa", "name": "Sensyatsya kuritsa", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.615929+05
Sensyatsya salyami	Sensyatsya salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sensyatsya salyami", "name": "Sensyatsya salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.616021+05
Servelat	Servelat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Servelat", "name": "Servelat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.616127+05
Shariki / qopcha	Shariki / qopcha	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shariki / qopcha", "name": "Shariki / qopcha", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.616225+05
Shashleek	Shashleek	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shashleek", "name": "Shashleek", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.616328+05
Shedriy axotnichiy	Shedriy axotnichiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shedriy axotnichiy", "name": "Shedriy axotnichiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.616423+05
Shedriy salyami	Shedriy salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shedriy salyami", "name": "Shedriy salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.616517+05
Shedriy shashlik	Shedriy shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shedriy shashlik", "name": "Shedriy shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.616613+05
Klassik Tvist Trufel	Klassik Tvist Trufel	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Klassik Tvist Trufel", "name": "Klassik Tvist Trufel", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.551542+05
Qars qurs kids	Qars qurs kids	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qars qurs kids", "name": "Qars qurs kids", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.607394+05
Qars qurs shashlik	Qars qurs shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qars qurs shashlik", "name": "Qars qurs shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.607602+05
Ringo banan	Ringo banan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ringo banan", "name": "Ringo banan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.611538+05
Ringo apelsin	Ringo apelsin	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ringo apelsin", "name": "Ringo apelsin", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.611428+05
Rolﬁ	Rolﬁ	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rolﬁ", "name": "Rolﬁ", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.612559+05
Rosli asarti	Rosli asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rosli asarti", "name": "Rosli asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.613205+05
Shipovnik	Shipovnik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shipovnik", "name": "Shipovnik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.616712+05
Shirin qizil qalampir	Shirin qizil qalampir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shirin qizil qalampir", "name": "Shirin qizil qalampir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.616809+05
Shok banan	Shok banan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shok banan", "name": "Shok banan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.616905+05
Shok shaftoli	Shok shaftoli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shok shaftoli", "name": "Shok shaftoli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.617101+05
Shoxasar mini vaﬂi 250gr vishnya	Shoxasar mini vaﬂi 250gr vishnya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shoxasar mini vaﬂi 250gr vishnya", "name": "Shoxasar mini vaﬂi 250gr vishnya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.618515+05
Silk decor Palett paket	Silk decor Palett paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Silk decor Palett paket", "name": "Silk decor Palett paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.618621+05
Silver choy paket 400gr	Silver choy paket 400gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Silver choy paket 400gr", "name": "Silver choy paket 400gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.619483+05
Silver choy paket 50gr	Silver choy paket 50gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Silver choy paket 50gr", "name": "Silver choy paket 50gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.619588+05
Silver choy pekt 400gr	Silver choy pekt 400gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Silver choy pekt 400gr", "name": "Silver choy pekt 400gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.619703+05
Silver metal paket	Silver metal paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Silver metal paket", "name": "Silver metal paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.619818+05
Simba krab chips	Simba krab chips	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Simba krab chips", "name": "Simba krab chips", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.619922+05
Simba kukruz apelsin	Simba kukruz apelsin	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Simba kukruz apelsin", "name": "Simba kukruz apelsin", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.620027+05
Simba kukruz qulpnay	Simba kukruz qulpnay	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Simba kukruz qulpnay", "name": "Simba kukruz qulpnay", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.620235+05
Simba pista	Simba pista	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Simba pista", "name": "Simba pista", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.620339+05
Sinel abrikos	Sinel abrikos	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sinel abrikos", "name": "Sinel abrikos", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.620466+05
Sinel banan	Sinel banan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sinel banan", "name": "Sinel banan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.620621+05
Sinel chernika	Sinel chernika	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sinel chernika", "name": "Sinel chernika", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.620732+05
Sinel klubnika	Sinel klubnika	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sinel klubnika", "name": "Sinel klubnika", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.620859+05
Sinel mango	Sinel mango	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sinel mango", "name": "Sinel mango", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.620955+05
Sinel molochniy	Sinel molochniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sinel molochniy", "name": "Sinel molochniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.621075+05
Sinel sgushonka	Sinel sgushonka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sinel sgushonka", "name": "Sinel sgushonka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.621183+05
Sinel shokolad	Sinel shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sinel shokolad", "name": "Sinel shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.621289+05
Sirok kakos	Sirok kakos	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sirok kakos", "name": "Sirok kakos", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.621927+05
Sirok panda shokolad	Sirok panda shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sirok panda shokolad", "name": "Sirok panda shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.622231+05
Sirok panda vanilniy	Sirok panda vanilniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sirok panda vanilniy", "name": "Sirok panda vanilniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.622343+05
opp 710/30 pf	OPP 710/30	Kg		homashyo	{"uom": "Kg", "code": "opp 710/30 pf", "name": "OPP 710/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.891684+05
Al baraka keks	Al baraka keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Al baraka keks", "name": "Al baraka keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.476714+05
Lider plitka halod payka	Lider plitka halod payka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lider plitka halod payka", "name": "Lider plitka halod payka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.561017+05
Rols	Rols	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rols", "name": "Rols", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.612242+05
Shok qulupnay	Shok qulupnay	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shok qulupnay", "name": "Shok qulupnay", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.617006+05
Shoxasar mini vaﬂi 250gr limon	Shoxasar mini vaﬂi 250gr limon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shoxasar mini vaﬂi 250gr limon", "name": "Shoxasar mini vaﬂi 250gr limon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.617713+05
Silk decor Premium paket	Silk decor Premium paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Silk decor Premium paket", "name": "Silk decor Premium paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.618733+05
Silver choy paket	Silver choy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Silver choy paket", "name": "Silver choy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.619197+05
Silver choy paket 200gr	Silver choy paket 200gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Silver choy paket 200gr", "name": "Silver choy paket 200gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.619358+05
Sirok smarodina	Sirok smarodina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sirok smarodina", "name": "Sirok smarodina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.622549+05
Sladkaya Kichik Paket	Sladkaya Kichik Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sladkaya Kichik Paket", "name": "Sladkaya Kichik Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.62295+05
Sladkaya Kichik Rulon	Sladkaya Kichik Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sladkaya Kichik Rulon", "name": "Sladkaya Kichik Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.623053+05
Sladkaya Kotta Paket UZ	Sladkaya Kotta Paket UZ	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sladkaya Kotta Paket UZ", "name": "Sladkaya Kotta Paket UZ", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.623259+05
Smaylik olma	Smaylik olma	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Smaylik olma", "name": "Smaylik olma", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.624216+05
Smaylik qlupney	Smaylik qlupney	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Smaylik qlupney", "name": "Smaylik qlupney", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.624311+05
Snejinka sol 1.2kg paket	Snejinka sol 1.2kg paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Snejinka sol 1.2kg paket", "name": "Snejinka sol 1.2kg paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.624404+05
Snejinka sol 2400ugr paket	Snejinka sol 2400ugr paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Snejinka sol 2400ugr paket", "name": "Snejinka sol 2400ugr paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.624501+05
Snejinka sol 800gr paket	Snejinka sol 800gr paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Snejinka sol 800gr paket", "name": "Snejinka sol 800gr paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.624607+05
Sneks team	Sneks team	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sneks team", "name": "Sneks team", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.624734+05
Snikers	Snikers	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Snikers", "name": "Snikers", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.624833+05
Sof qurt	Sof qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sof qurt", "name": "Sof qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.624937+05
Soft plus 3kg parashok	Soft plus 3kg parashok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Soft plus 3kg parashok", "name": "Soft plus 3kg parashok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.625043+05
Soﬁ	Soﬁ	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Soﬁ", "name": "Soﬁ", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.625139+05
Spays metal paket	Spays metal paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Spays metal paket", "name": "Spays metal paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.625339+05
Spays zira eran	Spays zira eran	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Spays zira eran", "name": "Spays zira eran", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.625438+05
Spays zira kumina	Spays zira kumina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Spays zira kumina", "name": "Spays zira kumina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.625538+05
Spinner	Spinner	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Spinner", "name": "Spinner", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.625636+05
Spring 400gr paket	Spring 400gr paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Spring 400gr paket", "name": "Spring 400gr paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.625734+05
Spring paket	Spring paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Spring paket", "name": "Spring paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.625929+05
Spring power	Spring power	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Spring power", "name": "Spring power", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.626033+05
Starnuts	Starnuts	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Starnuts", "name": "Starnuts", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.626132+05
Sulaymon Paket 50 Dona	Sulaymon Paket 50 Dona	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sulaymon Paket 50 Dona", "name": "Sulaymon Paket 50 Dona", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.626229+05
Sulaymon semechka	Sulaymon semechka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sulaymon semechka", "name": "Sulaymon semechka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.626422+05
Sulton Paket 50 Dona	Sulton Paket 50 Dona	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sulton Paket 50 Dona", "name": "Sulton Paket 50 Dona", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.626518+05
Frendo 5D teletena	Frendo 5D teletena	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frendo 5D teletena", "name": "Frendo 5D teletena", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.528523+05
Salom-angry birds	Salom-angry birds	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Salom-angry birds", "name": "Salom-angry birds", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.614813+05
Sancho 50gr	Sancho 50gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sancho 50gr", "name": "Sancho 50gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.614919+05
Sladok	Sladok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sladok", "name": "Sladok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.623748+05
Smayl keks	Smayl keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Smayl keks", "name": "Smayl keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.62402+05
Smaylik apelsin	Smaylik apelsin	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Smaylik apelsin", "name": "Smaylik apelsin", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.624117+05
Sunny	Sunny	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sunny", "name": "Sunny", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.626616+05
Sup/asnova	Sup/asnova	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sup/asnova", "name": "Sup/asnova", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.626709+05
Sup/asnova achiq	Sup/asnova achiq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sup/asnova achiq", "name": "Sup/asnova achiq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.626805+05
Super Kids	Super Kids	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Super Kids", "name": "Super Kids", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.626913+05
Super korin paket	Super korin paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Super korin paket", "name": "Super korin paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.627007+05
Tadim 3D	Tadim 3D	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Tadim 3D", "name": "Tadim 3D", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.629248+05
Taggis 500gr qizil	Taggis 500gr qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Taggis 500gr qizil", "name": "Taggis 500gr qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.629367+05
Taim paket	Taim paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Taim paket", "name": "Taim paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.629468+05
Tanxo kukruz	Tanxo kukruz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Tanxo kukruz", "name": "Tanxo kukruz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.629709+05
Testo somsa	Testo somsa	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Testo somsa", "name": "Testo somsa", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.630352+05
Tik tak	Tik tak	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Tik tak", "name": "Tik tak", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.630605+05
Todello Assorti	Todello Assorti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Todello Assorti", "name": "Todello Assorti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.630711+05
Tom jere paket	Tom jere paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Tom jere paket", "name": "Tom jere paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.630817+05
Tone Kross	Tone Kross	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Tone Kross", "name": "Tone Kross", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.630922+05
Torento 5D assorti	Torento 5D assorti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Torento 5D assorti", "name": "Torento 5D assorti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.631019+05
Toretto 3D chili	Toretto 3D chili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto 3D chili", "name": "Toretto 3D chili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.631211+05
Toretto 3D sir	Toretto 3D sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto 3D sir", "name": "Toretto 3D sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.631334+05
Toretto 3D smetana	Toretto 3D smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto 3D smetana", "name": "Toretto 3D smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.631432+05
Toretto 5D chili	Toretto 5D chili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto 5D chili", "name": "Toretto 5D chili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.631554+05
Toretto 5D sir	Toretto 5D sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto 5D sir", "name": "Toretto 5D sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.631647+05
Toretto 5D smetana	Toretto 5D smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto 5D smetana", "name": "Toretto 5D smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.631742+05
Toretto 7D chili	Toretto 7D chili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto 7D chili", "name": "Toretto 7D chili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.631845+05
Toretto 7D telyatina	Toretto 7D telyatina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto 7D telyatina", "name": "Toretto 7D telyatina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.631939+05
Toretto 7D tuzli	Toretto 7D tuzli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto 7D tuzli", "name": "Toretto 7D tuzli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.632039+05
Toretto 8D chili	Toretto 8D chili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto 8D chili", "name": "Toretto 8D chili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.63219+05
Toretto 8D shashlik	Toretto 8D shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto 8D shashlik", "name": "Toretto 8D shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.632289+05
Toretto 9D chili	Toretto 9D chili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto 9D chili", "name": "Toretto 9D chili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.632397+05
Toretto 9D telyatina	Toretto 9D telyatina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto 9D telyatina", "name": "Toretto 9D telyatina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.632531+05
1000/150 pe pr vakuum	PE PR 1000/150	Kg		homashyo	{"uom": "Kg", "code": "1000/150 pe pr vakuum", "name": "PE PR 1000/150", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.441003+05
Sirok sitrus	Sirok sitrus	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sirok sitrus", "name": "Sirok sitrus", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.622443+05
Sirok vanil	Sirok vanil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sirok vanil", "name": "Sirok vanil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.622844+05
Sladkiy kichik rulon RUS	Sladkiy kichik rulon RUS	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sladkiy kichik rulon RUS", "name": "Sladkiy kichik rulon RUS", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.623577+05
Tabiy toza qurt paket	Tabiy toza qurt paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Tabiy toza qurt paket", "name": "Tabiy toza qurt paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.629146+05
Super semechki	Super semechki	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Super semechki", "name": "Super semechki", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.627492+05
Suxafrukta	Suxafrukta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Suxafrukta", "name": "Suxafrukta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.627741+05
Toretto konus chili	Toretto konus chili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto konus chili", "name": "Toretto konus chili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.634488+05
Toretto konus sir	Toretto konus sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto konus sir", "name": "Toretto konus sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.634611+05
Toretto kukruz paket	Toretto kukruz paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto kukruz paket", "name": "Toretto kukruz paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.634711+05
Toretto plittos	Toretto plittos	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto plittos", "name": "Toretto plittos", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.634808+05
Toretto suxarikiy	Toretto suxarikiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto suxarikiy", "name": "Toretto suxarikiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.634911+05
Toretto wavy chili 25g	Toretto wavy chili 25g	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto wavy chili 25g", "name": "Toretto wavy chili 25g", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.635013+05
Toretto wavy sir 25g	Toretto wavy sir 25g	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto wavy sir 25g", "name": "Toretto wavy sir 25g", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.635213+05
Tornado	Tornado	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Tornado", "name": "Tornado", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.635314+05
Toshkent bulichka	Toshkent bulichka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toshkent bulichka", "name": "Toshkent bulichka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.635416+05
Toster Kolbasa	Toster Kolbasa	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toster Kolbasa", "name": "Toster Kolbasa", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.635521+05
Toster Shashlik	Toster Shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toster Shashlik", "name": "Toster Shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.635619+05
Toster smetana	Toster smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toster smetana", "name": "Toster smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.635719+05
Toti tayim 5D	Toti tayim 5D	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toti tayim 5D", "name": "Toti tayim 5D", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.636355+05
Totli	Totli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Totli", "name": "Totli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.636452+05
Toxtaniyoz ota Jesko	Toxtaniyoz ota Jesko	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toxtaniyoz ota Jesko", "name": "Toxtaniyoz ota Jesko", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.63655+05
Toy-toy	Toy-toy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toy-toy", "name": "Toy-toy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.636648+05
Toys	Toys	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toys", "name": "Toys", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.636744+05
Tres+leta	Tres+leta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Tres+leta", "name": "Tres+leta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.636841+05
Troyka	Troyka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Troyka", "name": "Troyka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.636949+05
Tudey biyo qurt paket	Tudey biyo qurt paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Tudey biyo qurt paket", "name": "Tudey biyo qurt paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.63705+05
Turbo paket	Turbo paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Turbo paket", "name": "Turbo paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.637212+05
Turbo tuzli	Turbo tuzli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Turbo tuzli", "name": "Turbo tuzli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.637345+05
Turbo tuzsiz	Turbo tuzsiz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Turbo tuzsiz", "name": "Turbo tuzsiz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.637499+05
Turon un paket	Turon un paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Turon un paket", "name": "Turon un paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.637632+05
VAKUM PAKET 20/25	VAKUM PAKET 20/25	Kg		tayyor mahsulot	{"uom": "Kg", "code": "VAKUM PAKET 20/25", "name": "VAKUM PAKET 20/25", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.641095+05
Vauv	Vauv	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Vauv", "name": "Vauv", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.641695+05
Vaﬂi tim	Vaﬂi tim	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Vaﬂi tim", "name": "Vaﬂi tim", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.64185+05
Vaﬂo	Vaﬂo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Vaﬂo", "name": "Vaﬂo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.642175+05
Vesta kotta	Vesta kotta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Vesta kotta", "name": "Vesta kotta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.642829+05
Vita gummu	Vita gummu	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Vita gummu", "name": "Vita gummu", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.642948+05
payushi 4,5sm trubichka 30mk	Payushi 4,5sm Trubichka 30mk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "payushi 4,5sm trubichka 30mk", "name": "Payushi 4,5sm Trubichka 30mk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.932929+05
Super pista	Super pista	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Super pista", "name": "Super pista", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.627345+05
Makiz #245	Makiz #245	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz #245", "name": "Makiz #245", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.56644+05
Toretto golden ﬁsh sir	Toretto golden ﬁsh sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto golden ﬁsh sir", "name": "Toretto golden ﬁsh sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.634289+05
Toretto golden ﬁsh smetana	Toretto golden ﬁsh smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto golden ﬁsh smetana", "name": "Toretto golden ﬁsh smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.634389+05
Tvorg 200gr 5% paket	Tvorg 200gr 5% paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Tvorg 200gr 5% paket", "name": "Tvorg 200gr 5% paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.638018+05
Upakovka	CPP	Kg		homashyo	{"uom": "Kg", "code": "Upakovka", "name": "CPP", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.638977+05
Usluga reska izzat aka	Usluga reska izzat aka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Usluga reska izzat aka", "name": "Usluga reska izzat aka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.63908+05
Uzbegim	Uzbegim	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Uzbegim", "name": "Uzbegim", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.639193+05
Uzterra pista 100gr	Uzterra pista 100gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Uzterra pista 100gr", "name": "Uzterra pista 100gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.639535+05
Uzterra pista 100gr qora	Uzterra pista 100gr qora	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Uzterra pista 100gr qora", "name": "Uzterra pista 100gr qora", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.639686+05
Uzterra pista 160gr	Uzterra pista 160gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Uzterra pista 160gr", "name": "Uzterra pista 160gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.639952+05
Uzterra pista 160gr oq	Uzterra pista 160gr oq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Uzterra pista 160gr oq", "name": "Uzterra pista 160gr oq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.640206+05
Uzterra pista 20gr	Uzterra pista 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Uzterra pista 20gr", "name": "Uzterra pista 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.640394+05
Uzterra pista 40gr	Uzterra pista 40gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Uzterra pista 40gr", "name": "Uzterra pista 40gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.640503+05
Uzterra qovoq 30gr	Uzterra qovoq 30gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Uzterra qovoq 30gr", "name": "Uzterra qovoq 30gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.640611+05
Uzterra qovoq 60gr	Uzterra qovoq 60gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Uzterra qovoq 60gr", "name": "Uzterra qovoq 60gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.640718+05
Uzum slfetka	Uzum slfetka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Uzum slfetka", "name": "Uzum slfetka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.640815+05
VAKUM PAKET 15/25	VAKUM PAKET 15/25	Kg		tayyor mahsulot	{"uom": "Kg", "code": "VAKUM PAKET 15/25", "name": "VAKUM PAKET 15/25", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.640924+05
Valli	Valli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Valli", "name": "Valli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.641388+05
Vanilniy saxir	Vanilniy saxir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Vanilniy saxir", "name": "Vanilniy saxir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.64154+05
Vaﬂi time	Vaﬂi time	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Vaﬂi time", "name": "Vaﬂi time", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.64202+05
Veola Xna	Veola Xna	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Veola Xna", "name": "Veola Xna", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.642326+05
Vesna aboy paket	Vesna aboy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Vesna aboy paket", "name": "Vesna aboy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.642506+05
Vesta kichkina	Vesta kichkina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Vesta kichkina", "name": "Vesta kichkina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.642659+05
Xan decor paket	Xan decor paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xan decor paket", "name": "Xan decor paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.644014+05
Xlor	Xlor	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xlor", "name": "Xlor", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.644579+05
Xordiq Assorti	Xordiq Assorti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xordiq Assorti", "name": "Xordiq Assorti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.64472+05
Xrus tayim 3D 40gr salyami	Xrus tayim 3D 40gr salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 3D 40gr salyami", "name": "Xrus tayim 3D 40gr salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.646361+05
Xrus tayim 5D 20gr smetana	Xrus tayim 5D 20gr smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 5D 20gr smetana", "name": "Xrus tayim 5D 20gr smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.647923+05
Xrus tayim 5D 20gr steyk	Xrus tayim 5D 20gr steyk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 5D 20gr steyk", "name": "Xrus tayim 5D 20gr steyk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.648161+05
Jem 425/20	JEM 425/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 425/20", "name": "JEM 425/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.540601+05
Lenta 7sm	Lenta 7sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lenta 7sm", "name": "Lenta 7sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.560914+05
Makiz 400gr L403	Makiz 400gr L403	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz 400gr L403", "name": "Makiz 400gr L403", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.566927+05
Toretto golden ﬁsh chili	Toretto golden ﬁsh chili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto golden ﬁsh chili", "name": "Toretto golden ﬁsh chili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.634182+05
Tvorg 400gr 5% paket	Tvorg 400gr 5% paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Tvorg 400gr 5% paket", "name": "Tvorg 400gr 5% paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.638539+05
Twintos 3-5-7D	Twintos 3-5-7D	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Twintos 3-5-7D", "name": "Twintos 3-5-7D", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.63887+05
Xan ahmat salfetka	Xan ahmat salfetka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xan ahmat salfetka", "name": "Xan ahmat salfetka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.643855+05
Xello kiti marmelad	Xello kiti marmelad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xello kiti marmelad", "name": "Xello kiti marmelad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.644153+05
Xeppi chup 8 sm teshili	Xeppi chup 8 sm teshili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xeppi chup 8 sm teshili", "name": "Xeppi chup 8 sm teshili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.644306+05
Xitoy choy N110	Xitoy choy N110	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xitoy choy N110", "name": "Xitoy choy N110", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.644442+05
Xrus tayim 3D 20gr paprika	Xrus tayim 3D 20gr paprika	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 3D 20gr paprika", "name": "Xrus tayim 3D 20gr paprika", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.645606+05
Xrus tayim 3D 20gr pishloq	Xrus tayim 3D 20gr pishloq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 3D 20gr pishloq", "name": "Xrus tayim 3D 20gr pishloq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.645746+05
Xrus tayim 3D 20gr salyami	Xrus tayim 3D 20gr salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 3D 20gr salyami", "name": "Xrus tayim 3D 20gr salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.645902+05
Xrus tayim 3D 40gr barbiku	Xrus tayim 3D 40gr barbiku	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 3D 40gr barbiku", "name": "Xrus tayim 3D 40gr barbiku", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.646045+05
Xrus tayim 3D 40gr paprika	Xrus tayim 3D 40gr paprika	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 3D 40gr paprika", "name": "Xrus tayim 3D 40gr paprika", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.646206+05
Xrus tayim 3D 40gr shashlik	Xrus tayim 3D 40gr shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 3D 40gr shashlik", "name": "Xrus tayim 3D 40gr shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.646506+05
Xrus tayim 3D 40gr sir	Xrus tayim 3D 40gr sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 3D 40gr sir", "name": "Xrus tayim 3D 40gr sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.646667+05
Xrus tayim 3D 40gr smetana	Xrus tayim 3D 40gr smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 3D 40gr smetana", "name": "Xrus tayim 3D 40gr smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.646814+05
Xrus tayim 5D 20gr salyami	Xrus tayim 5D 20gr salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 5D 20gr salyami", "name": "Xrus tayim 5D 20gr salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.647183+05
Xrus tayim 5D 20gr shashlik	Xrus tayim 5D 20gr shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 5D 20gr shashlik", "name": "Xrus tayim 5D 20gr shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.647348+05
Xrus tayim 5D 20gr sir	Xrus tayim 5D 20gr sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 5D 20gr sir", "name": "Xrus tayim 5D 20gr sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.647548+05
Xrus tayim 5D 20gr tomat	Xrus tayim 5D 20gr tomat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 5D 20gr tomat", "name": "Xrus tayim 5D 20gr tomat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.64832+05
Xrusti 100g paket	Xrusti 100g paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrusti 100g paket", "name": "Xrusti 100g paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.64926+05
Xrustik Paket 80gr	Xrustik Paket 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustik Paket 80gr", "name": "Xrustik Paket 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.649378+05
Xrustof shashlik 37,5 sm	Xrustof shashlik 37,5 sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustof shashlik 37,5 sm", "name": "Xrustof shashlik 37,5 sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.650288+05
XrustoﬀGrenki sir 37,5sm	XrustoﬀGrenki sir 37,5sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "XrustoﬀGrenki sir 37,5sm", "name": "XrustoﬀGrenki sir 37,5sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.650809+05
Xrustoﬀgrenki kalbasa chorizo 27sm	Xrustoﬀgrenki kalbasa chorizo 27sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustoﬀgrenki kalbasa chorizo 27sm", "name": "Xrustoﬀgrenki kalbasa chorizo 27sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.65091+05
Xrustoﬀgril	Xrustoﬀgril	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustoﬀgril", "name": "Xrustoﬀgril", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.651464+05
Xrustoﬀkalbasa 26sm	Xrustoﬀkalbasa 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustoﬀkalbasa 26sm", "name": "Xrustoﬀkalbasa 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.651564+05
Xrustoﬀsir 26sm	Xrustoﬀsir 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustoﬀsir 26sm", "name": "Xrustoﬀsir 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.651974+05
Xrustoﬀsmetana zelen 26sm	Xrustoﬀsmetana zelen 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustoﬀsmetana zelen 26sm", "name": "Xrustoﬀsmetana zelen 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.652071+05
OPP 590/18 pol	OPP 590/18	Kg		homashyo	{"uom": "Kg", "code": "OPP 590/18 pol", "name": "OPP 590/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.587572+05
OPP 675/25 pol	OPP 675/25	Kg		homashyo	{"uom": "Kg", "code": "OPP 675/25 pol", "name": "OPP 675/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.590026+05
Tvorg 400gr 9% paket	Tvorg 400gr 9% paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Tvorg 400gr 9% paket", "name": "Tvorg 400gr 9% paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.638653+05
Twins	Twins	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Twins", "name": "Twins", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.638765+05
Xrum xrum kukruz paket	Xrum xrum kukruz paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrum xrum kukruz paket", "name": "Xrum xrum kukruz paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.645176+05
Xrumstik Paket Kotta	Xrumstik Paket Kotta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrumstik Paket Kotta", "name": "Xrumstik Paket Kotta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.645463+05
Xrus tayim 5D 40gr shashlik	Xrus tayim 5D 40gr shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 5D 40gr shashlik", "name": "Xrus tayim 5D 40gr shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.648676+05
Xrus tayim 5D 40gr sir	Xrus tayim 5D 40gr sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 5D 40gr sir", "name": "Xrus tayim 5D 40gr sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.648825+05
Xrust tayim 5D 40gr salyami	Xrust tayim 5D 40gr salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrust tayim 5D 40gr salyami", "name": "Xrust tayim 5D 40gr salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.649052+05
Xrust tayim 5D 40gr smetana	Xrust tayim 5D 40gr smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrust tayim 5D 40gr smetana", "name": "Xrust tayim 5D 40gr smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.649152+05
Xrustone	Xrustone	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustone", "name": "Xrustone", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.650392+05
Xrustoﬀ27sm qazi	Xrustoﬀ27sm qazi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustoﬀ27sm qazi", "name": "Xrustoﬀ27sm qazi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.650504+05
Xrustoﬀ27sm smetana	Xrustoﬀ27sm smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustoﬀ27sm smetana", "name": "Xrustoﬀ27sm smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.650604+05
Xrustoﬀ27sm tamat	Xrustoﬀ27sm tamat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustoﬀ27sm tamat", "name": "Xrustoﬀ27sm tamat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.650704+05
Xrustoﬀgrenki tay chili 27sm	Xrustoﬀgrenki tay chili 27sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustoﬀgrenki tay chili 27sm", "name": "Xrustoﬀgrenki tay chili 27sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.65114+05
Xrustoﬀgrenkiy tay chili 26sm	Xrustoﬀgrenkiy tay chili 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustoﬀgrenkiy tay chili 26sm", "name": "Xrustoﬀgrenkiy tay chili 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.651356+05
Xrustoﬀqazi 26sm	Xrustoﬀqazi 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustoﬀqazi 26sm", "name": "Xrustoﬀqazi 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.651676+05
Xrustoﬀsalyami 26sm	Xrustoﬀsalyami 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustoﬀsalyami 26sm", "name": "Xrustoﬀsalyami 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.65178+05
Xrustoﬀshashlik 26sm	Xrustoﬀshashlik 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustoﬀshashlik 26sm", "name": "Xrustoﬀshashlik 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.651882+05
Xrustyashki smetana 23sm	Xrustyashki smetana 23sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustyashki smetana 23sm", "name": "Xrustyashki smetana 23sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.652955+05
Xrustyashkiy baget salami 26sm	Xrustyashkiy baget salami 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustyashkiy baget salami 26sm", "name": "Xrustyashkiy baget salami 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.653233+05
Xrustyashkiy baget shashlika 26sm	Xrustyashkiy baget shashlika 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustyashkiy baget shashlika 26sm", "name": "Xrustyashkiy baget shashlika 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.653372+05
Xrustyashkiy baget smetana 26sm	Xrustyashkiy baget smetana 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustyashkiy baget smetana 26sm", "name": "Xrustyashkiy baget smetana 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.653644+05
Xrystoﬀgrenki qurt chiliy 26sm	Xrystoﬀgrenki qurt chiliy 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrystoﬀgrenki qurt chiliy 26sm", "name": "Xrystoﬀgrenki qurt chiliy 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.654182+05
Yammi	Yammi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yammi", "name": "Yammi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.654318+05
Yanchilgan qora muruch	Yanchilgan qora muruch	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yanchilgan qora muruch", "name": "Yanchilgan qora muruch", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.654444+05
Yashil 50gr	Yashil 50gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yashil 50gr", "name": "Yashil 50gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.654706+05
Yashil 50gr Halol	Yashil 50gr Halol	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yashil 50gr Halol", "name": "Yashil 50gr Halol", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.65483+05
Yello chef gourmet	Yello chef gourmet	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yello chef gourmet", "name": "Yello chef gourmet", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.655231+05
Yello krab	Yello krab	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yello krab", "name": "Yello krab", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.655371+05
cheers nachos sous salsa 210 gr 46 sm	Cheers Nachos Sous Salsa 210 Gr 46 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nachos sous salsa 210 gr 46 sm", "name": "Cheers Nachos Sous Salsa 210 Gr 46 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.691131+05
OPP 640/18 pol	OPP 640/18	Kg		homashyo	{"uom": "Kg", "code": "OPP 640/18 pol", "name": "OPP 640/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.588973+05
Rio suxariki	Rio suxariki	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rio suxariki", "name": "Rio suxariki", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.612138+05
Saqich lolipop	Saqich lolipop	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Saqich lolipop", "name": "Saqich lolipop", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.615125+05
Xordiq pista	Xordiq pista	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xordiq pista", "name": "Xordiq pista", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.644864+05
Xrusttik+ 80gr paket	Xrusttik+ 80gr paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrusttik+ 80gr paket", "name": "Xrusttik+ 80gr paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.65238+05
Xrustyashki salyami 35sm	Xrustyashki salyami 35sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustyashki salyami 35sm", "name": "Xrustyashki salyami 35sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.652479+05
Xrustyashki sir 23sm	Xrustyashki sir 23sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustyashki sir 23sm", "name": "Xrustyashki sir 23sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.652681+05
Xrustyashki sir 35sm	Xrustyashki sir 35sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustyashki sir 35sm", "name": "Xrustyashki sir 35sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.652803+05
Xrustyashkiy shashlik 23sm	Xrustyashkiy shashlik 23sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustyashkiy shashlik 23sm", "name": "Xrustyashkiy shashlik 23sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.653778+05
Yantarniy 50gr paket	Yantarniy 50gr paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yantarniy 50gr paket", "name": "Yantarniy 50gr paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.654569+05
Yashil 50gr cho'pli	Yashil 50gr cho'pli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yashil 50gr cho'pli", "name": "Yashil 50gr cho'pli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.654963+05
Yashil 90gr	Yashil 90gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yashil 90gr", "name": "Yashil 90gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.655095+05
Yello qazi	Yello qazi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yello qazi", "name": "Yello qazi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.655497+05
Yello smetana	Yello smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yello smetana", "name": "Yello smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.655628+05
Yello suxariki tomato	Yello suxariki tomato	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yello suxariki tomato", "name": "Yello suxariki tomato", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.655894+05
Yevro dekor aboy paket	Yevro dekor aboy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yevro dekor aboy paket", "name": "Yevro dekor aboy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.656034+05
Yogurt	Yogurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yogurt", "name": "Yogurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.656173+05
Yum yum 35gr xalodniy	Yum yum 35gr xalodniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yum yum 35gr xalodniy", "name": "Yum yum 35gr xalodniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.656443+05
ZIP PAKET 25/27	ZIP PAKET 25/27	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ZIP PAKET 25/27", "name": "ZIP PAKET 25/27", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.657015+05
Zelonniy chay limin paket	Zelonniy chay limin paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zelonniy chay limin paket", "name": "Zelonniy chay limin paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.65761+05
Zelonniy chay marskoy pekt	Zelonniy chay marskoy pekt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zelonniy chay marskoy pekt", "name": "Zelonniy chay marskoy pekt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.657746+05
Zizi 4,5gr	Zizi 4,5gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi 4,5gr", "name": "Zizi 4,5gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.658398+05
Zizi C vitamin	Zizi C vitamin	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi C vitamin", "name": "Zizi C vitamin", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.659252+05
Zizi ananas	Zizi ananas	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi ananas", "name": "Zizi ananas", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.659527+05
Zizi askarbinka	Zizi askarbinka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi askarbinka", "name": "Zizi askarbinka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.65971+05
Zizi askarbinka xalol	Zizi askarbinka xalol	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi askarbinka xalol", "name": "Zizi askarbinka xalol", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.660032+05
Zizi chupachups 14gr	Zizi chupachups 14gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi chupachups 14gr", "name": "Zizi chupachups 14gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.660301+05
Zizi elektra shok	Zizi elektra shok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi elektra shok", "name": "Zizi elektra shok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.660598+05
Zizi kola 5,6gr	Zizi kola 5,6gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi kola 5,6gr", "name": "Zizi kola 5,6gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.660723+05
Zizi lollipop	Zizi lollipop	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi lollipop", "name": "Zizi lollipop", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.661019+05
Zizi maza meva 960gr	Zizi maza meva 960gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi maza meva 960gr", "name": "Zizi maza meva 960gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.661306+05
Zizi mini shashlik	Zizi mini shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi mini shashlik", "name": "Zizi mini shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.661448+05
Zizi mm	Zizi mm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi mm", "name": "Zizi mm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.661592+05
Jem 555/20	JEM 555/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 555/20", "name": "JEM 555/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.542206+05
Sinel vishnevoy	Sinel vishnevoy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sinel vishnevoy", "name": "Sinel vishnevoy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.621619+05
Tvister marojniy	Tvister marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Tvister marojniy", "name": "Tvister marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.637895+05
Xrustik plus paket 50gr qora	Xrustik plus paket 50gr qora	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustik plus paket 50gr qora", "name": "Xrustik plus paket 50gr qora", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.650029+05
ZIP PAKET 25/30	ZIP PAKET 25/30	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ZIP PAKET 25/30", "name": "ZIP PAKET 25/30", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.657182+05
Zelico cooker	Zelico cooker	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zelico cooker", "name": "Zelico cooker", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.657471+05
Zira	Zira	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zira", "name": "Zira", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.65809+05
Ziyo aboy paket	Ziyo aboy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ziyo aboy paket", "name": "Ziyo aboy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.658247+05
Zizi 4gr	Zizi 4gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi 4gr", "name": "Zizi 4gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.658551+05
Zizi 5,5gr	Zizi 5,5gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi 5,5gr", "name": "Zizi 5,5gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.658699+05
Zizi 5,6gr mentol	Zizi 5,6gr mentol	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi 5,6gr mentol", "name": "Zizi 5,6gr mentol", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.658848+05
Zizi 840gr	Zizi 840gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi 840gr", "name": "Zizi 840gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.65911+05
Zizi chupachups 9gr	Zizi chupachups 9gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi chupachups 9gr", "name": "Zizi chupachups 9gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.660448+05
Zizi kukruz	Zizi kukruz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi kukruz", "name": "Zizi kukruz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.660862+05
Zizi maks paket	Zizi maks paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi maks paket", "name": "Zizi maks paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.661151+05
Zo'r pista	Zo'r pista	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zo'r pista", "name": "Zo'r pista", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.662797+05
Zo'r pista qizil	Zo'r pista qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zo'r pista qizil", "name": "Zo'r pista qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.662948+05
Zor pista qizil 60gr	Zor pista qizil 60gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zor pista qizil 60gr", "name": "Zor pista qizil 60gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.663092+05
Zo’r-zo’r qizil	Zo’r-zo’r qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zo’r-zo’r qizil", "name": "Zo’r-zo’r qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.663691+05
Zo’r-zo’r sariq	Zo’r-zo’r sariq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zo’r-zo’r sariq", "name": "Zo’r-zo’r sariq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.663832+05
Zòr pista 60gr kòk	Zòr pista 60gr kòk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zòr pista 60gr kòk", "name": "Zòr pista 60gr kòk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.664483+05
abrazes	Abrazes	Kg		tayyor mahsulot	{"uom": "Kg", "code": "abrazes", "name": "Abrazes", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.664672+05
afsona sifat va maza xalol	Afsona Sifat Va Maza Xalol	Kg		tayyor mahsulot	{"uom": "Kg", "code": "afsona sifat va maza xalol", "name": "Afsona Sifat Va Maza Xalol", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.664845+05
aisha chocolate waﬀers	Aisha Chocolate Waﬀers	Kg		tayyor mahsulot	{"uom": "Kg", "code": "aisha chocolate waﬀers", "name": "Aisha Chocolate Waﬀers", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.664982+05
ak emgek lapsha 1 kg paket	Ak Emgek Lapsha 1 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ak emgek lapsha 1 kg paket", "name": "Ak Emgek Lapsha 1 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.665278+05
ak emgek lapsha 400 gr paket	Ak Emgek Lapsha 400 Gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ak emgek lapsha 400 gr paket", "name": "Ak Emgek Lapsha 400 Gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.665451+05
akhmedov since 2020	Akhmedov Since 2020	Kg		tayyor mahsulot	{"uom": "Kg", "code": "akhmedov since 2020", "name": "Akhmedov Since 2020", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.665602+05
aladdin semechka qizil 90 gr	Aladdin Semechka Qizil 90 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "aladdin semechka qizil 90 gr", "name": "Aladdin Semechka Qizil 90 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.665747+05
aladin semechka aranjivi	Aladin Semechka Aranjivi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "aladin semechka aranjivi", "name": "Aladin Semechka Aranjivi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.665893+05
ali baba chempion paket	Ali Baba Chempion Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ali baba chempion paket", "name": "Ali Baba Chempion Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.666041+05
ali baba chempion sendvich paket	Ali Baba Chempion Sendvich Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ali baba chempion sendvich paket", "name": "Ali Baba Chempion Sendvich Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.66619+05
ali bobo diyona qora marojniy	Ali Bobo Diyona Qora Marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ali bobo diyona qora marojniy", "name": "Ali Bobo Diyona Qora Marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.666343+05
ali bobo egzo	Ali Bobo Egzo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ali bobo egzo", "name": "Ali Bobo Egzo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.666503+05
ali bobo izabella	Ali Bobo Izabella	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ali bobo izabella", "name": "Ali Bobo Izabella", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.666649+05
OPP 885/18 Pol pf	OPP 885/18	Kg		homashyo	{"uom": "Kg", "code": "OPP 885/18 Pol pf", "name": "OPP 885/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.590877+05
ZIP PAKET 25/25	ZIP PAKET 25/25	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ZIP PAKET 25/25", "name": "ZIP PAKET 25/25", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.656854+05
Zam zam paket	Zam zam paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zam zam paket", "name": "Zam zam paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.657334+05
Zo’r-zo’r yashil	Zo’r-zo’r yashil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zo’r-zo’r yashil", "name": "Zo’r-zo’r yashil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.663993+05
Zo’r-zo’r priprava	Zo’r-zo’r priprava	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zo’r-zo’r priprava", "name": "Zo’r-zo’r priprava", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.663398+05
Zòr pista 100gr kòk	Zòr pista 100gr kòk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zòr pista 100gr kòk", "name": "Zòr pista 100gr kòk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.664138+05
Zòr pista 100gr qizil	Zòr pista 100gr qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zòr pista 100gr qizil", "name": "Zòr pista 100gr qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.664314+05
ali tekstil paket	Ali Tekstil Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ali tekstil paket", "name": "Ali Tekstil Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.666967+05
alibobo ribbon	Alibobo Ribbon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "alibobo ribbon", "name": "Alibobo Ribbon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.667127+05
almond suxafrukt 40gr asartiy	Almond Suxafrukt 40gr Asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "almond suxafrukt 40gr asartiy", "name": "Almond Suxafrukt 40gr Asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.667533+05
almond zip paket asarti	Almond Zip Paket Asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "almond zip paket asarti", "name": "Almond Zip Paket Asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.667668+05
almont sirniy palichka 45gr paket	Almont Sirniy Palichka 45gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "almont sirniy palichka 45gr paket", "name": "Almont Sirniy Palichka 45gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.667828+05
aloe chamoline 20 sht	Aloe Chamoline 20 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "aloe chamoline 20 sht", "name": "Aloe Chamoline 20 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.667983+05
aloe chamomili 120 sht	Aloe Chamomili 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "aloe chamomili 120 sht", "name": "Aloe Chamomili 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.668112+05
aloe extract 15ta	Aloe Extract 15ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "aloe extract 15ta", "name": "Aloe Extract 15ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.668253+05
alumin+qog'oz 14sm	Alumin+qog'oz 14sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "alumin+qog'oz 14sm", "name": "Alumin+qog'oz 14sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.668522+05
amb bon debut plombir 100 gr	Amb Bon Debut Plombir 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "amb bon debut plombir 100 gr", "name": "Amb Bon Debut Plombir 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.668661+05
amb ﬁstashka 60 gr	Amb Fistashka 60 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "amb ﬁstashka 60 gr", "name": "Amb Fistashka 60 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.668819+05
andijon chempion xot dog 4 dona	Andijon Chempion Xot Dog 4 Dona	Kg		tayyor mahsulot	{"uom": "Kg", "code": "andijon chempion xot dog 4 dona", "name": "Andijon Chempion Xot Dog 4 Dona", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.668953+05
arginal slados kok	Arginal Slados Kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "arginal slados kok", "name": "Arginal Slados Kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.670063+05
arginal slados qizil	Arginal Slados Qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "arginal slados qizil", "name": "Arginal Slados Qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.670221+05
armiya etalon 1	Armiya Etalon 1	Kg		tayyor mahsulot	{"uom": "Kg", "code": "armiya etalon 1", "name": "Armiya Etalon 1", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.670384+05
armiya galet paket	Armiya Galet Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "armiya galet paket", "name": "Armiya Galet Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.670685+05
aroma 3 kg poroshok paket	Aroma 3 Kg Poroshok Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "aroma 3 kg poroshok paket", "name": "Aroma 3 Kg Poroshok Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.67095+05
aroma qogoz sochiq 2 dona	Aroma Qogoz Sochiq 2 Dona	Kg		tayyor mahsulot	{"uom": "Kg", "code": "aroma qogoz sochiq 2 dona", "name": "Aroma Qogoz Sochiq 2 Dona", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.671093+05
arvin keks	Arvin Keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "arvin keks", "name": "Arvin Keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.671241+05
atlas konus 80gr	Atlas Konus 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "atlas konus 80gr", "name": "Atlas Konus 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.671827+05
avstriyskiy zavarnoy 0,5kg	Avstriyskiy Zavarnoy 0,5kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "avstriyskiy zavarnoy 0,5kg", "name": "Avstriyskiy Zavarnoy 0,5kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.672109+05
ayran	Ayran	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ayran", "name": "Ayran", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.672539+05
ays grim 70gr pingivin	Ays Grim 70gr Pingivin	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ays grim 70gr pingivin", "name": "Ays Grim 70gr Pingivin", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.672671+05
aziya aboy paket	Aziya Aboy Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "aziya aboy paket", "name": "Aziya Aboy Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.673802+05
OPP 940/30 Pol pf	OPP 940/30	Kg		homashyo	{"uom": "Kg", "code": "OPP 940/30 Pol pf", "name": "OPP 940/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.591461+05
Zo’r-zo’r jemchuk	Zo’r-zo’r jemchuk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zo’r-zo’r jemchuk", "name": "Zo’r-zo’r jemchuk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.663244+05
Zo’r-zo’r prozrachniy	Zo’r-zo’r prozrachniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zo’r-zo’r prozrachniy", "name": "Zo’r-zo’r prozrachniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.663544+05
aloe vera 120ta	Aloe Vera 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "aloe vera 120ta", "name": "Aloe Vera 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.668379+05
area kids zip paket	Area Kids Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "area kids zip paket", "name": "Area Kids Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.669877+05
armiya etalon 2	Armiya Etalon 2	Kg		tayyor mahsulot	{"uom": "Kg", "code": "armiya etalon 2", "name": "Armiya Etalon 2", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.670543+05
asiya hause dekor paket	Asiya Hause Dekor Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "asiya hause dekor paket", "name": "Asiya Hause Dekor Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.671379+05
asl qurt paket	Asl Qurt Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "asl qurt paket", "name": "Asl Qurt Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.671536+05
ays grim 9999 100gr	Ays Grim 9999 100gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ays grim 9999 100gr", "name": "Ays Grim 9999 100gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.672827+05
ays grim choko 110gr	Ays Grim Choko 110gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ays grim choko 110gr", "name": "Ays Grim Choko 110gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.673103+05
ayva family 120 sht	Ayva Family 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ayva family 120 sht", "name": "Ayva Family 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.673642+05
aziya preprava	Aziya Preprava	Kg		tayyor mahsulot	{"uom": "Kg", "code": "aziya preprava", "name": "Aziya Preprava", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.673969+05
baby shower wet wipes 120ta	Baby Shower Wet Wipes 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "baby shower wet wipes 120ta", "name": "Baby Shower Wet Wipes 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.674106+05
balday bonus ideal qurt	Balday Bonus Ideal Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "balday bonus ideal qurt", "name": "Balday Bonus Ideal Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.674249+05
bamby baby salfetka	Bamby Baby Salfetka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bamby baby salfetka", "name": "Bamby Baby Salfetka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.674389+05
banitto	Banitto	Kg		tayyor mahsulot	{"uom": "Kg", "code": "banitto", "name": "Banitto", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.674517+05
baraka semechka 2 kg paket	Baraka Semechka 2 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "baraka semechka 2 kg paket", "name": "Baraka Semechka 2 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.674935+05
barakat krupa 3 kg paket	Barakat Krupa 3 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "barakat krupa 3 kg paket", "name": "Barakat Krupa 3 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.675069+05
baranki prazranchniy rulon	Baranki Prazranchniy Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "baranki prazranchniy rulon", "name": "Baranki Prazranchniy Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.675335+05
barankiy bulichka paket	Barankiy Bulichka Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "barankiy bulichka paket", "name": "Barankiy Bulichka Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.675472+05
barbol erke	Barbol Erke	Kg		tayyor mahsulot	{"uom": "Kg", "code": "barbol erke", "name": "Barbol Erke", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.675619+05
barbol tamshan	Barbol Tamshan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "barbol tamshan", "name": "Barbol Tamshan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.675891+05
barbol tamshan paket	Barbol Tamshan Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "barbol tamshan paket", "name": "Barbol Tamshan Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.676015+05
barer 3d paket jenskiy	Barer 3d Paket Jenskiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "barer 3d paket jenskiy", "name": "Barer 3d Paket Jenskiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.676155+05
barfak plombir 45 gr	Barfak Plombir 45 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "barfak plombir 45 gr", "name": "Barfak Plombir 45 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.676288+05
baxt atirgul paket	Baxt Atirgul Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "baxt atirgul paket", "name": "Baxt Atirgul Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.676419+05
belek tvist	Belek Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "belek tvist", "name": "Belek Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.677298+05
bellagio	Bellagio	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bellagio", "name": "Bellagio", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.67745+05
bellito konfet	Bellito Konfet	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bellito konfet", "name": "Bellito Konfet", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.677585+05
beneo jidkiy abou	Beneo Jidkiy Abou	Kg		tayyor mahsulot	{"uom": "Kg", "code": "beneo jidkiy abou", "name": "Beneo Jidkiy Abou", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.677898+05
berrak tekistil	Berrak Tekistil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "berrak tekistil", "name": "Berrak Tekistil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.678061+05
bibo 1999 paket	Bibo 1999 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bibo 1999 paket", "name": "Bibo 1999 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.678236+05
Marsh mellov kotta 54.5sm	Marsh mellov kotta 54.5sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Marsh mellov kotta 54.5sm", "name": "Marsh mellov kotta 54.5sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.57318+05
almond arexi paket	Almond Arexi Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "almond arexi paket", "name": "Almond Arexi Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.667407+05
anor 70 gr	Anor 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "anor 70 gr", "name": "Anor 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.669104+05
OPPM 730/20	OPPM 730/20	Kg		homashyo	{"uom": "Kg", "code": "OPPM 730/20", "name": "OPPM 730/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.591767+05
beby luru paket	Beby Luru Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "beby luru paket", "name": "Beby Luru Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.677007+05
banitto zamok paket	Banitto Zamok Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "banitto zamok paket", "name": "Banitto Zamok Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.674798+05
bek 20gr aranjiviy yengi	Bek 20gr Aranjiviy Yengi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bek 20gr aranjiviy yengi", "name": "Bek 20gr Aranjiviy Yengi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.677153+05
bibo semechka 3 kg paket	Bibo Semechka 3 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bibo semechka 3 kg paket", "name": "Bibo Semechka 3 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.678373+05
big bos burger paket	Big Bos Burger Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "big bos burger paket", "name": "Big Bos Burger Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.678521+05
big boss sendvich paket	Big Boss Sendvich Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "big boss sendvich paket", "name": "Big Boss Sendvich Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.678665+05
bio qurut 8 gr	Bio Qurut 8 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bio qurut 8 gr", "name": "Bio Qurut 8 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.678973+05
bizzar 1kg	Bizzar 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bizzar 1kg", "name": "Bizzar 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.679132+05
bloom baby girl yashil siyoh 120 sht	Bloom Baby Girl Yashil Siyoh 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bloom baby girl yashil siyoh 120 sht", "name": "Bloom Baby Girl Yashil Siyoh 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.679569+05
bloom baby luna 120 sht	Bloom Baby Luna 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bloom baby luna 120 sht", "name": "Bloom Baby Luna 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.679718+05
bloom limon fresh 120 sht	Bloom Limon Fresh 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bloom limon fresh 120 sht", "name": "Bloom Limon Fresh 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.680175+05
bloomy arab night musk perfume 120 sht	Bloomy Arab Night Musk Perfume 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bloomy arab night musk perfume 120 sht", "name": "Bloomy Arab Night Musk Perfume 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.680327+05
bmw pista 100 gr	Bmw Pista 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bmw pista 100 gr", "name": "Bmw Pista 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.680598+05
boba vitamin	Boba Vitamin	Kg		tayyor mahsulot	{"uom": "Kg", "code": "boba vitamin", "name": "Boba Vitamin", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.680754+05
bolsun zamok paket usluga	Bolsun Zamok Paket Usluga	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bolsun zamok paket usluga", "name": "Bolsun Zamok Paket Usluga", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.680885+05
bomba qizil	Bomba Qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bomba qizil", "name": "Bomba Qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.681141+05
bond max	Bond Max	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bond max", "name": "Bond Max", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.681284+05
bondu bio qurt	Bondu Bio Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bondu bio qurt", "name": "Bondu Bio Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.681425+05
bonito zip paket	Bonito Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bonito zip paket", "name": "Bonito Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.681576+05
bonitto kids skoch paket sergili	Bonitto Kids Skoch Paket Sergili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bonitto kids skoch paket sergili", "name": "Bonitto Kids Skoch Paket Sergili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.681716+05
bravo bio qurt paket	Bravo Bio Qurt Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bravo bio qurt paket", "name": "Bravo Bio Qurt Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.682775+05
bravo halodniy payka	Bravo Halodniy Payka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bravo halodniy payka", "name": "Bravo Halodniy Payka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.682941+05
bravo keks kok rang	Bravo Keks Kok Rang	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bravo keks kok rang", "name": "Bravo Keks Kok Rang", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.683097+05
brend pista paket	Brend Pista Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "brend pista paket", "name": "Brend Pista Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.683236+05
bricet ice 90 gr	Bricet Ice 90 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bricet ice 90 gr", "name": "Bricet Ice 90 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.683378+05
brio waﬂe 150 gr orange	Brio Waﬂe 150 Gr Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "brio waﬂe 150 gr orange", "name": "Brio Waﬂe 150 Gr Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.683526+05
brio waﬄe 150 gr xalol orange	Brio Waﬄe 150 Gr Xalol Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "brio waﬄe 150 gr xalol orange", "name": "Brio Waﬄe 150 Gr Xalol Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.683676+05
ezo 2kg rulon	Ezo 2kg Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ezo 2kg rulon", "name": "Ezo 2kg Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.726111+05
Pompik mega pack kukruz kichik rulon	Pompik mega pack kukruz kichik rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pompik mega pack kukruz kichik rulon", "name": "Pompik mega pack kukruz kichik rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.603703+05
ays grim arbuz-dinya 65gr	Ays Grim Arbuz-dinya 65gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ays grim arbuz-dinya 65gr", "name": "Ays Grim Arbuz-dinya 65gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.672963+05
baxt iphone kichkina paket	Baxt Iphone Kichkina Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "baxt iphone kichkina paket", "name": "Baxt Iphone Kichkina Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.676588+05
boom zvachka 3.5 gr	Boom Zvachka 3.5 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "boom zvachka 3.5 gr", "name": "Boom Zvachka 3.5 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.682469+05
bonu gold	Bonu Gold	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bonu gold", "name": "Bonu Gold", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.682171+05
boss tamshan sendvich 190 gr	Boss Tamshan Sendvich 190 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "boss tamshan sendvich 190 gr", "name": "Boss Tamshan Sendvich 190 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.682626+05
bu o'sha qurt	Bu O'sha Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bu o'sha qurt", "name": "Bu O'sha Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.683823+05
bubli parashok paket	Bubli Parashok Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bubli parashok paket", "name": "Bubli Parashok Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.68397+05
buna bio qurt	Buna Bio Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "buna bio qurt", "name": "Buna Bio Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.684104+05
candy love 15 gr	Candy Love 15 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "candy love 15 gr", "name": "Candy Love 15 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.684535+05
candy love 70 gr	Candy Love 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "candy love 70 gr", "name": "Candy Love 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.684675+05
ccc fa plus 130 gr	Ccc Fa Plus 130 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ccc fa plus 130 gr", "name": "Ccc Fa Plus 130 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.684803+05
cekirdik pista 5 kg	Cekirdik Pista 5 Kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cekirdik pista 5 kg", "name": "Cekirdik Pista 5 Kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.684942+05
chamomile 120ta	Chamomile 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chamomile 120ta", "name": "Chamomile 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.685216+05
chaps krab	Chaps Krab	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps krab", "name": "Chaps Krab", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.68535+05
chaps krab 38 sm	Chaps Krab 38 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps krab 38 sm", "name": "Chaps Krab 38 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.68549+05
chaps krab kotta 36 sm	Chaps Krab Kotta 36 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps krab kotta 36 sm", "name": "Chaps Krab Kotta 36 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.68563+05
chaps salyami	Chaps Salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps salyami", "name": "Chaps Salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.685764+05
chaps shalik 38 sm	Chaps Shalik 38 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps shalik 38 sm", "name": "Chaps Shalik 38 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.6859+05
chaps shashlik	Chaps Shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps shashlik", "name": "Chaps Shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.686039+05
chaps tandir kabob 38 sm	Chaps Tandir Kabob 38 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps tandir kabob 38 sm", "name": "Chaps Tandir Kabob 38 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.68744+05
chaq chuq semechka	Chaq Chuq Semechka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaq chuq semechka", "name": "Chaq Chuq Semechka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.687738+05
cheers krab 130 gr 41 sm	Cheers Krab 130 Gr 41 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers krab 130 gr 41 sm", "name": "Cheers Krab 130 Gr 41 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.688651+05
cheers krab 210 gr 46 sm	Cheers Krab 210 Gr 46 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers krab 210 gr 46 sm", "name": "Cheers Krab 210 Gr 46 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.688784+05
cheers krab 45 gr 33 sm	Cheers Krab 45 Gr 33 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers krab 45 gr 33 sm", "name": "Cheers Krab 45 Gr 33 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.688923+05
cheers nachos kalbasa chili 27 gr 31 sm	Cheers Nachos Kalbasa Chili 27 Gr 31 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nachos kalbasa chili 27 gr 31 sm", "name": "Cheers Nachos Kalbasa Chili 27 Gr 31 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.689217+05
cheers nachos kalbaski chili 70 gr 35 sm	Cheers Nachos Kalbaski Chili 70 Gr 35 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nachos kalbaski chili 70 gr 35 sm", "name": "Cheers Nachos Kalbaski Chili 70 Gr 35 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.68951+05
cheers nachos nacho 130 gr 41 sm	Cheers Nachos Nacho 130 Gr 41 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nachos nacho 130 gr 41 sm", "name": "Cheers Nachos Nacho 130 Gr 41 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.689667+05
cheers nachos nacho 210 gr 46 sm	Cheers Nachos Nacho 210 Gr 46 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nachos nacho 210 gr 46 sm", "name": "Cheers Nachos Nacho 210 Gr 46 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.689923+05
cheers nachos nacho 27 gr 31 sm	Cheers Nachos Nacho 27 Gr 31 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nachos nacho 27 gr 31 sm", "name": "Cheers Nachos Nacho 27 Gr 31 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.690082+05
cheers nachos nacho 70 gr 35 sm	Cheers Nachos Nacho 70 Gr 35 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nachos nacho 70 gr 35 sm", "name": "Cheers Nachos Nacho 70 Gr 35 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.690255+05
Qars qurs 15gr chili	Qars qurs 15gr chili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qars qurs 15gr chili", "name": "Qars qurs 15gr chili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.607052+05
bolsun zip paket	Bolsun Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bolsun zip paket", "name": "Bolsun Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.681016+05
bonu shokolad paket	Bonu Shokolad Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bonu shokolad paket", "name": "Bonu Shokolad Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.682301+05
cheers juzon chips	Cheers Juzon Chips	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers juzon chips", "name": "Cheers Juzon Chips", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.688487+05
chaps tandir kabob	Chaps Tandir Kabob	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps tandir kabob", "name": "Chaps Tandir Kabob", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.687299+05
cheers nachos sir 210 gr 46 sm	Cheers Nachos Sir 210 Gr 46 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nachos sir 210 gr 46 sm", "name": "Cheers Nachos Sir 210 Gr 46 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.690401+05
cheers nachos sir 27 gr 31 sm	Cheers Nachos Sir 27 Gr 31 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nachos sir 27 gr 31 sm", "name": "Cheers Nachos Sir 27 Gr 31 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.690532+05
cheers nachos sir 70 gr 35 sm	Cheers Nachos Sir 70 Gr 35 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nachos sir 70 gr 35 sm", "name": "Cheers Nachos Sir 70 Gr 35 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.690674+05
cheers nachos sous salsa 70 gr 35 sm	Cheers Nachos Sous Salsa 70 Gr 35 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nachos sous salsa 70 gr 35 sm", "name": "Cheers Nachos Sous Salsa 70 Gr 35 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.691273+05
cheers steyk burger 45 gr 33 sm	Cheers Steyk Burger 45 Gr 33 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers steyk burger 45 gr 33 sm", "name": "Cheers Steyk Burger 45 Gr 33 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.691831+05
chempion bolajon burger andijon paket	Chempion Bolajon Burger Andijon Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion bolajon burger andijon paket", "name": "Chempion Bolajon Burger Andijon Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.692145+05
chempion donar sendvich andijon	Chempion Donar Sendvich Andijon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion donar sendvich andijon", "name": "Chempion Donar Sendvich Andijon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.693099+05
chempion gamburfer paket tishkent	Chempion Gamburfer Paket Tishkent	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion gamburfer paket tishkent", "name": "Chempion Gamburfer Paket Tishkent", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.693231+05
chempion gamburger andijon	Chempion Gamburger Andijon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion gamburger andijon", "name": "Chempion Gamburger Andijon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.693379+05
chempion gamburger samarqand paket	Chempion Gamburger Samarqand Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion gamburger samarqand paket", "name": "Chempion Gamburger Samarqand Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.693521+05
chempion kalbasa kurka	Chempion Kalbasa Kurka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion kalbasa kurka", "name": "Chempion Kalbasa Kurka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.693662+05
chempion kurka 2X	chempion kurka 2X	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion kurka 2X", "name": "chempion kurka 2X", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.693794+05
chempion qurt	Chempion Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion qurt", "name": "Chempion Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.694071+05
chempion samarqand big sendvich rulon	Chempion Samarqand Big Sendvich Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion samarqand big sendvich rulon", "name": "Chempion Samarqand Big Sendvich Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.694338+05
chempion sendvich andijon 2X	chempion sendvich andijon 2X	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion sendvich andijon 2X", "name": "chempion sendvich andijon 2X", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.694593+05
chempion sendvich paket	Chempion Sendvich Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion sendvich paket", "name": "Chempion Sendvich Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.694728+05
chempion shashlik andijon paket	Chempion Shashlik Andijon Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion shashlik andijon paket", "name": "Chempion Shashlik Andijon Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.694854+05
chempion shashlik sendvich samarqand	Chempion Shashlik Sendvich Samarqand	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion shashlik sendvich samarqand", "name": "Chempion Shashlik Sendvich Samarqand", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.69502+05
chempion shaurma bolajon paket samarqand	Chempion Shaurma Bolajon Paket Samarqand	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion shaurma bolajon paket samarqand", "name": "Chempion Shaurma Bolajon Paket Samarqand", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.695147+05
chikago chipsa	Chikago Chipsa	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chikago chipsa", "name": "Chikago Chipsa", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.696325+05
chinar bio qur paket 4,7	Chinar Bio Qur Paket 4,7	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chinar bio qur paket 4,7", "name": "Chinar Bio Qur Paket 4,7", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.697235+05
pokiza vafelniy plombir	Pokiza Vafelniy Plombir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pokiza vafelniy plombir", "name": "Pokiza Vafelniy Plombir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.955207+05
zizi mazali 960gr paket	Zizi Mazali 960gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi mazali 960gr paket", "name": "Zizi Mazali 960gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.05349+05
Red marmalad askarbinka	Red marmalad askarbinka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Red marmalad askarbinka", "name": "Red marmalad askarbinka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.610823+05
chaps smetana 38 sm	Chaps Smetana 38 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps smetana 38 sm", "name": "Chaps Smetana 38 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.687135+05
chaps tandir kebab 36 sm	Chaps Tandir Kebab 36 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps tandir kebab 36 sm", "name": "Chaps Tandir Kebab 36 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.687593+05
chempion chikken longer andijon	Chempion Chikken Longer Andijon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion chikken longer andijon", "name": "Chempion Chikken Longer Andijon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.692792+05
chempion bolajon toshkent paket	Chempion Bolajon Toshkent Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion bolajon toshkent paket", "name": "Chempion Bolajon Toshkent Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.692451+05
chempion chikken longer samarqand	Chempion Chikken Longer Samarqand	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion chikken longer samarqand", "name": "Chempion Chikken Longer Samarqand", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.692954+05
chempion shourma pita	Chempion Shourma Pita	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion shourma pita", "name": "Chempion Shourma Pita", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.695793+05
chempion toster noni paket	Chempion Toster Noni Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion toster noni paket", "name": "Chempion Toster Noni Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.696048+05
chiki boom jolly mix 40 gr	Chiki Boom Jolly Mix 40 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chiki boom jolly mix 40 gr", "name": "Chiki Boom Jolly Mix 40 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.696654+05
chiki boom kakao 40 gr	Chiki Boom Kakao 40 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chiki boom kakao 40 gr", "name": "Chiki Boom Kakao 40 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.696793+05
chikko mini wafer	Chikko Mini Wafer	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chikko mini wafer", "name": "Chikko Mini Wafer", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.696938+05
chinigi burger paket	Chinigi Burger Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chinigi burger paket", "name": "Chinigi Burger Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.697374+05
chips miks	Chips Miks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chips miks", "name": "Chips Miks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.697527+05
choco plus oq uzor	Choco Plus Oq Uzor	Kg		tayyor mahsulot	{"uom": "Kg", "code": "choco plus oq uzor", "name": "Choco Plus Oq Uzor", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.69766+05
chocofun 25 gr	Chocofun 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chocofun 25 gr", "name": "Chocofun 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.697836+05
chocolate fantasy 65 gr	Chocolate Fantasy 65 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chocolate fantasy 65 gr", "name": "Chocolate Fantasy 65 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.69798+05
chocolate wafer 30 gr orange	Chocolate Wafer 30 Gr Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chocolate wafer 30 gr orange", "name": "Chocolate Wafer 30 Gr Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.698129+05
choko boom cokkies vanil	Choko Boom Cokkies Vanil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "choko boom cokkies vanil", "name": "Choko Boom Cokkies Vanil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.698295+05
choko kendiy keks	Choko Kendiy Keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "choko kendiy keks", "name": "Choko Kendiy Keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.698433+05
choko milk italya 80gr	Choko Milk Italya 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "choko milk italya 80gr", "name": "Choko Milk Italya 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.698582+05
choko pokiza	Choko Pokiza	Kg		tayyor mahsulot	{"uom": "Kg", "code": "choko pokiza", "name": "Choko Pokiza", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.69873+05
choko rols asartiy yangi	Choko Rols Asartiy Yangi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "choko rols asartiy yangi", "name": "Choko Rols Asartiy Yangi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.69887+05
choko sendvich 110 gr	Choko Sendvich 110 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "choko sendvich 110 gr", "name": "Choko Sendvich 110 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.699015+05
chokolate 500gr paket	Chokolate 500gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chokolate 500gr paket", "name": "Chokolate 500gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.699192+05
chop 7.5sm teshigli	Chop 7.5sm Teshigli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chop 7.5sm teshigli", "name": "Chop 7.5sm Teshigli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.699472+05
chop 7sm	Chop 7sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chop 7sm", "name": "Chop 7sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.699655+05
chop 7sm teshikli	Chop 7sm Teshikli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chop 7sm teshikli", "name": "Chop 7sm Teshikli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.699817+05
chop 8,5sm teshigli	Chop 8,5sm Teshigli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chop 8,5sm teshigli", "name": "Chop 8,5sm Teshigli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.699984+05
chop 8sm teshigli	Chop 8sm Teshigli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chop 8sm teshigli", "name": "Chop 8sm Teshigli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.700153+05
choy paket 30/61	Choy Paket 30/61	Kg		tayyor mahsulot	{"uom": "Kg", "code": "choy paket 30/61", "name": "Choy Paket 30/61", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.700289+05
Jem 960/25	JEM 960/25	Kg		homashyo	{"uom": "Kg", "code": "Jem 960/25", "name": "JEM 960/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.545008+05
Mega Super pista	Mega Super pista	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mega Super pista", "name": "Mega Super pista", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.576756+05
chempion burger 5 dona paket	Chempion Burger 5 Dona Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion burger 5 dona paket", "name": "Chempion Burger 5 Dona Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.692632+05
chempion bolajon chikken longer samarqand	Chempion Bolajon Chikken Longer Samarqand	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion bolajon chikken longer samarqand", "name": "Chempion Bolajon Chikken Longer Samarqand", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.692302+05
Xams	Xams	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xams", "name": "Xams", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.643704+05
chudo samara 70 gr	Chudo Samara 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chudo samara 70 gr", "name": "Chudo Samara 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.700722+05
collagen asarty	Collagen Asarty	Kg		tayyor mahsulot	{"uom": "Kg", "code": "collagen asarty", "name": "Collagen Asarty", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.701577+05
cotton jidkiy aboy	Cotton Jidkiy Aboy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cotton jidkiy aboy", "name": "Cotton Jidkiy Aboy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.701868+05
cresta zip paket	Cresta Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cresta zip paket", "name": "Cresta Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.705212+05
crisp-x shokolad 70 gr 35 sm	Crisp-x Shokolad 70 Gr 35 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "crisp-x shokolad 70 gr 35 sm", "name": "Crisp-x Shokolad 70 Gr 35 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.705688+05
cross bio kurt ziyo	Cross Bio Kurt Ziyo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cross bio kurt ziyo", "name": "Cross Bio Kurt Ziyo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.705824+05
cross bio qurt	Cross Bio Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cross bio qurt", "name": "Cross Bio Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.705977+05
crunch gofret	Crunch Gofret	Kg		tayyor mahsulot	{"uom": "Kg", "code": "crunch gofret", "name": "Crunch Gofret", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.706129+05
dala qurt paket	Dala Qurt Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dala qurt paket", "name": "Dala Qurt Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.706268+05
damashniy rulet asartiy 80gr	Damashniy Rulet Asartiy 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "damashniy rulet asartiy 80gr", "name": "Damashniy Rulet Asartiy 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.706403+05
day salfetka 72 sht qizil kok	Day Salfetka 72 Sht Qizil Kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "day salfetka 72 sht qizil kok", "name": "Day Salfetka 72 Sht Qizil Kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.707122+05
day salfetka 72 sht ramashka	Day Salfetka 72 Sht Ramashka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "day salfetka 72 sht ramashka", "name": "Day Salfetka 72 Sht Ramashka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.707262+05
day salfetka aloe vera 15 sht	Day Salfetka Aloe Vera 15 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "day salfetka aloe vera 15 sht", "name": "Day Salfetka Aloe Vera 15 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.707578+05
day salfetka aloe vera 72 sht	Day Salfetka Aloe Vera 72 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "day salfetka aloe vera 72 sht", "name": "Day Salfetka Aloe Vera 72 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.707717+05
delicao paket	Delicao Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "delicao paket", "name": "Delicao Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.707848+05
delis 250gr halol qizil	Delis 250gr Halol Qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "delis 250gr halol qizil", "name": "Delis 250gr Halol Qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.707989+05
delis 250gr sariq	Delis 250gr Sariq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "delis 250gr sariq", "name": "Delis 250gr Sariq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.708119+05
delis 250gr yashil xalol	Delis 250gr Yashil Xalol	Kg		tayyor mahsulot	{"uom": "Kg", "code": "delis 250gr yashil xalol", "name": "Delis 250gr Yashil Xalol", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.708258+05
delis 300gr malochniy	Delis 300gr Malochniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "delis 300gr malochniy", "name": "Delis 300gr Malochniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.70842+05
delis 300gr qora	Delis 300gr Qora	Kg		tayyor mahsulot	{"uom": "Kg", "code": "delis 300gr qora", "name": "Delis 300gr Qora", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.708586+05
delis 450gr malochniy	Delis 450gr Malochniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "delis 450gr malochniy", "name": "Delis 450gr Malochniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.70874+05
detiskiy pecheniy paket 180g	Detiskiy Pecheniy Paket 180g	Kg		tayyor mahsulot	{"uom": "Kg", "code": "detiskiy pecheniy paket 180g", "name": "Detiskiy Pecheniy Paket 180g", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.709406+05
dey salfetka jasmin 120ta	Dey Salfetka Jasmin 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dey salfetka jasmin 120ta", "name": "Dey Salfetka Jasmin 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.709736+05
guruch uzun nonli pokiston 2kg	Guruch Uzun Nonli Pokiston 2kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "guruch uzun nonli pokiston 2kg", "name": "Guruch Uzun Nonli Pokiston 2kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.742002+05
polyot	Polyot	Kg		tayyor mahsulot	{"uom": "Kg", "code": "polyot", "name": "Polyot", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.955364+05
Jem 975/20	JEM 975/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 975/20", "name": "JEM 975/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.545349+05
Purmilkiy tvorg zernistiy 9% 400 gr orange	Purmilkiy tvorg zernistiy 9% 400 gr orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Purmilkiy tvorg zernistiy 9% 400 gr orange", "name": "Purmilkiy tvorg zernistiy 9% 400 gr orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.606348+05
alif zvachka	Alif Zvachka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "alif zvachka", "name": "Alif Zvachka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.667255+05
chuda samara kivi 70 gr	Chuda Samara Kivi 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chuda samara kivi 70 gr", "name": "Chuda Samara Kivi 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.700566+05
Toretto golden dak sir	Toretto golden dak sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto golden dak sir", "name": "Toretto golden dak sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.634077+05
chudo samara slevaya vishnya	Chudo Samara Slevaya Vishnya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chudo samara slevaya vishnya", "name": "Chudo Samara Slevaya Vishnya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.701015+05
cosmos sovun	Cosmos Sovun	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cosmos sovun", "name": "Cosmos Sovun", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.701713+05
cpp / 20 mikron / 600	CPP 20/600	Kg		homashyo	{"uom": "Kg", "code": "cpp / 20 mikron / 600", "name": "CPP 20/600", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.702028+05
crisp-x shokolad 20 gr 31 sm	Crisp-x Shokolad 20 Gr 31 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "crisp-x shokolad 20 gr 31 sm", "name": "Crisp-x Shokolad 20 Gr 31 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.705531+05
deya apachi 900 gr	Deya Apachi 900 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "deya apachi 900 gr", "name": "Deya Apachi 900 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.709875+05
deya vilo dark qizil	Deya Vilo Dark Qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "deya vilo dark qizil", "name": "Deya Vilo Dark Qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.710568+05
diamond	Diamond	Kg		tayyor mahsulot	{"uom": "Kg", "code": "diamond", "name": "Diamond", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.710837+05
dida paket	Dida Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dida paket", "name": "Dida Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.710976+05
diizi max 80 gr	Diizi Max 80 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "diizi max 80 gr", "name": "Diizi Max 80 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.711682+05
dixi falga	Dixi Falga	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dixi falga", "name": "Dixi Falga", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.712684+05
dizzi max	Dizzi Max	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dizzi max", "name": "Dizzi Max", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.712823+05
dl-bi paket	Dl-bi Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dl-bi paket", "name": "Dl-bi Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.712962+05
doda cherry twist 80 gr	Doda Cherry Twist 80 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "doda cherry twist 80 gr", "name": "Doda Cherry Twist 80 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.713097+05
doda chuchvara 500 gr	Doda Chuchvara 500 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "doda chuchvara 500 gr", "name": "Doda Chuchvara 500 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.713357+05
dodo 140g qulupnay	Dodo 140g Qulupnay	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dodo 140g qulupnay", "name": "Dodo 140g Qulupnay", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.71392+05
dodo 140g vishna	Dodo 140g Vishna	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dodo 140g vishna", "name": "Dodo 140g Vishna", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.714067+05
dolce cake	Dolce Cake	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dolce cake", "name": "Dolce Cake", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.714199+05
dollar 100 tvist	Dollar 100 Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dollar 100 tvist", "name": "Dollar 100 Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.714353+05
don don 20g 9D-3D	don don 20g 9D-3D	Kg		tayyor mahsulot	{"uom": "Kg", "code": "don don 20g 9D-3D", "name": "don don 20g 9D-3D", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.714472+05
don don dastirxon 24sm	Don Don Dastirxon 24sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "don don dastirxon 24sm", "name": "Don Don Dastirxon 24sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.714751+05
don don suxariklar	Don Don Suxariklar	Kg		tayyor mahsulot	{"uom": "Kg", "code": "don don suxariklar", "name": "Don Don Suxariklar", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.715025+05
dona mazali pista	Dona Mazali Pista	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dona mazali pista", "name": "Dona Mazali Pista", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.715296+05
dona mazali pista oq	Dona Mazali Pista Oq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dona mazali pista oq", "name": "Dona Mazali Pista Oq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.715426+05
dona puﬀed	Dona Puﬀed	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dona puﬀed", "name": "Dona Puﬀed", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.715686+05
doni kurt paket 20 sht	Doni Kurt Paket 20 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "doni kurt paket 20 sht", "name": "Doni Kurt Paket 20 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.715952+05
doni pista 2 kg paket	Doni Pista 2 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "doni pista 2 kg paket", "name": "Doni Pista 2 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.716088+05
doni qurt 6gr yangi	Doni Qurt 6gr Yangi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "doni qurt 6gr yangi", "name": "Doni Qurt 6gr Yangi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.716228+05
frutti marojniy ali bobo	Frutti Marojniy Ali Bobo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "frutti marojniy ali bobo", "name": "Frutti Marojniy Ali Bobo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.731926+05
TWIN parasho'k paket	TWIN parasho'k paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "TWIN parasho'k paket", "name": "TWIN parasho'k paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.628107+05
Zo'r chips 5D smetana	Zo'r chips 5D smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zo'r chips 5D smetana", "name": "Zo'r chips 5D smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.662646+05
chudo samara tropik 70 gr	Chudo Samara Tropik 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chudo samara tropik 70 gr", "name": "Chudo Samara Tropik 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.701152+05
coconut fantasy 65 gr	Coconut Fantasy 65 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "coconut fantasy 65 gr", "name": "Coconut Fantasy 65 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.701293+05
deya vilo coconut kok	Deya Vilo Coconut Kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "deya vilo coconut kok", "name": "Deya Vilo Coconut Kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.710434+05
deya prazrachka	Deya Prazrachka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "deya prazrachka", "name": "Deya Prazrachka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.710302+05
deya zoopark	Deya Zoopark	Kg		tayyor mahsulot	{"uom": "Kg", "code": "deya zoopark", "name": "Deya Zoopark", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.710702+05
diddo shokolad	Diddo Shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "diddo shokolad", "name": "Diddo Shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.711131+05
doni qurt prazrachka	Doni Qurt Prazrachka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "doni qurt prazrachka", "name": "Doni Qurt Prazrachka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.716362+05
doniy pista 90gr	Doniy Pista 90gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "doniy pista 90gr", "name": "Doniy Pista 90gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.716771+05
dora izabella	Dora Izabella	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dora izabella", "name": "Dora Izabella", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.717665+05
dora ribbon	Dora Ribbon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dora ribbon", "name": "Dora Ribbon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.717802+05
duesl pf 5 sm	Duesl Pf 5 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "duesl pf 5 sm", "name": "Duesl Pf 5 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.717931+05
dusel 5-1 m paket	Dusel 5-1 M Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dusel 5-1 m paket", "name": "Dusel 5-1 M Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.718214+05
dusel elektrical qizil paket	Dusel Elektrical Qizil Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dusel elektrical qizil paket", "name": "Dusel Elektrical Qizil Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.718338+05
dusel pf 12 sm	Dusel Pf 12 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dusel pf 12 sm", "name": "Dusel Pf 12 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.718467+05
dusel pf 4 sm	Dusel Pf 4 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dusel pf 4 sm", "name": "Dusel Pf 4 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.718591+05
dusel prazrachka	Dusel Prazrachka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dusel prazrachka", "name": "Dusel Prazrachka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.718857+05
dusel premium paket	Dusel Premium Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dusel premium paket", "name": "Dusel Premium Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.718999+05
dusel premium product 22 sm	Dusel Premium Product 22 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dusel premium product 22 sm", "name": "Dusel Premium Product 22 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.719134+05
dusel qizil oq	Dusel Qizil Oq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dusel qizil oq", "name": "Dusel Qizil Oq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.719274+05
dusel rich	Dusel Rich	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dusel rich", "name": "Dusel Rich", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.719411+05
dusel rich 25sm	Dusel Rich 25sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dusel rich 25sm", "name": "Dusel Rich 25sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.71954+05
eco premium for baby 15+2 sht	Eco Premium For Baby 15+2 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "eco premium for baby 15+2 sht", "name": "Eco Premium For Baby 15+2 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.719804+05
eco salfaetka aroma parfum 20 sht	Eco Salfaetka Aroma Parfum 20 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "eco salfaetka aroma parfum 20 sht", "name": "Eco Salfaetka Aroma Parfum 20 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.720014+05
eko salfetka 72 sht ekstra	Eko Salfetka 72 Sht Ekstra	Kg		tayyor mahsulot	{"uom": "Kg", "code": "eko salfetka 72 sht ekstra", "name": "Eko Salfetka 72 Sht Ekstra", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.72055+05
eko salfetka Aloe 120ta	eko salfetka Aloe 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "eko salfetka Aloe 120ta", "name": "eko salfetka Aloe 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.720691+05
eko salfetka aroma parfum 72 ta	Eko Salfetka Aroma Parfum 72 Ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "eko salfetka aroma parfum 72 ta", "name": "Eko Salfetka Aroma Parfum 72 Ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.72083+05
eko salfetka aroma parfum grim 120 sht	Eko Salfetka Aroma Parfum Grim 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "eko salfetka aroma parfum grim 120 sht", "name": "Eko Salfetka Aroma Parfum Grim 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.720968+05
ganga grand marojniy 120gr	Ganga Grand Marojniy 120gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ganga grand marojniy 120gr", "name": "Ganga Grand Marojniy 120gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.733649+05
jem 575/30 pol	JEM 575/30	Kg		homashyo	{"uom": "Kg", "code": "jem 575/30 pol", "name": "JEM 575/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.764109+05
realniy plombir voronaya sgoshonka	Realniy Plombir Voronaya Sgoshonka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "realniy plombir voronaya sgoshonka", "name": "Realniy Plombir Voronaya Sgoshonka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.968857+05
Pelmeni paket 1kg	Pelmeni paket 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pelmeni paket 1kg", "name": "Pelmeni paket 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.598114+05
deya ketler kurasan malina 6 sht	Deya Ketler Kurasan Malina 6 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "deya ketler kurasan malina 6 sht", "name": "Deya Ketler Kurasan Malina 6 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.710016+05
deya ketler kurasan shokolad 6 sht	Deya Ketler Kurasan Shokolad 6 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "deya ketler kurasan shokolad 6 sht", "name": "Deya Ketler Kurasan Shokolad 6 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.710162+05
dodo 140g malina	Dodo 140g Malina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dodo 140g malina", "name": "Dodo 140g Malina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.713772+05
don milko paket	Don Milko Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "don milko paket", "name": "Don Milko Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.715166+05
doni jvachka asartiy	Doni Jvachka Asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "doni jvachka asartiy", "name": "Doni Jvachka Asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.715817+05
donkids 300gr	Donkids 300gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "donkids 300gr", "name": "Donkids 300gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.716899+05
eko salfetka ekstra 120ta	Eko Salfetka Ekstra 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "eko salfetka ekstra 120ta", "name": "Eko Salfetka Ekstra 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.7211+05
eko salfetka for baby 15ta	Eko Salfetka For Baby 15ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "eko salfetka for baby 15ta", "name": "Eko Salfetka For Baby 15ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.721231+05
elite futbolka L	elite futbolka L	Kg		tayyor mahsulot	{"uom": "Kg", "code": "elite futbolka L", "name": "elite futbolka L", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.722451+05
elitex fubolka M	elitex fubolka M	Kg		tayyor mahsulot	{"uom": "Kg", "code": "elitex fubolka M", "name": "elitex fubolka M", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.7226+05
elitex futbolka 3xl	Elitex Futbolka 3xl	Kg		tayyor mahsulot	{"uom": "Kg", "code": "elitex futbolka 3xl", "name": "Elitex Futbolka 3xl", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.72275+05
elitex futbolka XL	elitex futbolka XL	Kg		tayyor mahsulot	{"uom": "Kg", "code": "elitex futbolka XL", "name": "elitex futbolka XL", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.722894+05
elitex futbolka XXL	elitex futbolka XXL	Kg		tayyor mahsulot	{"uom": "Kg", "code": "elitex futbolka XXL", "name": "elitex futbolka XXL", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.723038+05
elitex mujskoy futbolka zip paket	Elitex Mujskoy Futbolka Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "elitex mujskoy futbolka zip paket", "name": "Elitex Mujskoy Futbolka Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.723582+05
elitex mujskoy mayka paket	Elitex Mujskoy Mayka Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "elitex mujskoy mayka paket", "name": "Elitex Mujskoy Mayka Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.723729+05
elitex svitshot mujskoy zip paket	Elitex Svitshot Mujskoy Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "elitex svitshot mujskoy zip paket", "name": "Elitex Svitshot Mujskoy Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.723887+05
elma kok oq 1 sht	Elma Kok Oq 1 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "elma kok oq 1 sht", "name": "Elma Kok Oq 1 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.724031+05
elﬁda tvist	Elﬁda Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "elﬁda tvist", "name": "Elﬁda Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.724328+05
enear kapsula zip paket	Enear Kapsula Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "enear kapsula zip paket", "name": "Enear Kapsula Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.724454+05
enerjiy +77	Enerjiy +77	Kg		tayyor mahsulot	{"uom": "Kg", "code": "enerjiy +77", "name": "Enerjiy +77", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.724603+05
erkatoy 65 gr	Erkatoy 65 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "erkatoy 65 gr", "name": "Erkatoy 65 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.724742+05
estello shokolad asarty	Estello Shokolad Asarty	Kg		tayyor mahsulot	{"uom": "Kg", "code": "estello shokolad asarty", "name": "Estello Shokolad Asarty", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.724876+05
etalon 1 kechki ovqat	Etalon 1 Kechki Ovqat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "etalon 1 kechki ovqat", "name": "Etalon 1 Kechki Ovqat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.725+05
etalon 2 kechki ovqat	Etalon 2 Kechki Ovqat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "etalon 2 kechki ovqat", "name": "Etalon 2 Kechki Ovqat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.725125+05
etalon nonushta 1	Etalon Nonushta 1	Kg		tayyor mahsulot	{"uom": "Kg", "code": "etalon nonushta 1", "name": "Etalon Nonushta 1", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.725268+05
etalon nonushta 2	Etalon Nonushta 2	Kg		tayyor mahsulot	{"uom": "Kg", "code": "etalon nonushta 2", "name": "Etalon Nonushta 2", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.725403+05
etalon tushlik 1 paket	Etalon Tushlik 1 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "etalon tushlik 1 paket", "name": "Etalon Tushlik 1 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.725541+05
etalon tushlik 2 paket	Etalon Tushlik 2 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "etalon tushlik 2 paket", "name": "Etalon Tushlik 2 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.725688+05
euro decor aboy nozim	Euro Decor Aboy Nozim	Kg		tayyor mahsulot	{"uom": "Kg", "code": "euro decor aboy nozim", "name": "Euro Decor Aboy Nozim", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.725822+05
ever see zip paket usluga	Ever See Zip Paket Usluga	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ever see zip paket usluga", "name": "Ever See Zip Paket Usluga", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.725968+05
jem 580/25 kar	JEM 580/25	Kg		homashyo	{"uom": "Kg", "code": "jem 580/25 kar", "name": "JEM 580/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.764279+05
RZ 510/25	RZ 510/25	Kg		tayyor mahsulot	{"uom": "Kg", "code": "RZ 510/25", "name": "RZ 510/25", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.610109+05
divino tvist	Divino Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "divino tvist", "name": "Divino Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.712399+05
diron tekistil	Diron Tekistil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "diron tekistil", "name": "Diron Tekistil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.712259+05
ekstra suharik 50gr	Ekstra Suharik 50gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ekstra suharik 50gr", "name": "Ekstra Suharik 50gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.722028+05
elemant frukta	Elemant Frukta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "elemant frukta", "name": "Elemant Frukta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.722167+05
elemant mix yangi	Elemant Mix Yangi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "elemant mix yangi", "name": "Elemant Mix Yangi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.722299+05
fayz duk duk	Fayz Duk Duk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fayz duk duk", "name": "Fayz Duk Duk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.726643+05
fayz tabiy qurt paket	Fayz Tabiy Qurt Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fayz tabiy qurt paket", "name": "Fayz Tabiy Qurt Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.727036+05
ferrel klubnika	Ferrel Klubnika	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ferrel klubnika", "name": "Ferrel Klubnika", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.727866+05
ferrel shokolad	Ferrel Shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ferrel shokolad", "name": "Ferrel Shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.728005+05
ferrel sugishonka	Ferrel Sugishonka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ferrel sugishonka", "name": "Ferrel Sugishonka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.728138+05
flexo cola	Flexo Cola	Kg		tayyor mahsulot	{"uom": "Kg", "code": "flexo cola", "name": "Flexo Cola", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.72828+05
fnp chaynie lojki 50 sht paket	Fnp Chaynie Lojki 50 Sht Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fnp chaynie lojki 50 sht paket", "name": "Fnp Chaynie Lojki 50 Sht Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.728406+05
fnp lojki 12 sht	Fnp Lojki 12 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fnp lojki 12 sht", "name": "Fnp Lojki 12 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.728551+05
fnp ﬁlki 12 sht	Fnp Filki 12 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fnp ﬁlki 12 sht", "name": "Fnp Filki 12 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.728694+05
form coﬀee 2 kg paket	Form Coﬀee 2 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "form coﬀee 2 kg paket", "name": "Form Coﬀee 2 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.728834+05
form coﬀee 20 gr	Form Coﬀee 20 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "form coﬀee 20 gr", "name": "Form Coﬀee 20 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.728973+05
forux jele	Forux Jele	Kg		tayyor mahsulot	{"uom": "Kg", "code": "forux jele", "name": "Forux Jele", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.729104+05
fozilov araxis karamel 120 gr	Fozilov Araxis Karamel 120 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fozilov araxis karamel 120 gr", "name": "Fozilov Araxis Karamel 120 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.729359+05
fozilov dubai ﬁstashka 100 gr	Fozilov Dubai Fistashka 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fozilov dubai ﬁstashka 100 gr", "name": "Fozilov Dubai Fistashka 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.72952+05
fozilov ice cream 120 gr	Fozilov Ice Cream 120 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fozilov ice cream 120 gr", "name": "Fozilov Ice Cream 120 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.729667+05
fozilov plambir 80 gr	Fozilov Plambir 80 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fozilov plambir 80 gr", "name": "Fozilov Plambir 80 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.729955+05
frade tvist	Frade Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "frade tvist", "name": "Frade Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.730093+05
frendo imperator	Frendo Imperator	Kg		tayyor mahsulot	{"uom": "Kg", "code": "frendo imperator", "name": "Frendo Imperator", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.730259+05
frendo imperator miks	Frendo Imperator Miks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "frendo imperator miks", "name": "Frendo Imperator Miks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.730398+05
fresgboll chop 8 sm	Fresgboll Chop 8 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fresgboll chop 8 sm", "name": "Fresgboll Chop 8 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.730538+05
fresh ones yagona 72 sht	Fresh Ones Yagona 72 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fresh ones yagona 72 sht", "name": "Fresh Ones Yagona 72 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.730683+05
freshboll metal paket	Freshboll Metal Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "freshboll metal paket", "name": "Freshboll Metal Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.730842+05
frozen kukruz	Frozen Kukruz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "frozen kukruz", "name": "Frozen Kukruz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.731259+05
frustick kivi	Frustick Kivi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "frustick kivi", "name": "Frustick Kivi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.731668+05
ganga enerji 80gr	Ganga Enerji 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ganga enerji 80gr", "name": "Ganga Enerji 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.733489+05
gold 50 gr	Gold 50 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gold 50 gr", "name": "Gold 50 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.735915+05
jem 600/35 pol	JEM 600/35	Kg		homashyo	{"uom": "Kg", "code": "jem 600/35 pol", "name": "JEM 600/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.764442+05
Shoxasar mini vaﬂi 250gr shokolad	Shoxasar mini vaﬂi 250gr shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shoxasar mini vaﬂi 250gr shokolad", "name": "Shoxasar mini vaﬂi 250gr shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.617885+05
ekstra suharik 20gr	Ekstra Suharik 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ekstra suharik 20gr", "name": "Ekstra Suharik 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.721886+05
ferrel apelsin	Ferrel Apelsin	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ferrel apelsin", "name": "Ferrel Apelsin", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.727576+05
freshboom asarty	Freshboom Asarty	Kg		tayyor mahsulot	{"uom": "Kg", "code": "freshboom asarty", "name": "Freshboom Asarty", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.731118+05
frukto lyod 80 gr	Frukto Lyod 80 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "frukto lyod 80 gr", "name": "Frukto Lyod 80 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.731386+05
funchuza 150 gr paket	Funchuza 150 Gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "funchuza 150 gr paket", "name": "Funchuza 150 Gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.732077+05
funchuza 200 gr paket	Funchuza 200 Gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "funchuza 200 gr paket", "name": "Funchuza 200 Gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.732218+05
funchuza 300 gr rulon	Funchuza 300 Gr Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "funchuza 300 gr rulon", "name": "Funchuza 300 Gr Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.732357+05
gambus 3d	Gambus 3d	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gambus 3d", "name": "Gambus 3d", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.732786+05
gambus 5D kotta	gambus 5D kotta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gambus 5D kotta", "name": "gambus 5D kotta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.732923+05
gambus 5d	Gambus 5d	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gambus 5d", "name": "Gambus 5d", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.733205+05
ganga daf marojniy	Ganga Daf Marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ganga daf marojniy", "name": "Ganga Daf Marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.733339+05
ganga marojniy arbuz+dinya	Ganga Marojniy Arbuz+dinya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ganga marojniy arbuz+dinya", "name": "Ganga Marojniy Arbuz+dinya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.73382+05
ganga plombir 60gr	Ganga Plombir 60gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ganga plombir 60gr", "name": "Ganga Plombir 60gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.733963+05
ganga super rajok 120gr	Ganga Super Rajok 120gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ganga super rajok 120gr", "name": "Ganga Super Rajok 120gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.73412+05
gazzak semechka 180 gr zip paket	Gazzak Semechka 180 Gr Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gazzak semechka 180 gr zip paket", "name": "Gazzak Semechka 180 Gr Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.734272+05
gazzak tuzlangan pista 90 gr	Gazzak Tuzlangan Pista 90 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gazzak tuzlangan pista 90 gr", "name": "Gazzak Tuzlangan Pista 90 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.734424+05
gektar karzinka sariq kartoshka 2,5kg paket	Gektar Karzinka Sariq Kartoshka 2,5kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gektar karzinka sariq kartoshka 2,5kg paket", "name": "Gektar Karzinka Sariq Kartoshka 2,5kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.73471+05
gelato 1 kg paket	Gelato 1 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gelato 1 kg paket", "name": "Gelato 1 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.73486+05
gerkules burger paket	Gerkules Burger Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gerkules burger paket", "name": "Gerkules Burger Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.735015+05
gerkules sendvich bolajpn	Gerkules Sendvich Bolajpn	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gerkules sendvich bolajpn", "name": "Gerkules Sendvich Bolajpn", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.735162+05
gerkules sendvich paket	Gerkules Sendvich Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gerkules sendvich paket", "name": "Gerkules Sendvich Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.735312+05
giza choko	Giza Choko	Kg		tayyor mahsulot	{"uom": "Kg", "code": "giza choko", "name": "Giza Choko", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.735459+05
giza vanil	Giza Vanil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "giza vanil", "name": "Giza Vanil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.735607+05
gofret wafers	Gofret Wafers	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gofret wafers", "name": "Gofret Wafers", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.735747+05
gold food xot-dog	Gold Food Xot-dog	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gold food xot-dog", "name": "Gold Food Xot-dog", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.736538+05
gold karamel xalodniy payka	Gold Karamel Xalodniy Payka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gold karamel xalodniy payka", "name": "Gold Karamel Xalodniy Payka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.736694+05
golden 70 gr	Golden 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "golden 70 gr", "name": "Golden 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.736993+05
grand 120g kok	Grand 120g Kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "grand 120g kok", "name": "Grand 120g Kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.738333+05
green cake	Green Cake	Kg		tayyor mahsulot	{"uom": "Kg", "code": "green cake", "name": "Green Cake", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.73882+05
guruch alanga 1kg	Guruch Alanga 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "guruch alanga 1kg", "name": "Guruch Alanga 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.740769+05
Yupqa lavash	Yupqa lavash	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yupqa lavash", "name": "Yupqa lavash", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.656697+05
feed up vlajniy salfetka	Feed Up Vlajniy Salfetka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "feed up vlajniy salfetka", "name": "Feed Up Vlajniy Salfetka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.72745+05
ferrel banan	Ferrel Banan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ferrel banan", "name": "Ferrel Banan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.727708+05
golden corn paket	Golden Corn Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "golden corn paket", "name": "Golden Corn Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.737166+05
funchuza 400 gr rulon	Funchuza 400 Gr Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "funchuza 400 gr rulon", "name": "Funchuza 400 Gr Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.732492+05
good luck 60gr	Good Luck 60gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "good luck 60gr", "name": "Good Luck 60gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.737519+05
good luck 65gr	Good Luck 65gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "good luck 65gr", "name": "Good Luck 65gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.7377+05
gost + sssr 100 gr	Gost + Sssr 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gost + sssr 100 gr", "name": "Gost + Sssr 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.737878+05
graf miks jitkiy aboy paket	Graf Miks Jitkiy Aboy Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "graf miks jitkiy aboy paket", "name": "Graf Miks Jitkiy Aboy Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.738181+05
grand 120g yashil	Grand 120g Yashil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "grand 120g yashil", "name": "Grand 120g Yashil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.738486+05
grenki molodoy chesnok 100 gr	Grenki Molodoy Chesnok 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "grenki molodoy chesnok 100 gr", "name": "Grenki Molodoy Chesnok 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.739002+05
grenki nasir asartiy	Grenki Nasir Asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "grenki nasir asartiy", "name": "Grenki Nasir Asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.739156+05
grenki salyami neapolitano 60 gr	Grenki Salyami Neapolitano 60 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "grenki salyami neapolitano 60 gr", "name": "Grenki Salyami Neapolitano 60 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.739313+05
grenki telyatina s adjikoy 180 gr	Grenki Telyatina S Adjikoy 180 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "grenki telyatina s adjikoy 180 gr", "name": "Grenki Telyatina S Adjikoy 180 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.739642+05
grenki tomat spaysi 180 gr	Grenki Tomat Spaysi 180 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "grenki tomat spaysi 180 gr", "name": "Grenki Tomat Spaysi 180 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.739803+05
grenki vyalenaya konina 100 gr	Grenki Vyalenaya Konina 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "grenki vyalenaya konina 100 gr", "name": "Grenki Vyalenaya Konina 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.740047+05
grenki vyalenaya konina 180 gr	Grenki Vyalenaya Konina 180 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "grenki vyalenaya konina 180 gr", "name": "Grenki Vyalenaya Konina 180 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.740293+05
grenki vyalenaya konina 60 gr	Grenki Vyalenaya Konina 60 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "grenki vyalenaya konina 60 gr", "name": "Grenki Vyalenaya Konina 60 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.740446+05
gummy world	Gummy World	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gummy world", "name": "Gummy World", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.740601+05
guruch alanga 2kg	Guruch Alanga 2kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "guruch alanga 2kg", "name": "Guruch Alanga 2kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.740915+05
guruch basmati	Guruch Basmati	Kg		tayyor mahsulot	{"uom": "Kg", "code": "guruch basmati", "name": "Guruch Basmati", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.741054+05
guruch lazer 1kg	Guruch Lazer 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "guruch lazer 1kg", "name": "Guruch Lazer 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.741209+05
gusto vaﬂi moloko 250 gr	Gusto Vaﬂi Moloko 250 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gusto vaﬂi moloko 250 gr", "name": "Gusto Vaﬂi Moloko 250 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.742329+05
gusto vaﬂi shokoload 25 gr	Gusto Vaﬂi Shokoload 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gusto vaﬂi shokoload 25 gr", "name": "Gusto Vaﬂi Shokoload 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.742493+05
hamroh semechka 20ta paket	Hamroh Semechka 20ta Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "hamroh semechka 20ta paket", "name": "Hamroh Semechka 20ta Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.743186+05
havas shakar 1 kg imperial	Havas Shakar 1 Kg Imperial	Kg		tayyor mahsulot	{"uom": "Kg", "code": "havas shakar 1 kg imperial", "name": "Havas Shakar 1 Kg Imperial", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.745317+05
husafa comfort kids 120 sht	Husafa Comfort Kids 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "husafa comfort kids 120 sht", "name": "Husafa Comfort Kids 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.746146+05
husfa salfetka merru mevali asarty 72 sht japan	Husfa Salfetka Merru Mevali Asarty 72 Sht Japan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "husfa salfetka merru mevali asarty 72 sht japan", "name": "Husfa Salfetka Merru Mevali Asarty 72 Sht Japan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.746299+05
pista 20gr kok	Pista 20gr Kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pista 20gr kok", "name": "Pista 20gr Kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.952292+05
apetito semechki 5 kg paket	Apetito Semechki 5 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "apetito semechki 5 kg paket", "name": "Apetito Semechki 5 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.669265+05
cpp 590/40	CPP 590/40	Kg		homashyo	{"uom": "Kg", "code": "cpp 590/40", "name": "CPP 590/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.703277+05
gambus 3D kotta	gambus 3D kotta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gambus 3D kotta", "name": "gambus 3D kotta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.732646+05
gold keks asartiy	Gold Keks Asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gold keks asartiy", "name": "Gold Keks Asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.736846+05
guruch oqshoq	Guruch Oqshoq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "guruch oqshoq", "name": "Guruch Oqshoq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.741677+05
gusto vaﬂi funduk 250 gr	Gusto Vaﬂi Funduk 250 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gusto vaﬂi funduk 250 gr", "name": "Gusto Vaﬂi Funduk 250 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.742164+05
halva	Halva	Kg		tayyor mahsulot	{"uom": "Kg", "code": "halva", "name": "Halva", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.742671+05
halva paket	Halva Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "halva paket", "name": "Halva Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.742843+05
hamroh semechka 10 gr	Hamroh Semechka 10 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "hamroh semechka 10 gr", "name": "Hamroh Semechka 10 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.743033+05
havas bulg'ur 900gr	Havas Bulg'ur 900gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "havas bulg'ur 900gr", "name": "Havas Bulg'ur 900gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.74462+05
havas grechka 1kg	Havas Grechka 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "havas grechka 1kg", "name": "Havas Grechka 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.744817+05
havas praz/niy 38,2 sm	Havas Praz/niy 38,2 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "havas praz/niy 38,2 sm", "name": "Havas Praz/niy 38,2 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.744971+05
havas prazrachniy 59.5 sm	Havas Prazrachniy 59.5 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "havas prazrachniy 59.5 sm", "name": "Havas Prazrachniy 59.5 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.745131+05
havas shakar 2 kg kok	Havas Shakar 2 Kg Kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "havas shakar 2 kg kok", "name": "Havas Shakar 2 Kg Kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.745476+05
havas shakar 2kg	Havas Shakar 2kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "havas shakar 2kg", "name": "Havas Shakar 2kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.745656+05
hello shokolad	Hello Shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "hello shokolad", "name": "Hello Shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.74582+05
humo for men 120 sht	Humo For Men 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "humo for men 120 sht", "name": "Humo For Men 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.745989+05
ibona keks paket	Ibona Keks Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ibona keks paket", "name": "Ibona Keks Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.746472+05
ice burger	Ice Burger	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ice burger", "name": "Ice Burger", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.746632+05
ice cream redbul 65 gr	Ice Cream Redbul 65 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ice cream redbul 65 gr", "name": "Ice Cream Redbul 65 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.746953+05
ice glice qora 110 gr	Ice Glice Qora 110 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ice glice qora 110 gr", "name": "Ice Glice Qora 110 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.74713+05
ice gold atlas marojniy	Ice Gold Atlas Marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ice gold atlas marojniy", "name": "Ice Gold Atlas Marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.747437+05
ice gold plombir marojniy	Ice Gold Plombir Marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ice gold plombir marojniy", "name": "Ice Gold Plombir Marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.747593+05
ice grim eskimo	Ice Grim Eskimo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ice grim eskimo", "name": "Ice Grim Eskimo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.747748+05
ice milk klubnika mango 70 gr	Ice Milk Klubnika Mango 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ice milk klubnika mango 70 gr", "name": "Ice Milk Klubnika Mango 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.747896+05
ice rocket 40 gr	Ice Rocket 40 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ice rocket 40 gr", "name": "Ice Rocket 40 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.748042+05
ichankiy aboy paket	Ichankiy Aboy Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ichankiy aboy paket", "name": "Ichankiy Aboy Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.748198+05
ichankiy aboy paket 5kg	Ichankiy Aboy Paket 5kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ichankiy aboy paket 5kg", "name": "Ichankiy Aboy Paket 5kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.748403+05
ideal cake bollo	Ideal Cake Bollo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ideal cake bollo", "name": "Ideal Cake Bollo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.748589+05
ideal keks	Ideal Keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ideal keks", "name": "Ideal Keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.748749+05
ilma qurt 40 gr	Ilma Qurt 40 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ilma qurt 40 gr", "name": "Ilma Qurt 40 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.74943+05
ilma qurt 8 gr	Ilma Qurt 8 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ilma qurt 8 gr", "name": "Ilma Qurt 8 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.749768+05
axmedov kroshka bombey 120 gr	Axmedov Kroshka Bombey 120 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "axmedov kroshka bombey 120 gr", "name": "Axmedov Kroshka Bombey 120 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.6724+05
day salfetka 120 sht yashil kok	Day Salfetka 120 Sht Yashil Kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "day salfetka 120 sht yashil kok", "name": "Day Salfetka 120 Sht Yashil Kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.706843+05
gold food burger	Gold Food Burger	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gold food burger", "name": "Gold Food Burger", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.736394+05
guruch nukus 1kg	Guruch Nukus 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "guruch nukus 1kg", "name": "Guruch Nukus 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.741516+05
Super pecheniy	Super pecheniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Super pecheniy", "name": "Super pecheniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.627232+05
barbol erke paket	Barbol Erke Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "barbol erke paket", "name": "Barbol Erke Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.675754+05
bizzar 500gr	Bizzar 500gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bizzar 500gr", "name": "Bizzar 500gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.679289+05
bloomy cotton 120 sht	Bloomy Cotton 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bloomy cotton 120 sht", "name": "Bloomy Cotton 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.680464+05
chempion samarqand suxariki baton	Chempion Samarqand Suxariki Baton	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion samarqand suxariki baton", "name": "Chempion Samarqand Suxariki Baton", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.694463+05
cpp 825/25	CPP 825/25	Kg		homashyo	{"uom": "Kg", "code": "cpp 825/25", "name": "CPP 825/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.704777+05
delis 450gr qora	Delis 450gr Qora	Kg		tayyor mahsulot	{"uom": "Kg", "code": "delis 450gr qora", "name": "Delis 450gr Qora", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.708883+05
ice gold asarti 500 gr paket	Ice Gold Asarti 500 Gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ice gold asarti 500 gr paket", "name": "Ice Gold Asarti 500 Gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.747285+05
ideal paket usluga	Ideal Paket Usluga	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ideal paket usluga", "name": "Ideal Paket Usluga", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.748928+05
ilma classic qurt	Ilma Classic Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ilma classic qurt", "name": "Ilma Classic Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.749081+05
ilma qurt 25 gr	Ilma Qurt 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ilma qurt 25 gr", "name": "Ilma Qurt 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.749242+05
imkon megic shokolad	Imkon Megic Shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "imkon megic shokolad", "name": "Imkon Megic Shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.749928+05
imkon plus stakanchik vanil	Imkon Plus Stakanchik Vanil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "imkon plus stakanchik vanil", "name": "Imkon Plus Stakanchik Vanil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.750511+05
imkon qurt	Imkon Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "imkon qurt", "name": "Imkon Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.750832+05
isko cookies 70 gr	Isko Cookies 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "isko cookies 70 gr", "name": "Isko Cookies 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.752556+05
isko hops xalod payka	Isko Hops Xalod Payka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "isko hops xalod payka", "name": "Isko Hops Xalod Payka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.75271+05
jec semechka 5 kg paket	Jec Semechka 5 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jec semechka 5 kg paket", "name": "Jec Semechka 5 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.757008+05
kukrus noviy god	Kukrus Noviy God	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kukrus noviy god", "name": "Kukrus Noviy God", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.787303+05
luna baby 120 sht	Luna Baby 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "luna baby 120 sht", "name": "Luna Baby 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.792493+05
mustafo holod milkman 65 gr	Mustafo Holod Milkman 65 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mustafo holod milkman 65 gr", "name": "Mustafo Holod Milkman 65 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.844327+05
orzu plombir sinkers	Orzu Plombir Sinkers	Kg		tayyor mahsulot	{"uom": "Kg", "code": "orzu plombir sinkers", "name": "Orzu Plombir Sinkers", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.926781+05
salafan paket 17.5/14	Salafan Paket 17.5/14	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salafan paket 17.5/14", "name": "Salafan Paket 17.5/14", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.974033+05
teddy bear klubnika banan 120 gr	Teddy Bear Klubnika Banan 120 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "teddy bear klubnika banan 120 gr", "name": "Teddy Bear Klubnika Banan 120 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.01846+05
waﬄe cake 6 sht 270 gr orange	Waﬄe Cake 6 Sht 270 Gr Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "waﬄe cake 6 sht 270 gr orange", "name": "Waﬄe Cake 6 Sht 270 Gr Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.040446+05
Mr free asartiy	Mr free asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mr free asartiy", "name": "Mr free asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.581838+05
Rosli Malina	Rosli Malina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rosli Malina", "name": "Rosli Malina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.612898+05
Shok shokolad	Shok shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shok shokolad", "name": "Shok shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.617205+05
cpp 800/35	CPP 800/35	Kg		homashyo	{"uom": "Kg", "code": "cpp 800/35", "name": "CPP 800/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.704621+05
delo v slivkax 1 kg paket	Delo V Slivkax 1 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "delo v slivkax 1 kg paket", "name": "Delo V Slivkax 1 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.709017+05
elitex kids futbolka rulon	Elitex Kids Futbolka Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "elitex kids futbolka rulon", "name": "Elitex Kids Futbolka Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.723435+05
Zizi shariki yangi	Zizi shariki yangi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi shariki yangi", "name": "Zizi shariki yangi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.661876+05
chudo samara klubnika 70 gr	Chudo Samara Klubnika 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chudo samara klubnika 70 gr", "name": "Chudo Samara Klubnika 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.70087+05
gost plombir medevoe 65 gr	Gost Plombir Medevoe 65 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gost plombir medevoe 65 gr", "name": "Gost Plombir Medevoe 65 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.738032+05
jem 640/20 kar	JEM 640/20	Kg		homashyo	{"uom": "Kg", "code": "jem 640/20 kar", "name": "JEM 640/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.765505+05
jeti batir qurt paket	Jeti Batir Qurt Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jeti batir qurt paket", "name": "Jeti Batir Qurt Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.771448+05
kalde falga	Kalde Falga	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kalde falga", "name": "Kalde Falga", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.773193+05
karamel paket	Karamel Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "karamel paket", "name": "Karamel Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.773665+05
karamelka zaychik 60gr marojniy	Karamelka Zaychik 60gr Marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "karamelka zaychik 60gr marojniy", "name": "Karamelka Zaychik 60gr Marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.773827+05
karavan qurt 5 gr	Karavan Qurt 5 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "karavan qurt 5 gr", "name": "Karavan Qurt 5 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.773991+05
kardinal marojniy 100gr	Kardinal Marojniy 100gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kardinal marojniy 100gr", "name": "Kardinal Marojniy 100gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.774148+05
kartoshka 365	Kartoshka 365	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kartoshka 365", "name": "Kartoshka 365", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.774451+05
kartoshka 365kun 3kg	Kartoshka 365kun 3kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kartoshka 365kun 3kg", "name": "Kartoshka 365kun 3kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.774584+05
kartoshkali vareniklar 500 gr	Kartoshkali Vareniklar 500 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kartoshkali vareniklar 500 gr", "name": "Kartoshkali Vareniklar 500 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.774864+05
karvon semechka 5 kg paket	Karvon Semechka 5 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "karvon semechka 5 kg paket", "name": "Karvon Semechka 5 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.775002+05
karzinka sariq kartoshka 2,5kg paket	Karzinka Sariq Kartoshka 2,5kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "karzinka sariq kartoshka 2,5kg paket", "name": "Karzinka Sariq Kartoshka 2,5kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.775138+05
kasr kusr 15 gr asarty	Kasr Kusr 15 Gr Asarty	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kasr kusr 15 gr asarty", "name": "Kasr Kusr 15 Gr Asarty", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.775278+05
kazinaki	Kazinaki	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kazinaki", "name": "Kazinaki", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.775425+05
kdm keks	Kdm Keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kdm keks", "name": "Kdm Keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.775575+05
keks chorniy smarodina	Keks Chorniy Smarodina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "keks chorniy smarodina", "name": "Keks Chorniy Smarodina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.775709+05
keks klubnichniy	Keks Klubnichniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "keks klubnichniy", "name": "Keks Klubnichniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.77585+05
keks varonniy sugishonka	Keks Varonniy Sugishonka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "keks varonniy sugishonka", "name": "Keks Varonniy Sugishonka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.77598+05
koko kivi max 75 gr	Koko Kivi Max 75 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "koko kivi max 75 gr", "name": "Koko Kivi Max 75 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.779258+05
kra kra jvachka	Kra Kra Jvachka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kra kra jvachka", "name": "Kra Kra Jvachka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.780892+05
krakers suxariki	Krakers Suxariki	Kg		tayyor mahsulot	{"uom": "Kg", "code": "krakers suxariki", "name": "Krakers Suxariki", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.781202+05
marsh mello miniy	Marsh Mello Miniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "marsh mello miniy", "name": "Marsh Mello Miniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.801072+05
pistello tuzliy yashil 04	Pistello Tuzliy Yashil 04	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pistello tuzliy yashil 04", "name": "Pistello Tuzliy Yashil 04", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.954102+05
buniy salfetka 120ta	Buniy Salfetka 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "buniy salfetka 120ta", "name": "Buniy Salfetka 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.684251+05
cheers steyk burger 210 gr 46 sm	Cheers Steyk Burger 210 Gr 46 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers steyk burger 210 gr 46 sm", "name": "Cheers Steyk Burger 210 Gr 46 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.691692+05
elma kok pushti nam salfetka 120 sht	Elma Kok Pushti Nam Salfetka 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "elma kok pushti nam salfetka 120 sht", "name": "Elma Kok Pushti Nam Salfetka 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.724188+05
fayz qurt paket ziyo	Fayz Qurt Paket Ziyo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fayz qurt paket ziyo", "name": "Fayz Qurt Paket Ziyo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.726903+05
fozilov luxus 100 gr	Fozilov Luxus 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fozilov luxus 100 gr", "name": "Fozilov Luxus 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.7298+05
kalon semechka 5kg paket	Kalon Semechka 5kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kalon semechka 5kg paket", "name": "Kalon Semechka 5kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.773343+05
kippers sovun	Kippers Sovun	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kippers sovun", "name": "Kippers Sovun", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.776821+05
koker choccy	Koker Choccy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "koker choccy", "name": "Koker Choccy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.777241+05
kirosis shanpun	Kirosis Shanpun	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kirosis shanpun", "name": "Kirosis Shanpun", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.77696+05
koker donni oq 70gr	Koker Donni Oq 70gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "koker donni oq 70gr", "name": "Koker Donni Oq 70gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.777405+05
koker donni qora 70gr	Koker Donni Qora 70gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "koker donni qora 70gr", "name": "Koker Donni Qora 70gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.777566+05
koker koniy 180gr	Koker Koniy 180gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "koker koniy 180gr", "name": "Koker Koniy 180gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.777722+05
koker lubimiy 65gr	Koker Lubimiy 65gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "koker lubimiy 65gr", "name": "Koker Lubimiy 65gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.777883+05
koker maks choccy 80g	Koker Maks Choccy 80g	Kg		tayyor mahsulot	{"uom": "Kg", "code": "koker maks choccy 80g", "name": "Koker Maks Choccy 80g", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.778029+05
koker mini choccy 13g	Koker Mini Choccy 13g	Kg		tayyor mahsulot	{"uom": "Kg", "code": "koker mini choccy 13g", "name": "Koker Mini Choccy 13g", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.778181+05
koker shikalad	Koker Shikalad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "koker shikalad", "name": "Koker Shikalad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.778462+05
koker tempo 60gr	Koker Tempo 60gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "koker tempo 60gr", "name": "Koker Tempo 60gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.778634+05
koko fresh arbuz dinya	Koko Fresh Arbuz Dinya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "koko fresh arbuz dinya", "name": "Koko Fresh Arbuz Dinya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.778939+05
koko frukto lyod arbuz	Koko Frukto Lyod Arbuz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "koko frukto lyod arbuz", "name": "Koko Frukto Lyod Arbuz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.779081+05
koko kukruz paket	Koko Kukruz Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "koko kukruz paket", "name": "Koko Kukruz Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.779412+05
komalina 5kg paket	Komalina 5kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "komalina 5kg paket", "name": "Komalina 5kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.779777+05
kornevin 5gr paket	Kornevin 5gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kornevin 5gr paket", "name": "Kornevin 5gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.780283+05
korovka bayram	Korovka Bayram	Kg		tayyor mahsulot	{"uom": "Kg", "code": "korovka bayram", "name": "Korovka Bayram", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.780592+05
korovka sweet	Korovka Sweet	Kg		tayyor mahsulot	{"uom": "Kg", "code": "korovka sweet", "name": "Korovka Sweet", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.780734+05
krakers grenkiy	Krakers Grenkiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "krakers grenkiy", "name": "Krakers Grenkiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.781037+05
krakkers mazali qurt	Krakkers Mazali Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "krakkers mazali qurt", "name": "Krakkers Mazali Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.781375+05
kreasy max 80 gr	Kreasy Max 80 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kreasy max 80 gr", "name": "Kreasy Max 80 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.781526+05
kreko shashlik 28gr	Kreko Shashlik 28gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kreko shashlik 28gr", "name": "Kreko Shashlik 28gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.782561+05
krember ole 182 gr	Krember Ole 182 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "krember ole 182 gr", "name": "Krember Ole 182 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.783688+05
kristal ekstra 800gr paket	Kristal Ekstra 800gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kristal ekstra 800gr paket", "name": "Kristal Ekstra 800gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.783839+05
kristal tuz 2kg	Kristal Tuz 2kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kristal tuz 2kg", "name": "Kristal Tuz 2kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.784282+05
kristal tuz rossiya 2,5 kg	Kristal Tuz Rossiya 2,5 Kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kristal tuz rossiya 2,5 kg", "name": "Kristal Tuz Rossiya 2,5 Kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.785156+05
ku-ku kukuruz 150 gr paket	Ku-ku Kukuruz 150 Gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ku-ku kukuruz 150 gr paket", "name": "Ku-ku Kukuruz 150 Gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.786308+05
mat 975/20 kar	MAT 975/20	Kg		homashyo	{"uom": "Kg", "code": "mat 975/20 kar", "name": "MAT 975/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.813096+05
mat 990/20	MAT 990/20	Kg		homashyo	{"uom": "Kg", "code": "mat 990/20", "name": "MAT 990/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.813249+05
klean sariq	Klean Sariq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "klean sariq", "name": "Klean Sariq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.777102+05
Tvorg 200gr 9% paket	Tvorg 200gr 9% paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Tvorg 200gr 9% paket", "name": "Tvorg 200gr 9% paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.63817+05
dusel pf 6 sm	Dusel Pf 6 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dusel pf 6 sm", "name": "Dusel Pf 6 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.71872+05
kreko shashlik 25 gr	Kreko Shashlik 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kreko shashlik 25 gr", "name": "Kreko Shashlik 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.782391+05
kreko smetana 28gr	Kreko Smetana 28gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kreko smetana 28gr", "name": "Kreko Smetana 28gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.783183+05
kreko tomat chili 50 gr	Kreko Tomat Chili 50 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kreko tomat chili 50 gr", "name": "Kreko Tomat Chili 50 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.783537+05
kristal tuz	Kristal Tuz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kristal tuz", "name": "Kristal Tuz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.783987+05
kristal tuz 1kg	Kristal Tuz 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kristal tuz 1kg", "name": "Kristal Tuz 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.784132+05
kristal tuz 2kg paket	Kristal Tuz 2kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kristal tuz 2kg paket", "name": "Kristal Tuz 2kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.78443+05
kristal tuz rossiya 1 kg	Kristal Tuz Rossiya 1 Kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kristal tuz rossiya 1 kg", "name": "Kristal Tuz Rossiya 1 Kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.78458+05
kristal tuz rossiya 2 kg	Kristal Tuz Rossiya 2 Kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kristal tuz rossiya 2 kg", "name": "Kristal Tuz Rossiya 2 Kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.784851+05
kristal yodlangan osh tuzi 1kg	Kristal Yodlangan Osh Tuzi 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kristal yodlangan osh tuzi 1kg", "name": "Kristal Yodlangan Osh Tuzi 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.785617+05
kristal yodlangan osh tuzi 2kg	Kristal Yodlangan Osh Tuzi 2kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kristal yodlangan osh tuzi 2kg", "name": "Kristal Yodlangan Osh Tuzi 2kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.785754+05
kroki 30gr	Kroki 30gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kroki 30gr", "name": "Kroki 30gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.785903+05
kruto7	Kruto7	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kruto7", "name": "Kruto7", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.786036+05
ku ku 250 gr paket	Ku Ku 250 Gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ku ku 250 gr paket", "name": "Ku Ku 250 Gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.786169+05
ku-ku kukuruz 2 kg paket	Ku-ku Kukuruz 2 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ku-ku kukuruz 2 kg paket", "name": "Ku-ku Kukuruz 2 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.786452+05
kuk suxarik smetana	Kuk Suxarik Smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kuk suxarik smetana", "name": "Kuk Suxarik Smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.787153+05
kukruz 777 qizil kok	Kukruz 777 Qizil Kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kukruz 777 qizil kok", "name": "Kukruz 777 Qizil Kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.78762+05
kuruk urotropin paket	Kuruk Urotropin Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kuruk urotropin paket", "name": "Kuruk Urotropin Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.788582+05
labubu xolonaya payka	Labubu Xolonaya Payka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "labubu xolonaya payka", "name": "Labubu Xolonaya Payka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.78875+05
lakta tvist	Lakta Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "lakta tvist", "name": "Lakta Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.788897+05
lamour	Lamour	Kg		tayyor mahsulot	{"uom": "Kg", "code": "lamour", "name": "Lamour", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.789049+05
lawa lawa biskvit	Lawa Lawa Biskvit	Kg		tayyor mahsulot	{"uom": "Kg", "code": "lawa lawa biskvit", "name": "Lawa Lawa Biskvit", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.789343+05
lazzat bio qurt 6 gr	Lazzat Bio Qurt 6 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "lazzat bio qurt 6 gr", "name": "Lazzat Bio Qurt 6 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.789494+05
lelingrat 1kg marojniy	Lelingrat 1kg Marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "lelingrat 1kg marojniy", "name": "Lelingrat 1kg Marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.789663+05
lendy tvist	Lendy Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "lendy tvist", "name": "Lendy Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.79019+05
limonika	Limonika	Kg		tayyor mahsulot	{"uom": "Kg", "code": "limonika", "name": "Limonika", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.791255+05
lola choko wafer asarti	Lola Choko Wafer Asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "lola choko wafer asarti", "name": "Lola Choko Wafer Asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.791406+05
loro parvoz tvist	Loro Parvoz Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "loro parvoz tvist", "name": "Loro Parvoz Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.791541+05
mat 735/20	MAT 735/20	Kg		homashyo	{"uom": "Kg", "code": "mat 735/20", "name": "MAT 735/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.81098+05
mcpp 225/20	Mcpp 225/20	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mcpp 225/20", "name": "Mcpp 225/20", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.815497+05
cpp 615/60	CPP 615/60	Kg		homashyo	{"uom": "Kg", "code": "cpp 615/60", "name": "CPP 615/60", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.703703+05
kreko shashlik 50 gr	Kreko Shashlik 50 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kreko shashlik 50 gr", "name": "Kreko Shashlik 50 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.782731+05
kreko sir 28gr	Kreko Sir 28gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kreko sir 28gr", "name": "Kreko Sir 28gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.782873+05
cpp 1150/35	CPP 1150/35	Kg		homashyo	{"uom": "Kg", "code": "cpp 1150/35", "name": "CPP 1150/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.702356+05
kukruz555 yashil	Kukruz555 Yashil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kukruz555 yashil", "name": "Kukruz555 Yashil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.788095+05
kukruzo paket	Kukruzo Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kukruzo paket", "name": "Kukruzo Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.78824+05
kukruz 888 jigarrang	Kukruz 888 Jigarrang	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kukruz 888 jigarrang", "name": "Kukruz 888 Jigarrang", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.787783+05
kukuruz 111	Kukuruz 111	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kukuruz 111", "name": "Kukuruz 111", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.788431+05
lelingrat marojniy 500gr	Lelingrat Marojniy 500gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "lelingrat marojniy 500gr", "name": "Lelingrat Marojniy 500gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.789964+05
li lac tvist	Li Lac Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "li lac tvist", "name": "Li Lac Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.790391+05
lider pista 2 kg paket	Lider Pista 2 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "lider pista 2 kg paket", "name": "Lider Pista 2 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.790946+05
lider semechki 5 kg paket	Lider Semechki 5 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "lider semechki 5 kg paket", "name": "Lider Semechki 5 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.791102+05
love is vaﬂi asarti	Love Is Vaﬂi Asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "love is vaﬂi asarti", "name": "Love Is Vaﬂi Asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.791695+05
loviya fasol 1kg	Loviya Fasol 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "loviya fasol 1kg", "name": "Loviya Fasol 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.791845+05
lubimaya keks	Lubimaya Keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "lubimaya keks", "name": "Lubimaya Keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.79199+05
luis vuitton asarti paket	Luis Vuitton Asarti Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "luis vuitton asarti paket", "name": "Luis Vuitton Asarti Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.79232+05
luna cake siyohrang 25 gr	Luna Cake Siyohrang 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "luna cake siyohrang 25 gr", "name": "Luna Cake Siyohrang 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.792822+05
magic roller 85 gr	Magic Roller 85 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "magic roller 85 gr", "name": "Magic Roller 85 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.793738+05
magnum amir plambir 65gr	Magnum Amir Plambir 65gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "magnum amir plambir 65gr", "name": "Magnum Amir Plambir 65gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.794346+05
magnum marojniy	Magnum Marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "magnum marojniy", "name": "Magnum Marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.794496+05
makado barbekyu	Makado Barbekyu	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makado barbekyu", "name": "Makado Barbekyu", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.794774+05
makado tomat	Makado Tomat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makado tomat", "name": "Makado Tomat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.794917+05
makaron pushing 2 kg paket	Makaron Pushing 2 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makaron pushing 2 kg paket", "name": "Makaron Pushing 2 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.795197+05
makiz 400gr C415	makiz 400gr C415	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz 400gr C415", "name": "makiz 400gr C415", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.795763+05
makiz 400gr C418	makiz 400gr C418	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz 400gr C418", "name": "makiz 400gr C418", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.795905+05
makiz 400gr C424	makiz 400gr C424	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz 400gr C424", "name": "makiz 400gr C424", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.796034+05
makiz 400gr C430	makiz 400gr C430	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz 400gr C430", "name": "makiz 400gr C430", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.796175+05
makiz 400gr C431	makiz 400gr C431	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz 400gr C431", "name": "makiz 400gr C431", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.796308+05
makiz 400gr C437	makiz 400gr C437	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz 400gr C437", "name": "makiz 400gr C437", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.796445+05
makiz 700 gr CU 785	makiz 700 gr CU 785	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz 700 gr CU 785", "name": "makiz 700 gr CU 785", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.796585+05
makiz 700gr C715	makiz 700gr C715	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz 700gr C715", "name": "makiz 700gr C715", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.796722+05
makiz 700gr C730	makiz 700gr C730	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz 700gr C730", "name": "makiz 700gr C730", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.796855+05
mat 435/18	MAT 435/18	Kg		homashyo	{"uom": "Kg", "code": "mat 435/18", "name": "MAT 435/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.803774+05
mat 755/20 kar	MAT 755/20	Kg		homashyo	{"uom": "Kg", "code": "mat 755/20 kar", "name": "MAT 755/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.811282+05
kukruz555	Kukruz555	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kukruz555", "name": "Kukruz555", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.787943+05
donut cake banan 25 gr	Donut Cake Banan 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "donut cake banan 25 gr", "name": "Donut Cake Banan 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.717034+05
kukruz 111	Kukruz 111	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kukruz 111", "name": "Kukruz 111", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.787457+05
makiz 1kg LV100	makiz 1kg LV100	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz 1kg LV100", "name": "makiz 1kg LV100", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.795338+05
makiz 400gr C409	makiz 400gr C409	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz 400gr C409", "name": "makiz 400gr C409", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.795627+05
makiz 400 gr mampar SQ	makiz 400 gr mampar SQ	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz 400 gr mampar SQ", "name": "makiz 400 gr mampar SQ", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.795485+05
makiz 800gr pasta R102	makiz 800gr pasta R102	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz 800gr pasta R102", "name": "makiz 800gr pasta R102", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.797003+05
makiz C709 700gr	makiz C709 700gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz C709 700gr", "name": "makiz C709 700gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.797158+05
makiz C718 700gr	makiz C718 700gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz C718 700gr", "name": "makiz C718 700gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.797328+05
makiz C731	makiz C731	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz C731", "name": "makiz C731", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.797492+05
makiz C737 700gr	makiz C737 700gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz C737 700gr", "name": "makiz C737 700gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.797641+05
makiz cu724 700 gr	Makiz Cu724 700 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz cu724 700 gr", "name": "Makiz Cu724 700 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.798302+05
makiz mol goshti chuchvara 1 kg	Makiz Mol Goshti Chuchvara 1 Kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz mol goshti chuchvara 1 kg", "name": "Makiz Mol Goshti Chuchvara 1 Kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.798768+05
makiz priprava	Makiz Priprava	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz priprava", "name": "Makiz Priprava", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.798923+05
malika shaxzoda 80 gr	Malika Shaxzoda 80 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "malika shaxzoda 80 gr", "name": "Malika Shaxzoda 80 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.799075+05
malochniy vkus 20gr	Malochniy Vkus 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "malochniy vkus 20gr", "name": "Malochniy Vkus 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.79924+05
mama 10ta paket	Mama 10ta Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mama 10ta paket", "name": "Mama 10ta Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.799481+05
marko 5D	marko 5D	Kg		tayyor mahsulot	{"uom": "Kg", "code": "marko 5D", "name": "marko 5D", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.799847+05
marko xrust 5d	Marko Xrust 5d	Kg		tayyor mahsulot	{"uom": "Kg", "code": "marko xrust 5d", "name": "Marko Xrust 5d", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.799994+05
markov' krasnaya 1 kg	Markov' Krasnaya 1 Kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "markov' krasnaya 1 kg", "name": "Markov' Krasnaya 1 Kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.800137+05
marojniy mers-tvins-katkat-smayl	Marojniy Mers-tvins-katkat-smayl	Kg		tayyor mahsulot	{"uom": "Kg", "code": "marojniy mers-tvins-katkat-smayl", "name": "Marojniy Mers-tvins-katkat-smayl", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.800282+05
marojniy qirmizi olma	Marojniy Qirmizi Olma	Kg		tayyor mahsulot	{"uom": "Kg", "code": "marojniy qirmizi olma", "name": "Marojniy Qirmizi Olma", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.800456+05
maroqand kurt ziyo	Maroqand Kurt Ziyo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "maroqand kurt ziyo", "name": "Maroqand Kurt Ziyo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.800602+05
marsh mello kosichka kichik	Marsh Mello Kosichka Kichik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "marsh mello kosichka kichik", "name": "Marsh Mello Kosichka Kichik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.800749+05
marsh mello mini paket	Marsh Mello Mini Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "marsh mello mini paket", "name": "Marsh Mello Mini Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.800894+05
marshmello	Marshmello	Kg		tayyor mahsulot	{"uom": "Kg", "code": "marshmello", "name": "Marshmello", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.801223+05
marvarid bio kurt paket	Marvarid Bio Kurt Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "marvarid bio kurt paket", "name": "Marvarid Bio Kurt Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.801381+05
marvarid bio qurt rulon	Marvarid Bio Qurt Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "marvarid bio qurt rulon", "name": "Marvarid Bio Qurt Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.801534+05
marvarid the best sirniy palochka	Marvarid The Best Sirniy Palochka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "marvarid the best sirniy palochka", "name": "Marvarid The Best Sirniy Palochka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.802148+05
maryam tekistil paket	Maryam Tekistil Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "maryam tekistil paket", "name": "Maryam Tekistil Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.802315+05
mat 1010/20 kar	MAT 1010/20	Kg		homashyo	{"uom": "Kg", "code": "mat 1010/20 kar", "name": "MAT 1010/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.802491+05
ays kola apelsin g energi 55 gr	Ays Kola Apelsin G Energi 55 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ays kola apelsin g energi 55 gr", "name": "Ays Kola Apelsin G Energi 55 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.673257+05
eco jidkie oboi	Eco Jidkie Oboi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "eco jidkie oboi", "name": "Eco Jidkie Oboi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.719665+05
makaron pushing 1 kg paket	Makaron Pushing 1 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makaron pushing 1 kg paket", "name": "Makaron Pushing 1 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.79506+05
makiz chuchvara paket 1kg	Makiz Chuchvara Paket 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz chuchvara paket 1kg", "name": "Makiz Chuchvara Paket 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.797965+05
kuk suxarik salyami	Kuk Suxarik Salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kuk suxarik salyami", "name": "Kuk Suxarik Salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.786889+05
makiz mannaya kruppa 700g	Makiz Mannaya Kruppa 700g	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz mannaya kruppa 700g", "name": "Makiz Mannaya Kruppa 700g", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.798601+05
mat 1310/20 pol	MAT 1310/20	Kg		homashyo	{"uom": "Kg", "code": "mat 1310/20 pol", "name": "MAT 1310/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.802662+05
mat 600/20 kar	MAT 600/20	Kg		homashyo	{"uom": "Kg", "code": "mat 600/20 kar", "name": "MAT 600/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.807442+05
maxi klubnika 75 gr	Maxi Klubnika 75 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "maxi klubnika 75 gr", "name": "Maxi Klubnika 75 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.81416+05
maydacha iz shokolada	Maydacha Iz Shokolada	Kg		tayyor mahsulot	{"uom": "Kg", "code": "maydacha iz shokolada", "name": "Maydacha Iz Shokolada", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.814762+05
maydacha pecheniy biskivit	Maydacha Pecheniy Biskivit	Kg		tayyor mahsulot	{"uom": "Kg", "code": "maydacha pecheniy biskivit", "name": "Maydacha Pecheniy Biskivit", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.814907+05
mayiz paket	Mayiz Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mayiz paket", "name": "Mayiz Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.815045+05
mazza 90gr marojniy	Mazza 90gr Marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mazza 90gr marojniy", "name": "Mazza 90gr Marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.815207+05
mazza tabiiy mahsulot qurt	Mazza Tabiiy Mahsulot Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mazza tabiiy mahsulot qurt", "name": "Mazza Tabiiy Mahsulot Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.815351+05
mega 1kg paket tuzli ruchkali	Mega 1kg Paket Tuzli Ruchkali	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega 1kg paket tuzli ruchkali", "name": "Mega 1kg Paket Tuzli Ruchkali", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.815639+05
mega 1kg paket tuzsiz ruchkali	Mega 1kg Paket Tuzsiz Ruchkali	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega 1kg paket tuzsiz ruchkali", "name": "Mega 1kg Paket Tuzsiz Ruchkali", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.815916+05
mega 2kg paket tuzli ruchkali	Mega 2kg Paket Tuzli Ruchkali	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega 2kg paket tuzli ruchkali", "name": "Mega 2kg Paket Tuzli Ruchkali", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.816068+05
mega 3kg paket tuzliy	Mega 3kg Paket Tuzliy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega 3kg paket tuzliy", "name": "Mega 3kg Paket Tuzliy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.816382+05
mega Super semechki	mega Super semechki	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega Super semechki", "name": "mega Super semechki", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.816675+05
mega crispy 300 gr	Mega Crispy 300 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega crispy 300 gr", "name": "Mega Crispy 300 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.817096+05
mega fresh semechka 60 gr tuzli	Mega Fresh Semechka 60 Gr Tuzli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega fresh semechka 60 gr tuzli", "name": "Mega Fresh Semechka 60 Gr Tuzli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.817931+05
mega kok qurt 30gr	Mega Kok Qurt 30gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega kok qurt 30gr", "name": "Mega Kok Qurt 30gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.818357+05
mega kok qurt 60gr	Mega Kok Qurt 60gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega kok qurt 60gr", "name": "Mega Kok Qurt 60gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.81863+05
mega pack wet towel 120 sht	Mega Pack Wet Towel 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega pack wet towel 120 sht", "name": "Mega Pack Wet Towel 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.819406+05
salfetka XNS kids 72ta tomi jeriy	salfetka XNS kids 72ta tomi jeriy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka XNS kids 72ta tomi jeriy", "name": "salfetka XNS kids 72ta tomi jeriy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.975646+05
salfetka husfa merry 120 sht	Salfetka Husfa Merry 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka husfa merry 120 sht", "name": "Salfetka Husfa Merry 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.979331+05
sladok zavariki 150gr paket	Sladok Zavariki 150gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sladok zavariki 150gr paket", "name": "Sladok Zavariki 150gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.001153+05
zarqand pryaniki vishnya 230 gr	Zarqand Pryaniki Vishnya 230 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zarqand pryaniki vishnya 230 gr", "name": "Zarqand Pryaniki Vishnya 230 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.049079+05
eko salfetka vlajniy 15ta	Eko Salfetka Vlajniy 15ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "eko salfetka vlajniy 15ta", "name": "Eko Salfetka Vlajniy 15ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.721503+05
kartoshka 365kun 3kg sariq	Kartoshka 365kun 3kg Sariq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kartoshka 365kun 3kg sariq", "name": "Kartoshka 365kun 3kg Sariq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.774716+05
macho uz paket	Macho Uz Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "macho uz paket", "name": "Macho Uz Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.793588+05
maya melnitsa 1kg paket	Maya Melnitsa 1kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "maya melnitsa 1kg paket", "name": "Maya Melnitsa 1kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.814463+05
maya melnitsa 2kg paket	Maya Melnitsa 2kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "maya melnitsa 2kg paket", "name": "Maya Melnitsa 2kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.814607+05
king chips 20 gr asarti	King Chips 20 Gr Asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "king chips 20 gr asarti", "name": "King Chips 20 Gr Asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.776528+05
mega bodom 40gr	Mega Bodom 40gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega bodom 40gr", "name": "Mega Bodom 40gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.816957+05
mega diyor 3 kg paket	Mega Diyor 3 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega diyor 3 kg paket", "name": "Mega Diyor 3 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.817368+05
mega diyor 5 kg paket	Mega Diyor 5 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega diyor 5 kg paket", "name": "Mega Diyor 5 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.817503+05
mega gajak 30gr	Mega Gajak 30gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega gajak 30gr", "name": "Mega Gajak 30gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.818075+05
mega handon pista 30gr	Mega Handon Pista 30gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega handon pista 30gr", "name": "Mega Handon Pista 30gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.81822+05
mega max 3 kg paket	Mega Max 3 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega max 3 kg paket", "name": "Mega Max 3 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.818777+05
mega max 5 kg paket	Mega Max 5 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega max 5 kg paket", "name": "Mega Max 5 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.818932+05
mega pack 120 sht	Mega Pack 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega pack 120 sht", "name": "Mega Pack 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.819243+05
mega pista 75gr	Mega Pista 75gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega pista 75gr", "name": "Mega Pista 75gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.819563+05
mega premum aranjiviy qurt	Mega Premum Aranjiviy Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega premum aranjiviy qurt", "name": "Mega Premum Aranjiviy Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.819876+05
mega tayim 20gr tuzliy paket	Mega Tayim 20gr Tuzliy Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega tayim 20gr tuzliy paket", "name": "Mega Tayim 20gr Tuzliy Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.820723+05
mega tayim 20gr tuzsiz paket	Mega Tayim 20gr Tuzsiz Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega tayim 20gr tuzsiz paket", "name": "Mega Tayim 20gr Tuzsiz Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.820903+05
mega time 90gr	Mega Time 90gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega time 90gr", "name": "Mega Time 90gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.821053+05
mega yeryong'oq 150g	Mega Yeryong'oq 150g	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega yeryong'oq 150g", "name": "Mega Yeryong'oq 150g", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.821202+05
mega yeryong'oq 150gr zip paket	Mega Yeryong'oq 150gr Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega yeryong'oq 150gr zip paket", "name": "Mega Yeryong'oq 150gr Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.821362+05
megic red rose 20 sht	Megic Red Rose 20 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "megic red rose 20 sht", "name": "Megic Red Rose 20 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.821663+05
metronex paket 30/10	Metronex Paket 30/10	Kg		tayyor mahsulot	{"uom": "Kg", "code": "metronex paket 30/10", "name": "Metronex Paket 30/10", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.823131+05
metronex paket 35/10	Metronex Paket 35/10	Kg		tayyor mahsulot	{"uom": "Kg", "code": "metronex paket 35/10", "name": "Metronex Paket 35/10", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.823309+05
mexmet keks	Mexmet Keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mexmet keks", "name": "Mexmet Keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.823616+05
milana tekistil paekt	Milana Tekistil Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "milana tekistil paekt", "name": "Milana Tekistil Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.824583+05
prazrachniy paket 20x30	Prazrachniy Paket 20x30	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachniy paket 20x30", "name": "Prazrachniy Paket 20x30", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.959284+05
vitamix 5 kg paket	Vitamix 5 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vitamix 5 kg paket", "name": "Vitamix 5 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.037954+05
vuelo coﬀee	Vuelo Coﬀee	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vuelo coﬀee", "name": "Vuelo Coﬀee", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.039625+05
xot lanch sochnaya kuritsa 90gr	Xot Lanch Sochnaya Kuritsa 90gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xot lanch sochnaya kuritsa 90gr", "name": "Xot Lanch Sochnaya Kuritsa 90gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.044647+05
ilma qurt 7 gr	Ilma Qurt 7 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ilma qurt 7 gr", "name": "Ilma Qurt 7 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.749601+05
jem 870/25	JEM 870/25	Kg		homashyo	{"uom": "Kg", "code": "jem 870/25", "name": "JEM 870/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.770728+05
mat 775/20 pol	MAT 775/20	Kg		homashyo	{"uom": "Kg", "code": "mat 775/20 pol", "name": "MAT 775/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.811566+05
mat765/20	MAT 765/20	Kg		homashyo	{"uom": "Kg", "code": "mat765/20", "name": "MAT 765/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.813405+05
mavis prazrachniy 27 sm	Mavis Prazrachniy 27 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mavis prazrachniy 27 sm", "name": "Mavis Prazrachniy 27 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.813557+05
mavis suﬂe asarty 28 gr xolodniy	Mavis Suﬂe Asarty 28 Gr Xolodniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mavis suﬂe asarty 28 gr xolodniy", "name": "Mavis Suﬂe Asarty 28 Gr Xolodniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.813869+05
maxito	Maxito	Kg		tayyor mahsulot	{"uom": "Kg", "code": "maxito", "name": "Maxito", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.814324+05
miray donut cake 30 gr orange	Miray Donut Cake 30 Gr Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miray donut cake 30 gr orange", "name": "Miray Donut Cake 30 Gr Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.826936+05
monny salfetka 72 sht japan	Monny Salfetka 72 Sht Japan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "monny salfetka 72 sht japan", "name": "Monny Salfetka 72 Sht Japan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.832144+05
mono elektrik 0.60	Mono Elektrik 0.60	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mono elektrik 0.60", "name": "Mono Elektrik 0.60", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.832302+05
mega oq semechka 100gr	Mega Oq Semechka 100gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega oq semechka 100gr", "name": "Mega Oq Semechka 100gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.819079+05
mumu qurt	Mumu Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mumu qurt", "name": "Mumu Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.840056+05
murads serdechka	Murads Serdechka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "murads serdechka", "name": "Murads Serdechka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.840199+05
musa BissGo dubai 80 gr	musa BissGo dubai 80 gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musa BissGo dubai 80 gr", "name": "musa BissGo dubai 80 gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.840693+05
musa magnit 100 gr	Musa Magnit 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musa magnit 100 gr", "name": "Musa Magnit 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.84118+05
musafo holod bambini 65 gr	Musafo Holod Bambini 65 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musafo holod bambini 65 gr", "name": "Musafo Holod Bambini 65 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.841321+05
musatafo holod arda hiva	Musatafo Holod Arda Hiva	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musatafo holod arda hiva", "name": "Musatafo Holod Arda Hiva", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.841467+05
musatfo holod toping 65 gr	Musatfo Holod Toping 65 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musatfo holod toping 65 gr", "name": "Musatfo Holod Toping 65 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.841916+05
musaﬀo bio keﬁr 450gr 2,5%	Musaﬀo Bio Keﬁr 450gr 2,5%	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musaﬀo bio keﬁr 450gr 2,5%", "name": "Musaﬀo Bio Keﬁr 450gr 2,5%", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.842083+05
musaﬀo prazrachniy qurt	Musaﬀo Prazrachniy Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musaﬀo prazrachniy qurt", "name": "Musaﬀo Prazrachniy Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.842692+05
musaﬀo qurt 1 hst	Musaﬀo Qurt 1 Hst	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musaﬀo qurt 1 hst", "name": "Musaﬀo Qurt 1 Hst", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.842835+05
musaﬀo sirok olchali	Musaﬀo Sirok Olchali	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musaﬀo sirok olchali", "name": "Musaﬀo Sirok Olchali", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.843446+05
musaﬀo sirok vanilli	Musaﬀo Sirok Vanilli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musaﬀo sirok vanilli", "name": "Musaﬀo Sirok Vanilli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.844173+05
muzqaymoq 365 kun kofe tamli 120 gr	Muzqaymoq 365 Kun Kofe Tamli 120 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "muzqaymoq 365 kun kofe tamli 120 gr", "name": "Muzqaymoq 365 Kun Kofe Tamli 120 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.844515+05
myagkoe pechene s chernika djem	Myagkoe Pechene S Chernika Djem	Kg		tayyor mahsulot	{"uom": "Kg", "code": "myagkoe pechene s chernika djem", "name": "Myagkoe Pechene S Chernika Djem", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.844679+05
opp 1030/35	OPP 1030/35	Kg		homashyo	{"uom": "Kg", "code": "opp 1030/35", "name": "OPP 1030/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.854+05
opp 1100/30 PFF kar	OPP PFF 1100/30	Kg		homashyo	{"uom": "Kg", "code": "opp 1100/30 PFF kar", "name": "OPP PFF 1100/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.85433+05
opp 20/15 paket	Opp 20/15 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp 20/15 paket", "name": "Opp 20/15 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.860338+05
opp 455/18 pff kar	OPP PFF 455/18	Kg		homashyo	{"uom": "Kg", "code": "opp 455/18 pff kar", "name": "OPP PFF 455/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.867063+05
pury milky sirok banan 40 gr	Pury Milky Sirok Banan 40 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pury milky sirok banan 40 gr", "name": "Pury Milky Sirok Banan 40 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.963262+05
suli max shokolad	Suli Max Shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "suli max shokolad", "name": "Suli Max Shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.011106+05
jem 455/20	JEM 455/20	Kg		homashyo	{"uom": "Kg", "code": "jem 455/20", "name": "JEM 455/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.760633+05
mega kok qurt 46gr	Mega Kok Qurt 46gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega kok qurt 46gr", "name": "Mega Kok Qurt 46gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.818495+05
mig jidkiye oboi	Mig Jidkiye Oboi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mig jidkiye oboi", "name": "Mig Jidkiye Oboi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.823771+05
opp 30/22 paket	Opp 30/22 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp 30/22 paket", "name": "Opp 30/22 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.861094+05
opp 425/20	OPP 425/20	Kg		homashyo	{"uom": "Kg", "code": "opp 425/20", "name": "OPP 425/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.864892+05
sardor 100 gr 16 sht rulon	Sardor 100 Gr 16 Sht Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor 100 gr 16 sht rulon", "name": "Sardor 100 Gr 16 Sht Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.984092+05
saﬁna 90 gr	Saﬁna 90 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "saﬁna 90 gr", "name": "Saﬁna 90 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.9891+05
jem 530/25 kar	JEM 530/25	Kg		homashyo	{"uom": "Kg", "code": "jem 530/25 kar", "name": "JEM 530/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.763168+05
musaﬀo keﬁr 450gr 1%	Musaﬀo Keﬁr 450gr 1%	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musaﬀo keﬁr 450gr 1%", "name": "Musaﬀo Keﬁr 450gr 1%", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.842382+05
musaﬀo sirok kakaoli	Musaﬀo Sirok Kakaoli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musaﬀo sirok kakaoli", "name": "Musaﬀo Sirok Kakaoli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.843156+05
myagkoe pechene s klubnichnim djem	Myagkoe Pechene S Klubnichnim Djem	Kg		tayyor mahsulot	{"uom": "Kg", "code": "myagkoe pechene s klubnichnim djem", "name": "Myagkoe Pechene S Klubnichnim Djem", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.844829+05
myaxkoye pechaniy c shokoladniy	Myaxkoye Pechaniy C Shokoladniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "myaxkoye pechaniy c shokoladniy", "name": "Myaxkoye Pechaniy C Shokoladniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.844981+05
n	N	Kg		tayyor mahsulot	{"uom": "Kg", "code": "n", "name": "N", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.845122+05
naggets chili	Naggets Chili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "naggets chili", "name": "Naggets Chili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.845287+05
nakta beshbarmak 400 gr paket	Nakta Beshbarmak 400 Gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nakta beshbarmak 400 gr paket", "name": "Nakta Beshbarmak 400 Gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.845429+05
nam salfetka qoqon 120 dona	Nam Salfetka Qoqon 120 Dona	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nam salfetka qoqon 120 dona", "name": "Nam Salfetka Qoqon 120 Dona", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.84572+05
nasir korin 30gr	Nasir Korin 30gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nasir korin 30gr", "name": "Nasir Korin 30gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.845863+05
nasir korin paket	Nasir Korin Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nasir korin paket", "name": "Nasir Korin Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.846009+05
nasir kreko salyami 28gr	Nasir Kreko Salyami 28gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nasir kreko salyami 28gr", "name": "Nasir Kreko Salyami 28gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.846153+05
nasr simba chips salyami 25 gr	Nasr Simba Chips Salyami 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nasr simba chips salyami 25 gr", "name": "Nasr Simba Chips Salyami 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.846848+05
nasr simba kokat va smetana 25 gr	Nasr Simba Kokat Va Smetana 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nasr simba kokat va smetana 25 gr", "name": "Nasr Simba Kokat Va Smetana 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.847141+05
nasr simba pamidor 25 gr	Nasr Simba Pamidor 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nasr simba pamidor 25 gr", "name": "Nasr Simba Pamidor 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.847281+05
navro'z semechka 100gr	Navro'z Semechka 100gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "navro'z semechka 100gr", "name": "Navro'z Semechka 100gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.847861+05
navroz semechka 100 gr	Navroz Semechka 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "navroz semechka 100 gr", "name": "Navroz Semechka 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.847999+05
navruz oq pista 40 gr	Navruz Oq Pista 40 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "navruz oq pista 40 gr", "name": "Navruz Oq Pista 40 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.848134+05
nemat limon 50gr	Nemat Limon 50gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nemat limon 50gr", "name": "Nemat Limon 50gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.848287+05
newﬂex termosystem	Newﬂex Termosystem	Kg		tayyor mahsulot	{"uom": "Kg", "code": "newﬂex termosystem", "name": "Newﬂex Termosystem", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.84859+05
nil pak salfetka 120ta	Nil Pak Salfetka 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nil pak salfetka 120ta", "name": "Nil Pak Salfetka 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.848741+05
nilpak salfetka 1 dona	Nilpak Salfetka 1 Dona	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nilpak salfetka 1 dona", "name": "Nilpak Salfetka 1 Dona", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.848884+05
salfetk dushanbe 120ta	Salfetk Dushanbe 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetk dushanbe 120ta", "name": "Salfetk Dushanbe 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.974883+05
salfetka aroma 15 sht	Salfetka Aroma 15 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka aroma 15 sht", "name": "Salfetka Aroma 15 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.976363+05
1010/45 pe pr toza	PE PR 1010/45	Kg		homashyo	{"uom": "Kg", "code": "1010/45 pe pr toza", "name": "PE PR 1010/45", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.442898+05
cheers nachos sous salsa 130 gr 41 sm	Cheers Nachos Sous Salsa 130 Gr 41 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nachos sous salsa 130 gr 41 sm", "name": "Cheers Nachos Sous Salsa 130 Gr 41 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.690969+05
mumtoz paket	Mumtoz Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mumtoz paket", "name": "Mumtoz Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.839921+05
musatafo holod prize 65 gr	Musatafo Holod Prize 65 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musatafo holod prize 65 gr", "name": "Musatafo Holod Prize 65 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.841604+05
musatafo holod sultan eskimo	Musatafo Holod Sultan Eskimo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musatafo holod sultan eskimo", "name": "Musatafo Holod Sultan Eskimo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.841761+05
jem 490/50 kar	JEM 490/50	Kg		homashyo	{"uom": "Kg", "code": "jem 490/50 kar", "name": "JEM 490/50", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.761512+05
nasr simba chips qaymoq 25 gr	Nasr Simba Chips Qaymoq 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nasr simba chips qaymoq 25 gr", "name": "Nasr Simba Chips Qaymoq 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.846716+05
nasr pf 13 sm	Nasr Pf 13 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nasr pf 13 sm", "name": "Nasr Pf 13 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.8463+05
nasr vazvrat	Nasr Vazvrat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nasr vazvrat", "name": "Nasr Vazvrat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.847557+05
naturalnoe yagoda 70 gr	Naturalnoe Yagoda 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "naturalnoe yagoda 70 gr", "name": "Naturalnoe Yagoda 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.847708+05
nilpak salfetka 50 sht	Nilpak Salfetka 50 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nilpak salfetka 50 sht", "name": "Nilpak Salfetka 50 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.84921+05
novamix 10 kg paket	Novamix 10 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "novamix 10 kg paket", "name": "Novamix 10 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.849374+05
noxat nut 900gr	Noxat Nut 900gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "noxat nut 900gr", "name": "Noxat Nut 900gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.849524+05
nu pogodi 60 gr	Nu Pogodi 60 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nu pogodi 60 gr", "name": "Nu Pogodi 60 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.849675+05
nukus dlisha 500 gr asarti paket	Nukus Dlisha 500 Gr Asarti Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nukus dlisha 500 gr asarti paket", "name": "Nukus Dlisha 500 Gr Asarti Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.849825+05
nukus dlisha anor 60 gr	Nukus Dlisha Anor 60 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nukus dlisha anor 60 gr", "name": "Nukus Dlisha Anor 60 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.849975+05
nukus dlisha keks	Nukus Dlisha Keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nukus dlisha keks", "name": "Nukus Dlisha Keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.850127+05
nuppy tvist	Nuppy Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nuppy tvist", "name": "Nuppy Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.850307+05
nyam nyam kukuruz paket	Nyam Nyam Kukuruz Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nyam nyam kukuruz paket", "name": "Nyam Nyam Kukuruz Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.850534+05
obi hayot salfetka	Obi Hayot Salfetka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "obi hayot salfetka", "name": "Obi Hayot Salfetka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.850854+05
odno chips	Odno Chips	Kg		tayyor mahsulot	{"uom": "Kg", "code": "odno chips", "name": "Odno Chips", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.851008+05
okey	Okey	Kg		tayyor mahsulot	{"uom": "Kg", "code": "okey", "name": "Okey", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.851309+05
old spice captain 115 gr	Old Spice Captain 115 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "old spice captain 115 gr", "name": "Old Spice Captain 115 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.85146+05
oldin biday uni 1kg paket	Oldin Biday Uni 1kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "oldin biday uni 1kg paket", "name": "Oldin Biday Uni 1kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.851612+05
ole pechenye 14 gr	Ole Pechenye 14 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ole pechenye 14 gr", "name": "Ole Pechenye 14 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.851756+05
opp  1350/20	OPP 1350/20	Kg		homashyo	{"uom": "Kg", "code": "opp  1350/20", "name": "OPP 1350/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.851912+05
opp  1350/30	OPP 1350/30	Kg		homashyo	{"uom": "Kg", "code": "opp  1350/30", "name": "OPP 1350/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.852069+05
opp 10x15 skoch paket	Opp 10x15 Skoch Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp 10x15 skoch paket", "name": "Opp 10x15 Skoch Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.854182+05
opp 20x30 skoch paket	Opp 20x30 Skoch Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp 20x30 skoch paket", "name": "Opp 20x30 Skoch Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.860495+05
opp 24/35 skoch paket	Opp 24/35 Skoch Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp 24/35 skoch paket", "name": "Opp 24/35 Skoch Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.860637+05
opp 25/40 skoch paket	Opp 25/40 Skoch Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp 25/40 skoch paket", "name": "Opp 25/40 Skoch Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.860782+05
opp 35x50 skoch paket	Opp 35x50 Skoch Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp 35x50 skoch paket", "name": "Opp 35x50 Skoch Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.861572+05
kreko salyami 50 gr	Kreko Salyami 50 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kreko salyami 50 gr", "name": "Kreko Salyami 50 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.782122+05
nakta beshbarmak 900 gr paket	Nakta Beshbarmak 900 Gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nakta beshbarmak 900 gr paket", "name": "Nakta Beshbarmak 900 Gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.845574+05
nasr simba barbekyu shashlik 25 gr	Nasr Simba Barbekyu Shashlik 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nasr simba barbekyu shashlik 25 gr", "name": "Nasr Simba Barbekyu Shashlik 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.846434+05
opp 12x17 skoch paket	Opp 12x17 Skoch Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp 12x17 skoch paket", "name": "Opp 12x17 Skoch Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.85607+05
opp 16x24 skoch paket	Opp 16x24 Skoch Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp 16x24 skoch paket", "name": "Opp 16x24 Skoch Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.859872+05
cheers dropz chili	Cheers Dropz Chili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers dropz chili", "name": "Cheers Dropz Chili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.688335+05
kuk suxarik sir	Kuk Suxarik Sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kuk suxarik sir", "name": "Kuk Suxarik Sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.787014+05
nasr simba chips shashlik barbekyu 25 gr	Nasr Simba Chips Shashlik Barbekyu 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nasr simba chips shashlik barbekyu 25 gr", "name": "Nasr Simba Chips Shashlik Barbekyu 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.846998+05
nemat xalodniy payka	Nemat Xalodniy Payka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nemat xalodniy payka", "name": "Nemat Xalodniy Payka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.848437+05
opp 915	OPP 915	Kg		homashyo	{"uom": "Kg", "code": "opp 915", "name": "OPP 915", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.905919+05
opp skoch paket 30/40	Opp Skoch Paket 30/40	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp skoch paket 30/40", "name": "Opp Skoch Paket 30/40", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.908951+05
opp+cpp 22sm	Opp+cpp 22sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp+cpp 22sm", "name": "Opp+cpp 22sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.909589+05
orbita 70 gr	Orbita 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "orbita 70 gr", "name": "Orbita 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.925762+05
orzu arbuz dinya banan 60 gr	Orzu Arbuz Dinya Banan 60 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "orzu arbuz dinya banan 60 gr", "name": "Orzu Arbuz Dinya Banan 60 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.925909+05
orzu black gold 999.9	Orzu Black Gold 999.9	Kg		tayyor mahsulot	{"uom": "Kg", "code": "orzu black gold 999.9", "name": "Orzu Black Gold 999.9", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.926046+05
orzu juice lyod 80 gr asarti	Orzu Juice Lyod 80 Gr Asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "orzu juice lyod 80 gr asarti", "name": "Orzu Juice Lyod 80 Gr Asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.926195+05
orzu supper vkusnyashka 50 gr	Orzu Supper Vkusnyashka 50 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "orzu supper vkusnyashka 50 gr", "name": "Orzu Supper Vkusnyashka 50 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.927231+05
osh tuzi kristal 2kg paket	Osh Tuzi Kristal 2kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "osh tuzi kristal 2kg paket", "name": "Osh Tuzi Kristal 2kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.927391+05
osiyo un 1kg paket	Osiyo Un 1kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "osiyo un 1kg paket", "name": "Osiyo Un 1kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.927542+05
osiyo un 2kg	Osiyo Un 2kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "osiyo un 2kg", "name": "Osiyo Un 2kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.927687+05
ozbegim bolajon chikken paket	Ozbegim Bolajon Chikken Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ozbegim bolajon chikken paket", "name": "Ozbegim Bolajon Chikken Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.928271+05
ozbegim sendvich paket	Ozbegim Sendvich Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ozbegim sendvich paket", "name": "Ozbegim Sendvich Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.928747+05
ozru plombir melody 100 gr	Ozru Plombir Melody 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ozru plombir melody 100 gr", "name": "Ozru Plombir Melody 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.929069+05
paket 100x150	Paket 100x150	Kg		tayyor mahsulot	{"uom": "Kg", "code": "paket 100x150", "name": "Paket 100x150", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.929229+05
paket 10x20	Paket 10x20	Kg		tayyor mahsulot	{"uom": "Kg", "code": "paket 10x20", "name": "Paket 10x20", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.929364+05
paket 18,5x6,5	Paket 18,5x6,5	Kg		tayyor mahsulot	{"uom": "Kg", "code": "paket 18,5x6,5", "name": "Paket 18,5x6,5", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.929538+05
paket 215x110	Paket 215x110	Kg		tayyor mahsulot	{"uom": "Kg", "code": "paket 215x110", "name": "Paket 215x110", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.929696+05
paket 21×13,5 sm	Paket 21×13,5 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "paket 21×13,5 sm", "name": "Paket 21×13,5 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.929858+05
paket 235x110	Paket 235x110	Kg		tayyor mahsulot	{"uom": "Kg", "code": "paket 235x110", "name": "Paket 235x110", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.930031+05
palma jidkiy aboy	Palma Jidkiy Aboy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "palma jidkiy aboy", "name": "Palma Jidkiy Aboy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.93021+05
palyot qurt rulon	Palyot Qurt Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "palyot qurt rulon", "name": "Palyot Qurt Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.930388+05
salfetka avellla asarti 72 sht	Salfetka Avellla Asarti 72 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka avellla asarti 72 sht", "name": "Salfetka Avellla Asarti 72 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.976512+05
chaps sir	Chaps Sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps sir", "name": "Chaps Sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.686312+05
chaps sir 38 sm	Chaps Sir 38 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps sir 38 sm", "name": "Chaps Sir 38 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.686575+05
chaps sir kotta 36 sm	Chaps Sir Kotta 36 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps sir kotta 36 sm", "name": "Chaps Sir Kotta 36 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.686713+05
opp 1350/20 pff msp	OPP PFF MSP 1350/20	Kg		homashyo	{"uom": "Kg", "code": "opp 1350/20 pff msp", "name": "OPP PFF MSP 1350/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.856625+05
opp skoch paket 35/50	Opp Skoch Paket 35/50	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp skoch paket 35/50", "name": "Opp Skoch Paket 35/50", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.909105+05
opp paket 15.5/21	Opp Paket 15.5/21	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp paket 15.5/21", "name": "Opp Paket 15.5/21", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.908411+05
pankie morojniy 80 gr	Pankie Morojniy 80 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pankie morojniy 80 gr", "name": "Pankie Morojniy 80 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.930861+05
papay 200 dona paket	Papay 200 Dona Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "papay 200 dona paket", "name": "Papay 200 Dona Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.931031+05
parvoz ayiq	Parvoz Ayiq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "parvoz ayiq", "name": "Parvoz Ayiq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.931189+05
parvoz dollar tvist	Parvoz Dollar Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "parvoz dollar tvist", "name": "Parvoz Dollar Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.93136+05
parvoz paket 1kg	Parvoz Paket 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "parvoz paket 1kg", "name": "Parvoz Paket 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.931495+05
patirchi sir 170 gr	Patirchi Sir 170 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "patirchi sir 170 gr", "name": "Patirchi Sir 170 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.9318+05
patitos 20gr asartiy	Patitos 20gr Asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "patitos 20gr asartiy", "name": "Patitos 20gr Asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.93195+05
patitos chips shashlik 20gr	Patitos Chips Shashlik 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "patitos chips shashlik 20gr", "name": "Patitos Chips Shashlik 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.932261+05
payushi 10sm	Payushi 10sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "payushi 10sm", "name": "Payushi 10sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.932418+05
payushi 25x35	Payushi 25x35	Kg		tayyor mahsulot	{"uom": "Kg", "code": "payushi 25x35", "name": "Payushi 25x35", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.932586+05
payushi 28cm	Payushi 28cm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "payushi 28cm", "name": "Payushi 28cm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.932767+05
payushi 4sm	Payushi 4sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "payushi 4sm", "name": "Payushi 4sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.9331+05
payushi 5,5sm	Payushi 5,5sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "payushi 5,5sm", "name": "Payushi 5,5sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.933257+05
payushi 7sm	Payushi 7sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "payushi 7sm", "name": "Payushi 7sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.933586+05
payushi 98mm	Payushi 98mm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "payushi 98mm", "name": "Payushi 98mm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.933752+05
payushi opp 24sm	Payushi Opp 24sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "payushi opp 24sm", "name": "Payushi Opp 24sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.934254+05
payushiy 12 sm	Payushiy 12 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "payushiy 12 sm", "name": "Payushiy 12 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.934426+05
payushiy 23sm	Payushiy 23sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "payushiy 23sm", "name": "Payushiy 23sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.934605+05
pe oq	PE oq	Kg		homashyo	{"uom": "Kg", "code": "pe oq", "name": "PE oq", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.934952+05
pe paket 32x45	Pe Paket 32x45	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pe paket 32x45", "name": "Pe Paket 32x45", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.93531+05
pe paket 40x45	Pe Paket 40x45	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pe paket 40x45", "name": "Pe Paket 40x45", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.935809+05
pe paket 85 mk	Pe Paket 85 Mk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pe paket 85 mk", "name": "Pe Paket 85 Mk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.935969+05
pe paket 95 mk	Pe Paket 95 Mk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pe paket 95 mk", "name": "Pe Paket 95 Mk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.936162+05
pe prazrachka 59sm	Pe Prazrachka 59sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pe prazrachka 59sm", "name": "Pe Prazrachka 59sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.936629+05
pe+spp 505mm	Pe+spp 505mm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pe+spp 505mm", "name": "Pe+spp 505mm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.936778+05
penda	Penda	Kg		tayyor mahsulot	{"uom": "Kg", "code": "penda", "name": "Penda", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.936927+05
peres krasniy slatkiy	Peres Krasniy Slatkiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "peres krasniy slatkiy", "name": "Peres Krasniy Slatkiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.937226+05
perlofka yormasi 1kg	Perlofka Yormasi 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "perlofka yormasi 1kg", "name": "Perlofka Yormasi 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.93738+05
pero nam salfetka	Pero Nam Salfetka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pero nam salfetka", "name": "Pero Nam Salfetka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.937548+05
ma go rulon	Ma Go Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ma go rulon", "name": "Ma Go Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.793297+05
mat 620/20 kar	MAT 620/20	Kg		homashyo	{"uom": "Kg", "code": "mat 620/20 kar", "name": "MAT 620/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.807924+05
opp plyonka	OPP plyonka	Kg		homashyo	{"uom": "Kg", "code": "opp plyonka", "name": "OPP plyonka", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.908573+05
panda myata karamel	Panda Myata Karamel	Kg		tayyor mahsulot	{"uom": "Kg", "code": "panda myata karamel", "name": "Panda Myata Karamel", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.930552+05
ower black	Ower Black	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ower black", "name": "Ower Black", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.927833+05
ozbegim bolajon sendvich paket	Ozbegim Bolajon Sendvich Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ozbegim bolajon sendvich paket", "name": "Ozbegim Bolajon Sendvich Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.928435+05
ozbegim chicken paket	Ozbegim Chicken Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ozbegim chicken paket", "name": "Ozbegim Chicken Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.928588+05
ozoda keks	Ozoda Keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ozoda keks", "name": "Ozoda Keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.928922+05
pe peket 100 mk	Pe Peket 100 Mk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pe peket 100 mk", "name": "Pe Peket 100 Mk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.936304+05
pet	PET	Kg		homashyo	{"uom": "Kg", "code": "pet", "name": "PET", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.937932+05
pet + pe 27,2 sm	Pet + Pe 27,2 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet + pe 27,2 sm", "name": "Pet + Pe 27,2 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.938244+05
pet + pe 39,5 sm	Pet + Pe 39,5 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet + pe 39,5 sm", "name": "Pet + Pe 39,5 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.938415+05
pet + pe 43 sm	Pet + Pe 43 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet + pe 43 sm", "name": "Pet + Pe 43 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.938581+05
pet + pe 80 sm rulon	Pet + Pe 80 Sm Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet + pe 80 sm rulon", "name": "Pet + Pe 80 Sm Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.938893+05
pet + pe paket	Pet + Pe Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet + pe paket", "name": "Pet + Pe Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.939075+05
pet + pr pe	Pet + Pr Pe	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet + pr pe", "name": "Pet + Pr Pe", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.939257+05
pet 100012	PET 100012	Kg		homashyo	{"uom": "Kg", "code": "pet 100012", "name": "PET 100012", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.939748+05
pet 12 mikron	PET 12 mikron	Kg		homashyo	{"uom": "Kg", "code": "pet 12 mikron", "name": "PET 12 mikron", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.939909+05
pet+cpp	Pet+cpp	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet+cpp", "name": "Pet+cpp", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.949768+05
pet+opp	Pet+opp	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet+opp", "name": "Pet+opp", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.949907+05
pet+pe	Pet+pe	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet+pe", "name": "Pet+pe", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.950223+05
pet+pe 15x35 paket	Pet+pe 15x35 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet+pe 15x35 paket", "name": "Pet+pe 15x35 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.950409+05
pet+pe 20x30	Pet+pe 20x30	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet+pe 20x30", "name": "Pet+pe 20x30", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.950659+05
pet+pe 20x30 paket	Pet+pe 20x30 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet+pe 20x30 paket", "name": "Pet+pe 20x30 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.950828+05
pet+spp 26sm	Pet+spp 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet+spp 26sm", "name": "Pet+spp 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.951001+05
pet12+pe40 26sm	Pet12+pe40 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet12+pe40 26sm", "name": "Pet12+pe40 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.951159+05
pf 120x30	Pf 120x30	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pf 120x30", "name": "Pf 120x30", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.951307+05
pf 130x18	Pf 130x18	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pf 130x18", "name": "Pf 130x18", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.951471+05
pf 15 sm	Pf 15 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pf 15 sm", "name": "Pf 15 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.951626+05
pf 22sm	Pf 22sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pf 22sm", "name": "Pf 22sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.951787+05
pf 25/35	Pf 25/35	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pf 25/35", "name": "Pf 25/35", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.951941+05
pistachi	Pistachi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pistachi", "name": "Pistachi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.952759+05
piyoz 1,5 kg paket	Piyoz 1,5 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "piyoz 1,5 kg paket", "name": "Piyoz 1,5 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.954421+05
piyoz 365 2.5kg	Piyoz 365 2.5kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "piyoz 365 2.5kg", "name": "Piyoz 365 2.5kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.954596+05
pompik corn stick 35 gr	Pompik Corn Stick 35 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pompik corn stick 35 gr", "name": "Pompik Corn Stick 35 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.955925+05
magiv roller shokolad 85 gr	Magiv Roller Shokolad 85 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "magiv roller shokolad 85 gr", "name": "Magiv Roller Shokolad 85 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.794032+05
mango 70 gr	Mango 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mango 70 gr", "name": "Mango 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.79966+05
oymomo 520 mm	Oymomo 520 Mm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "oymomo 520 mm", "name": "Oymomo 520 Mm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.927985+05
oymomo asarti 500 mm	Oymomo Asarti 500 Mm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "oymomo asarti 500 mm", "name": "Oymomo Asarti 500 Mm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.928116+05
pe paket 35x45	Pe Paket 35x45	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pe paket 35x45", "name": "Pe Paket 35x45", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.935472+05
pe paket 40x40	Pe Paket 40x40	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pe paket 40x40", "name": "Pe Paket 40x40", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.935636+05
pistachi 20gr	Pistachi 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pistachi 20gr", "name": "Pistachi 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.952917+05
pistello tuzliy oq kotta	Pistello Tuzliy Oq Kotta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pistello tuzliy oq kotta", "name": "Pistello Tuzliy Oq Kotta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.953557+05
pom pik 300gr paket	Pom Pik 300gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pom pik 300gr paket", "name": "Pom Pik 300gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.9555+05
pompik 100gr paket	Pompik 100gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pompik 100gr paket", "name": "Pompik 100gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.95565+05
pompik 90gr paket	Pompik 90gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pompik 90gr paket", "name": "Pompik 90gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.955777+05
pompik corn sticks 40 gr	Pompik Corn Sticks 40 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pompik corn sticks 40 gr", "name": "Pompik Corn Sticks 40 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.956064+05
popkorin paket	Popkorin Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "popkorin paket", "name": "Popkorin Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.956214+05
poppy pop	Poppy Pop	Kg		tayyor mahsulot	{"uom": "Kg", "code": "poppy pop", "name": "Poppy Pop", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.956354+05
portativ ovkat paket	Portativ Ovkat Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "portativ ovkat paket", "name": "Portativ Ovkat Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.956489+05
pover marmelad	Pover Marmelad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pover marmelad", "name": "Pover Marmelad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.956623+05
pover marmelat engi	Pover Marmelat Engi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pover marmelat engi", "name": "Pover Marmelat Engi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.956989+05
prada diyor EA7140 gr usluga	prada diyor EA7140 gr usluga	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prada diyor EA7140 gr usluga", "name": "prada diyor EA7140 gr usluga", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.957144+05
praniki 365 kun malina jemli paket	Praniki 365 Kun Malina Jemli Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "praniki 365 kun malina jemli paket", "name": "Praniki 365 Kun Malina Jemli Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.957438+05
praniki 365 kun plombir tami paket	Praniki 365 Kun Plombir Tami Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "praniki 365 kun plombir tami paket", "name": "Praniki 365 Kun Plombir Tami Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.957575+05
prazrachka 31sm	Prazrachka 31sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachka 31sm", "name": "Prazrachka 31sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.958413+05
prazrachniy 12sm	Prazrachniy 12sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachniy 12sm", "name": "Prazrachniy 12sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.958563+05
prazrachniy rulon 26sm	Prazrachniy Rulon 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachniy rulon 26sm", "name": "Prazrachniy Rulon 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.959731+05
prazrachniy skocht paket 25/35	Prazrachniy Skocht Paket 25/35	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachniy skocht paket 25/35", "name": "Prazrachniy Skocht Paket 25/35", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.959918+05
prazrachniy skotch paket 20/29	Prazrachniy Skotch Paket 20/29	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachniy skotch paket 20/29", "name": "Prazrachniy Skotch Paket 20/29", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.960065+05
premium cotton zamok paket	Premium Cotton Zamok Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "premium cotton zamok paket", "name": "Premium Cotton Zamok Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.960934+05
primer mayka zip paket	Primer Mayka Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "primer mayka zip paket", "name": "Primer Mayka Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.961275+05
prince 50 gr	Prince 50 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prince 50 gr", "name": "Prince 50 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.961415+05
princessa vaﬂi 25 gr	Princessa Vaﬂi 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "princessa vaﬂi 25 gr", "name": "Princessa Vaﬂi 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.961554+05
priprava kurinnaya	Priprava Kurinnaya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "priprava kurinnaya", "name": "Priprava Kurinnaya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.962183+05
pryanik pf 23sm	Pryanik Pf 23sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pryanik pf 23sm", "name": "Pryanik Pf 23sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.962503+05
pure milky sirok kakao	Pure Milky Sirok Kakao	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pure milky sirok kakao", "name": "Pure Milky Sirok Kakao", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.962668+05
mat 470/20 kar	MAT 470/20	Kg		homashyo	{"uom": "Kg", "code": "mat 470/20 kar", "name": "MAT 470/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.804718+05
payushi 5.5sm	Payushi 5.5sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "payushi 5.5sm", "name": "Payushi 5.5sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.933415+05
payushi 9sm	Payushi 9sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "payushi 9sm", "name": "Payushi 9sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.934079+05
pe paket 30/40	Pe Paket 30/40	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pe paket 30/40", "name": "Pe Paket 30/40", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.93513+05
plambir marojniy	Plambir Marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "plambir marojniy", "name": "Plambir Marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.954751+05
prayniki asal 600gr	Prayniki Asal 600gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prayniki asal 600gr", "name": "Prayniki Asal 600gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.957991+05
prazrachka 14sm	Prazrachka 14sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachka 14sm", "name": "Prazrachka 14sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.958124+05
prazrachka 19sm	Prazrachka 19sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachka 19sm", "name": "Prazrachka 19sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.958269+05
prazrachniy 15x15 paket	Prazrachniy 15x15 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachniy 15x15 paket", "name": "Prazrachniy 15x15 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.958697+05
prazrachniy 17x16,5 paket	Prazrachniy 17x16,5 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachniy 17x16,5 paket", "name": "Prazrachniy 17x16,5 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.958839+05
prazrachniy 46 sm	Prazrachniy 46 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachniy 46 sm", "name": "Prazrachniy 46 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.959146+05
prazrachniy pe skoch paket	Prazrachniy Pe Skoch Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachniy pe skoch paket", "name": "Prazrachniy Pe Skoch Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.95957+05
prazrachniy vakum	Prazrachniy Vakum	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachniy vakum", "name": "Prazrachniy Vakum", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.960209+05
prazrachniy zip paket 35sm	Prazrachniy Zip Paket 35sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachniy zip paket 35sm", "name": "Prazrachniy Zip Paket 35sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.960506+05
premium baby aloe 20 sht	Premium Baby Aloe 20 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "premium baby aloe 20 sht", "name": "Premium Baby Aloe 20 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.960726+05
princhy	Princhy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "princhy", "name": "Princhy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.961703+05
priprava govyadina	Priprava Govyadina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "priprava govyadina", "name": "Priprava Govyadina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.961866+05
priprava govyadina ostraya	Priprava Govyadina Ostraya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "priprava govyadina ostraya", "name": "Priprava Govyadina Ostraya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.962027+05
priprava kurinnaya ostraya	Priprava Kurinnaya Ostraya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "priprava kurinnaya ostraya", "name": "Priprava Kurinnaya Ostraya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.962332+05
qars qars	Qars Qars	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qars qars", "name": "Qars Qars", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.963914+05
qars qars karamel	Qars Qars Karamel	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qars qars karamel", "name": "Qars Qars Karamel", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.964089+05
qars qars karamel kotta	Qars Qars Karamel Kotta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qars qars karamel kotta", "name": "Qars Qars Karamel Kotta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.964245+05
qars qars kichik klasicheski	Qars Qars Kichik Klasicheski	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qars qars kichik klasicheski", "name": "Qars Qars Kichik Klasicheski", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.964415+05
qars qars klasika kotta	Qars Qars Klasika Kotta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qars qars klasika kotta", "name": "Qars Qars Klasika Kotta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.964652+05
qars qars popkorin karamel	Qars Qars Popkorin Karamel	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qars qars popkorin karamel", "name": "Qars Qars Popkorin Karamel", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.964806+05
qars qars shokalat kotta	Qars Qars Shokalat Kotta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qars qars shokalat kotta", "name": "Qars Qars Shokalat Kotta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.964966+05
qars qurs 30 gr arab	Qars Qurs 30 Gr Arab	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qars qurs 30 gr arab", "name": "Qars Qurs 30 Gr Arab", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.965105+05
qars qurs 5d 20 gr	Qars Qurs 5d 20 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qars qurs 5d 20 gr", "name": "Qars Qurs 5d 20 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.965266+05
qars qurs asartiy	Qars Qurs Asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qars qurs asartiy", "name": "Qars Qurs Asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.965406+05
qars qurs xalol 20	Qars Qurs Xalol 20	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qars qurs xalol 20", "name": "Qars Qurs Xalol 20", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.965979+05
qizil yasmiq 900gr	Qizil Yasmiq 900gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qizil yasmiq 900gr", "name": "Qizil Yasmiq 900gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.966126+05
ramis poroshok 1,8 kg	Ramis Poroshok 1,8 Kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ramis poroshok 1,8 kg", "name": "Ramis Poroshok 1,8 Kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.967682+05
mat 595/20 kar	MAT 595/20	Kg		homashyo	{"uom": "Kg", "code": "mat 595/20 kar", "name": "MAT 595/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.807105+05
mat 825/20 kar	MAT 825/20	Kg		homashyo	{"uom": "Kg", "code": "mat 825/20 kar", "name": "MAT 825/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.812211+05
playowo zip paket	Playowo Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "playowo zip paket", "name": "Playowo Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.954897+05
pokiza malak	Pokiza Malak	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pokiza malak", "name": "Pokiza Malak", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.955053+05
qog'oz salfetkalar paket	Qog'oz Salfetkalar Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qog'oz salfetkalar paket", "name": "Qog'oz Salfetkalar Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.966265+05
qars qurs qurt	Qars Qurs Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qars qurs qurt", "name": "Qars Qurs Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.965696+05
qozoxiston tvist	Qozoxiston Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qozoxiston tvist", "name": "Qozoxiston Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.966407+05
qudus paket	Qudus Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qudus paket", "name": "Qudus Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.966548+05
quritilgan meva paket	Quritilgan Meva Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "quritilgan meva paket", "name": "Quritilgan Meva Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.966685+05
qurt 100dona paket	Qurt 100dona Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qurt 100dona paket", "name": "Qurt 100dona Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.966836+05
quvnoq suxariki	Quvnoq Suxariki	Kg		tayyor mahsulot	{"uom": "Kg", "code": "quvnoq suxariki", "name": "Quvnoq Suxariki", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.966969+05
quvonch kungaboqar mag'zi	Quvonch Kungaboqar Mag'zi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "quvonch kungaboqar mag'zi", "name": "Quvonch Kungaboqar Mag'zi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.967103+05
ramanov pista yashil 85gr	Ramanov Pista Yashil 85gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ramanov pista yashil 85gr", "name": "Ramanov Pista Yashil 85gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.967253+05
ramis parashok 3 kg yashil	Ramis Parashok 3 Kg Yashil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ramis parashok 3 kg yashil", "name": "Ramis Parashok 3 Kg Yashil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.967387+05
realniy plombir malina 100 gr	Realniy Plombir Malina 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "realniy plombir malina 100 gr", "name": "Realniy Plombir Malina 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.968689+05
rena parashok paket	Rena Parashok Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "rena parashok paket", "name": "Rena Parashok Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.969555+05
repost zip paket	Repost Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "repost zip paket", "name": "Repost Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.969707+05
ret mini	Ret Mini	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ret mini", "name": "Ret Mini", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.969854+05
rohat lazat 2kg paket	Rohat Lazat 2kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "rohat lazat 2kg paket", "name": "Rohat Lazat 2kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.970026+05
rojok ice grand 85 gr yashil	Rojok Ice Grand 85 Gr Yashil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "rojok ice grand 85 gr yashil", "name": "Rojok Ice Grand 85 Gr Yashil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.970173+05
rokko	Rokko	Kg		tayyor mahsulot	{"uom": "Kg", "code": "rokko", "name": "Rokko", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.970473+05
romanov pista	Romanov Pista	Kg		tayyor mahsulot	{"uom": "Kg", "code": "romanov pista", "name": "Romanov Pista", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.970631+05
romantik	Romantik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "romantik", "name": "Romantik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.970873+05
romantik baltik	Romantik Baltik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "romantik baltik", "name": "Romantik Baltik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.971106+05
roshka vaﬀers xolodnaya payka	Roshka Vaﬀers Xolodnaya Payka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "roshka vaﬀers xolodnaya payka", "name": "Roshka Vaﬀers Xolodnaya Payka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.971454+05
roxat osh tuzi paket	Roxat Osh Tuzi Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "roxat osh tuzi paket", "name": "Roxat Osh Tuzi Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.971772+05
roze vet vipis 120ta	Roze Vet Vipis 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "roze vet vipis 120ta", "name": "Roze Vet Vipis 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.972056+05
rustam opp skoch paket	Rustam Opp Skoch Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "rustam opp skoch paket", "name": "Rustam Opp Skoch Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.972235+05
rustam payushiy opp	Rustam Payushiy Opp	Kg		tayyor mahsulot	{"uom": "Kg", "code": "rustam payushiy opp", "name": "Rustam Payushiy Opp", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.972384+05
s	S	Kg		tayyor mahsulot	{"uom": "Kg", "code": "s", "name": "S", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.972551+05
s novim godom paket	S Novim Godom Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "s novim godom paket", "name": "S Novim Godom Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.972687+05
sab sendvich	Sab Sendvich	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sab sendvich", "name": "Sab Sendvich", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.972835+05
sadaf pista 2 kg paket	Sadaf Pista 2 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sadaf pista 2 kg paket", "name": "Sadaf Pista 2 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.972991+05
musaﬀo bio kiﬁr 900gr 2.5%	Musaﬀo Bio Kiﬁr 900gr 2.5%	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musaﬀo bio kiﬁr 900gr 2.5%", "name": "Musaﬀo Bio Kiﬁr 900gr 2.5%", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.842234+05
qars aqrs kichik shokalad	Qars Aqrs Kichik Shokalad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qars aqrs kichik shokalad", "name": "Qars Aqrs Kichik Shokalad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.963749+05
qars qurs sir paekt	Qars Qurs Sir Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qars qurs sir paekt", "name": "Qars Qurs Sir Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.965843+05
rekord qurt	Rekord Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "rekord qurt", "name": "Rekord Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.969387+05
realniy plombir ﬁshtashka 100 gr	Realniy Plombir Fishtashka 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "realniy plombir ﬁshtashka 100 gr", "name": "Realniy Plombir Fishtashka 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.969027+05
rose wet wipes 210ta	Rose Wet Wipes 210ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "rose wet wipes 210ta", "name": "Rose Wet Wipes 210ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.971299+05
rosi trufel	Rosi Trufel	Kg		tayyor mahsulot	{"uom": "Kg", "code": "rosi trufel", "name": "Rosi Trufel", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.97161+05
roxat pista 10gr	Roxat Pista 10gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "roxat pista 10gr", "name": "Roxat Pista 10gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.971912+05
sakafan pekt 36x44sm	Sakafan Pekt 36x44sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sakafan pekt 36x44sm", "name": "Sakafan Pekt 36x44sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.973512+05
salfeta avello baby 120 sht asarti	Salfeta Avello Baby 120 Sht Asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfeta avello baby 120 sht asarti", "name": "Salfeta Avello Baby 120 Sht Asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.974743+05
salfetka XNS freshwipes 15ta	salfetka XNS freshwipes 15ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka XNS freshwipes 15ta", "name": "salfetka XNS freshwipes 15ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.975219+05
salfetka XNS kids 120ta	salfetka XNS kids 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka XNS kids 120ta", "name": "salfetka XNS kids 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.975367+05
salfetka XNS kids 120ta bunny wipes	salfetka XNS kids 120ta bunny wipes	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka XNS kids 120ta bunny wipes", "name": "salfetka XNS kids 120ta bunny wipes", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.975505+05
salfetka XNS man-women 20ta	salfetka XNS man-women 20ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka XNS man-women 20ta", "name": "salfetka XNS man-women 20ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.975786+05
salfetka aloe baby 15 sht	Salfetka Aloe Baby 15 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka aloe baby 15 sht", "name": "Salfetka Aloe Baby 15 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.975926+05
salfetka antibacti red rose 72 sht	Salfetka Antibacti Red Rose 72 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka antibacti red rose 72 sht", "name": "Salfetka Antibacti Red Rose 72 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.976213+05
salfetka avello men just for 72 sht	Salfetka Avello Men Just For 72 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka avello men just for 72 sht", "name": "Salfetka Avello Men Just For 72 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.976677+05
salfetka avello premium 120 sht	Salfetka Avello Premium 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka avello premium 120 sht", "name": "Salfetka Avello Premium 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.976829+05
salfetka blesk 120 sht yashil kok	Salfetka Blesk 120 Sht Yashil Kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka blesk 120 sht yashil kok", "name": "Salfetka Blesk 120 Sht Yashil Kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.977113+05
salfetka blesk 120sht	Salfetka Blesk 120sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka blesk 120sht", "name": "Salfetka Blesk 120sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.977255+05
salfetka blesk 12ta	Salfetka Blesk 12ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka blesk 12ta", "name": "Salfetka Blesk 12ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.977395+05
salfetka blesk 72 sht yashil kok	Salfetka Blesk 72 Sht Yashil Kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka blesk 72 sht yashil kok", "name": "Salfetka Blesk 72 Sht Yashil Kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.977535+05
salfetka for men 15ta	Salfetka For Men 15ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka for men 15ta", "name": "Salfetka For Men 15ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.979007+05
salfetka for women 120ta	Salfetka For Women 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka for women 120ta", "name": "Salfetka For Women 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.979151+05
salfetka magik antibakti 120 sht	Salfetka Magik Antibakti 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka magik antibakti 120 sht", "name": "Salfetka Magik Antibakti 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.979484+05
salfetka megic baby 72 sht	Salfetka Megic Baby 72 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka megic baby 72 sht", "name": "Salfetka Megic Baby 72 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.979805+05
salfetka nil pak bebiy 120ta	Salfetka Nil Pak Bebiy 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka nil pak bebiy 120ta", "name": "Salfetka Nil Pak Bebiy 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.979958+05
satti qurt 50 gr	Satti Qurt 50 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "satti qurt 50 gr", "name": "Satti Qurt 50 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.987427+05
trubichka 50ta paket	Trubichka 50ta Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "trubichka 50ta paket", "name": "Trubichka 50ta Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.023082+05
musaﬀo sirok shaftolili	Musaﬀo Sirok Shaftolili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musaﬀo sirok shaftolili", "name": "Musaﬀo Sirok Shaftolili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.843879+05
realniy plombir klubnika 100 gr	Realniy Plombir Klubnika 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "realniy plombir klubnika 100 gr", "name": "Realniy Plombir Klubnika 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.968528+05
red marmelad	Red Marmelad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "red marmelad", "name": "Red Marmelad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.969197+05
salfetka buny parfum 15ta	Salfetka Buny Parfum 15ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka buny parfum 15ta", "name": "Salfetka Buny Parfum 15ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.977982+05
salfetka eko for men 10ta	Salfetka Eko For Men 10ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka eko for men 10ta", "name": "Salfetka Eko For Men 10ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.978707+05
salfetka for men 120ta	Salfetka For Men 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka for men 120ta", "name": "Salfetka For Men 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.978856+05
salfetka ramiz kok jigarang 100 sht	Salfetka Ramiz Kok Jigarang 100 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka ramiz kok jigarang 100 sht", "name": "Salfetka Ramiz Kok Jigarang 100 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.980115+05
salfetka red rose for men 120 sht	Salfetka Red Rose For Men 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka red rose for men 120 sht", "name": "Salfetka Red Rose For Men 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.980264+05
salfetka sakura 120ta	Salfetka Sakura 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka sakura 120ta", "name": "Salfetka Sakura 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.980422+05
salfetka sensative antibacti 72 sht	Salfetka Sensative Antibacti 72 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka sensative antibacti 72 sht", "name": "Salfetka Sensative Antibacti 72 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.980588+05
salfetka sensative baby 72 sht	Salfetka Sensative Baby 72 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka sensative baby 72 sht", "name": "Salfetka Sensative Baby 72 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.980766+05
salfetka shower my baby 120 sht	Salfetka Shower My Baby 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka shower my baby 120 sht", "name": "Salfetka Shower My Baby 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.980999+05
salfetka viktoriya 15 sht	Salfetka Viktoriya 15 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka viktoriya 15 sht", "name": "Salfetka Viktoriya 15 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.981185+05
salfetka wet vipes Baby 120ta	salfetka wet vipes Baby 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka wet vipes Baby 120ta", "name": "salfetka wet vipes Baby 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.98136+05
salfetka xns buniy wet towels 120ta	Salfetka Xns Buniy Wet Towels 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka xns buniy wet towels 120ta", "name": "Salfetka Xns Buniy Wet Towels 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.981535+05
salfetka xns paris 120ta	Salfetka Xns Paris 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka xns paris 120ta", "name": "Salfetka Xns Paris 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.981722+05
samarqand chempion suxariki	Samarqand Chempion Suxariki	Kg		tayyor mahsulot	{"uom": "Kg", "code": "samarqand chempion suxariki", "name": "Samarqand Chempion Suxariki", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.981886+05
sancho 80gr smetana salyami	Sancho 80gr Smetana Salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sancho 80gr smetana salyami", "name": "Sancho 80gr Smetana Salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.982048+05
sancho chips	Sancho Chips	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sancho chips", "name": "Sancho Chips", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.982196+05
sandee chikken paket	Sandee Chikken Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sandee chikken paket", "name": "Sandee Chikken Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.982372+05
sardor 150 gr 12 sht rulon uz	Sardor 150 Gr 12 Sht Rulon Uz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor 150 gr 12 sht rulon uz", "name": "Sardor 150 Gr 12 Sht Rulon Uz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.984415+05
sardor grenki asarti	Sardor Grenki Asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor grenki asarti", "name": "Sardor Grenki Asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.984724+05
sardor kok krab	Sardor Kok Krab	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor kok krab", "name": "Sardor Kok Krab", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.984897+05
sardor kok salyami	Sardor Kok Salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor kok salyami", "name": "Sardor Kok Salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.98506+05
sardor kok shashlik	Sardor Kok Shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor kok shashlik", "name": "Sardor Kok Shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.985205+05
sardor kok sir	Sardor Kok Sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor kok sir", "name": "Sardor Kok Sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.985354+05
sardor kok smetana 70gr	Sardor Kok Smetana 70gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor kok smetana 70gr", "name": "Sardor Kok Smetana 70gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.985511+05
sardor kok smetana Xl	sardor kok smetana Xl	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor kok smetana Xl", "name": "sardor kok smetana Xl", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.985653+05
velik	Velik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "velik", "name": "Velik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.033584+05
opp  875/30	OPP 875/30	Kg		homashyo	{"uom": "Kg", "code": "opp  875/30", "name": "OPP 875/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.852847+05
opp 1350/30 kar	OPP 1350/30	Kg		homashyo	{"uom": "Kg", "code": "opp 1350/30 kar", "name": "OPP 1350/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.857202+05
salfetka day 120 sht	Salfetka Day 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka day 120 sht", "name": "Salfetka Day 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.978116+05
salfetka eko beybiy 10ta	Salfetka Eko Beybiy 10ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka eko beybiy 10ta", "name": "Salfetka Eko Beybiy 10ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.978394+05
opp 710/30	OPP 710/30	Kg		homashyo	{"uom": "Kg", "code": "opp 710/30", "name": "OPP 710/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.891532+05
sandey biskuit	Sandey Biskuit	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sandey biskuit", "name": "Sandey Biskuit", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.983195+05
santexnika premium falga	Santexnika Premium Falga	Kg		tayyor mahsulot	{"uom": "Kg", "code": "santexnika premium falga", "name": "Santexnika Premium Falga", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.983917+05
sardor 140 gr 16 sht rulon uz	Sardor 140 Gr 16 Sht Rulon Uz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor 140 gr 16 sht rulon uz", "name": "Sardor 140 Gr 16 Sht Rulon Uz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.984267+05
sardor kukruz palichkiy	Sardor Kukruz Palichkiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor kukruz palichkiy", "name": "Sardor Kukruz Palichkiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.985797+05
sardor pista 20 gr toshkent arab	Sardor Pista 20 Gr Toshkent Arab	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor pista 20 gr toshkent arab", "name": "Sardor Pista 20 Gr Toshkent Arab", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.985948+05
sardor semechk 100 gr 16 sht paket	Sardor Semechk 100 Gr 16 Sht Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor semechk 100 gr 16 sht paket", "name": "Sardor Semechk 100 Gr 16 Sht Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.98609+05
sardor semechka 200 gr 10 sht paket	Sardor Semechka 200 Gr 10 Sht Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor semechka 200 gr 10 sht paket", "name": "Sardor Semechka 200 Gr 10 Sht Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.986248+05
sardor semechka 50 gr 20 sht paket	Sardor Semechka 50 Gr 20 Sht Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor semechka 50 gr 20 sht paket", "name": "Sardor Semechka 50 Gr 20 Sht Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.986398+05
sardor simba 5kg paket	Sardor Simba 5kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor simba 5kg paket", "name": "Sardor Simba 5kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.986549+05
sardor simba kok kichik	Sardor Simba Kok Kichik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor simba kok kichik", "name": "Sardor Simba Kok Kichik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.986693+05
sardor simba qizil kotta	Sardor Simba Qizil Kotta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor simba qizil kotta", "name": "Sardor Simba Qizil Kotta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.98684+05
sardor simba yashil	Sardor Simba Yashil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor simba yashil", "name": "Sardor Simba Yashil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.986986+05
sardor simba yashil kichik	Sardor Simba Yashil Kichik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor simba yashil kichik", "name": "Sardor Simba Yashil Kichik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.987136+05
sarmak 1 kg paket	Sarmak 1 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sarmak 1 kg paket", "name": "Sarmak 1 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.987281+05
saﬁna 9999 140 gr	Saﬁna 9999 140 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "saﬁna 9999 140 gr", "name": "Saﬁna 9999 140 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.98924+05
schaslivki 70 gr	Schaslivki 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "schaslivki 70 gr", "name": "Schaslivki 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.989558+05
semechka tilla qozon	Semechka Tilla Qozon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "semechka tilla qozon", "name": "Semechka Tilla Qozon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.989768+05
semechka yadrio	Semechka Yadrio	Kg		tayyor mahsulot	{"uom": "Kg", "code": "semechka yadrio", "name": "Semechka Yadrio", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.989958+05
semka XL 2 kg paket	semka XL 2 kg paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "semka XL 2 kg paket", "name": "semka XL 2 kg paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.990135+05
sendvich chempion andijon paket	Sendvich Chempion Andijon Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sendvich chempion andijon paket", "name": "Sendvich Chempion Andijon Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.990295+05
sendvich chempion bolajon andijon	Sendvich Chempion Bolajon Andijon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sendvich chempion bolajon andijon", "name": "Sendvich Chempion Bolajon Andijon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.990613+05
sendvich chempion toshkent paket	Sendvich Chempion Toshkent Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sendvich chempion toshkent paket", "name": "Sendvich Chempion Toshkent Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.990768+05
sendvich took eti menen	Sendvich Took Eti Menen	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sendvich took eti menen", "name": "Sendvich Took Eti Menen", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.990985+05
shaja qurt	Shaja Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shaja qurt", "name": "Shaja Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.991425+05
shakar 1kg	Shakar 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shakar 1kg", "name": "Shakar 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.991589+05
opp 380/20 pff  pol	OPP PFF 380/20	Kg		homashyo	{"uom": "Kg", "code": "opp 380/20 pff  pol", "name": "OPP PFF 380/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.862664+05
opp 675/25 kar	OPP 675/25	Kg		homashyo	{"uom": "Kg", "code": "opp 675/25 kar", "name": "OPP 675/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.889044+05
sandia dinya 65 gr	Sandia Dinya 65 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sandia dinya 65 gr", "name": "Sandia Dinya 65 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.983551+05
santal 1,5 litr gazlangan	Santal 1,5 Litr Gazlangan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "santal 1,5 litr gazlangan", "name": "Santal 1,5 Litr Gazlangan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.983756+05
sayqal energy 80gr	Sayqal Energy 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sayqal energy 80gr", "name": "Sayqal Energy 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.988334+05
sayqal maks 80gr	Sayqal Maks 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sayqal maks 80gr", "name": "Sayqal Maks 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.988625+05
sayqal milliy 80gr	Sayqal Milliy 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sayqal milliy 80gr", "name": "Sayqal Milliy 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.988778+05
sayqal vanil + frend 110gr	Sayqal Vanil + Frend 110gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sayqal vanil + frend 110gr", "name": "Sayqal Vanil + Frend 110gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.988931+05
shampinoni 300gr	Shampinoni 300gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shampinoni 300gr", "name": "Shampinoni 300gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.991741+05
shampinoni 500gr	Shampinoni 500gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shampinoni 500gr", "name": "Shampinoni 500gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.991873+05
shams hoddog	Shams Hoddog	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shams hoddog", "name": "Shams Hoddog", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.992022+05
shams kuriniy sasiska	Shams Kuriniy Sasiska	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shams kuriniy sasiska", "name": "Shams Kuriniy Sasiska", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.992161+05
shariki paket	Shariki Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shariki paket", "name": "Shariki Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.992318+05
sharq group falga	Sharq Group Falga	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sharq group falga", "name": "Sharq Group Falga", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.992466+05
sharq sosiska	Sharq Sosiska	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sharq sosiska", "name": "Sharq Sosiska", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.992644+05
shashlik marinad rulon	Shashlik Marinad Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shashlik marinad rulon", "name": "Shashlik Marinad Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.992821+05
shax qurt	Shax Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shax qurt", "name": "Shax Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.992982+05
shoxasar kookes	Shoxasar Kookes	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shoxasar kookes", "name": "Shoxasar Kookes", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.994978+05
shoxasar mini maﬃni	Shoxasar Mini Maﬃni	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shoxasar mini maﬃni", "name": "Shoxasar Mini Maﬃni", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.995129+05
shoxasar payushiy 13 sm	Shoxasar Payushiy 13 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shoxasar payushiy 13 sm", "name": "Shoxasar Payushiy 13 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.995307+05
shoxasar sponj keks	Shoxasar Sponj Keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shoxasar sponj keks", "name": "Shoxasar Sponj Keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.995597+05
shoxasar vaﬂiy	Shoxasar Vaﬂiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shoxasar vaﬂiy", "name": "Shoxasar Vaﬂiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.995732+05
shoxasar waﬄe cake 45 gr	Shoxasar Waﬄe Cake 45 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shoxasar waﬄe cake 45 gr", "name": "Shoxasar Waﬄe Cake 45 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.995874+05
shoxzoda 80gr	Shoxzoda 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shoxzoda 80gr", "name": "Shoxzoda 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.996031+05
siklomat natriy paket	Siklomat Natriy Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "siklomat natriy paket", "name": "Siklomat Natriy Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.996196+05
silliqlangan noxat 1kg	Silliqlangan Noxat 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "silliqlangan noxat 1kg", "name": "Silliqlangan Noxat 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.996333+05
silver choy 100 gr paket	Silver Choy 100 Gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "silver choy 100 gr paket", "name": "Silver Choy 100 Gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.996486+05
silver iran 50gr paket	Silver Iran 50gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "silver iran 50gr paket", "name": "Silver Iran 50gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.996629+05
sim bom	Sim Bom	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sim bom", "name": "Sim Bom", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.996761+05
sim sim 2kg paket	Sim Sim 2kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sim sim 2kg paket", "name": "Sim Sim 2kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.9969+05
simba abrazes	Simba Abrazes	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba abrazes", "name": "Simba Abrazes", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.997046+05
simba chips pishloq	Simba Chips Pishloq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba chips pishloq", "name": "Simba Chips Pishloq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.997188+05
simba chips qaymoq 14gr	Simba Chips Qaymoq 14gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba chips qaymoq 14gr", "name": "Simba Chips Qaymoq 14gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.997329+05
simba chips salyami 14gr	Simba Chips Salyami 14gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba chips salyami 14gr", "name": "Simba Chips Salyami 14gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.997606+05
opp 470/20 kar	OPP 470/20	Kg		homashyo	{"uom": "Kg", "code": "opp 470/20 kar", "name": "OPP 470/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.869047+05
sayqal anor 80gr	Sayqal Anor 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sayqal anor 80gr", "name": "Sayqal Anor 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.988177+05
sayqal ice cream 110 gr	Sayqal Ice Cream 110 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sayqal ice cream 110 gr", "name": "Sayqal Ice Cream 110 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.988476+05
shoxasar keks 55sm	Shoxasar Keks 55sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shoxasar keks 55sm", "name": "Shoxasar Keks 55sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.994666+05
shoxasar 14/30	Shoxasar 14/30	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shoxasar 14/30", "name": "Shoxasar 14/30", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.994343+05
shoxasar keks mini maﬃni	Shoxasar Keks Mini Maﬃni	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shoxasar keks mini maﬃni", "name": "Shoxasar Keks Mini Maﬃni", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.994816+05
simba chips salyami 25gr	Simba Chips Salyami 25gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba chips salyami 25gr", "name": "Simba Chips Salyami 25gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.99774+05
simba chips shashlik 14gr	Simba Chips Shashlik 14gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba chips shashlik 14gr", "name": "Simba Chips Shashlik 14gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.99788+05
simba chips shashlik 25gr	Simba Chips Shashlik 25gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba chips shashlik 25gr", "name": "Simba Chips Shashlik 25gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.998043+05
simba-XL paket	simba-XL paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba-XL paket", "name": "simba-XL paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.999298+05
simbom obrazes	Simbom Obrazes	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simbom obrazes", "name": "Simbom Obrazes", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.999459+05
sir kadirov qizil	Sir Kadirov Qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sir kadirov qizil", "name": "Sir Kadirov Qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.999605+05
sir kadyroﬀ	Sir Kadyroﬀ	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sir kadyroﬀ", "name": "Sir Kadyroﬀ", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.999772+05
sir koziy paket	Sir Koziy Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sir koziy paket", "name": "Sir Koziy Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.999951+05
sladok nejenka 150gr	Sladok Nejenka 150gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sladok nejenka 150gr", "name": "Sladok Nejenka 150gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.000103+05
sladok pishechki 250gr	Sladok Pishechki 250gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sladok pishechki 250gr", "name": "Sladok Pishechki 250gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.000564+05
sladok zavariki 150gr	Sladok Zavariki 150gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sladok zavariki 150gr", "name": "Sladok Zavariki 150gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.000951+05
slivochnoe morojennoe 60 gr	Slivochnoe Morojennoe 60 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "slivochnoe morojennoe 60 gr", "name": "Slivochnoe Morojennoe 60 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.001582+05
smaylik asarti meva	Smaylik Asarti Meva	Kg		tayyor mahsulot	{"uom": "Kg", "code": "smaylik asarti meva", "name": "Smaylik Asarti Meva", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.001746+05
smile candy tablet	Smile Candy Tablet	Kg		tayyor mahsulot	{"uom": "Kg", "code": "smile candy tablet", "name": "Smile Candy Tablet", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.001891+05
sohil qurt	Sohil Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sohil qurt", "name": "Sohil Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.002031+05
solixa zip paket usluga	Solixa Zip Paket Usluga	Kg		tayyor mahsulot	{"uom": "Kg", "code": "solixa zip paket usluga", "name": "Solixa Zip Paket Usluga", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.00219+05
soﬀlucia for men	Soﬀlucia For Men	Kg		tayyor mahsulot	{"uom": "Kg", "code": "soﬀlucia for men", "name": "Soﬀlucia For Men", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.002356+05
speliy arbuz 65 gr	Speliy Arbuz 65 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "speliy arbuz 65 gr", "name": "Speliy Arbuz 65 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.002511+05
sponge layer cake	Sponge Layer Cake	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sponge layer cake", "name": "Sponge Layer Cake", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.002674+05
sponje kek asartiy	Sponje Kek Asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sponje kek asartiy", "name": "Sponje Kek Asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.00283+05
spring baby booom	Spring Baby Booom	Kg		tayyor mahsulot	{"uom": "Kg", "code": "spring baby booom", "name": "Spring Baby Booom", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.003314+05
spring chempion marojniy	Spring Chempion Marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "spring chempion marojniy", "name": "Spring Chempion Marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.003478+05
spring choco gold sevimli Record	spring choco gold sevimli Record	Kg		tayyor mahsulot	{"uom": "Kg", "code": "spring choco gold sevimli Record", "name": "spring choco gold sevimli Record", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.003709+05
spring panda 60gr	Spring Panda 60gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "spring panda 60gr", "name": "Spring Panda 60gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.004023+05
spring plombirushka	Spring Plombirushka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "spring plombirushka", "name": "Spring Plombirushka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.004176+05
spring vakum paket	Spring Vakum Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "spring vakum paket", "name": "Spring Vakum Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.004329+05
opp 500/30 pol	OPP 500/30	Kg		homashyo	{"uom": "Kg", "code": "opp 500/30 pol", "name": "OPP 500/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.872504+05
shox pie	Shox Pie	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shox pie", "name": "Shox Pie", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.994188+05
shoxasar Donuts	shoxasar Donuts	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shoxasar Donuts", "name": "shoxasar Donuts", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.994514+05
simba corn milky 50gr	Simba Corn Milky 50gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba corn milky 50gr", "name": "Simba Corn Milky 50gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.998661+05
simba kukruz olma 80gr	Simba Kukruz Olma 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba kukruz olma 80gr", "name": "Simba Kukruz Olma 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.999112+05
sladok nejinka 250 gr	Sladok Nejinka 250 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sladok nejinka 250 gr", "name": "Sladok Nejinka 250 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.00025+05
sladok pishechki	Sladok Pishechki	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sladok pishechki", "name": "Sladok Pishechki", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.000401+05
sssr fa plus 130 gr	Sssr Fa Plus 130 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sssr fa plus 130 gr", "name": "Sssr Fa Plus 130 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.004474+05
sssr morojniy 100 gr qizil	Sssr Morojniy 100 Gr Qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sssr morojniy 100 gr qizil", "name": "Sssr Morojniy 100 Gr Qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.00463+05
sssr morojniy 130 gr	Sssr Morojniy 130 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sssr morojniy 130 gr", "name": "Sssr Morojniy 130 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.004787+05
standart lavash 20 sht paket	Standart Lavash 20 Sht Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "standart lavash 20 sht paket", "name": "Standart Lavash 20 Sht Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.008056+05
stanles falga	Stanles Falga	Kg		tayyor mahsulot	{"uom": "Kg", "code": "stanles falga", "name": "Stanles Falga", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.008239+05
stiks suxarik shashlik 55gr	Stiks Suxarik Shashlik 55gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "stiks suxarik shashlik 55gr", "name": "Stiks Suxarik Shashlik 55gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.00908+05
stiks suxarik sir 55gr	Stiks Suxarik Sir 55gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "stiks suxarik sir 55gr", "name": "Stiks Suxarik Sir 55gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.009242+05
stiks suxarik smetana 55gr	Stiks Suxarik Smetana 55gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "stiks suxarik smetana 55gr", "name": "Stiks Suxarik Smetana 55gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.009414+05
stiks suxariki 55gr asartiy	Stiks Suxariki 55gr Asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "stiks suxariki 55gr asartiy", "name": "Stiks Suxariki 55gr Asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.009576+05
suli yormasi 500gr	Suli Yormasi 500gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "suli yormasi 500gr", "name": "Suli Yormasi 500gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.011506+05
sulton keks	Sulton Keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sulton keks", "name": "Sulton Keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.011836+05
sulton kids skotch paket	Sulton Kids Skotch Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sulton kids skotch paket", "name": "Sulton Kids Skotch Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.012096+05
super protein	Super Protein	Kg		tayyor mahsulot	{"uom": "Kg", "code": "super protein", "name": "Super Protein", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.012886+05
super smile semechka	Super Smile Semechka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "super smile semechka", "name": "Super Smile Semechka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.013071+05
supreme spicy 60 gr	Supreme Spicy 60 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "supreme spicy 60 gr", "name": "Supreme Spicy 60 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.013502+05
sushki hroom paket	Sushki Hroom Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sushki hroom paket", "name": "Sushki Hroom Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.013655+05
sutliy marojniy malochniy	Sutliy Marojniy Malochniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sutliy marojniy malochniy", "name": "Sutliy Marojniy Malochniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.013875+05
suxari uz salyami 25gr	Suxari Uz Salyami 25gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "suxari uz salyami 25gr", "name": "Suxari Uz Salyami 25gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.014233+05
suxari uz shashlik 25gr	Suxari Uz Shashlik 25gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "suxari uz shashlik 25gr", "name": "Suxari Uz Shashlik 25gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.014501+05
suxari uz sir 25gr	Suxari Uz Sir 25gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "suxari uz sir 25gr", "name": "Suxari Uz Sir 25gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.014674+05
suxari uz smetana 25gr	Suxari Uz Smetana 25gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "suxari uz smetana 25gr", "name": "Suxari Uz Smetana 25gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.014882+05
suxogrand qurt	Suxogrand Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "suxogrand qurt", "name": "Suxogrand Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.015125+05
svetliy gorod eskimo 80 gr	Svetliy Gorod Eskimo 80 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "svetliy gorod eskimo 80 gr", "name": "Svetliy Gorod Eskimo 80 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.015335+05
opp 515/30 PF kar	OPP 515/30	Kg		homashyo	{"uom": "Kg", "code": "opp 515/30 PF kar", "name": "OPP 515/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.87432+05
opp 575/18 pol	OPP 575/18	Kg		homashyo	{"uom": "Kg", "code": "opp 575/18 pol", "name": "OPP 575/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.880064+05
simba kukruz apelsin 80gr	Simba Kukruz Apelsin 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba kukruz apelsin 80gr", "name": "Simba Kukruz Apelsin 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.998799+05
simba kukruz klubnika 80gr	Simba Kukruz Klubnika 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba kukruz klubnika 80gr", "name": "Simba Kukruz Klubnika 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.99895+05
stiks suxariki smetana 20gr	Stiks Suxariki Smetana 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "stiks suxariki smetana 20gr", "name": "Stiks Suxariki Smetana 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.010043+05
strelka paket	Strelka Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "strelka paket", "name": "Strelka Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.010357+05
strike chips 20gr asartiy	Strike Chips 20gr Asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "strike chips 20gr asartiy", "name": "Strike Chips 20gr Asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.010562+05
taggis 500gr kok	Taggis 500gr Kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "taggis 500gr kok", "name": "Taggis 500gr Kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.015725+05
tamshan sendvich 170 gr	Tamshan Sendvich 170 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tamshan sendvich 170 gr", "name": "Tamshan Sendvich 170 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.016027+05
tanho pasta 300 gr	Tanho Pasta 300 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tanho pasta 300 gr", "name": "Tanho Pasta 300 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.016283+05
tanho pasta 500 gr	Tanho Pasta 500 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tanho pasta 500 gr", "name": "Tanho Pasta 500 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.016482+05
tanho pasta 700 gr	Tanho Pasta 700 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tanho pasta 700 gr", "name": "Tanho Pasta 700 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.01667+05
tanlov lavash 25 sht	Tanlov Lavash 25 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tanlov lavash 25 sht", "name": "Tanlov Lavash 25 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.016843+05
tanlov lavash paket	Tanlov Lavash Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tanlov lavash paket", "name": "Tanlov Lavash Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.017004+05
tarento toyis plus 5d	Tarento Toyis Plus 5d	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tarento toyis plus 5d", "name": "Tarento Toyis Plus 5d", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.017175+05
test	Test	Kg		tayyor mahsulot	{"uom": "Kg", "code": "test", "name": "Test", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.018963+05
test kraska	test kraska	Kg		kraska	{"uom": "Kg", "code": "test kraska", "name": "test kraska", "warehouse": "", "item_group": "kraska"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.019139+05
testo dlya katlama	Testo Dlya Katlama	Kg		tayyor mahsulot	{"uom": "Kg", "code": "testo dlya katlama", "name": "Testo Dlya Katlama", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.019311+05
testo dlya samsa paket	Testo Dlya Samsa Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "testo dlya samsa paket", "name": "Testo Dlya Samsa Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.019482+05
the qurt 5 ta kadr	The Qurt 5 Ta Kadr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "the qurt 5 ta kadr", "name": "The Qurt 5 Ta Kadr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.019634+05
the qurt 6 ta kadr	The Qurt 6 Ta Kadr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "the qurt 6 ta kadr", "name": "The Qurt 6 Ta Kadr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.019804+05
time vaﬂi rulon	Time Vaﬂi Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "time vaﬂi rulon", "name": "Time Vaﬂi Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.02011+05
tinny tvist	Tinny Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tinny tvist", "name": "Tinny Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.020279+05
tojik chop 8sm tejilik	Tojik Chop 8sm Tejilik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tojik chop 8sm tejilik", "name": "Tojik Chop 8sm Tejilik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.020591+05
tojik dey salfetka 120 sht	Tojik Dey Salfetka 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tojik dey salfetka 120 sht", "name": "Tojik Dey Salfetka 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.020757+05
toretto nacho 18 gr asarty	Toretto Nacho 18 Gr Asarty	Kg		tayyor mahsulot	{"uom": "Kg", "code": "toretto nacho 18 gr asarty", "name": "Toretto Nacho 18 Gr Asarty", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.021144+05
toretto pop chips	Toretto Pop Chips	Kg		tayyor mahsulot	{"uom": "Kg", "code": "toretto pop chips", "name": "Toretto Pop Chips", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.021488+05
toretto semechka 100 gr	Toretto Semechka 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "toretto semechka 100 gr", "name": "Toretto Semechka 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.021853+05
toxtaniyoz ota gril	Toxtaniyoz Ota Gril	Kg		tayyor mahsulot	{"uom": "Kg", "code": "toxtaniyoz ota gril", "name": "Toxtaniyoz Ota Gril", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.022201+05
toy paxta	Toy Paxta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "toy paxta", "name": "Toy Paxta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.022356+05
tri papa zip paket usluga	Tri Papa Zip Paket Usluga	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tri papa zip paket usluga", "name": "Tri Papa Zip Paket Usluga", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.022522+05
trokler 1kg	Trokler 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "trokler 1kg", "name": "Trokler 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.022736+05
trokler 500gr	Trokler 500gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "trokler 500gr", "name": "Trokler 500gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.022896+05
opp 540/30 PFF kar	OPP PFF 540/30	Kg		homashyo	{"uom": "Kg", "code": "opp 540/30 PFF kar", "name": "OPP PFF 540/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.877292+05
stiks suxarik salyami 20gr	Stiks Suxarik Salyami 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "stiks suxarik salyami 20gr", "name": "Stiks Suxarik Salyami 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.008727+05
stiks suxik asartiy 20gr	Stiks Suxik Asartiy 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "stiks suxik asartiy 20gr", "name": "Stiks Suxik Asartiy 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.010188+05
super gold 100 gr	Super Gold 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "super gold 100 gr", "name": "Super Gold 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.012706+05
tasty 1kg paket	Tasty 1kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tasty 1kg paket", "name": "Tasty 1kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.017702+05
tattu qurt	Tattu Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tattu qurt", "name": "Tattu Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.018117+05
tayfun paket	Tayfun Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tayfun paket", "name": "Tayfun Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.018287+05
teramisu tvist	Teramisu Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "teramisu tvist", "name": "Teramisu Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.018801+05
trubichka 7,5x24 paket	Trubichka 7,5x24 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "trubichka 7,5x24 paket", "name": "Trubichka 7,5x24 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.023245+05
trubichka napitka pket	Trubichka Napitka Pket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "trubichka napitka pket", "name": "Trubichka Napitka Pket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.023402+05
trubichka paket 24x36	Trubichka Paket 24x36	Kg		tayyor mahsulot	{"uom": "Kg", "code": "trubichka paket 24x36", "name": "Trubichka Paket 24x36", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.023559+05
twins kitket vaﬂi	Twins Kitket Vaﬂi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "twins kitket vaﬂi", "name": "Twins Kitket Vaﬂi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.026602+05
two bite 300gr	Two Bite 300gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "two bite 300gr", "name": "Two Bite 300gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.026896+05
two bite 320 gr	Two Bite 320 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "two bite 320 gr", "name": "Two Bite 320 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.027053+05
two bite qizil 32 gr	Two Bite Qizil 32 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "two bite qizil 32 gr", "name": "Two Bite Qizil 32 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.027417+05
udabrenia 50gr paket	Udabrenia 50gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "udabrenia 50gr paket", "name": "Udabrenia 50gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.027543+05
uluga pavlotti zamok paket	Uluga Pavlotti Zamok Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "uluga pavlotti zamok paket", "name": "Uluga Pavlotti Zamok Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.027698+05
uma salfetka	Uma Salfetka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "uma salfetka", "name": "Uma Salfetka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.027856+05
uma salfetka 30ta	Uma Salfetka 30ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "uma salfetka 30ta", "name": "Uma Salfetka 30ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.027989+05
ummu asartiy	Ummu Asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ummu asartiy", "name": "Ummu Asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.028106+05
ummu suxarik	Ummu Suxarik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ummu suxarik", "name": "Ummu Suxarik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.028244+05
un 2kg paket	Un 2kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "un 2kg paket", "name": "Un 2kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.02837+05
un 5kg paket	Un 5kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "un 5kg paket", "name": "Un 5kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.028475+05
usliga pechat	Usliga Pechat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usliga pechat", "name": "Usliga Pechat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.028594+05
usluga dusel paket	Usluga Dusel Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga dusel paket", "name": "Usluga Dusel Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.028789+05
usluga kiddo zip paket	Usluga Kiddo Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga kiddo zip paket", "name": "Usluga Kiddo Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.028886+05
usluga milano 25x33	Usluga Milano 25x33	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga milano 25x33", "name": "Usluga Milano 25x33", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.028992+05
usluga milano qizil qora	Usluga Milano Qizil Qora	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga milano qizil qora", "name": "Usluga Milano Qizil Qora", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.029347+05
usluga repost zip paket	Usluga Repost Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga repost zip paket", "name": "Usluga Repost Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.030088+05
usluga xolodniy payka	Usluga Xolodniy Payka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga xolodniy payka", "name": "Usluga Xolodniy Payka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.030832+05
usluga zip paket vlider	Usluga Zip Paket Vlider	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga zip paket vlider", "name": "Usluga Zip Paket Vlider", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.030963+05
chempion shaurma pishloq samarqand rulon	Chempion Shaurma Pishloq Samarqand Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion shaurma pishloq samarqand rulon", "name": "Chempion Shaurma Pishloq Samarqand Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.695418+05
opp 595/20 pf kar	OPP 595/20	Kg		homashyo	{"uom": "Kg", "code": "opp 595/20 pf kar", "name": "OPP 595/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.882884+05
opp 665/30 pol	OPP 665/30	Kg		homashyo	{"uom": "Kg", "code": "opp 665/30 pol", "name": "OPP 665/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.88835+05
tasty pelmen	Tasty Pelmen	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tasty pelmen", "name": "Tasty Pelmen", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.017891+05
opp 720/30 PF pol	OPP 720/30	Kg		homashyo	{"uom": "Kg", "code": "opp 720/30 PF pol", "name": "OPP 720/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.893555+05
tvorog tabiiy 9% 200 gr orange	Tvorog Tabiiy 9% 200 Gr Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tvorog tabiiy 9% 200 gr orange", "name": "Tvorog Tabiiy 9% 200 Gr Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.026432+05
usluga paket	Usluga Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga paket", "name": "Usluga Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.029588+05
uzbechka bulochka	Uzbechka Bulochka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "uzbechka bulochka", "name": "Uzbechka Bulochka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.031463+05
vafelniy xit	Vafelniy Xit	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vafelniy xit", "name": "Vafelniy Xit", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.031706+05
vakum 25/35	Vakum 25/35	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vakum 25/35", "name": "Vakum 25/35", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.031882+05
vakum paket 20/30	Vakum Paket 20/30	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vakum paket 20/30", "name": "Vakum Paket 20/30", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.032043+05
vakum paket 21/30	Vakum Paket 21/30	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vakum paket 21/30", "name": "Vakum Paket 21/30", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.032169+05
vakum paket 24/32	Vakum Paket 24/32	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vakum paket 24/32", "name": "Vakum Paket 24/32", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.032306+05
vakum paket 30/35	Vakum Paket 30/35	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vakum paket 30/35", "name": "Vakum Paket 30/35", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.03245+05
vanilno kafeyniy 50/50	Vanilno Kafeyniy 50/50	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vanilno kafeyniy 50/50", "name": "Vanilno Kafeyniy 50/50", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.032862+05
vanna zip paket	Vanna Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vanna zip paket", "name": "Vanna Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.032999+05
vanted kanfet	Vanted Kanfet	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vanted kanfet", "name": "Vanted Kanfet", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.03316+05
vazira djazzy kiwi 100 gr	Vazira Djazzy Kiwi 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vazira djazzy kiwi 100 gr", "name": "Vazira Djazzy Kiwi 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.033321+05
vazira praprava dlya pelmen	Vazira Praprava Dlya Pelmen	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vazira praprava dlya pelmen", "name": "Vazira Praprava Dlya Pelmen", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.033459+05
velona banan	Velona Banan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "velona banan", "name": "Velona Banan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.033873+05
velona golubika	Velona Golubika	Kg		tayyor mahsulot	{"uom": "Kg", "code": "velona golubika", "name": "Velona Golubika", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.034037+05
velona shokolad	Velona Shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "velona shokolad", "name": "Velona Shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.034218+05
velona yogurt	Velona Yogurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "velona yogurt", "name": "Velona Yogurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.034524+05
velza paket	Velza Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "velza paket", "name": "Velza Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.03466+05
veral 24 sm	Veral 24 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "veral 24 sm", "name": "Veral 24 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.035281+05
vesta electronik paket	Vesta Electronik Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vesta electronik paket", "name": "Vesta Electronik Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.035711+05
vintovka malina	Vintovka Malina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vintovka malina", "name": "Vintovka Malina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.037161+05
vintovka mix	Vintovka Mix	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vintovka mix", "name": "Vintovka Mix", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.037312+05
vitamin s liti	Vitamin S Liti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vitamin s liti", "name": "Vitamin S Liti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.037812+05
vkusno by seva katleti paket	Vkusno By Seva Katleti Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vkusno by seva katleti paket", "name": "Vkusno By Seva Katleti Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.038663+05
vkusno by seva somsa paket	Vkusno By Seva Somsa Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vkusno by seva somsa paket", "name": "Vkusno By Seva Somsa Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.038925+05
vkusno by seva testo paket	Vkusno By Seva Testo Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vkusno by seva testo paket", "name": "Vkusno By Seva Testo Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.039084+05
opp 485/18 pol	OPP 485/18	Kg		homashyo	{"uom": "Kg", "code": "opp 485/18 pol", "name": "OPP 485/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.870598+05
opp 485/30 kar	OPP 485/30	Kg		homashyo	{"uom": "Kg", "code": "opp 485/30 kar", "name": "OPP 485/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.870905+05
tvigga paket	Tvigga Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tvigga paket", "name": "Tvigga Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.024267+05
tvist 500 euro	Tvist 500 Euro	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tvist 500 euro", "name": "Tvist 500 Euro", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.024442+05
tvorog tabiiy 5 % yashil	Tvorog Tabiiy 5 % Yashil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tvorog tabiiy 5 % yashil", "name": "Tvorog Tabiiy 5 % Yashil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.026262+05
usluga playovo 22/26	Usluga Playovo 22/26	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga playovo 22/26", "name": "Usluga Playovo 22/26", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.029784+05
usluga the kucher zamok paket	Usluga The Kucher Zamok Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga the kucher zamok paket", "name": "Usluga The Kucher Zamok Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.030183+05
usluga xeppi foks 35/40 paket zamok	Usluga Xeppi Foks 35/40 Paket Zamok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga xeppi foks 35/40 paket zamok", "name": "Usluga Xeppi Foks 35/40 Paket Zamok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.030717+05
uvitilgan noxot paket	Uvitilgan Noxot Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "uvitilgan noxot paket", "name": "Uvitilgan Noxot Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.031177+05
velona vishnya	Velona Vishnya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "velona vishnya", "name": "Velona Vishnya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.034381+05
vesta kvadro 27,5 sm	Vesta Kvadro 27,5 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vesta kvadro 27,5 sm", "name": "Vesta Kvadro 27,5 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.035867+05
vesta kvadro 28 sm	Vesta Kvadro 28 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vesta kvadro 28 sm", "name": "Vesta Kvadro 28 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.03602+05
vich tucher	Vich Tucher	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vich tucher", "name": "Vich Tucher", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.036168+05
vintovka kivi	Vintovka Kivi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vintovka kivi", "name": "Vintovka Kivi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.036623+05
vitagum vitamin zip paket	Vitagum Vitamin Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vitagum vitamin zip paket", "name": "Vitagum Vitamin Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.037465+05
vitamin s ekstra	Vitamin S Ekstra	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vitamin s ekstra", "name": "Vitamin S Ekstra", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.037625+05
vkusno by seva dolma paket	Vkusno By Seva Dolma Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vkusno by seva dolma paket", "name": "Vkusno By Seva Dolma Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.038532+05
vkusno by seva sirniki paket	Vkusno By Seva Sirniki Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vkusno by seva sirniki paket", "name": "Vkusno By Seva Sirniki Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.038795+05
vodiy solyonniy 2 kg paket	Vodiy Solyonniy 2 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vodiy solyonniy 2 kg paket", "name": "Vodiy Solyonniy 2 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.039348+05
vontet 4sm prazrachniy	Vontet 4sm Prazrachniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vontet 4sm prazrachniy", "name": "Vontet 4sm Prazrachniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.03949+05
wanted mayda yozuv	Wanted Mayda Yozuv	Kg		tayyor mahsulot	{"uom": "Kg", "code": "wanted mayda yozuv", "name": "Wanted Mayda Yozuv", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.039749+05
waﬄe cake 45 gr orange	Waﬄe Cake 45 Gr Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "waﬄe cake 45 gr orange", "name": "Waﬄe Cake 45 Gr Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.04031+05
weetah moloko 115 gr	Weetah Moloko 115 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "weetah moloko 115 gr", "name": "Weetah Moloko 115 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.040584+05
weetah shokolad 115 gr	Weetah Shokolad 115 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "weetah shokolad 115 gr", "name": "Weetah Shokolad 115 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.040716+05
wet wipes ko'k salfetka 120ta	Wet Wipes Ko'k Salfetka 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "wet wipes ko'k salfetka 120ta", "name": "Wet Wipes Ko'k Salfetka 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.041017+05
xamkor pista tuzlangan 80 gr	Xamkor Pista Tuzlangan 80 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xamkor pista tuzlangan 80 gr", "name": "Xamkor Pista Tuzlangan 80 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.041197+05
xan decor umar	Xan Decor Umar	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xan decor umar", "name": "Xan Decor Umar", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.041618+05
xan dekor kichik	Xan Dekor Kichik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xan dekor kichik", "name": "Xan Dekor Kichik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.041805+05
xasan semechki 20gr	Xasan Semechki 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xasan semechki 20gr", "name": "Xasan Semechki 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.042106+05
xavas semechka115gr	Xavas Semechka115gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xavas semechka115gr", "name": "Xavas Semechka115gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.042266+05
xleb tosterniy	Xleb Tosterniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xleb tosterniy", "name": "Xleb Tosterniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.043509+05
xns vlajniy salfetka oq	Xns Vlajniy Salfetka Oq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xns vlajniy salfetka oq", "name": "Xns Vlajniy Salfetka Oq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.044057+05
opp 765/30 spp	OPP 765/30	Kg		homashyo	{"uom": "Kg", "code": "opp 765/30 spp", "name": "OPP 765/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.897504+05
opp metal folga 860/6 polietil	OPP metal folga 860/6	Kg		homashyo	{"uom": "Kg", "code": "opp metal folga 860/6 polietil", "name": "OPP metal folga 860/6", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.908248+05
usluga playovo 25/28	Usluga Playovo 25/28	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga playovo 25/28", "name": "Usluga Playovo 25/28", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.029889+05
usluga prazrachniy zamok pket	Usluga Prazrachniy Zamok Pket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga prazrachniy zamok pket", "name": "Usluga Prazrachniy Zamok Pket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.029991+05
veral 22 sm	Veral 22 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "veral 22 sm", "name": "Veral 22 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.035111+05
warm technik falga	Warm Technik Falga	Kg		tayyor mahsulot	{"uom": "Kg", "code": "warm technik falga", "name": "Warm Technik Falga", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.040173+05
wet wipes 15ta	Wet Wipes 15ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "wet wipes 15ta", "name": "Wet Wipes 15ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.040843+05
xan dekor paket ekstra	Xan Dekor Paket Ekstra	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xan dekor paket ekstra", "name": "Xan Dekor Paket Ekstra", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.041952+05
xavas semichka 100gr	Xavas Semichka 100gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xavas semichka 100gr", "name": "Xavas Semichka 100gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.042414+05
xello kitiy engi	Xello Kitiy Engi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xello kitiy engi", "name": "Xello Kitiy Engi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.042568+05
xeppi foks 30/40	Xeppi Foks 30/40	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xeppi foks 30/40", "name": "Xeppi Foks 30/40", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.043166+05
xeppi foks 35/40	Xeppi Foks 35/40	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xeppi foks 35/40", "name": "Xeppi Foks 35/40", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.043326+05
xlebushki grenki 30 gr	Xlebushki Grenki 30 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xlebushki grenki 30 gr", "name": "Xlebushki Grenki 30 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.043651+05
xman mayka paket	Xman Mayka Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xman mayka paket", "name": "Xman Mayka Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.043787+05
xns salfetka 120 sht	Xns Salfetka 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xns salfetka 120 sht", "name": "Xns Salfetka 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.043917+05
xon qurt paket	Xon Qurt Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xon qurt paket", "name": "Xon Qurt Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.04435+05
xrustayim 3d 90gr	Xrustayim 3d 90gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xrustayim 3d 90gr", "name": "Xrustayim 3d 90gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.044907+05
xrustayim 5d 90gr	Xrustayim 5d 90gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xrustayim 5d 90gr", "name": "Xrustayim 5d 90gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.04505+05
xrustik 80gr paket	Xrustik 80gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xrustik 80gr paket", "name": "Xrustik 80gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.045202+05
ya hachu plambir Dora	ya hachu plambir Dora	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ya hachu plambir Dora", "name": "ya hachu plambir Dora", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.045638+05
yam yam kukuruz paket	Yam Yam Kukuruz Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "yam yam kukuruz paket", "name": "Yam Yam Kukuruz Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.046191+05
yengil sirok banan	Yengil Sirok Banan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "yengil sirok banan", "name": "Yengil Sirok Banan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.046338+05
yengil sirok kakao	Yengil Sirok Kakao	Kg		tayyor mahsulot	{"uom": "Kg", "code": "yengil sirok kakao", "name": "Yengil Sirok Kakao", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.046483+05
yengil sirok klubnika	Yengil Sirok Klubnika	Kg		tayyor mahsulot	{"uom": "Kg", "code": "yengil sirok klubnika", "name": "Yengil Sirok Klubnika", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.04664+05
yengil sirok vanil	Yengil Sirok Vanil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "yengil sirok vanil", "name": "Yengil Sirok Vanil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.046783+05
yevro dekor paket fyaletviy	Yevro Dekor Paket Fyaletviy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "yevro dekor paket fyaletviy", "name": "Yevro Dekor Paket Fyaletviy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.046925+05
yulduz muka 2kg paket	Yulduz Muka 2kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "yulduz muka 2kg paket", "name": "Yulduz Muka 2kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.047072+05
zam zam kukruz paket	Zam Zam Kukruz Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zam zam kukruz paket", "name": "Zam Zam Kukruz Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.048381+05
zamir salfetka asarti	Zamir Salfetka Asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zamir salfetka asarti", "name": "Zamir Salfetka Asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.048546+05
zamok paket zizi vana	Zamok Paket Zizi Vana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zamok paket zizi vana", "name": "Zamok Paket Zizi Vana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.048674+05
zari	Zari	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zari", "name": "Zari", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.048816+05
eko salfetka 72 sht aloye	Eko Salfetka 72 Sht Aloye	Kg		tayyor mahsulot	{"uom": "Kg", "code": "eko salfetka 72 sht aloye", "name": "Eko Salfetka 72 Sht Aloye", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.720162+05
opp 535/18 kar	OPP 535/18	Kg		homashyo	{"uom": "Kg", "code": "opp 535/18 kar", "name": "OPP 535/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.876588+05
opp 790/20 pff kar	OPP PFF 790/20	Kg		homashyo	{"uom": "Kg", "code": "opp 790/20 pff kar", "name": "OPP PFF 790/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.898841+05
veral 31 sm	Veral 31 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "veral 31 sm", "name": "Veral 31 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.035427+05
versace eclat sovun	Versace Eclat Sovun	Kg		tayyor mahsulot	{"uom": "Kg", "code": "versace eclat sovun", "name": "Versace Eclat Sovun", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.035574+05
ya hachu plambir Ali bobo	ya hachu plambir Ali bobo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ya hachu plambir Ali bobo", "name": "ya hachu plambir Ali bobo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.045492+05
yummi keks 30 gr	Yummi Keks 30 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "yummi keks 30 gr", "name": "Yummi Keks 30 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.04737+05
yummy keks asartiy	Yummy Keks Asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "yummy keks asartiy", "name": "Yummy Keks Asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.047515+05
yummy qurt	Yummy Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "yummy qurt", "name": "Yummy Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.047659+05
yuna diva 100 gr	Yuna Diva 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "yuna diva 100 gr", "name": "Yuna Diva 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.047805+05
yupo biscuit	Yupo Biscuit	Kg		tayyor mahsulot	{"uom": "Kg", "code": "yupo biscuit", "name": "Yupo Biscuit", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.047956+05
za-zu pechenye s kakao	Za-zu Pechenye S Kakao	Kg		tayyor mahsulot	{"uom": "Kg", "code": "za-zu pechenye s kakao", "name": "Za-zu Pechenye S Kakao", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.048247+05
zaxro 1kg paket asarti	Zaxro 1kg Paket Asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zaxro 1kg paket asarti", "name": "Zaxro 1kg Paket Asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.049221+05
zaxro 500 gr paket	Zaxro 500 Gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zaxro 500 gr paket", "name": "Zaxro 500 Gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.049359+05
zaychik bonito vaﬂi	Zaychik Bonito Vaﬂi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zaychik bonito vaﬂi", "name": "Zaychik Bonito Vaﬂi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.049507+05
zaychik molochniy vaﬂi	Zaychik Molochniy Vaﬂi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zaychik molochniy vaﬂi", "name": "Zaychik Molochniy Vaﬂi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.049641+05
zelen' priprava 10 sm	Zelen' Priprava 10 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zelen' priprava 10 sm", "name": "Zelen' Priprava 10 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.049771+05
zelen' priprava 12 sm	Zelen' Priprava 12 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zelen' priprava 12 sm", "name": "Zelen' Priprava 12 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.049906+05
zenit frutto ninja 70 gr	Zenit Frutto Ninja 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zenit frutto ninja 70 gr", "name": "Zenit Frutto Ninja 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.050042+05
zenit obi novvot 80 gr	Zenit Obi Novvot 80 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zenit obi novvot 80 gr", "name": "Zenit Obi Novvot 80 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.050171+05
zip paket 12/20	Zip Paket 12/20	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zip paket 12/20", "name": "Zip Paket 12/20", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.050304+05
zip paket 26/36	Zip Paket 26/36	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zip paket 26/36", "name": "Zip Paket 26/36", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.050427+05
ziyo bio qurt 30 gr rulon	Ziyo Bio Qurt 30 Gr Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ziyo bio qurt 30 gr rulon", "name": "Ziyo Bio Qurt 30 Gr Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.050701+05
ziyo qurt paekt	Ziyo Qurt Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ziyo qurt paekt", "name": "Ziyo Qurt Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.050831+05
zizi alumin+qog'oz 16,8sm	Zizi Alumin+qog'oz 16,8sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi alumin+qog'oz 16,8sm", "name": "Zizi Alumin+qog'oz 16,8sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.051465+05
zizi chupachupis 14gr rulon	Zizi Chupachupis 14gr Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi chupachupis 14gr rulon", "name": "Zizi Chupachupis 14gr Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.051817+05
zizi chupachups 20gr	Zizi Chupachups 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi chupachups 20gr", "name": "Zizi Chupachups 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.051964+05
zizi karamel	Zizi Karamel	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi karamel", "name": "Zizi Karamel", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.05239+05
zizi kislinki	Zizi Kislinki	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi kislinki", "name": "Zizi Kislinki", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.052523+05
zizi lenta 2 sm	Zizi Lenta 2 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi lenta 2 sm", "name": "Zizi Lenta 2 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.052817+05
zizi lolipop 800gr	Zizi Lolipop 800gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi lolipop 800gr", "name": "Zizi Lolipop 800gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.052946+05
zizi lolipop 800gr paket	Zizi Lolipop 800gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi lolipop 800gr paket", "name": "Zizi Lolipop 800gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.053076+05
zizi lolipop yangi	Zizi Lolipop Yangi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi lolipop yangi", "name": "Zizi Lolipop Yangi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.053221+05
zizi mazali 700gr paket	Zizi Mazali 700gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi mazali 700gr paket", "name": "Zizi Mazali 700gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.053358+05
opp 830/30	OPP 830/30	Kg		homashyo	{"uom": "Kg", "code": "opp 830/30", "name": "OPP 830/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.901601+05
xeppi foks 28/35	Xeppi Foks 28/35	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xeppi foks 28/35", "name": "Xeppi Foks 28/35", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.043023+05
yakar rose love roster asarty paket	Yakar Rose Love Roster Asarty Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "yakar rose love roster asarty paket", "name": "Yakar Rose Love Roster Asarty Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.046056+05
Barakali chuchvala 1kg paket	Barakali chuchvala 1kg paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Barakali chuchvala 1kg paket", "name": "Barakali chuchvala 1kg paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.496108+05
Bingo Jidkiy Aboy	Bingo Jidkiy Aboy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bingo Jidkiy Aboy", "name": "Bingo Jidkiy Aboy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.504289+05
Chicco kukruz paket	Chicco kukruz paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Chicco kukruz paket", "name": "Chicco kukruz paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.513414+05
Jem 460/20	JEM 460/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 460/20", "name": "JEM 460/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.541219+05
Magic shokalat	Magic shokalat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Magic shokalat", "name": "Magic shokalat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.56557+05
Makiz #161	Makiz #161	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz #161", "name": "Makiz #161", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.566329+05
zizi 14gr	Zizi 14gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi 14gr", "name": "Zizi 14gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.050968+05
zizi 5sm padvyorka	Zizi 5sm Padvyorka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi 5sm padvyorka", "name": "Zizi 5sm Padvyorka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.051282+05
zizi braslet	Zizi Braslet	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi braslet", "name": "Zizi Braslet", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.051617+05
zizi chupachups 700gr	Zizi Chupachups 700gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi chupachups 700gr", "name": "Zizi Chupachups 700gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.05211+05
zizi chupachups 960gr	Zizi Chupachups 960gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi chupachups 960gr", "name": "Zizi Chupachups 960gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.052249+05
zizi koko mummu	Zizi Koko Mummu	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi koko mummu", "name": "Zizi Koko Mummu", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.052669+05
zizi monstir	Zizi Monstir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi monstir", "name": "Zizi Monstir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.053634+05
zizi monstor rolli 20 gr	Zizi Monstor Rolli 20 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi monstor rolli 20 gr", "name": "Zizi Monstor Rolli 20 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.053933+05
zizi nileyka	Zizi Nileyka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi nileyka", "name": "Zizi Nileyka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.054083+05
zizi pikolla	Zizi Pikolla	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi pikolla", "name": "Zizi Pikolla", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.054245+05
zizi skoch paket	Zizi Skoch Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi skoch paket", "name": "Zizi Skoch Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.054514+05
zizi stiks	Zizi Stiks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi stiks", "name": "Zizi Stiks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.054654+05
zizi usluga reska	Zizi Usluga Reska	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi usluga reska", "name": "Zizi Usluga Reska", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.054919+05
zizi yeryong'oq	Zizi Yeryong'oq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi yeryong'oq", "name": "Zizi Yeryong'oq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.055056+05
zor premium semechka 80 gr	Zor Premium Semechka 80 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zor premium semechka 80 gr", "name": "Zor Premium Semechka 80 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.055217+05
zor zor qurt	Zor Zor Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zor zor qurt", "name": "Zor Zor Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.055359+05
zuxro kreml sovetskiy plombir	Zuxro Kreml Sovetskiy Plombir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zuxro kreml sovetskiy plombir", "name": "Zuxro Kreml Sovetskiy Plombir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.055642+05
zuxro marvarid 65 gr	Zuxro Marvarid 65 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zuxro marvarid 65 gr", "name": "Zuxro Marvarid 65 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.055784+05
zuxro sendvich plombir v pechenye 100 gr	Zuxro Sendvich Plombir V Pechenye 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zuxro sendvich plombir v pechenye 100 gr", "name": "Zuxro Sendvich Plombir V Pechenye 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.055919+05
zuxro sutli muzqaymoq 70 gr	Zuxro Sutli Muzqaymoq 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zuxro sutli muzqaymoq 70 gr", "name": "Zuxro Sutli Muzqaymoq 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.056066+05
ﬁberello	Fiberello	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ﬁberello", "name": "Fiberello", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.056341+05
ﬁfa	Fifa	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ﬁfa", "name": "Fifa", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.056465+05
ﬂowers paket	Flowers Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ﬂowers paket", "name": "Flowers Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.056597+05
Achoib	Achoib	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Achoib", "name": "Achoib", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.471952+05
Asl sfat hot dog sosiski kuriniy	Asl sfat hot dog sosiski kuriniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Asl sfat hot dog sosiski kuriniy", "name": "Asl sfat hot dog sosiski kuriniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.491296+05
BYD xalodniy payka	BYD xalodniy payka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "BYD xalodniy payka", "name": "BYD xalodniy payka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.493525+05
za-za pesochnoye pechenya	Za-za Pesochnoye Pechenya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "za-za pesochnoye pechenya", "name": "Za-za Pesochnoye Pechenya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.048104+05
Kosmos Stix shashlik	Kosmos Stix shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmos Stix shashlik", "name": "Kosmos Stix shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.554951+05
Kristal tuz 3kg paket	Kristal tuz 3kg paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kristal tuz 3kg paket", "name": "Kristal tuz 3kg paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.556072+05
Makiz vermishel LV103	Makiz vermishel LV103	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz vermishel LV103", "name": "Makiz vermishel LV103", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.571541+05
Marvel Hulk salyami	Marvel Hulk salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Marvel Hulk salyami", "name": "Marvel Hulk salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.574322+05
Mega plus paket qizil	Mega plus paket qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mega plus paket qizil", "name": "Mega plus paket qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.577413+05
PRAZ/NIY ZIP PAKET 35/45	PRAZ/NIY ZIP PAKET 35/45	Kg		tayyor mahsulot	{"uom": "Kg", "code": "PRAZ/NIY ZIP PAKET 35/45", "name": "PRAZ/NIY ZIP PAKET 35/45", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.595081+05
PY vakum 630x80	PY vakum 630x80	Kg		tayyor mahsulot	{"uom": "Kg", "code": "PY vakum 630x80", "name": "PY vakum 630x80", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.595359+05
Patitos chips smetana 20gr	Patitos chips smetana 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Patitos chips smetana 20gr", "name": "Patitos chips smetana 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.596638+05
Pitos kichik oq shokalat 3vid	Pitos kichik oq shokalat 3vid	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pitos kichik oq shokalat 3vid", "name": "Pitos kichik oq shokalat 3vid", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.602109+05
Pitos kichik qora shokalat 3vid	Pitos kichik qora shokalat 3vid	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pitos kichik qora shokalat 3vid", "name": "Pitos kichik qora shokalat 3vid", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.602225+05
Purmilkiy tvorg zernistiy 5% 400 gr yashil	Purmilkiy tvorg zernistiy 5% 400 gr yashil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Purmilkiy tvorg zernistiy 5% 400 gr yashil", "name": "Purmilkiy tvorg zernistiy 5% 400 gr yashil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.606118+05
Rara Nusa Pket	Rara Nusa Pket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rara Nusa Pket", "name": "Rara Nusa Pket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.610531+05
Rosti paket 500gr	Rosti paket 500gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rosti paket 500gr", "name": "Rosti paket 500gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.613533+05
Salfetka comfort 20dona	Salfetka comfort 20dona	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Salfetka comfort 20dona", "name": "Salfetka comfort 20dona", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.614716+05
Sancho 80gr tamat-sir	Sancho 80gr tamat-sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sancho 80gr tamat-sir", "name": "Sancho 80gr tamat-sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.615023+05
Silk decor eco paket	Silk decor eco paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Silk decor eco paket", "name": "Silk decor eco paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.618832+05
Zizi askarbinka gulikoza	Zizi askarbinka gulikoza	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi askarbinka gulikoza", "name": "Zizi askarbinka gulikoza", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.659899+05
Zizi podvyortka 7sm	Zizi podvyortka 7sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi podvyortka 7sm", "name": "Zizi podvyortka 7sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.661736+05
ali bobo nemo	Ali Bobo Nemo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ali bobo nemo", "name": "Ali Bobo Nemo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.666811+05
armiya opp skoch paket 20/30	Armiya Opp Skoch Paket 20/30	Kg		tayyor mahsulot	{"uom": "Kg", "code": "armiya opp skoch paket 20/30", "name": "Armiya Opp Skoch Paket 20/30", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.670817+05
avella salfetka 15 sht asarti	Avella Salfetka 15 Sht Asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "avella salfetka 15 sht asarti", "name": "Avella Salfetka 15 Sht Asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.671962+05
baxt ipjone kotta paket	Baxt Ipjone Kotta Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "baxt ipjone kotta paket", "name": "Baxt Ipjone Kotta Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.676727+05
beneo aboy paket umar	Beneo Aboy Paket Umar	Kg		tayyor mahsulot	{"uom": "Kg", "code": "beneo aboy paket umar", "name": "Beneo Aboy Paket Umar", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.677732+05
opp 515/18 pf	OPP 515/18	Kg		homashyo	{"uom": "Kg", "code": "opp 515/18 pf", "name": "OPP 515/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.873848+05
opp 515/18 pol	OPP 515/18	Kg		homashyo	{"uom": "Kg", "code": "opp 515/18 pol", "name": "OPP 515/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.874014+05
opp 840/20 pff kar	OPP PFF 840/20	Kg		homashyo	{"uom": "Kg", "code": "opp 840/20 pff kar", "name": "OPP PFF 840/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.902842+05
Jesko Paket	Jesko Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Jesko Paket", "name": "Jesko Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.545876+05
Kendi gold kotta pamadka	Kendi gold kotta pamadka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kendi gold kotta pamadka", "name": "Kendi gold kotta pamadka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.550141+05
Klassik karamel rulon	Klassik karamel rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Klassik karamel rulon", "name": "Klassik karamel rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.551696+05
zizi pikolla 1kg	Zizi Pikolla 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi pikolla 1kg", "name": "Zizi Pikolla 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.054379+05
zizi usluga paket	Zizi Usluga Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi usluga paket", "name": "Zizi Usluga Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.054787+05
Xrus tayim 5D 40gr barbeku	Xrus tayim 5D 40gr barbeku	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 5D 40gr barbeku", "name": "Xrus tayim 5D 40gr barbeku", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.64849+05
Xrusttik+ 50gr paket	Xrusttik+ 50gr paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrusttik+ 50gr paket", "name": "Xrusttik+ 50gr paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.652173+05
Yoqimtoy kukruz rulonda	Yoqimtoy kukruz rulonda	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yoqimtoy kukruz rulonda", "name": "Yoqimtoy kukruz rulonda", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.656313+05
bonitto sergili zamok paket	Bonitto Sergili Zamok Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bonitto sergili zamok paket", "name": "Bonitto Sergili Zamok Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.681858+05
chamepion samarqand sendvich qizil paket	Chamepion Samarqand Sendvich Qizil Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chamepion samarqand sendvich qizil paket", "name": "Chamepion Samarqand Sendvich Qizil Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.685073+05
chaps smetan kotta 36 sm	Chaps Smetan Kotta 36 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps smetan kotta 36 sm", "name": "Chaps Smetan Kotta 36 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.686851+05
cheers nachos sous + salsa 27 gr 31 sm	Cheers Nachos Sous + Salsa 27 Gr 31 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nachos sous + salsa 27 gr 31 sm", "name": "Cheers Nachos Sous + Salsa 27 Gr 31 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.690824+05
cheers nashos sir 130 gr 41 sm	Cheers Nashos Sir 130 Gr 41 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nashos sir 130 gr 41 sm", "name": "Cheers Nashos Sir 130 Gr 41 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.691418+05
chempion andijon burger 5 dona paket	Chempion Andijon Burger 5 Dona Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion andijon burger 5 dona paket", "name": "Chempion Andijon Burger 5 Dona Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.692004+05
chempion shaurma paket samarqand	Chempion Shaurma Paket Samarqand	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion shaurma paket samarqand", "name": "Chempion Shaurma Paket Samarqand", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.695278+05
chuda samara chernika ejevika	Chuda Samara Chernika Ejevika	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chuda samara chernika ejevika", "name": "Chuda Samara Chernika Ejevika", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.700432+05
damashniy rulet asartiy 90gr	Damashniy Rulet Asartiy 90gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "damashniy rulet asartiy 90gr", "name": "Damashniy Rulet Asartiy 90gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.70654+05
freshboll qora metka	Freshboll Qora Metka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "freshboll qora metka", "name": "Freshboll Qora Metka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.730983+05
jessica 20/25 zip paket	Jessica 20/25 Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jessica 20/25 zip paket", "name": "Jessica 20/25 Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.771015+05
kanadskiy sasiska ziyonur	Kanadskiy Sasiska Ziyonur	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kanadskiy sasiska ziyonur", "name": "Kanadskiy Sasiska Ziyonur", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.773516+05
koko felicita eskimo	Koko Felicita Eskimo	Kg		tayyor mahsulot	{"uom": "Kg", "code": "koko felicita eskimo", "name": "Koko Felicita Eskimo", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.778789+05
luna cake qizil 25 gr	Luna Cake Qizil 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "luna cake qizil 25 gr", "name": "Luna Cake Qizil 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.792656+05
makiz CU2 2 kg paket	makiz CU2 2 kg paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz CU2 2 kg paket", "name": "makiz CU2 2 kg paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.797799+05
mega 2kg paket tuzsiz ruchkali	Mega 2kg Paket Tuzsiz Ruchkali	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega 2kg paket tuzsiz ruchkali", "name": "Mega 2kg Paket Tuzsiz Ruchkali", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.816226+05
mega yeryong'oq 50gr	Mega Yeryong'oq 50gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega yeryong'oq 50gr", "name": "Mega Yeryong'oq 50gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.821517+05
miray bruni cake 25 gr orange	Miray Bruni Cake 25 Gr Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miray bruni cake 25 gr orange", "name": "Miray Bruni Cake 25 Gr Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.826784+05
musa BissGo Chocolate 90 gr	musa BissGo Chocolate 90 gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musa BissGo Chocolate 90 gr", "name": "musa BissGo Chocolate 90 gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.840406+05
nasr stick suxarik 50 gr asarti	Nasr Stick Suxarik 50 Gr Asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nasr stick suxarik 50 gr asarti", "name": "Nasr Stick Suxarik 50 Gr Asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.847422+05
opp 515/30 kar	OPP 515/30	Kg		homashyo	{"uom": "Kg", "code": "opp 515/30 kar", "name": "OPP 515/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.874754+05
opp 515/30 pol	OPP 515/30	Kg		homashyo	{"uom": "Kg", "code": "opp 515/30 pol", "name": "OPP 515/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.874893+05
safo kanada sasiska xalol	Safo Kanada Sasiska Xalol	Kg		tayyor mahsulot	{"uom": "Kg", "code": "safo kanada sasiska xalol", "name": "Safo Kanada Sasiska Xalol", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.973348+05
X48 Kopeyka	X48 Kopeyka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "X48 Kopeyka", "name": "X48 Kopeyka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.643099+05
XAVAS PRAZ/NIY 38,5 SM	XAVAS PRAZ/NIY 38,5 SM	Kg		tayyor mahsulot	{"uom": "Kg", "code": "XAVAS PRAZ/NIY 38,5 SM", "name": "XAVAS PRAZ/NIY 38,5 SM", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.64326+05
Toretto Konus Telyatina	Toretto Konus Telyatina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto Konus Telyatina", "name": "Toretto Konus Telyatina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.632644+05
ezo parashok 2ka paket	Ezo Parashok 2ka Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ezo parashok 2ka paket", "name": "Ezo Parashok 2ka Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.726371+05
guruch lazer 2kg	Guruch Lazer 2kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "guruch lazer 2kg", "name": "Guruch Lazer 2kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.741351+05
guruch uzun donli pokisto	Guruch Uzun Donli Pokisto	Kg		tayyor mahsulot	{"uom": "Kg", "code": "guruch uzun donli pokisto", "name": "Guruch Uzun Donli Pokisto", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.741841+05
jec bio qurt 100 dona	Jec Bio Qurt 100 Dona	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jec bio qurt 100 dona", "name": "Jec Bio Qurt 100 Dona", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.756871+05
mega 5kg paket tuzliy	Mega 5kg Paket Tuzliy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega 5kg paket tuzliy", "name": "Mega 5kg Paket Tuzliy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.816523+05
musaﬀo qurt 6 sht 30 gr	Musaﬀo Qurt 6 Sht 30 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musaﬀo qurt 6 sht 30 gr", "name": "Musaﬀo Qurt 6 Sht 30 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.843004+05
nilpak salfetka 100 dona paket	Nilpak Salfetka 100 Dona Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nilpak salfetka 100 dona paket", "name": "Nilpak Salfetka 100 Dona Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.849052+05
opp 30x40 skoch paket	Opp 30x40 Skoch Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp 30x40 skoch paket", "name": "Opp 30x40 Skoch Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.861251+05
opp 520/18	OPP 520/18	Kg		homashyo	{"uom": "Kg", "code": "opp 520/18", "name": "OPP 520/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.875029+05
opp 865/25 kar	OPP 865/25	Kg		homashyo	{"uom": "Kg", "code": "opp 865/25 kar", "name": "OPP 865/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.904557+05
oppm 735/25	OPPM 735/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 735/25", "name": "OPPM 735/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.920176+05
perﬀecto paket	Perﬀecto Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "perﬀecto paket", "name": "Perﬀecto Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.937719+05
pet+palitelen	Pet+palitelen	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet+palitelen", "name": "Pet+palitelen", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.950059+05
pippo shokolad	Pippo Shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pippo shokolad", "name": "Pippo Shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.952123+05
pista 20gr qizil	Pista 20gr Qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pista 20gr qizil", "name": "Pista 20gr Qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.952441+05
pure milky sirok kakos	Pure Milky Sirok Kakos	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pure milky sirok kakos", "name": "Pure Milky Sirok Kakos", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.962817+05
ramiz salfetka yashil qora 100 sht	Ramiz Salfetka Yashil Qora 100 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ramiz salfetka yashil qora 100 sht", "name": "Ramiz Salfetka Yashil Qora 100 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.967817+05
rojok ice grand kok ice glice	Rojok Ice Grand Kok Ice Glice	Kg		tayyor mahsulot	{"uom": "Kg", "code": "rojok ice grand kok ice glice", "name": "Rojok Ice Grand Kok Ice Glice", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.970331+05
sadee gamburger paket	Sadee Gamburger Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sadee gamburger paket", "name": "Sadee Gamburger Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.973172+05
salfetka bloomy baby 120 sht	Salfetka Bloomy Baby 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka bloomy baby 120 sht", "name": "Salfetka Bloomy Baby 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.977692+05
salfetka day 120 sht ramashka	Salfetka Day 120 Sht Ramashka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka day 120 sht ramashka", "name": "Salfetka Day 120 Sht Ramashka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.97825+05
saykal 1 kg paket assorti	Saykal 1 Kg Paket Assorti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "saykal 1 kg paket assorti", "name": "Saykal 1 Kg Paket Assorti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.987571+05
sherin makaron 400 gr	Sherin Makaron 400 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sherin makaron 400 gr", "name": "Sherin Makaron 400 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.99315+05
shoxasar qars qars shokolad	Shoxasar Qars Qars Shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shoxasar qars qars shokolad", "name": "Shoxasar Qars Qars Shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.995452+05
1110/30 pe pr toza	PE PR 1110/30	Kg		homashyo	{"uom": "Kg", "code": "1110/30 pe pr toza", "name": "PE PR 1110/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.443689+05
donut cake cherry 25 gr orange	Donut Cake Cherry 25 Gr Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "donut cake cherry 25 gr orange", "name": "Donut Cake Cherry 25 Gr Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.717391+05
eko salfetka meva asartiy 10ta	Eko Salfetka Meva Asartiy 10ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "eko salfetka meva asartiy 10ta", "name": "Eko Salfetka Meva Asartiy 10ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.721362+05
ezo 3kg rulon	Ezo 3kg Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ezo 3kg rulon", "name": "Ezo 3kg Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.726244+05
oq qand paket	Oq Qand Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "oq qand paket", "name": "Oq Qand Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.925465+05
orzu merci 60 gr	Orzu Merci 60 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "orzu merci 60 gr", "name": "Orzu Merci 60 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.926338+05
pastila paket	Pastila Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pastila paket", "name": "Pastila Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.931637+05
Mevali musqaymoq asarti	Mevali musqaymoq asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mevali musqaymoq asarti", "name": "Mevali musqaymoq asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.578922+05
Mevali musqaymoq asarti 80gr	Mevali musqaymoq asarti 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mevali musqaymoq asarti 80gr", "name": "Mevali musqaymoq asarti 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.579018+05
grenki salyami neopalitano 100 gr	Grenki Salyami Neopalitano 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "grenki salyami neopalitano 100 gr", "name": "Grenki Salyami Neopalitano 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.739475+05
Metal zip paket 18/25	Metal zip paket 18/25	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Metal zip paket 18/25", "name": "Metal zip paket 18/25", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.578695+05
ice cream pirat + ruletto 45 gr	Ice Cream Pirat + Ruletto 45 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ice cream pirat + ruletto 45 gr", "name": "Ice Cream Pirat + Ruletto 45 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.746785+05
isko abrazes rulon	Isko Abrazes Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "isko abrazes rulon", "name": "Isko Abrazes Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.752404+05
jeti batir motsarella paket	Jeti Batir Motsarella Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jeti batir motsarella paket", "name": "Jeti Batir Motsarella Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.77131+05
kemira luks 100gr paket	Kemira Luks 100gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kemira luks 100gr paket", "name": "Kemira Luks 100gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.776111+05
kreko kuritsa 28gr	Kreko Kuritsa 28gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kreko kuritsa 28gr", "name": "Kreko Kuritsa 28gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.781671+05
kuk suxarik 28gr shashlik	Kuk Suxarik 28gr Shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kuk suxarik 28gr shashlik", "name": "Kuk Suxarik 28gr Shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.786596+05
luna fresh ekanom 100 sht	Luna Fresh Ekanom 100 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "luna fresh ekanom 100 sht", "name": "Luna Fresh Ekanom 100 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.793016+05
makiz chuchvara paket 300gr	Makiz Chuchvara Paket 300gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz chuchvara paket 300gr", "name": "Makiz Chuchvara Paket 300gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.798133+05
opp 1010/30	OPP 1010/30	Kg		homashyo	{"uom": "Kg", "code": "opp 1010/30", "name": "OPP 1010/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.853137+05
opp 14x20 skoch paket	Opp 14x20 Skoch Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp 14x20 skoch paket", "name": "Opp 14x20 Skoch Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.858691+05
opp 520/20 pf	OPP 520/20	Kg		homashyo	{"uom": "Kg", "code": "opp 520/20 pf", "name": "OPP 520/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.875302+05
opp skocht paket rustam	Opp Skocht Paket Rustam	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp skocht paket rustam", "name": "Opp Skocht Paket Rustam", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.909269+05
tutzor lagmon	Tutzor Lagmon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tutzor lagmon", "name": "Tutzor Lagmon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.02391+05
usluga milano premum qora	Usluga Milano Premum Qora	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga milano premum qora", "name": "Usluga Milano Premum Qora", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.029244+05
usluga tuti frutti xtoy laminatsiya	Usluga Tuti Frutti Xtoy Laminatsiya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga tuti frutti xtoy laminatsiya", "name": "Usluga Tuti Frutti Xtoy Laminatsiya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.030287+05
vkusno by seva blinchiki s govyadinoy paket	Vkusno By Seva Blinchiki S Govyadinoy Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vkusno by seva blinchiki s govyadinoy paket", "name": "Vkusno By Seva Blinchiki S Govyadinoy Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.038109+05
vodiy ne solyonniy 2 kg paket	Vodiy Ne Solyonniy 2 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vodiy ne solyonniy 2 kg paket", "name": "Vodiy Ne Solyonniy 2 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.039212+05
zarqand pryaniki klubnika 230 gr	Zarqand Pryaniki Klubnika 230 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zarqand pryaniki klubnika 230 gr", "name": "Zarqand Pryaniki Klubnika 230 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.048949+05
zizi monstor rolli 13 gr	Zizi Monstor Rolli 13 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zizi monstor rolli 13 gr", "name": "Zizi Monstor Rolli 13 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.05378+05
Mega pistachio paket	Mega pistachio paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mega pistachio paket", "name": "Mega pistachio paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.577301+05
Mega premum tuzli	Mega premum tuzli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mega premum tuzli", "name": "Mega premum tuzli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.577651+05
opp plyonka 615/18 mikron	OPP plyonka 615/18	Kg		homashyo	{"uom": "Kg", "code": "opp plyonka 615/18 mikron", "name": "OPP plyonka 615/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.908758+05
slayki kak ranshe plambir 70 gr	Slayki Kak Ranshe Plambir 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "slayki kak ranshe plambir 70 gr", "name": "Slayki Kak Ranshe Plambir 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.001424+05
spring klassik miks	Spring Klassik Miks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "spring klassik miks", "name": "Spring Klassik Miks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.003861+05
tortillas sir tandir kabob 20 gr	Tortillas Sir Tandir Kabob 20 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tortillas sir tandir kabob 20 gr", "name": "Tortillas Sir Tandir Kabob 20 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.022044+05
Mega sulton pista 90gr	Mega sulton pista 90gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mega sulton pista 90gr", "name": "Mega sulton pista 90gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.577846+05
Megabayt Paket 5Kg	Megabayt Paket 5Kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Megabayt Paket 5Kg", "name": "Megabayt Paket 5Kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.577944+05
Mega premum tuzsiz	Mega premum tuzsiz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mega premum tuzsiz", "name": "Mega premum tuzsiz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.57775+05
Megabayt suhariki	Megabayt suhariki	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Megabayt suhariki", "name": "Megabayt suhariki", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.578039+05
Mehir parashok	Mehir parashok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mehir parashok", "name": "Mehir parashok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.57815+05
Mejik kofe paket	Mejik kofe paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mejik kofe paket", "name": "Mejik kofe paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.578362+05
Melodi	Melodi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Melodi", "name": "Melodi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.578466+05
Mersi	Mersi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mersi", "name": "Mersi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.578563+05
Mexir keks	Mexir keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mexir keks", "name": "Mexir keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.579116+05
Million benimo asarti	Million benimo asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Million benimo asarti", "name": "Million benimo asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.580075+05
Mini rulet kakao	Mini rulet kakao	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mini rulet kakao", "name": "Mini rulet kakao", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.580669+05
Mini rulet sugishonka	Mini rulet sugishonka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mini rulet sugishonka", "name": "Mini rulet sugishonka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.580988+05
Mix	Mix	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mix", "name": "Mix", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.58109+05
Mojiza aboy pket	Mojiza aboy pket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mojiza aboy pket", "name": "Mojiza aboy pket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.581426+05
Multi ﬂesh	Multi ﬂesh	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Multi ﬂesh", "name": "Multi ﬂesh", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.583002+05
Multimafe	Multimafe	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Multimafe", "name": "Multimafe", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.583102+05
Munis aka paket	Munis aka paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Munis aka paket", "name": "Munis aka paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.583213+05
Musa bella paket	Musa bella paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Musa bella paket", "name": "Musa bella paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.583332+05
Musa desert paket	Musa desert paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Musa desert paket", "name": "Musa desert paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.583444+05
Musaﬀo kiﬁr 900gr 1%	Musaﬀo kiﬁr 900gr 1%	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Musaﬀo kiﬁr 900gr 1%", "name": "Musaﬀo kiﬁr 900gr 1%", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.583767+05
Musli	Musli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Musli", "name": "Musli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.583976+05
Musqaymoq-365kun stakan 100g	Musqaymoq-365kun stakan 100g	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Musqaymoq-365kun stakan 100g", "name": "Musqaymoq-365kun stakan 100g", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.584231+05
Musqaymoq-365kun vanil plombir 120g	Musqaymoq-365kun vanil plombir 120g	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Musqaymoq-365kun vanil plombir 120g", "name": "Musqaymoq-365kun vanil plombir 120g", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.58434+05
opp 530/25	OPP 530/25	Kg		homashyo	{"uom": "Kg", "code": "opp 530/25", "name": "OPP 530/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.876305+05
opp 545/25	OPP 545/25	Kg		homashyo	{"uom": "Kg", "code": "opp 545/25", "name": "OPP 545/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.878329+05
oppm 375/20	OPPM 375/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 375/20", "name": "OPPM 375/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.912864+05
tarvuzcha+qovuncha 55gr	Tarvuzcha+qovuncha 55gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tarvuzcha+qovuncha 55gr", "name": "Tarvuzcha+qovuncha 55gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.017361+05
xom 40gr paket	Xom 40gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xom 40gr paket", "name": "Xom 40gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.044205+05
Mega 20shtuk Tuzli paket	Mega 20shtuk Tuzli paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mega 20shtuk Tuzli paket", "name": "Mega 20shtuk Tuzli paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.57585+05
Mega prazrachniy paket	Mega prazrachniy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mega prazrachniy paket", "name": "Mega prazrachniy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.577529+05
velona asartiy	Velona Asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "velona asartiy", "name": "Velona Asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.03372+05
xamkor pista tuzlanmagan 80 gr	Xamkor Pista Tuzlanmagan 80 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xamkor pista tuzlanmagan 80 gr", "name": "Xamkor Pista Tuzlanmagan 80 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.041419+05
Mejik kofe	Mejik kofe	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mejik kofe", "name": "Mejik kofe", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.578257+05
Mini rulet persik	Mini rulet persik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mini rulet persik", "name": "Mini rulet persik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.580777+05
Mini rulet qulipne	Mini rulet qulipne	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mini rulet qulipne", "name": "Mini rulet qulipne", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.580889+05
Mix Rachki	Mix Rachki	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mix Rachki", "name": "Mix Rachki", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.581193+05
Mojiza 7D	Mojiza 7D	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mojiza 7D", "name": "Mojiza 7D", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.581315+05
Mono	Mono	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mono", "name": "Mono", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.581523+05
Mono elektrik 0.59	Mono elektrik 0.59	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mono elektrik 0.59", "name": "Mono elektrik 0.59", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.581625+05
Moryachki Twist	Moryachki Twist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Moryachki Twist", "name": "Moryachki Twist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.581731+05
Musa ekzotik paket	Musa ekzotik paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Musa ekzotik paket", "name": "Musa ekzotik paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.583544+05
Nachos 3-5-7D	Nachos 3-5-7D	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Nachos 3-5-7D", "name": "Nachos 3-5-7D", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.584624+05
Najim maks paket	Najim maks paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Najim maks paket", "name": "Najim maks paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.584735+05
Nasiba keks	Nasiba keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Nasiba keks", "name": "Nasiba keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.584833+05
Nasir semika 90gr	Nasir semika 90gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Nasir semika 90gr", "name": "Nasir semika 90gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.584942+05
Natural fresh	Natural fresh	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Natural fresh", "name": "Natural fresh", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.585064+05
Navruz pista 40gr	Navruz pista 40gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Navruz pista 40gr", "name": "Navruz pista 40gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.585187+05
Neo marmalad	Neo marmalad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Neo marmalad", "name": "Neo marmalad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.585283+05
Neznayka vaﬂi	Neznayka vaﬂi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Neznayka vaﬂi", "name": "Neznayka vaﬂi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.585383+05
Nilufar sovun	Nilufar sovun	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Nilufar sovun", "name": "Nilufar sovun", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.585481+05
Nixol	Nixol	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Nixol", "name": "Nixol", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.585688+05
Nussa aboy paket	Nussa aboy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Nussa aboy paket", "name": "Nussa aboy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.585788+05
Oltin Don Paket Brak	Oltin Don Paket Brak	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Oltin Don Paket Brak", "name": "Oltin Don Paket Brak", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.591991+05
Omad salyami	Omad salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Omad salyami", "name": "Omad salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.592228+05
Omad sir	Omad sir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Omad sir", "name": "Omad sir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.592332+05
Sirok panda kakos	Sirok panda kakos	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sirok panda kakos", "name": "Sirok panda kakos", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.622071+05
Xrystoﬀgrenki kalbasa chorizo 26sm	Xrystoﬀgrenki kalbasa chorizo 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrystoﬀgrenki kalbasa chorizo 26sm", "name": "Xrystoﬀgrenki kalbasa chorizo 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.653906+05
doni saralangan pista tuzli	Doni Saralangan Pista Tuzli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "doni saralangan pista tuzli", "name": "Doni Saralangan Pista Tuzli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.716496+05
kristal tuz rossiya ekstra 800 gr	Kristal Tuz Rossiya Ekstra 800 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kristal tuz rossiya ekstra 800 gr", "name": "Kristal Tuz Rossiya Ekstra 800 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.785459+05
opp 550/18 kar	OPP 550/18	Kg		homashyo	{"uom": "Kg", "code": "opp 550/18 kar", "name": "OPP 550/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.878492+05
opp 550/18 pol	OPP 550/18	Kg		homashyo	{"uom": "Kg", "code": "opp 550/18 pol", "name": "OPP 550/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.878644+05
oppm 805/20 kar	OPPM 805/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 805/20 kar", "name": "OPPM 805/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.921693+05
pet 580/12 pol	PET 580/12	Kg		homashyo	{"uom": "Kg", "code": "pet 580/12 pol", "name": "PET 580/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.943196+05
Mini rulet banan	Mini rulet banan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mini rulet banan", "name": "Mini rulet banan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.580552+05
pet 750/12	PET 750/12	Kg		homashyo	{"uom": "Kg", "code": "pet 750/12", "name": "PET 750/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.947665+05
Miloner salyami	Miloner salyami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Miloner salyami", "name": "Miloner salyami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.580174+05
Mimos skoch paket	Mimos skoch paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mimos skoch paket", "name": "Mimos skoch paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.580442+05
Osiyo Poroshok	Osiyo Poroshok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Osiyo Poroshok", "name": "Osiyo Poroshok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.593123+05
Spays lavroviy list paket	Spays lavroviy list paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Spays lavroviy list paket", "name": "Spays lavroviy list paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.625244+05
aisha wafers xolodniy payka	Aisha Wafers Xolodniy Payka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "aisha wafers xolodniy payka", "name": "Aisha Wafers Xolodniy Payka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.665114+05
day salfetka 72 sht yashil kok	Day Salfetka 72 Sht Yashil Kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "day salfetka 72 sht yashil kok", "name": "Day Salfetka 72 Sht Yashil Kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.707424+05
eko salfetka7 2 sht bebbi grim	Eko Salfetka7 2 Sht Bebbi Grim	Kg		tayyor mahsulot	{"uom": "Kg", "code": "eko salfetka7 2 sht bebbi grim", "name": "Eko Salfetka7 2 Sht Bebbi Grim", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.721748+05
fazo semechka 2 kg paket	Fazo Semechka 2 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fazo semechka 2 kg paket", "name": "Fazo Semechka 2 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.727318+05
isko mini bravn 200gr	Isko Mini Bravn 200gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "isko mini bravn 200gr", "name": "Isko Mini Bravn 200gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.753023+05
isko mitti bruni xolodnaya payka	Isko Mitti Bruni Xolodnaya Payka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "isko mitti bruni xolodnaya payka", "name": "Isko Mitti Bruni Xolodnaya Payka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.753185+05
isko pop cake banan 17 gr	Isko Pop Cake Banan 17 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "isko pop cake banan 17 gr", "name": "Isko Pop Cake Banan 17 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.753364+05
isko pop cake cherry ﬂavor 17 gr	Isko Pop Cake Cherry Flavor 17 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "isko pop cake cherry ﬂavor 17 gr", "name": "Isko Pop Cake Cherry Flavor 17 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.753519+05
isko popkek 200gr	Isko Popkek 200gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "isko popkek 200gr", "name": "Isko Popkek 200gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.753828+05
iz karzinki piyoz 1.5kg paket	Iz Karzinki Piyoz 1.5kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "iz karzinki piyoz 1.5kg paket", "name": "Iz Karzinki Piyoz 1.5kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.753995+05
nasr simba chips pomidor va qalampir 25 gr	Nasr Simba Chips Pomidor Va Qalampir 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "nasr simba chips pomidor va qalampir 25 gr", "name": "Nasr Simba Chips Pomidor Va Qalampir 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.846584+05
opp 565/25	OPP 565/25	Kg		homashyo	{"uom": "Kg", "code": "opp 565/25", "name": "OPP 565/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.879467+05
opp 570/18	OPP 570/18	Kg		homashyo	{"uom": "Kg", "code": "opp 570/18", "name": "OPP 570/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.879757+05
opp 580/20 pff pol	OPP PFF 580/20	Kg		homashyo	{"uom": "Kg", "code": "opp 580/20 pff pol", "name": "OPP PFF 580/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.881181+05
pet + opp payushiy	Pet + Opp Payushiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet + opp payushiy", "name": "Pet + Opp Payushiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.938086+05
pure milky tvorog zernistiy 400 gr	Pure Milky Tvorog Zernistiy 400 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pure milky tvorog zernistiy 400 gr", "name": "Pure Milky Tvorog Zernistiy 400 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.963113+05
sandee sandvich paket	Sandee Sandvich Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sandee sandvich paket", "name": "Sandee Sandvich Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.982696+05
sayqal 70gr	Sayqal 70gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sayqal 70gr", "name": "Sayqal 70gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.987717+05
stiks suxariki shashlik 20gr	Stiks Suxariki Shashlik 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "stiks suxariki shashlik 20gr", "name": "Stiks Suxariki Shashlik 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.009735+05
xot dog chempion paket samarqand	Xot Dog Chempion Paket Samarqand	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xot dog chempion paket samarqand", "name": "Xot Dog Chempion Paket Samarqand", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.0445+05
Oq qand	Oq qand	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Oq qand", "name": "Oq qand", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.592528+05
Orzu	Orzu	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Orzu", "name": "Orzu", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.592897+05
Orzu qurt	Orzu qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Orzu qurt", "name": "Orzu qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.593007+05
orzu plombir sincers 100 gr	Orzu Plombir Sincers 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "orzu plombir sincers 100 gr", "name": "Orzu Plombir Sincers 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.926648+05
orzu super vkusnyashka 70 gr	Orzu Super Vkusnyashka 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "orzu super vkusnyashka 70 gr", "name": "Orzu Super Vkusnyashka 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.927086+05
patitos chips kuritsa 20gr	Patitos Chips Kuritsa 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "patitos chips kuritsa 20gr", "name": "Patitos Chips Kuritsa 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.932098+05
premium cotton zamok paket usluga	Premium Cotton Zamok Paket Usluga	Kg		tayyor mahsulot	{"uom": "Kg", "code": "premium cotton zamok paket usluga", "name": "Premium Cotton Zamok Paket Usluga", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.961103+05
frustick klubnika persik laym	Frustick Klubnika Persik Laym	Kg		tayyor mahsulot	{"uom": "Kg", "code": "frustick klubnika persik laym", "name": "Frustick Klubnika Persik Laym", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.731792+05
imperial guruch alanga 1 kg	Imperial Guruch Alanga 1 Kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "imperial guruch alanga 1 kg", "name": "Imperial Guruch Alanga 1 Kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.751306+05
imperial lazer gruch 1kg	Imperial Lazer Gruch 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "imperial lazer gruch 1kg", "name": "Imperial Lazer Gruch 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.751458+05
imperiya makaron 400 gr	Imperiya Makaron 400 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "imperiya makaron 400 gr", "name": "Imperiya Makaron 400 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.751598+05
innwood	Innwood	Kg		tayyor mahsulot	{"uom": "Kg", "code": "innwood", "name": "Innwood", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.751773+05
irmik mane krupa 1kg paket	Irmik Mane Krupa 1kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "irmik mane krupa 1kg paket", "name": "Irmik Mane Krupa 1kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.751928+05
isko 16sm	Isko 16sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "isko 16sm", "name": "Isko 16sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.752091+05
isko 17sm	Isko 17sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "isko 17sm", "name": "Isko 17sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.752247+05
isko hops xalod payka 40gr	Isko Hops Xalod Payka 40gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "isko hops xalod payka 40gr", "name": "Isko Hops Xalod Payka 40gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.752868+05
iz korzinki brownie cake 8 sht	Iz Korzinki Brownie Cake 8 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "iz korzinki brownie cake 8 sht", "name": "Iz Korzinki Brownie Cake 8 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.754623+05
iz korzinki donut cake marshmello 6 sht	Iz Korzinki Donut Cake Marshmello 6 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "iz korzinki donut cake marshmello 6 sht", "name": "Iz Korzinki Donut Cake Marshmello 6 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.754906+05
iz korzinki pie cake 30 gr orange	Iz Korzinki Pie Cake 30 Gr Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "iz korzinki pie cake 30 gr orange", "name": "Iz Korzinki Pie Cake 30 Gr Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.755045+05
iz korzinki wafer cubes funduk 200 gr	Iz Korzinki Wafer Cubes Funduk 200 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "iz korzinki wafer cubes funduk 200 gr", "name": "Iz Korzinki Wafer Cubes Funduk 200 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.75533+05
iz korzinki wafer cubes kakos 200 gr	Iz Korzinki Wafer Cubes Kakos 200 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "iz korzinki wafer cubes kakos 200 gr", "name": "Iz Korzinki Wafer Cubes Kakos 200 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.755474+05
iz korznki chocolate wafer 30 gr orange	Iz Korznki Chocolate Wafer 30 Gr Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "iz korznki chocolate wafer 30 gr orange", "name": "Iz Korznki Chocolate Wafer 30 Gr Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.755752+05
jaida	Jaida	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jaida", "name": "Jaida", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.755898+05
jajji grenki 30gr	Jajji Grenki 30gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jajji grenki 30gr", "name": "Jajji Grenki 30gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.75631+05
jajji steyk + salyami 15 gr	Jajji Steyk + Salyami 15 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jajji steyk + salyami 15 gr", "name": "Jajji Steyk + Salyami 15 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.756454+05
jayma zippaket	Jayma Zippaket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jayma zippaket", "name": "Jayma Zippaket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.756736+05
jele 4sm	Jele 4sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jele 4sm", "name": "Jele 4sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.757147+05
jele prazrachniy 4sm	Jele Prazrachniy 4sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jele prazrachniy 4sm", "name": "Jele Prazrachniy 4sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.757287+05
jeleyka zombi 15 gr	Jeleyka Zombi 15 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jeleyka zombi 15 gr", "name": "Jeleyka Zombi 15 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.757684+05
jolly molly 70 gr	Jolly Molly 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jolly molly 70 gr", "name": "Jolly Molly 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.771895+05
jonn waﬀers milk	Jonn Waﬀers Milk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jonn waﬀers milk", "name": "Jonn Waﬀers Milk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.772357+05
kak ranshe 1 kg paket	Kak Ranshe 1 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kak ranshe 1 kg paket", "name": "Kak Ranshe 1 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.772877+05
opp 580/25 pol	OPP 580/25	Kg		homashyo	{"uom": "Kg", "code": "opp 580/25 pol", "name": "OPP 580/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.881446+05
opp 600/30 pf	OPP 600/30	Kg		homashyo	{"uom": "Kg", "code": "opp 600/30 pf", "name": "OPP 600/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.883757+05
pet 630/12 pol	PET 630/12	Kg		homashyo	{"uom": "Kg", "code": "pet 630/12 pol", "name": "PET 630/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.944319+05
pet 655/12 pol	PET 655/12	Kg		homashyo	{"uom": "Kg", "code": "pet 655/12 pol", "name": "PET 655/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.944802+05
prazrachniy zip paket	Prazrachniy Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachniy zip paket", "name": "Prazrachniy Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.960349+05
imkon rajok 110gr	Imkon Rajok 110gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "imkon rajok 110gr", "name": "Imkon Rajok 110gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.750991+05
imorat jidjiy aboy	Imorat Jidjiy Aboy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "imorat jidjiy aboy", "name": "Imorat Jidjiy Aboy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.751144+05
jolly molly 15 gr	Jolly Molly 15 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jolly molly 15 gr", "name": "Jolly Molly 15 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.77176+05
jolly molly 8 gr	Jolly Molly 8 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jolly molly 8 gr", "name": "Jolly Molly 8 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.772051+05
jolly molly pencil mix metal	Jolly Molly Pencil Mix Metal	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jolly molly pencil mix metal", "name": "Jolly Molly Pencil Mix Metal", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.772209+05
juda mazza burger	Juda Mazza Burger	Kg		tayyor mahsulot	{"uom": "Kg", "code": "juda mazza burger", "name": "Juda Mazza Burger", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.772536+05
jussiya 70 gr asarty	Jussiya 70 Gr Asarty	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jussiya 70 gr asarty", "name": "Jussiya 70 Gr Asarty", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.772719+05
kaklet klassik paket	Kaklet Klassik Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kaklet klassik paket", "name": "Kaklet Klassik Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.773044+05
mega pista 90gr	Mega Pista 90gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega pista 90gr", "name": "Mega Pista 90gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.819715+05
mega semechka 180gr zip paket	Mega Semechka 180gr Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega semechka 180gr zip paket", "name": "Mega Semechka 180gr Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.820057+05
mega sirniy palichka	Mega Sirniy Palichka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega sirniy palichka", "name": "Mega Sirniy Palichka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.82033+05
mega sulton 20shtuk paket	Mega Sulton 20shtuk Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega sulton 20shtuk paket", "name": "Mega Sulton 20shtuk Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.820531+05
memor valik paket	Memor Valik Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "memor valik paket", "name": "Memor Valik Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.822018+05
mini biskvit 5 sht	Mini Biskvit 5 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mini biskvit 5 sht", "name": "Mini Biskvit 5 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.825494+05
miray foxs kakos 200 gr	Miray Foxs Kakos 200 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miray foxs kakos 200 gr", "name": "Miray Foxs Kakos 200 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.827226+05
miray payushiy 22,5	Miray Payushiy 22,5	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miray payushiy 22,5", "name": "Miray Payushiy 22,5", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.827523+05
miray payushiy 27,5 sm	Miray Payushiy 27,5 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miray payushiy 27,5 sm", "name": "Miray Payushiy 27,5 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.827669+05
miray pie cake 8 sht orange	Miray Pie Cake 8 Sht Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miray pie cake 8 sht orange", "name": "Miray Pie Cake 8 Sht Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.82815+05
miray romni cake xaloniy payka 45gr	Miray Romni Cake Xaloniy Payka 45gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miray romni cake xaloniy payka 45gr", "name": "Miray Romni Cake Xaloniy Payka 45gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.828513+05
mishka buni	Mishka Buni	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mishka buni", "name": "Mishka Buni", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.828673+05
mix max yashil	Mix Max Yashil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mix max yashil", "name": "Mix Max Yashil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.82897+05
mod fudbolka dlya malchikov paket	Mod Fudbolka Dlya Malchikov Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mod fudbolka dlya malchikov paket", "name": "Mod Fudbolka Dlya Malchikov Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.829571+05
mojjo keshyu 40 gr	Mojjo Keshyu 40 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mojjo keshyu 40 gr", "name": "Mojjo Keshyu 40 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.831032+05
mojjo qurt	Mojjo Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mojjo qurt", "name": "Mojjo Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.831502+05
monny salfetka 120 sht japan	Monny Salfetka 120 Sht Japan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "monny salfetka 120 sht japan", "name": "Monny Salfetka 120 Sht Japan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.831992+05
mono elektrik 17x35 paket	Mono Elektrik 17x35 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mono elektrik 17x35 paket", "name": "Mono Elektrik 17x35 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.833611+05
mono elektrik 26x10 paket	Mono Elektrik 26x10 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mono elektrik 26x10 paket", "name": "Mono Elektrik 26x10 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.833915+05
opp 625/30 kar 3	OPP 625/30	Kg		homashyo	{"uom": "Kg", "code": "opp 625/30 kar 3", "name": "OPP 625/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.885425+05
pitos maks	Pitos Maks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pitos maks", "name": "Pitos Maks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.954265+05
vintovka dinya	Vintovka Dinya	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vintovka dinya", "name": "Vintovka Dinya", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.036329+05
jessica 25/30 zip paket	Jessica 25/30 Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jessica 25/30 zip paket", "name": "Jessica 25/30 Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.77115+05
jeti batir suluguni paket	Jeti Batir Suluguni Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jeti batir suluguni paket", "name": "Jeti Batir Suluguni Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.771592+05
jeleyka tutti frutti 15 gr	Jeleyka Tutti Frutti 15 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jeleyka tutti frutti 15 gr", "name": "Jeleyka Tutti Frutti 15 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.757426+05
minions marmelad	Minions Marmelad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "minions marmelad", "name": "Minions Marmelad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.826214+05
mino sasiska xalol	Mino Sasiska Xalol	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mino sasiska xalol", "name": "Mino Sasiska Xalol", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.826352+05
miray foxs funduk 200 gr	Miray Foxs Funduk 200 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miray foxs funduk 200 gr", "name": "Miray Foxs Funduk 200 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.8271+05
miray pie cake 8 sht kok	Miray Pie Cake 8 Sht Kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miray pie cake 8 sht kok", "name": "Miray Pie Cake 8 Sht Kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.827986+05
mix max asarty	Mix Max Asarty	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mix max asarty", "name": "Mix Max Asarty", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.828825+05
miyahkiy pecheni chernika kichik	Miyahkiy Pecheni Chernika Kichik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miyahkiy pecheni chernika kichik", "name": "Miyahkiy Pecheni Chernika Kichik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.829105+05
miyahkiy pecheni kulibnika kichik	Miyahkiy Pecheni Kulibnika Kichik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miyahkiy pecheni kulibnika kichik", "name": "Miyahkiy Pecheni Kulibnika Kichik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.829251+05
miyahkiy pecheni shokalad kichik	Miyahkiy Pecheni Shokalad Kichik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miyahkiy pecheni shokalad kichik", "name": "Miyahkiy Pecheni Shokalad Kichik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.829391+05
mod mayka dlya devuchek paket	Mod Mayka Dlya Devuchek Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mod mayka dlya devuchek paket", "name": "Mod Mayka Dlya Devuchek Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.829838+05
mojiza chips	Mojiza Chips	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mojiza chips", "name": "Mojiza Chips", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.829998+05
mojjo 120gr zip paket	Mojjo 120gr Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mojjo 120gr zip paket", "name": "Mojjo 120gr Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.830147+05
mojjo 2 kg paket	Mojjo 2 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mojjo 2 kg paket", "name": "Mojjo 2 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.830299+05
mojjo 20 sht paket	Mojjo 20 Sht Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mojjo 20 sht paket", "name": "Mojjo 20 Sht Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.830437+05
mojjo 3 kg paket	Mojjo 3 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mojjo 3 kg paket", "name": "Mojjo 3 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.830591+05
mojjo 5gr	Mojjo 5gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mojjo 5gr", "name": "Mojjo 5gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.830741+05
mojjo bodom 40 gr	Mojjo Bodom 40 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mojjo bodom 40 gr", "name": "Mojjo Bodom 40 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.830887+05
mojjo pista	Mojjo Pista	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mojjo pista", "name": "Mojjo Pista", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.831182+05
mojjo pista 30 gr	Mojjo Pista 30 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mojjo pista 30 gr", "name": "Mojjo Pista 30 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.831349+05
mojjo qurt 30 gr	Mojjo Qurt 30 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mojjo qurt 30 gr", "name": "Mojjo Qurt 30 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.831662+05
molochniy qurt paket	Molochniy Qurt Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "molochniy qurt paket", "name": "Molochniy Qurt Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.831839+05
mono elektrik 00.6	Mono Elektrik 00.6	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mono elektrik 00.6", "name": "Mono Elektrik 00.6", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.832697+05
mono elektrik 10/32 skotch paket	Mono Elektrik 10/32 Skotch Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mono elektrik 10/32 skotch paket", "name": "Mono Elektrik 10/32 Skotch Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.833179+05
mono elektrik 10x30 paket	Mono Elektrik 10x30 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mono elektrik 10x30 paket", "name": "Mono Elektrik 10x30 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.833472+05
mono elektrik 29x15 paket	Mono Elektrik 29x15 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mono elektrik 29x15 paket", "name": "Mono Elektrik 29x15 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.834056+05
mono elektrik praznichniy	Mono Elektrik Praznichniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mono elektrik praznichniy", "name": "Mono Elektrik Praznichniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.834362+05
mors bogatirskiy 2 litr	Mors Bogatirskiy 2 Litr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mors bogatirskiy 2 litr", "name": "Mors Bogatirskiy 2 Litr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.834508+05
opp 645/20 PFF kar	OPP PFF 645/20	Kg		homashyo	{"uom": "Kg", "code": "opp 645/20 PFF kar", "name": "OPP PFF 645/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.886413+05
pury milky sirok klubnika 40 gr	Pury Milky Sirok Klubnika 40 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pury milky sirok klubnika 40 gr", "name": "Pury Milky Sirok Klubnika 40 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.963433+05
mini rem	Mini Rem	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mini rem", "name": "Mini Rem", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.825936+05
milano zip paket	Milano Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "milano zip paket", "name": "Milano Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.824896+05
mini maﬁni jem asartiy	Mini Maﬁni Jem Asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mini maﬁni jem asartiy", "name": "Mini Maﬁni Jem Asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.825788+05
mini fruti 60gr	Mini Fruti 60gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mini fruti 60gr", "name": "Mini Fruti 60gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.825636+05
minions 70 gr	Minions 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "minions 70 gr", "name": "Minions 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.826074+05
miray brownie cake 8 sht 200 gr	Miray Brownie Cake 8 Sht 200 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miray brownie cake 8 sht 200 gr", "name": "Miray Brownie Cake 8 Sht 200 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.826484+05
miray bruni 25 gr	Miray Bruni 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miray bruni 25 gr", "name": "Miray Bruni 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.826622+05
miray mini pie 180 gr orange	Miray Mini Pie 180 Gr Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miray mini pie 180 gr orange", "name": "Miray Mini Pie 180 Gr Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.827377+05
miray pie cake 30 gr kok	Miray Pie Cake 30 Gr Kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miray pie cake 30 gr kok", "name": "Miray Pie Cake 30 Gr Kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.827812+05
miray praz/niy 22/30	Miray Praz/niy 22/30	Kg		tayyor mahsulot	{"uom": "Kg", "code": "miray praz/niy 22/30", "name": "Miray Praz/niy 22/30", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.828351+05
mono elektrik 007	Mono Elektrik 007	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mono elektrik 007", "name": "Mono Elektrik 007", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.832876+05
mono elektrik 10/27,75 paket	Mono Elektrik 10/27,75 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mono elektrik 10/27,75 paket", "name": "Mono Elektrik 10/27,75 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.833032+05
mono elektrik 10/37 paket	Mono Elektrik 10/37 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mono elektrik 10/37 paket", "name": "Mono Elektrik 10/37 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.833328+05
mono elektrik 17x40 paket	Mono Elektrik 17x40 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mono elektrik 17x40 paket", "name": "Mono Elektrik 17x40 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.833765+05
mono elektrik 38x11 paket	Mono Elektrik 38x11 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mono elektrik 38x11 paket", "name": "Mono Elektrik 38x11 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.834215+05
mosh 1kg	Mosh 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mosh 1kg", "name": "Mosh 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.834659+05
mpet +pe 40cm	Mpet +pe 40cm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mpet +pe 40cm", "name": "Mpet +pe 40cm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.83482+05
opp 650/35 kar	OPP 650/35	Kg		homashyo	{"uom": "Kg", "code": "opp 650/35 kar", "name": "OPP 650/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.886973+05
opp 675/35	OPP 675/35	Kg		homashyo	{"uom": "Kg", "code": "opp 675/35", "name": "OPP 675/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.889353+05
opp+spp 190/40	Opp+spp 190/40	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp+spp 190/40", "name": "Opp+spp 190/40", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.909774+05
realniy plambir 100gr	Realniy Plambir 100gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "realniy plambir 100gr", "name": "Realniy Plambir 100gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.968236+05
salfetka aloe comfort 72 sht	Salfetka Aloe Comfort 72 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka aloe comfort 72 sht", "name": "Salfetka Aloe Comfort 72 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.976072+05
sandey kukruz paket	Sandey Kukruz Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sandey kukruz paket", "name": "Sandey Kukruz Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.983376+05
wanted shkalat paket 900gr	Wanted Shkalat Paket 900gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "wanted shkalat paket 900gr", "name": "Wanted Shkalat Paket 900gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.039903+05
xeppi foks 25/35	Xeppi Foks 25/35	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xeppi foks 25/35", "name": "Xeppi Foks 25/35", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.042895+05
kolly molly percil mix 15 gr	Kolly Molly Percil Mix 15 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kolly molly percil mix 15 gr", "name": "Kolly Molly Percil Mix 15 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.779571+05
milliy osh tuzi paket	Milliy Osh Tuzi Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "milliy osh tuzi paket", "name": "Milliy Osh Tuzi Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.825356+05
opp+spp 380/50	Opp+spp 380/50	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp+spp 380/50", "name": "Opp+spp 380/50", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.909998+05
pz 500/25	Pz 500/25	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pz 500/25", "name": "Pz 500/25", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.96359+05
sayqal 80gr	Sayqal 80gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sayqal 80gr", "name": "Sayqal 80gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.988023+05
spp 190/20	Spp 190/20	Kg		tayyor mahsulot	{"uom": "Kg", "code": "spp 190/20", "name": "Spp 190/20", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.002986+05
spp 220/20	Spp 220/20	Kg		tayyor mahsulot	{"uom": "Kg", "code": "spp 220/20", "name": "Spp 220/20", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.003135+05
strike rich mansion inferno	Strike Rich Mansion Inferno	Kg		tayyor mahsulot	{"uom": "Kg", "code": "strike rich mansion inferno", "name": "Strike Rich Mansion Inferno", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.01073+05
milk vafer	Milk Vafer	Kg		tayyor mahsulot	{"uom": "Kg", "code": "milk vafer", "name": "Milk Vafer", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.825207+05
mikki cruspy kukuruz 100 gr rulon	Mikki Cruspy Kukuruz 100 Gr Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mikki cruspy kukuruz 100 gr rulon", "name": "Mikki Cruspy Kukuruz 100 Gr Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.824127+05
Lovis marojniy	Lovis marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lovis marojniy", "name": "Lovis marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.56217+05
Marvel spider man shashlik	Marvel spider man shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Marvel spider man shashlik", "name": "Marvel spider man shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.574434+05
Mikki maus paket	Mikki maus paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mikki maus paket", "name": "Mikki maus paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.579553+05
Milkos paket	Milkos paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Milkos paket", "name": "Milkos paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.579873+05
Mr stick suxarik 25gr asartiy	Mr stick suxarik 25gr asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mr stick suxarik 25gr asartiy", "name": "Mr stick suxarik 25gr asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.582782+05
OPP 725/20 Pol pff	OPP POL PFF 725/20	Kg		homashyo	{"uom": "Kg", "code": "OPP 725/20 Pol pff", "name": "OPP POL PFF 725/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.590338+05
Paket 2kg	Paket 2kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Paket 2kg", "name": "Paket 2kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.595582+05
Pelmeni rulon qizil 500gr	Pelmeni rulon qizil 500gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pelmeni rulon qizil 500gr", "name": "Pelmeni rulon qizil 500gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.599374+05
Rastitel/masla	Rastitel/masla	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Rastitel/masla", "name": "Rastitel/masla", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.610723+05
Simba kukruz olma	Simba kukruz olma	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Simba kukruz olma", "name": "Simba kukruz olma", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.620128+05
Taretto 111-555-777	Taretto 111-555-777	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Taretto 111-555-777", "name": "Taretto 111-555-777", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.630094+05
Toretto wavy shashlik 25g	Toretto wavy shashlik 25g	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto wavy shashlik 25g", "name": "Toretto wavy shashlik 25g", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.635113+05
Xrustyashki shashlik 35sm	Xrustyashki shashlik 35sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustyashki shashlik 35sm", "name": "Xrustyashki shashlik 35sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.652575+05
Yello suxariki chiken	Yello suxariki chiken	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Yello suxariki chiken", "name": "Yello suxariki chiken", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.655775+05
Zizi 500gr karamel	Zizi 500gr karamel	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi 500gr karamel", "name": "Zizi 500gr karamel", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.658982+05
Zo'r chips 5D chili	Zo'r chips 5D chili	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zo'r chips 5D chili", "name": "Zo'r chips 5D chili", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.662301+05
banitto Mod sergili skoch paket	banitto Mod sergili skoch paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "banitto Mod sergili skoch paket", "name": "banitto Mod sergili skoch paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.674665+05
chaps shashlik kotta 36 sm	Chaps Shashlik Kotta 36 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps shashlik kotta 36 sm", "name": "Chaps Shashlik Kotta 36 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.686176+05
jem 1350/25	JEM 1350/25	Kg		homashyo	{"uom": "Kg", "code": "jem 1350/25", "name": "JEM 1350/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.758489+05
metronex paket 22/10	Metronex Paket 22/10	Kg		tayyor mahsulot	{"uom": "Kg", "code": "metronex paket 22/10", "name": "Metronex Paket 22/10", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.822947+05
opp 885/18  PF kar	OPP 885/18	Kg		homashyo	{"uom": "Kg", "code": "opp 885/18  PF kar", "name": "OPP 885/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.90501+05
two bite choco sendvich 330 gr	Two Bite Choco Sendvich 330 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "two bite choco sendvich 330 gr", "name": "Two Bite Choco Sendvich 330 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.027249+05
Choko rols banan	Choko rols banan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Choko rols banan", "name": "Choko rols banan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.515116+05
Kalon semechka 5kg	Kalon semechka 5kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kalon semechka 5kg", "name": "Kalon semechka 5kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.547226+05
Like sponge cake	Like sponge cake	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Like sponge cake", "name": "Like sponge cake", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.56123+05
Pittos trubichka smetana	Pittos trubichka smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pittos trubichka smetana", "name": "Pittos trubichka smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.60295+05
sandey biskivit	Sandey Biskivit	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sandey biskivit", "name": "Sandey Biskivit", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.983042+05
vakum paket 35/15	Vakum Paket 35/15	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vakum paket 35/15", "name": "Vakum Paket 35/15", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.032588+05
zip paket 30 / 25	Zip Paket 30/25	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zip paket 30 / 25", "name": "Zip Paket 30/25", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.050559+05
sandee donar paket	Sandee Donar Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sandee donar paket", "name": "Sandee Donar Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.982532+05
usluga playova 28/30	Usluga Playova 28/30	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga playova 28/30", "name": "Usluga Playova 28/30", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.029686+05
Dora Oshka plombir 15%	Dora Oshka plombir 15%	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Dora Oshka plombir 15%", "name": "Dora Oshka plombir 15%", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.52228+05
Ekskuliziv pista	Ekskuliziv pista	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ekskuliziv pista", "name": "Ekskuliziv pista", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.523546+05
Fayz-m shalvar paket	Fayz-m shalvar paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Fayz-m shalvar paket", "name": "Fayz-m shalvar paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.526856+05
Frutella jele 70gr	Frutella jele 70gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frutella jele 70gr", "name": "Frutella jele 70gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.530127+05
Hello kitty marmelat	Hello kitty marmelat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Hello kitty marmelat", "name": "Hello kitty marmelat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.533904+05
Imperiya 70gr asartiy	Imperiya 70gr asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Imperiya 70gr asartiy", "name": "Imperiya 70gr asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.535321+05
Jem 500/20	JEM 500/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 500/20", "name": "JEM 500/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.541461+05
Jem 510/20	JEM 510/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 510/20", "name": "JEM 510/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.541584+05
Kosta 1kg	Kosta 1kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosta 1kg", "name": "Kosta 1kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.555264+05
Makiz N175 paket	Makiz N175 paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz N175 paket", "name": "Makiz N175 paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.569798+05
Maks kofe paket	Maks kofe paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Maks kofe paket", "name": "Maks kofe paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.57177+05
Sayqal 500gr asartiy paket	Sayqal 500gr asartiy paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sayqal 500gr asartiy paket", "name": "Sayqal 500gr asartiy paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.615633+05
Sulaymon Paket Max 20 Dona	Sulaymon Paket Max 20 Dona	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sulaymon Paket Max 20 Dona", "name": "Sulaymon Paket Max 20 Dona", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.626324+05
Toretto asartiy	Toretto asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto asartiy", "name": "Toretto asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.633762+05
Xrustik plus 777 kotta 80gr paket	Xrustik plus 777 kotta 80gr paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustik plus 777 kotta 80gr paket", "name": "Xrustik plus 777 kotta 80gr paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.649614+05
Xrustyashki smetana 35sm	Xrustyashki smetana 35sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustyashki smetana 35sm", "name": "Xrustyashki smetana 35sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.6531+05
Xrustyashkiy baget sir 26sm	Xrustyashkiy baget sir 26sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustyashkiy baget sir 26sm", "name": "Xrustyashkiy baget sir 26sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.653509+05
bonnur xalodniy payka	Bonnur Xalodniy Payka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bonnur xalodniy payka", "name": "Bonnur Xalodniy Payka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.682009+05
cheers nachos kalbaski chili 210 gr 46 sm	Cheers Nachos Kalbaski Chili 210 Gr 46 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nachos kalbaski chili 210 gr 46 sm", "name": "Cheers Nachos Kalbaski Chili 210 Gr 46 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.689366+05
chempion samarqand big sendvich	Chempion Samarqand Big Sendvich	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion samarqand big sendvich", "name": "Chempion Samarqand Big Sendvich", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.694207+05
chiki boom araxis 40 gr	Chiki Boom Araxis 40 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chiki boom araxis 40 gr", "name": "Chiki Boom Araxis 40 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.696494+05
deluxe chewing gum 3,5 gr asarty	Deluxe Chewing Gum 3,5 Gr Asarty	Kg		tayyor mahsulot	{"uom": "Kg", "code": "deluxe chewing gum 3,5 gr asarty", "name": "Deluxe Chewing Gum 3,5 Gr Asarty", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.709158+05
shox korin 30gr	Shox Korin 30gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shox korin 30gr", "name": "Shox Korin 30gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.994034+05
simba chips to'vuq 14gr	Simba Chips To'vuq 14gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba chips to'vuq 14gr", "name": "Simba Chips To'vuq 14gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.99851+05
st01 1280/20	ST01 1280/20	Kg		homashyo	{"uom": "Kg", "code": "st01 1280/20", "name": "ST01 1280/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.005563+05
Chuda fkus	Chuda fkus	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Chuda fkus", "name": "Chuda fkus", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.519703+05
Kubik rubik asartiy	Kubik rubik asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kubik rubik asartiy", "name": "Kubik rubik asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.556844+05
Lelingrad marojniy	Lelingrad marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Lelingrad marojniy", "name": "Lelingrad marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.560809+05
usluga xeppi foks 30/40 paket zamok	Usluga Xeppi Foks 30/40 Paket Zamok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga xeppi foks 30/40 paket zamok", "name": "Usluga Xeppi Foks 30/40 Paket Zamok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.030603+05
Aroma tea 0.33	Aroma tea 0.33	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Aroma tea 0.33", "name": "Aroma tea 0.33", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.485486+05
Eko sfat aboy paket 3kg	Eko sfat aboy paket 3kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Eko sfat aboy paket 3kg", "name": "Eko sfat aboy paket 3kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.522827+05
Fanat 3D tomat spays	Fanat 3D tomat spays	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Fanat 3D tomat spays", "name": "Fanat 3D tomat spays", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.525714+05
Gold 999.9 kottasi	Gold 999.9 kottasi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Gold 999.9 kottasi", "name": "Gold 999.9 kottasi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.53128+05
Jem  495/25	JEM 495/25	Kg		homashyo	{"uom": "Kg", "code": "Jem  495/25", "name": "JEM 495/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.536657+05
Jem 515/20	JEM 515/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 515/20", "name": "JEM 515/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.541778+05
Kaﬀeino aranjiviy	Kaﬀeino aranjiviy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kaﬀeino aranjiviy", "name": "Kaﬀeino aranjiviy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.548525+05
Kosmos Stix steyk	Kosmos Stix steyk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kosmos Stix steyk", "name": "Kosmos Stix steyk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.555049+05
Legenda halodniy payka	Legenda halodniy payka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Legenda halodniy payka", "name": "Legenda halodniy payka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.560699+05
Nivea so'vun asartiy	Nivea so'vun asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Nivea so'vun asartiy", "name": "Nivea so'vun asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.585584+05
Omad smetana	Omad smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Omad smetana", "name": "Omad smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.592429+05
PAYuShIY 120/20 TRUBOChKA	PAYuShIY 120/20 TRUBOChKA	Kg		tayyor mahsulot	{"uom": "Kg", "code": "PAYuShIY 120/20 TRUBOChKA", "name": "PAYuShIY 120/20 TRUBOChKA", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.593397+05
Pelmeni paket 500gr	Pelmeni paket 500gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pelmeni paket 500gr", "name": "Pelmeni paket 500gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.599016+05
Sardor simba	Sardor simba	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sardor simba", "name": "Sardor simba", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.615536+05
Shox bom zvachka klubnika olma shokolad persik	Shox bom zvachka klubnika olma shokolad persik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shox bom zvachka klubnika olma shokolad persik", "name": "Shox bom zvachka klubnika olma shokolad persik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.617303+05
Super mario	Super mario	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Super mario", "name": "Super mario", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.627113+05
Toretto golden dak shashlik	Toretto golden dak shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Toretto golden dak shashlik", "name": "Toretto golden dak shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.63397+05
Uzterra fri salyami shashlik tabaka 20gr	Uzterra fri salyami shashlik tabaka 20gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Uzterra fri salyami shashlik tabaka 20gr", "name": "Uzterra fri salyami shashlik tabaka 20gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.639303+05
Xrusttik+ 50gr sho'kalat paket	Xrusttik+ 50gr sho'kalat paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrusttik+ 50gr sho'kalat paket", "name": "Xrusttik+ 50gr sho'kalat paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.652276+05
dilmuratov saxarniy rojok 100 gr	Dilmuratov Saxarniy Rojok 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dilmuratov saxarniy rojok 100 gr", "name": "Dilmuratov Saxarniy Rojok 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.711837+05
golden sticks sirniy paket	Golden Sticks Sirniy Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "golden sticks sirniy paket", "name": "Golden Sticks Sirniy Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.737345+05
imkon plus the best 90 gr	Imkon Plus The Best 90 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "imkon plus the best 90 gr", "name": "Imkon Plus The Best 90 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.75069+05
kars kurs xalol 30 gr xrust	Kars Kurs Xalol 30 Gr Xrust	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kars kurs xalol 30 gr xrust", "name": "Kars Kurs Xalol 30 Gr Xrust", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.774289+05
korovka 500 gr asarti paket nodir	Korovka 500 Gr Asarti Paket Nodir	Kg		tayyor mahsulot	{"uom": "Kg", "code": "korovka 500 gr asarti paket nodir", "name": "Korovka 500 Gr Asarti Paket Nodir", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.780451+05
stiks payushiy 18gr	Stiks Payushiy 18gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "stiks payushiy 18gr", "name": "Stiks Payushiy 18gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.008559+05
yakar chupa chups arbuz	Yakar Chupa Chups Arbuz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "yakar chupa chups arbuz", "name": "Yakar Chupa Chups Arbuz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.045922+05
Arapcha zip paket asartiy sho'rtik	Arapcha zip paket asartiy sho'rtik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Arapcha zip paket asartiy sho'rtik", "name": "Arapcha zip paket asartiy sho'rtik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.483912+05
Aroma chup teshili 8 sm	Aroma chup teshili 8 sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Aroma chup teshili 8 sm", "name": "Aroma chup teshili 8 sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.484962+05
Djelli Djoq jele skoch paket	Djelli Djoq jele skoch paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Djelli Djoq jele skoch paket", "name": "Djelli Djoq jele skoch paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.521508+05
Kid keks	Kid keks	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kid keks", "name": "Kid keks", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.550273+05
Klassik Karamel Paket	Klassik Karamel Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Klassik Karamel Paket", "name": "Klassik Karamel Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.551252+05
mega fresh 100 gr semechka tuzsiz	Mega Fresh 100 Gr Semechka Tuzsiz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega fresh 100 gr semechka tuzsiz", "name": "Mega Fresh 100 Gr Semechka Tuzsiz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.817789+05
metal paket 9x14	Metal Paket 9x14	Kg		tayyor mahsulot	{"uom": "Kg", "code": "metal paket 9x14", "name": "Metal Paket 9x14", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.822158+05
mikki zor kukuruz paket	Mikki Zor Kukuruz Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mikki zor kukuruz paket", "name": "Mikki Zor Kukuruz Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.824435+05
mpet 815/12 pol	MPET 815/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 815/12 pol", "name": "MPET 815/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.83812+05
msp 190/20	Msp 190/20	Kg		tayyor mahsulot	{"uom": "Kg", "code": "msp 190/20", "name": "Msp 190/20", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.839478+05
musaﬀo keﬁr 450gr 2,5%	Musaﬀo Keﬁr 450gr 2,5%	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musaﬀo keﬁr 450gr 2,5%", "name": "Musaﬀo Keﬁr 450gr 2,5%", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.842537+05
oq qand qizil-yashil-malochniy	Oq Qand Qizil-yashil-malochniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "oq qand qizil-yashil-malochniy", "name": "Oq Qand Qizil-yashil-malochniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.925609+05
orzu plombir 500 gr paket	Orzu Plombir 500 Gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "orzu plombir 500 gr paket", "name": "Orzu Plombir 500 Gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.926487+05
orzu saxarniy rojok 100 gr	Orzu Saxarniy Rojok 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "orzu saxarniy rojok 100 gr", "name": "Orzu Saxarniy Rojok 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.926938+05
pe 1180/20	Pe 1180/20	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pe 1180/20", "name": "Pe 1180/20", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.934797+05
pet + pe 50 sm toxtaniyoz ota	Pet + Pe 50 Sm Toxtaniyoz Ota	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pet + pe 50 sm toxtaniyoz ota", "name": "Pet + Pe 50 Sm Toxtaniyoz Ota", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.938728+05
pista 20gr sariq	Pista 20gr Sariq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pista 20gr sariq", "name": "Pista 20gr Sariq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.952622+05
prazrachniy paket palitelen	Prazrachniy Paket Palitelen	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachniy paket palitelen", "name": "Prazrachniy Paket Palitelen", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.959431+05
realniy plambir 100g krem bryule	Realniy Plambir 100g Krem Bryule	Kg		tayyor mahsulot	{"uom": "Kg", "code": "realniy plambir 100g krem bryule", "name": "Realniy Plambir 100g Krem Bryule", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.96795+05
sardor 200 gr 10 sht paket uz	Sardor 200 Gr 10 Sht Paket Uz	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sardor 200 gr 10 sht paket uz", "name": "Sardor 200 Gr 10 Sht Paket Uz", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.984575+05
sheyx ice cream 90 gr	Sheyx Ice Cream 90 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sheyx ice cream 90 gr", "name": "Sheyx Ice Cream 90 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.993315+05
simba chips qaymoq 25gr	Simba Chips Qaymoq 25gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba chips qaymoq 25gr", "name": "Simba Chips Qaymoq 25gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.997467+05
simba chips tandir kabob 25gr	Simba Chips Tandir Kabob 25gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba chips tandir kabob 25gr", "name": "Simba Chips Tandir Kabob 25gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.998339+05
sunukers-maks-daf	Sunukers-maks-daf	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sunukers-maks-daf", "name": "Sunukers-maks-daf", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.012505+05
teddy bear vanil kofe 120 gr	Teddy Bear Vanil Kofe 120 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "teddy bear vanil kofe 120 gr", "name": "Teddy Bear Vanil Kofe 120 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.018631+05
tvigga bruniy shokolad	Tvigga Bruniy Shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tvigga bruniy shokolad", "name": "Tvigga Bruniy Shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.024108+05
wantet kanfet kichik	Wantet Kanfet Kichik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "wantet kanfet kichik", "name": "Wantet Kanfet Kichik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.040044+05
mega aranjiviy qurt 39.5sm	Mega Aranjiviy Qurt 39.5sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega aranjiviy qurt 39.5sm", "name": "Mega Aranjiviy Qurt 39.5sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.816816+05
mega diyor 2 kg paket	Mega Diyor 2 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega diyor 2 kg paket", "name": "Mega Diyor 2 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.817224+05
mega fresh 100 gr semechka tuzli	Mega Fresh 100 Gr Semechka Tuzli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mega fresh 100 gr semechka tuzli", "name": "Mega Fresh 100 Gr Semechka Tuzli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.817645+05
ohana kid zip paket	Ohana Kid Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "ohana kid zip paket", "name": "Ohana Kid Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.851158+05
venture marojniy fruktoviy	Venture Marojniy Fruktoviy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "venture marojniy fruktoviy", "name": "Venture Marojniy Fruktoviy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.034961+05
Dide	Dide	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Dide", "name": "Dide", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-07-03 10:30:03.927715+05	2026-07-03 10:30:03.927715+05
mat 615/20 kar	MAT 615/20	Kg		homashyo	{"uom": "Kg", "code": "mat 615/20 kar", "name": "MAT 615/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.807608+05
Cho’p (boshida no’malum hajmda, 8 bo’lishi kk)	Cho’p (boshida no’malum hajmda, 8 bo’lishi kk)	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Cho’p (boshida no’malum hajmda, 8 bo’lishi kk)", "name": "Cho’p (boshida no’malum hajmda, 8 bo’lishi kk)", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.515871+05
donut cake chokolate 30 gr orange	Donut Cake Chokolate 30 Gr Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "donut cake chokolate 30 gr orange", "name": "Donut Cake Chokolate 30 Gr Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.717528+05
gold 999 11g	Gold 999 11g	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gold 999 11g", "name": "Gold 999 11g", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.736071+05
iz karzinki qizil kartoshka 2,5kg paket	Iz Karzinki Qizil Kartoshka 2,5kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "iz karzinki qizil kartoshka 2,5kg paket", "name": "Iz Karzinki Qizil Kartoshka 2,5kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.754155+05
kreko smetana 25 gr	Kreko Smetana 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kreko smetana 25 gr", "name": "Kreko Smetana 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.783032+05
makiz mannaya kruppa 300gr	Makiz Mannaya Kruppa 300gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "makiz mannaya kruppa 300gr", "name": "Makiz Mannaya Kruppa 300gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.798448+05
melek usluga reska	Melek Usluga Reska	Kg		tayyor mahsulot	{"uom": "Kg", "code": "melek usluga reska", "name": "Melek Usluga Reska", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.821822+05
metronex kotta	Metronex Kotta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "metronex kotta", "name": "Metronex Kotta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.822761+05
mikki ku-ku kukuruz paket	Mikki Ku-ku Kukuruz Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mikki ku-ku kukuruz paket", "name": "Mikki Ku-ku Kukuruz Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.824281+05
musaﬀo sirok kokosli	Musaﬀo Sirok Kokosli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musaﬀo sirok kokosli", "name": "Musaﬀo Sirok Kokosli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.843292+05
opp 420/20 pol	OPP 420/20	Kg		homashyo	{"uom": "Kg", "code": "opp 420/20 pol", "name": "OPP 420/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.864594+05
opp 445/20	OPP 445/20	Kg		homashyo	{"uom": "Kg", "code": "opp 445/20", "name": "OPP 445/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.866493+05
pure milky sirok vanil	Pure Milky Sirok Vanil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pure milky sirok vanil", "name": "Pure Milky Sirok Vanil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.962963+05
salfetka megic + antibacti + red rose 15 sht	Salfetka Megic + Antibacti + Red Rose 15 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka megic + antibacti + red rose 15 sht", "name": "Salfetka Megic + Antibacti + Red Rose 15 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.979668+05
stikcs suxariks 50 gr asarti	Stikcs Suxariks 50 Gr Asarti	Kg		tayyor mahsulot	{"uom": "Kg", "code": "stikcs suxariks 50 gr asarti", "name": "Stikcs Suxariks 50 Gr Asarti", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.008402+05
tilla qozon 70/30	Tilla Qozon 70/30	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tilla qozon 70/30", "name": "Tilla Qozon 70/30", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.019956+05
usluga milano premium qizil	Usluga Milano Premium Qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga milano premium qizil", "name": "Usluga Milano Premium Qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.029143+05
vkusno by seva blinchiki s tvorogom paket	Vkusno By Seva Blinchiki S Tvorogom Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vkusno by seva blinchiki s tvorogom paket", "name": "Vkusno By Seva Blinchiki S Tvorogom Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.038252+05
xumoyun sladok zavariki 150 gr paket	Xumoyun Sladok Zavariki 150 Gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xumoyun sladok zavariki 150 gr paket", "name": "Xumoyun Sladok Zavariki 150 Gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.045345+05
Qars qurs 15gr gribi smetana	Qars qurs 15gr gribi smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qars qurs 15gr gribi smetana", "name": "Qars qurs 15gr gribi smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.607274+05
TANLOV PRAZ/NIY PAKET	TANLOV PRAZ/NIY PAKET	Kg		tayyor mahsulot	{"uom": "Kg", "code": "TANLOV PRAZ/NIY PAKET", "name": "TANLOV PRAZ/NIY PAKET", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.627988+05
Xrystoﬀgrenki qurt chili 27sm	Xrystoﬀgrenki qurt chili 27sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrystoﬀgrenki qurt chili 27sm", "name": "Xrystoﬀgrenki qurt chili 27sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.654042+05
cheers nachos kalbasa chili 130 gr 41 sm	Cheers Nachos Kalbasa Chili 130 Gr 41 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers nachos kalbasa chili 130 gr 41 sm", "name": "Cheers Nachos Kalbasa Chili 130 Gr 41 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.689065+05
doda chuchvara 1000 kg	Doda Chuchvara 1000 Kg	Kg		tayyor mahsulot	{"uom": "Kg", "code": "doda chuchvara 1000 kg", "name": "Doda Chuchvara 1000 Kg", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.713233+05
don don suxarik 30gr asartiy	Don Don Suxarik 30gr Asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "don don suxarik 30gr asartiy", "name": "Don Don Suxarik 30gr Asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.714899+05
usluga chasti paket	Usluga Chasti Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga chasti paket", "name": "Usluga Chasti Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.028692+05
usluga milano qora	Usluga Milano Qora	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga milano qora", "name": "Usluga Milano Qora", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.029489+05
Mr stick suxarik 50gr asartiy	Mr stick suxarik 50gr asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mr stick suxarik 50gr asartiy", "name": "Mr stick suxarik 50gr asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.582897+05
OPP 525/20 pol	OPP 525/20	Kg		homashyo	{"uom": "Kg", "code": "OPP 525/20 pol", "name": "OPP 525/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.586624+05
Uzterra fri salyami shashlik tabaka 50gr	Uzterra fri salyami shashlik tabaka 50gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Uzterra fri salyami shashlik tabaka 50gr", "name": "Uzterra fri salyami shashlik tabaka 50gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.639405+05
XEPPI FOKS 22/26	XEPPI FOKS 22/26	Kg		tayyor mahsulot	{"uom": "Kg", "code": "XEPPI FOKS 22/26", "name": "XEPPI FOKS 22/26", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.64341+05
crisp-x shokolad 130 gr 41 sm	Crisp-x Shokolad 130 Gr 41 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "crisp-x shokolad 130 gr 41 sm", "name": "Crisp-x Shokolad 130 Gr 41 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.705363+05
dilmuratov sssr 100 gr	Dilmuratov Sssr 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dilmuratov sssr 100 gr", "name": "Dilmuratov Sssr 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.712122+05
elitex futbolka s dlinnoy rukavom	Elitex Futbolka S Dlinnoy Rukavom	Kg		tayyor mahsulot	{"uom": "Kg", "code": "elitex futbolka s dlinnoy rukavom", "name": "Elitex Futbolka S Dlinnoy Rukavom", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.723173+05
gektar iz karzinki qizil kartoshka 2,5kg paket	Gektar Iz Karzinki Qizil Kartoshka 2,5kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gektar iz karzinki qizil kartoshka 2,5kg paket", "name": "Gektar Iz Karzinki Qizil Kartoshka 2,5kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.734573+05
imkon plus shokolad sendvich 100 gr	Imkon Plus Shokolad Sendvich 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "imkon plus shokolad sendvich 100 gr", "name": "Imkon Plus Shokolad Sendvich 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.750188+05
iz korzinki brownie cake 25 gr orange	Iz Korzinki Brownie Cake 25 Gr Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "iz korzinki brownie cake 25 gr orange", "name": "Iz Korzinki Brownie Cake 25 Gr Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.754478+05
iz korzinki sendvich vanil 100 gr	Iz Korzinki Sendvich Vanil 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "iz korzinki sendvich vanil 100 gr", "name": "Iz Korzinki Sendvich Vanil 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.755191+05
lavanda 15ta XNS salfetka	lavanda 15ta XNS salfetka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "lavanda 15ta XNS salfetka", "name": "lavanda 15ta XNS salfetka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.789187+05
metal zip paket 29/20	Metal Zip Paket 29/20	Kg		tayyor mahsulot	{"uom": "Kg", "code": "metal zip paket 29/20", "name": "Metal Zip Paket 29/20", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.822313+05
mikki crispy kukuruz 150 gr rulon	Mikki Crispy Kukuruz 150 Gr Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "mikki crispy kukuruz 150 gr rulon", "name": "Mikki Crispy Kukuruz 150 Gr Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.823933+05
opp 885/35 pf kor	OPP 885/35	Kg		homashyo	{"uom": "Kg", "code": "opp 885/35 pf kor", "name": "OPP 885/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.905486+05
salafan paket 13/28	Salafan Paket 13/28	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salafan paket 13/28", "name": "Salafan Paket 13/28", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.973676+05
salafan paket 14x18	Salafan Paket 14x18	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salafan paket 14x18", "name": "Salafan Paket 14x18", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.973833+05
salfaetka xns kid soft fresh 72 sht	Salfaetka Xns Kid Soft Fresh 72 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfaetka xns kid soft fresh 72 sht", "name": "Salfaetka Xns Kid Soft Fresh 72 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.974589+05
salfetk dushanbe 15ta	Salfetk Dushanbe 15ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetk dushanbe 15ta", "name": "Salfetk Dushanbe 15ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.975041+05
tojik 30/25 paket	Tojik 30/25 Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "tojik 30/25 paket", "name": "Tojik 30/25 Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.020438+05
usluga xeppi foks 28/35 paket zamok	Usluga Xeppi Foks 28/35 Paket Zamok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "usluga xeppi foks 28/35 paket zamok", "name": "Usluga Xeppi Foks 28/35 Paket Zamok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.030498+05
xot lanch sochnaya kuritsa ostriy 90gr	Xot Lanch Sochnaya Kuritsa Ostriy 90gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xot lanch sochnaya kuritsa ostriy 90gr", "name": "Xot Lanch Sochnaya Kuritsa Ostriy 90gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.044768+05
Batonchik assorti (starnuts, choco mix, mussli)	Batonchik assorti (starnuts, choco mix, mussli)	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Batonchik assorti (starnuts, choco mix, mussli)", "name": "Batonchik assorti (starnuts, choco mix, mussli)", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.497491+05
Bek fyalet pista 40gr	Bek fyalet pista 40gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bek fyalet pista 40gr", "name": "Bek fyalet pista 40gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.498627+05
Maksi salyami qizil	Maksi salyami qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Maksi salyami qizil", "name": "Maksi salyami qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.572016+05
chempion toster sendvich paket	Chempion Toster Sendvich Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion toster sendvich paket", "name": "Chempion Toster Sendvich Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.696189+05
salafan paket 21x15sm	Salafan Paket 21x15sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salafan paket 21x15sm", "name": "Salafan Paket 21x15sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.974234+05
475/70 pe oq	PE OQ 475/70	Kg		homashyo	{"uom": "Kg", "code": "475/70 pe oq", "name": "PE OQ 475/70", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.446383+05
605/45 pe pr vakuum	PE PR 605/45	Kg		homashyo	{"uom": "Kg", "code": "605/45 pe pr vakuum", "name": "PE PR 605/45", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.454339+05
Jem 885/20	JEM 885/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 885/20", "name": "JEM 885/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.544793+05
jem 390/30 kar	JEM 390/30	Kg		homashyo	{"uom": "Kg", "code": "jem 390/30 kar", "name": "JEM 390/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.759403+05
opp  855/18	OPP 855/18	Kg		homashyo	{"uom": "Kg", "code": "opp  855/18", "name": "OPP 855/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.85268+05
Jem 615/20	JEM 615/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 615/20", "name": "JEM 615/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.54327+05
Jem 625/20	JEM 625/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 625/20", "name": "JEM 625/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.543647+05
560/60 pe pr toza	PE PR 560/60	Kg		homashyo	{"uom": "Kg", "code": "560/60 pe pr toza", "name": "PE PR 560/60", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.451613+05
opp 675/18	OPP 675/18	Kg		homashyo	{"uom": "Kg", "code": "opp 675/18", "name": "OPP 675/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.888512+05
opp 675/18 pol	OPP 675/18	Kg		homashyo	{"uom": "Kg", "code": "opp 675/18 pol", "name": "OPP 675/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.888728+05
opp 740/30 PF kar	OPP 740/30	Kg		homashyo	{"uom": "Kg", "code": "opp 740/30 PF kar", "name": "OPP 740/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.895826+05
opp 850/40 kar	OPP 850/40	Kg		homashyo	{"uom": "Kg", "code": "opp 850/40 kar", "name": "OPP 850/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.903906+05
oppm 340/25	OPPM 340/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 340/25", "name": "OPPM 340/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.912515+05
oppm 685/20 kar	OPPM 685/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 685/20 kar", "name": "OPPM 685/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.919108+05
jem 390/25 kar	JEM 390/25	Kg		homashyo	{"uom": "Kg", "code": "jem 390/25 kar", "name": "JEM 390/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.759116+05
OPP 535/25 pol	OPP 535/25	Kg		homashyo	{"uom": "Kg", "code": "OPP 535/25 pol", "name": "OPP 535/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.586816+05
OPP 545/18 pol	OPP 545/18	Kg		homashyo	{"uom": "Kg", "code": "OPP 545/18 pol", "name": "OPP 545/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.587296+05
665/85 pe pr oq	PE PR 665/85	Kg		homashyo	{"uom": "Kg", "code": "665/85 pe pr oq", "name": "PE PR 665/85", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.457963+05
Afsona marojni	Afsona marojni	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Afsona marojni", "name": "Afsona marojni", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.473913+05
Agro-Bravo Tvorojniy	Agro-Bravo Tvorojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Agro-Bravo Tvorojniy", "name": "Agro-Bravo Tvorojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.475009+05
1125/85 pe pr toza	PE PR 1125/85	Kg		homashyo	{"uom": "Kg", "code": "1125/85 pe pr toza", "name": "PE PR 1125/85", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.444077+05
Adal sendvich	Adal sendvich	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Adal sendvich", "name": "Adal sendvich", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.472384+05
Adras aboy 3kg paekt	Adras aboy 3kg paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Adras aboy 3kg paekt", "name": "Adras aboy 3kg paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.472665+05
Aiva enrgiy-shokolad-olma	Aiva enrgiy-shokolad-olma	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Aiva enrgiy-shokolad-olma", "name": "Aiva enrgiy-shokolad-olma", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.475588+05
Ali bobo Oshka plombir 15%	Ali bobo Oshka plombir 15%	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ali bobo Oshka plombir 15%", "name": "Ali bobo Oshka plombir 15%", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.479844+05
Almond qurt samarqand paket	Almond qurt samarqand paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Almond qurt samarqand paket", "name": "Almond qurt samarqand paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.481058+05
Arapcha zip paket asartiy fudbolka	Arapcha zip paket asartiy fudbolka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Arapcha zip paket asartiy fudbolka", "name": "Arapcha zip paket asartiy fudbolka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.483659+05
Aroma 1.5 kg parashok paket	Aroma 1.5 kg parashok paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Aroma 1.5 kg parashok paket", "name": "Aroma 1.5 kg parashok paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.484706+05
Asiya salfetka	Asiya salfetka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Asiya salfetka", "name": "Asiya salfetka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.487785+05
Askarbinka vitamin C banan	Askarbinka vitamin C banan	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Askarbinka vitamin C banan", "name": "Askarbinka vitamin C banan", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.48983+05
Askarbinka vitamin C qulolupne	Askarbinka vitamin C qulolupne	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Askarbinka vitamin C qulolupne", "name": "Askarbinka vitamin C qulolupne", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.490138+05
Afsona	Afsona	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Afsona", "name": "Afsona", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.473651+05
Chikko qizil paket	Chikko qizil paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Chikko qizil paket", "name": "Chikko qizil paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.513836+05
Frendo 9D shashlik	Frendo 9D shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frendo 9D shashlik", "name": "Frendo 9D shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.52929+05
Frutella jele sentir paket	Frutella jele sentir paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frutella jele sentir paket", "name": "Frutella jele sentir paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.530288+05
Frutella jele sko'ch paket	Frutella jele sko'ch paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Frutella jele sko'ch paket", "name": "Frutella jele sko'ch paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.530453+05
Giotto	Giotto	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Giotto", "name": "Giotto", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.531122+05
Ice grand rajok 110gr	Ice grand rajok 110gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Ice grand rajok 110gr", "name": "Ice grand rajok 110gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.534527+05
Kafolat tuz 1kg paket	Kafolat tuz 1kg paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kafolat tuz 1kg paket", "name": "Kafolat tuz 1kg paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.54683+05
Karamel Assorti (Xalo, Moxito, Dolce)	Karamel Assorti (Xalo, Moxito, Dolce)	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Karamel Assorti (Xalo, Moxito, Dolce)", "name": "Karamel Assorti (Xalo, Moxito, Dolce)", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.547751+05
Mega crecker sasiska	Mega crecker sasiska	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Mega crecker sasiska", "name": "Mega crecker sasiska", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.577186+05
NUKUS GURUCh 2 KG	NUKUS GURUCh 2 KG	Kg		tayyor mahsulot	{"uom": "Kg", "code": "NUKUS GURUCh 2 KG", "name": "NUKUS GURUCh 2 KG", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.584472+05
Purmilkiy tvorg zernistiy 9% 200 gr orange	Purmilkiy tvorg zernistiy 9% 200 gr orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Purmilkiy tvorg zernistiy 9% 200 gr orange", "name": "Purmilkiy tvorg zernistiy 9% 200 gr orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.606231+05
Qanotchi	Qanotchi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qanotchi", "name": "Qanotchi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.606548+05
Spring 500gr paket	Spring 500gr paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Spring 500gr paket", "name": "Spring 500gr paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.625835+05
Xrustoﬀgrenki tomat ostriy 37,5sm	Xrustoﬀgrenki tomat ostriy 37,5sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustoﬀgrenki tomat ostriy 37,5sm", "name": "Xrustoﬀgrenki tomat ostriy 37,5sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.651245+05
Zet marmalat	Zet marmalat	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zet marmalat", "name": "Zet marmalat", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.657903+05
Zizi xalol asardi 5,6 gr	Zizi xalol asardi 5,6 gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zizi xalol asardi 5,6 gr", "name": "Zizi xalol asardi 5,6 gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.662019+05
bloom PH 5.5 120 sht pushti qora	bloom PH 5.5 120 sht pushti qora	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bloom PH 5.5 120 sht pushti qora", "name": "bloom PH 5.5 120 sht pushti qora", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.679431+05
bloom forfort girl 120 sht	Bloom Forfort Girl 120 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "bloom forfort girl 120 sht", "name": "Bloom Forfort Girl 120 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.680012+05
Barakali chuchvala 500gr paket	Barakali chuchvala 500gr paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Barakali chuchvala 500gr paket", "name": "Barakali chuchvala 500gr paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.496619+05
Beneo jitkiy aboy paket umar	Beneo jitkiy aboy paket umar	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Beneo jitkiy aboy paket umar", "name": "Beneo jitkiy aboy paket umar", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.501798+05
Best vakumniy	Best vakumniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Best vakumniy", "name": "Best vakumniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.502068+05
Bon gusto N149	Bon gusto N149	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bon gusto N149", "name": "Bon gusto N149", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.506606+05
Bonitto 740mm paket	Bonitto 740mm paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Bonitto 740mm paket", "name": "Bonitto 740mm paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.507123+05
Chikko kòk rulon	Chikko kòk rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Chikko kòk rulon", "name": "Chikko kòk rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.513697+05
Dizayin dekor qizil	Dizayin dekor qizil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Dizayin dekor qizil", "name": "Dizayin dekor qizil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.521096+05
Grand marojniy 120g	Grand marojniy 120g	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Grand marojniy 120g", "name": "Grand marojniy 120g", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.533035+05
Xordiq semechka	Xordiq semechka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xordiq semechka", "name": "Xordiq semechka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.645004+05
Qars qur maksimum 30gr xalol palichka	Qars qur maksimum 30gr xalol palichka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qars qur maksimum 30gr xalol palichka", "name": "Qars qur maksimum 30gr xalol palichka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.606749+05
RZ 495/25	RZ 495/25	Kg		tayyor mahsulot	{"uom": "Kg", "code": "RZ 495/25", "name": "RZ 495/25", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.609991+05
Shoxasar mini vaﬂi 250gr asartiy	Shoxasar mini vaﬂi 250gr asartiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Shoxasar mini vaﬂi 250gr asartiy", "name": "Shoxasar mini vaﬂi 250gr asartiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.617541+05
Sinel smorodina	Sinel smorodina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sinel smorodina", "name": "Sinel smorodina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.621426+05
Sladkaya Kotta Paket KZ	Sladkaya Kotta Paket KZ	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sladkaya Kotta Paket KZ", "name": "Sladkaya Kotta Paket KZ", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.623159+05
Slivichniy	Slivichniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Slivichniy", "name": "Slivichniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.623903+05
Supreme classic 60 gr	Supreme classic 60 gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Supreme classic 60 gr", "name": "Supreme classic 60 gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.627634+05
Tutti frutti / tabletka	Tutti frutti / tabletka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Tutti frutti / tabletka", "name": "Tutti frutti / tabletka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.637752+05
XNS vlajniy salfetka kok	XNS vlajniy salfetka kok	Kg		tayyor mahsulot	{"uom": "Kg", "code": "XNS vlajniy salfetka kok", "name": "XNS vlajniy salfetka kok", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.643556+05
Xrust tayim 5D 40gr paprika	Xrust tayim 5D 40gr paprika	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrust tayim 5D 40gr paprika", "name": "Xrust tayim 5D 40gr paprika", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.64895+05
Xrustof salyami 37,5 sm	Xrustof salyami 37,5 sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustof salyami 37,5 sm", "name": "Xrustof salyami 37,5 sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.650142+05
Zo'r chips 5D shashlik	Zo'r chips 5D shashlik	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Zo'r chips 5D shashlik", "name": "Zo'r chips 5D shashlik", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.662485+05
baranki prazrachniy 40 mk	Baranki Prazrachniy 40 Mk	Kg		tayyor mahsulot	{"uom": "Kg", "code": "baranki prazrachniy 40 mk", "name": "Baranki Prazrachniy 40 Mk", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.675204+05
chaps smetana	Chaps Smetana	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chaps smetana", "name": "Chaps Smetana", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.686993+05
chempion qarsildoq tovuq	Chempion Qarsildoq Tovuq	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chempion qarsildoq tovuq", "name": "Chempion Qarsildoq Tovuq", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.693936+05
chiko smetana + shashlik 15 gr	Chiko Smetana + Shashlik 15 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "chiko smetana + shashlik 15 gr", "name": "Chiko Smetana + Shashlik 15 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.697084+05
doda fruity twist 80 gr	Doda Fruity Twist 80 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "doda fruity twist 80 gr", "name": "Doda Fruity Twist 80 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.713494+05
dona mazali pista ﬁyalet	Dona Mazali Pista Fiyalet	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dona mazali pista ﬁyalet", "name": "Dona Mazali Pista Fiyalet", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.715558+05
keyt tvist	Keyt Tvist	Kg		tayyor mahsulot	{"uom": "Kg", "code": "keyt tvist", "name": "Keyt Tvist", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.77639+05
kreko smetana 50 gr	Kreko Smetana 50 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kreko smetana 50 gr", "name": "Kreko Smetana 50 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.783358+05
lucia exite for men 20 sht	Lucia Exite For Men 20 Sht	Kg		tayyor mahsulot	{"uom": "Kg", "code": "lucia exite for men 20 sht", "name": "Lucia Exite For Men 20 Sht", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.792153+05
lux 60 gr	Lux 60 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "lux 60 gr", "name": "Lux 60 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.793157+05
Kislinki	Kislinki	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kislinki", "name": "Kislinki", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.5507+05
Krafers american	Krafers american	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Krafers american", "name": "Krafers american", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.555376+05
Kristal tuz 500g paket	Kristal tuz 500g paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kristal tuz 500g paket", "name": "Kristal tuz 500g paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.556223+05
Milano sutli	Milano sutli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Milano sutli", "name": "Milano sutli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.579771+05
Nussa rara yengi 40gr rulon	Nussa rara yengi 40gr rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Nussa rara yengi 40gr rulon", "name": "Nussa rara yengi 40gr rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.586033+05
Pelmeni paket yashil 300gr	Pelmeni paket yashil 300gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Pelmeni paket yashil 300gr", "name": "Pelmeni paket yashil 300gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.599226+05
Million 3D	Million 3D	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Million 3D", "name": "Million 3D", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.579973+05
Musaﬀo kiﬁr 900gr 2,5%	Musaﬀo kiﬁr 900gr 2,5%	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Musaﬀo kiﬁr 900gr 2,5%", "name": "Musaﬀo kiﬁr 900gr 2,5%", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.583868+05
Nussa o'g'il bolali paket	Nussa o'g'il bolali paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Nussa o'g'il bolali paket", "name": "Nussa o'g'il bolali paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.585919+05
Purmilkiy tvorg zernistiy 5 % 200 gr yashil	Purmilkiy tvorg zernistiy 5 % 200 gr yashil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Purmilkiy tvorg zernistiy 5 % 200 gr yashil", "name": "Purmilkiy tvorg zernistiy 5 % 200 gr yashil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.606014+05
Qars qur halol 30gr	Qars qur halol 30gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qars qur halol 30gr", "name": "Qars qur halol 30gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.60665+05
Qars qur xalol 50gr	Qars qur xalol 50gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Qars qur xalol 50gr", "name": "Qars qur xalol 50gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.606938+05
Sir chechil	Sir chechil	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Sir chechil", "name": "Sir chechil", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.621808+05
Suxariki yello salami	Suxariki yello salami	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Suxariki yello salami", "name": "Suxariki yello salami", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.627853+05
Xrus tayim 5D 20gr barbikyu	Xrus tayim 5D 20gr barbikyu	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrus tayim 5D 20gr barbikyu", "name": "Xrus tayim 5D 20gr barbikyu", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.646996+05
Xrustik plus 50gr paket	Xrustik plus 50gr paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustik plus 50gr paket", "name": "Xrustik plus 50gr paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.649489+05
Xrustoﬀgrenki myasa na uglax 37,5sm	Xrustoﬀgrenki myasa na uglax 37,5sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Xrustoﬀgrenki myasa na uglax 37,5sm", "name": "Xrustoﬀgrenki myasa na uglax 37,5sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.651019+05
axmedob bombey alumin	Axmedob Bombey Alumin	Kg		tayyor mahsulot	{"uom": "Kg", "code": "axmedob bombey alumin", "name": "Axmedob Bombey Alumin", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.672255+05
cheers snack burger 130 gr 41 sm	Cheers Snack Burger 130 Gr 41 Sm	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers snack burger 130 gr 41 sm", "name": "Cheers Snack Burger 130 Gr 41 Sm", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.691552+05
detisko salfetka XNS 120ta	detisko salfetka XNS 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "detisko salfetka XNS 120ta", "name": "detisko salfetka XNS 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.709584+05
don don 3D 5D 7D 9D	don don 3D 5D 7D 9D	Kg		tayyor mahsulot	{"uom": "Kg", "code": "don don 3D 5D 7D 9D", "name": "don don 3D 5D 7D 9D", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.714613+05
dusel 3 m paket	Dusel 3 M Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "dusel 3 m paket", "name": "Dusel 3 M Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.718078+05
gold 999.9 zolotoy slitok 80 gr fa plus	Gold 999.9 Zolotoy Slitok 80 Gr Fa Plus	Kg		tayyor mahsulot	{"uom": "Kg", "code": "gold 999.9 zolotoy slitok 80 gr fa plus", "name": "Gold 999.9 Zolotoy Slitok 80 Gr Fa Plus", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.736221+05
grand yashil 120 gr matroskin	Grand Yashil 120 Gr Matroskin	Kg		tayyor mahsulot	{"uom": "Kg", "code": "grand yashil 120 gr matroskin", "name": "Grand Yashil 120 Gr Matroskin", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.738641+05
iz korzinki donut cake 30 gr	Iz Korzinki Donut Cake 30 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "iz korzinki donut cake 30 gr", "name": "Iz Korzinki Donut Cake 30 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.754763+05
iz korzinki waﬄe cake 45 gr orange	Iz Korzinki Waﬄe Cake 45 Gr Orange	Kg		tayyor mahsulot	{"uom": "Kg", "code": "iz korzinki waﬄe cake 45 gr orange", "name": "Iz Korzinki Waﬄe Cake 45 Gr Orange", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.755614+05
jajji 30 gr doppi	Jajji 30 Gr Doppi	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jajji 30 gr doppi", "name": "Jajji 30 Gr Doppi", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.75616+05
jajji xlebushki grenki 70 gr	Jajji Xlebushki Grenki 70 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "jajji xlebushki grenki 70 gr", "name": "Jajji Xlebushki Grenki 70 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.756595+05
kristal tuz rossiya 2 kg paket	Kristal Tuz Rossiya 2 Kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kristal tuz rossiya 2 kg paket", "name": "Kristal Tuz Rossiya 2 Kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.785002+05
magnus marojniy	Magnus Marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "magnus marojniy", "name": "Magnus Marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.794645+05
metronex kichkina	Metronex Kichkina	Kg		tayyor mahsulot	{"uom": "Kg", "code": "metronex kichkina", "name": "Metronex Kichkina", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.82261+05
Kanada chikken wianers	Kanada chikken wianers	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Kanada chikken wianers", "name": "Kanada chikken wianers", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.547356+05
King	King	Kg		tayyor mahsulot	{"uom": "Kg", "code": "King", "name": "King", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.550559+05
Makiz #285	Makiz #285	Kg		tayyor mahsulot	{"uom": "Kg", "code": "Makiz #285", "name": "Makiz #285", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.566585+05
stiks suxarik salyami 55gr	Stiks Suxarik Salyami 55gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "stiks suxarik salyami 55gr", "name": "Stiks Suxarik Salyami 55gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.008914+05
fayz zvezda marojniy	Fayz Zvezda Marojniy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "fayz zvezda marojniy", "name": "Fayz Zvezda Marojniy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.727179+05
opp 1400/25 pol	OPP 1400/25	Kg		homashyo	{"uom": "Kg", "code": "opp 1400/25 pol", "name": "OPP 1400/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.85761+05
opp 440/30 kar	OPP 440/30	Kg		homashyo	{"uom": "Kg", "code": "opp 440/30 kar", "name": "OPP 440/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.866336+05
opp 455/18 kar	OPP 455/18	Kg		homashyo	{"uom": "Kg", "code": "opp 455/18 kar", "name": "OPP 455/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.866788+05
opp 468/20 kar	OPP 468/20	Kg		homashyo	{"uom": "Kg", "code": "opp 468/20 kar", "name": "OPP 468/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.868718+05
opp+cpp 220/40	Opp+cpp 220/40	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp+cpp 220/40", "name": "Opp+cpp 220/40", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.909419+05
praniki 365 kun kofe plombir tamli paket	Praniki 365 Kun Kofe Plombir Tamli Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "praniki 365 kun kofe plombir tamli paket", "name": "Praniki 365 Kun Kofe Plombir Tamli Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.95729+05
cheers dropz apelsin	Cheers Dropz Apelsin	Kg		tayyor mahsulot	{"uom": "Kg", "code": "cheers dropz apelsin", "name": "Cheers Dropz Apelsin", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.688024+05
doda pelmeni qizil 400 gr	Doda Pelmeni Qizil 400 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "doda pelmeni qizil 400 gr", "name": "Doda Pelmeni Qizil 400 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.713638+05
isko pop kek 18gr xalodniy payka	Isko Pop Kek 18gr Xalodniy Payka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "isko pop kek 18gr xalodniy payka", "name": "Isko Pop Kek 18gr Xalodniy Payka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.753665+05
jem 650/25 kar	JEM 650/25	Kg		homashyo	{"uom": "Kg", "code": "jem 650/25 kar", "name": "JEM 650/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.7658+05
jem 795/25 kar	JEM 795/25	Kg		homashyo	{"uom": "Kg", "code": "jem 795/25 kar", "name": "JEM 795/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.769609+05
kreko salyami 25 gr	Kreko Salyami 25 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "kreko salyami 25 gr", "name": "Kreko Salyami 25 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.781972+05
magic roller ﬁstashka 85 gr	Magic Roller Fistashka 85 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "magic roller ﬁstashka 85 gr", "name": "Magic Roller Fistashka 85 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.793888+05
mat 815/20 kar	MAT 815/20	Kg		homashyo	{"uom": "Kg", "code": "mat 815/20 kar", "name": "MAT 815/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.812053+05
metro qurt	Metro Qurt	Kg		tayyor mahsulot	{"uom": "Kg", "code": "metro qurt", "name": "Metro Qurt", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.822456+05
mpet 850/12 pol	MPET 850/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 850/12 pol", "name": "MPET 850/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.839027+05
opp  640/20	OPP 640/20	Kg		homashyo	{"uom": "Kg", "code": "opp  640/20", "name": "OPP 640/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.85238+05
prayniki 365 kun qurutilgan sutli paket	Prayniki 365 Kun Qurutilgan Sutli Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prayniki 365 kun qurutilgan sutli paket", "name": "Prayniki 365 Kun Qurutilgan Sutli Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.957717+05
prazrachniy 26sm zip paket	Prazrachniy 26sm Zip Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "prazrachniy 26sm zip paket", "name": "Prazrachniy 26sm Zip Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.958987+05
shox bum sitrus arbuz roza	Shox Bum Sitrus Arbuz Roza	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shox bum sitrus arbuz roza", "name": "Shox Bum Sitrus Arbuz Roza", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.99385+05
sandee sendvich kids paket	Sandee Sendvich Kids Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sandee sendvich kids paket", "name": "Sandee Sendvich Kids Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.982867+05
simba chips tandir kabob 14gr	Simba Chips Tandir Kabob 14gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "simba chips tandir kabob 14gr", "name": "Simba Chips Tandir Kabob 14gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.998196+05
suvni zararlantirish vositasi paket	Suvni Zararlantirish Vositasi Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "suvni zararlantirish vositasi paket", "name": "Suvni Zararlantirish Vositasi Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.01407+05
marvarid the best qurt 30+5 gr paket	Marvarid The Best Qurt 30+5 Gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "marvarid the best qurt 30+5 gr paket", "name": "Marvarid The Best Qurt 30+5 Gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.8017+05
opp 510/18 kar	OPP 510/18	Kg		homashyo	{"uom": "Kg", "code": "opp 510/18 kar", "name": "OPP 510/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.873187+05
opp 545/18 kar 3	OPP 545/18	Kg		homashyo	{"uom": "Kg", "code": "opp 545/18 kar 3", "name": "OPP 545/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.877997+05
opp 580/20 pff kar	OPP PFF 580/20	Kg		homashyo	{"uom": "Kg", "code": "opp 580/20 pff kar", "name": "OPP PFF 580/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.88102+05
milano skoch paket	Milano Skoch Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "milano skoch paket", "name": "Milano Skoch Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.824733+05
musa bissgo wild berries 100 gr	Musa Bissgo Wild Berries 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musa bissgo wild berries 100 gr", "name": "Musa Bissgo Wild Berries 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.84103+05
musaﬀo sirok qulipnayli	Musaﬀo Sirok Qulipnayli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musaﬀo sirok qulipnayli", "name": "Musaﬀo Sirok Qulipnayli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.843737+05
opp 17/17 paket payushiy	Opp 17/17 Paket Payushiy	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp 17/17 paket payushiy", "name": "Opp 17/17 Paket Payushiy", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.860043+05
opp 660/40 PFF kar	OPP PFF 660/40	Kg		homashyo	{"uom": "Kg", "code": "opp 660/40 PFF kar", "name": "OPP PFF 660/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.887544+05
opp 720/30 kar pf	OPP 720/30	Kg		homashyo	{"uom": "Kg", "code": "opp 720/30 kar pf", "name": "OPP 720/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.894047+05
trubichka paket dlya kaktel	Trubichka Paket Dlya Kaktel	Kg		tayyor mahsulot	{"uom": "Kg", "code": "trubichka paket dlya kaktel", "name": "Trubichka Paket Dlya Kaktel", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.023738+05
vkusno by seva chuchvara 500 gr paket	Vkusno By Seva Chuchvara 500 Gr Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vkusno by seva chuchvara 500 gr paket", "name": "Vkusno By Seva Chuchvara 500 Gr Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.038385+05
opp 450/35 Pol pf	OPP 450/35	Kg		homashyo	{"uom": "Kg", "code": "opp 450/35 Pol pf", "name": "OPP 450/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.86664+05
opp 475/35	OPP 475/35	Kg		homashyo	{"uom": "Kg", "code": "opp 475/35", "name": "OPP 475/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.870119+05
oppm 1020/20 kar	OPPM 1020/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 1020/20 kar", "name": "OPPM 1020/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.910763+05
pistello tuzli qora 01	Pistello Tuzli Qora 01	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pistello tuzli qora 01", "name": "Pistello Tuzli Qora 01", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.953397+05
qars qurs maksimum 15gr halol palichka	Qars Qurs Maksimum 15gr Halol Palichka	Kg		tayyor mahsulot	{"uom": "Kg", "code": "qars qurs maksimum 15gr halol palichka", "name": "Qars Qurs Maksimum 15gr Halol Palichka", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.96555+05
salfetka bebikar qizil atirgul 120ta	Salfetka Bebikar Qizil Atirgul 120ta	Kg		tayyor mahsulot	{"uom": "Kg", "code": "salfetka bebikar qizil atirgul 120ta", "name": "Salfetka Bebikar Qizil Atirgul 120ta", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.976975+05
sayqal 70gr enerji	Sayqal 70gr Enerji	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sayqal 70gr enerji", "name": "Sayqal 70gr Enerji", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.987869+05
sendvich chempion bolajob samarqand paket	Sendvich Chempion Bolajob Samarqand Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "sendvich chempion bolajob samarqand paket", "name": "Sendvich Chempion Bolajob Samarqand Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.990463+05
shirin diyor 1kg paket	Shirin Diyor 1kg Paket	Kg		tayyor mahsulot	{"uom": "Kg", "code": "shirin diyor 1kg paket", "name": "Shirin Diyor 1kg Paket", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.993488+05
vanil sandvich 100 gr	Vanil Sandvich 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "vanil sandvich 100 gr", "name": "Vanil Sandvich 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.032731+05
xeppi foks 25/30	Xeppi Foks 25/30	Kg		tayyor mahsulot	{"uom": "Kg", "code": "xeppi foks 25/30", "name": "Xeppi Foks 25/30", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.042743+05
yakar chop 10 sm teshikli	Yakar Chop 10 Sm Teshikli	Kg		tayyor mahsulot	{"uom": "Kg", "code": "yakar chop 10 sm teshikli", "name": "Yakar Chop 10 Sm Teshikli", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.045772+05
zuxro coﬀee plombir pechenye 100 gr	Zuxro Coﬀee Plombir Pechenye 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "zuxro coﬀee plombir pechenye 100 gr", "name": "Zuxro Coﬀee Plombir Pechenye 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.055508+05
msp 200/20	Msp 200/20	Kg		tayyor mahsulot	{"uom": "Kg", "code": "msp 200/20", "name": "Msp 200/20", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.839617+05
musa bissgo caramel 100 gr	Musa Bissgo Caramel 100 Gr	Kg		tayyor mahsulot	{"uom": "Kg", "code": "musa bissgo caramel 100 gr", "name": "Musa Bissgo Caramel 100 Gr", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.84088+05
opp 420/30	OPP 420/30	Kg		homashyo	{"uom": "Kg", "code": "opp 420/30", "name": "OPP 420/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.864737+05
opp 435/20 pol	OPP 435/20	Kg		homashyo	{"uom": "Kg", "code": "opp 435/20 pol", "name": "OPP 435/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.866053+05
opp 775/30 PFF pol	OPP PFF 775/30	Kg		homashyo	{"uom": "Kg", "code": "opp 775/30 PFF pol", "name": "OPP PFF 775/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.898471+05
opp 825/30 pf pol	OPP 825/30	Kg		homashyo	{"uom": "Kg", "code": "opp 825/30 pf pol", "name": "OPP 825/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.901292+05
opp 975/30 PF kar	OPP 975/30	Kg		homashyo	{"uom": "Kg", "code": "opp 975/30 PF kar", "name": "OPP 975/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.907197+05
opp metal 19 sm rulon	Opp Metal 19 Sm Rulon	Kg		tayyor mahsulot	{"uom": "Kg", "code": "opp metal 19 sm rulon", "name": "Opp Metal 19 Sm Rulon", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.908097+05
peres chorniy molita	Peres Chorniy Molita	Kg		tayyor mahsulot	{"uom": "Kg", "code": "peres chorniy molita", "name": "Peres Chorniy Molita", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.937084+05
pet 715/12	PET 715/12	Kg		homashyo	{"uom": "Kg", "code": "pet 715/12", "name": "PET 715/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.946551+05
pistello tuzli oq 03	Pistello Tuzli Oq 03	Kg		tayyor mahsulot	{"uom": "Kg", "code": "pistello tuzli oq 03", "name": "Pistello Tuzli Oq 03", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.953224+05
opp 460/25 kar	OPP 460/25	Kg		homashyo	{"uom": "Kg", "code": "opp 460/25 kar", "name": "OPP 460/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.867966+05
opp 470/18pf pol	OPP 470/18pf	Kg		homashyo	{"uom": "Kg", "code": "opp 470/18pf pol", "name": "OPP 470/18pf", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.868861+05
opp 530/25 kar	OPP 530/25	Kg		homashyo	{"uom": "Kg", "code": "opp 530/25 kar", "name": "OPP 530/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.876446+05
opp 620/25	OPP 620/25	Kg		homashyo	{"uom": "Kg", "code": "opp 620/25", "name": "OPP 620/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.885275+05
opp 740/20 pf kar 3	OPP 740/20	Kg		homashyo	{"uom": "Kg", "code": "opp 740/20 pf kar 3", "name": "OPP 740/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.895673+05
realniy plambir 100g shokolad	Realniy Plambir 100g Shokolad	Kg		tayyor mahsulot	{"uom": "Kg", "code": "realniy plambir 100g shokolad", "name": "Realniy Plambir 100g Shokolad", "warehouse": "", "item_group": "tayyor mahsulot"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.968094+05
cpp 550/20	CPP 550/20	Kg		homashyo	{"uom": "Kg", "code": "cpp 550/20", "name": "CPP 550/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.70299+05
jem 530/25 kar 3	JEM 530/25	Kg		homashyo	{"uom": "Kg", "code": "jem 530/25 kar 3", "name": "JEM 530/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.76332+05
jem 610/25 kar	JEM 610/25	Kg		homashyo	{"uom": "Kg", "code": "jem 610/25 kar", "name": "JEM 610/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.764578+05
jem 610/35 kar	JEM 610/35	Kg		homashyo	{"uom": "Kg", "code": "jem 610/35 kar", "name": "JEM 610/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.764731+05
jem 650/25	JEM 650/25	Kg		homashyo	{"uom": "Kg", "code": "jem 650/25", "name": "JEM 650/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.765653+05
mat 1420/20	MAT 1420/20	Kg		homashyo	{"uom": "Kg", "code": "mat 1420/20", "name": "MAT 1420/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.802814+05
mat 425/20 kar 3	MAT 425/20	Kg		homashyo	{"uom": "Kg", "code": "mat 425/20 kar 3", "name": "MAT 425/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.803627+05
mat 675/20 pol	MAT 675/20	Kg		homashyo	{"uom": "Kg", "code": "mat 675/20 pol", "name": "MAT 675/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.80957+05
mat 750/18 kar	MAT 750/18	Kg		homashyo	{"uom": "Kg", "code": "mat 750/18 kar", "name": "MAT 750/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.811127+05
mpet 715/12 pol	MPET 715/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 715/12 pol", "name": "MPET 715/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.837673+05
opp 1200/35	OPP 1200/35	Kg		homashyo	{"uom": "Kg", "code": "opp 1200/35", "name": "OPP 1200/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.854942+05
opp 1350/20 pff pol	OPP PFF 1350/20	Kg		homashyo	{"uom": "Kg", "code": "opp 1350/20 pff pol", "name": "OPP PFF 1350/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.856772+05
opp 1520/20 pff	OPP PFF 1520/20	Kg		homashyo	{"uom": "Kg", "code": "opp 1520/20 pff", "name": "OPP PFF 1520/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.858853+05
opp 390/18  kar	OPP 390/18	Kg		homashyo	{"uom": "Kg", "code": "opp 390/18  kar", "name": "OPP 390/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.862986+05
opp 405/18 pol	OPP 405/18	Kg		homashyo	{"uom": "Kg", "code": "opp 405/18 pol", "name": "OPP 405/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.863486+05
opp 465/25	OPP 465/25	Kg		homashyo	{"uom": "Kg", "code": "opp 465/25", "name": "OPP 465/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.868413+05
opp 520/18 kar	OPP 520/18	Kg		homashyo	{"uom": "Kg", "code": "opp 520/18 kar", "name": "OPP 520/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.875172+05
opp 520/30 kar	OPP 520/30	Kg		homashyo	{"uom": "Kg", "code": "opp 520/30 kar", "name": "OPP 520/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.875434+05
opp 520/35 kar	OPP 520/35	Kg		homashyo	{"uom": "Kg", "code": "opp 520/35 kar", "name": "OPP 520/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.87559+05
opp 550/30 kar	OPP 550/30	Kg		homashyo	{"uom": "Kg", "code": "opp 550/30 kar", "name": "OPP 550/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.8788+05
opp 600/20 kar	OPP 600/20	Kg		homashyo	{"uom": "Kg", "code": "opp 600/20 kar", "name": "OPP 600/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.883225+05
opp 610/30 kar	OPP 610/30	Kg		homashyo	{"uom": "Kg", "code": "opp 610/30 kar", "name": "OPP 610/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.8842+05
opp 615/20 kar	OPP 615/20	Kg		homashyo	{"uom": "Kg", "code": "opp 615/20 kar", "name": "OPP 615/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.884674+05
opp 630/20 pff spp	OPP 630/20	Kg		homashyo	{"uom": "Kg", "code": "opp 630/20 pff spp", "name": "OPP 630/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.885563+05
opp 645/20 kar	OPP 645/20	Kg		homashyo	{"uom": "Kg", "code": "opp 645/20 kar", "name": "OPP 645/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.886543+05
opp 645/20 pol	OPP 645/20	Kg		homashyo	{"uom": "Kg", "code": "opp 645/20 pol", "name": "OPP 645/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.886692+05
opp 660/20 kar	OPP 660/20	Kg		homashyo	{"uom": "Kg", "code": "opp 660/20 kar", "name": "OPP 660/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.887258+05
opp 680/35 kar 3	OPP 680/35	Kg		homashyo	{"uom": "Kg", "code": "opp 680/35 kar 3", "name": "OPP 680/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.889809+05
opp 765/25 pol	OPP 765/25	Kg		homashyo	{"uom": "Kg", "code": "opp 765/25 pol", "name": "OPP 765/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.897217+05
opp 885/18 pol	OPP 885/18	Kg		homashyo	{"uom": "Kg", "code": "opp 885/18 pol", "name": "OPP 885/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.905327+05
opp 895/25	OPP 895/25	Kg		homashyo	{"uom": "Kg", "code": "opp 895/25", "name": "OPP 895/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.905636+05
oppm 1845/25	OPPM 1845/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 1845/25", "name": "OPPM 1845/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.911369+05
oppm 465/20 pol	OPPM 465/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 465/20 pol", "name": "OPPM 465/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.914158+05
oppm 480/25 pol	OPPM 480/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 480/25 pol", "name": "OPPM 480/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.914485+05
oppm 795/35 pol	OPPM 795/35	Kg		homashyo	{"uom": "Kg", "code": "oppm 795/35 pol", "name": "OPPM 795/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.921503+05
pet 515/12 pol	PET 515/12	Kg		homashyo	{"uom": "Kg", "code": "pet 515/12 pol", "name": "PET 515/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.942188+05
pet 595/12	PET 595/12	Kg		homashyo	{"uom": "Kg", "code": "pet 595/12", "name": "PET 595/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.943852+05
MCP / 20 mikron / 720	MCP 20/720	Kg		homashyo	{"uom": "Kg", "code": "MCP / 20 mikron / 720", "name": "MCP 20/720", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.56494+05
OPP 695/18 pol	OPP 695/18	Kg		homashyo	{"uom": "Kg", "code": "OPP 695/18 pol", "name": "OPP 695/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.590127+05
515/45 pe pr toza	PE PR 515/45	Kg		homashyo	{"uom": "Kg", "code": "515/45 pe pr toza", "name": "PE PR 515/45", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.449165+05
525/30 pe pr toza	PE PR 525/30	Kg		homashyo	{"uom": "Kg", "code": "525/30 pe pr toza", "name": "PE PR 525/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.449698+05
560/30 pe pr toza	PE PR 560/30	Kg		homashyo	{"uom": "Kg", "code": "560/30 pe pr toza", "name": "PE PR 560/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.451113+05
575/80 pe pr oq	PE PR 575/80	Kg		homashyo	{"uom": "Kg", "code": "575/80 pe pr oq", "name": "PE PR 575/80", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.452395+05
580/80 pe pr oq	PE PR 580/80	Kg		homashyo	{"uom": "Kg", "code": "580/80 pe pr oq", "name": "PE PR 580/80", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.453037+05
590/70 pe pr oq	PE PR 590/70	Kg		homashyo	{"uom": "Kg", "code": "590/70 pe pr oq", "name": "PE PR 590/70", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.453307+05
590/70 pe pr toza	PE PR 590/70	Kg		homashyo	{"uom": "Kg", "code": "590/70 pe pr toza", "name": "PE PR 590/70", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.453572+05
635/30 pe pr toza	PE PR 635/30	Kg		homashyo	{"uom": "Kg", "code": "635/30 pe pr toza", "name": "PE PR 635/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.456183+05
640/45 pe pr toza	PE PR 640/45	Kg		homashyo	{"uom": "Kg", "code": "640/45 pe pr toza", "name": "PE PR 640/45", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.456565+05
795/60 pe pr toza	PE PR 795/60	Kg		homashyo	{"uom": "Kg", "code": "795/60 pe pr toza", "name": "PE PR 795/60", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.466237+05
805/40 pe pr  vakuum	PE PR 805/40	Kg		homashyo	{"uom": "Kg", "code": "805/40 pe pr  vakuum", "name": "PE PR 805/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.467114+05
850/80 pe pr oq	PE PR 850/80	Kg		homashyo	{"uom": "Kg", "code": "850/80 pe pr oq", "name": "PE PR 850/80", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.469001+05
CPP / 20 mikron / 500	CPP 20/500	Kg		homashyo	{"uom": "Kg", "code": "CPP / 20 mikron / 500", "name": "CPP 20/500", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.51222+05
Jem 580/20	JEM 580/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 580/20", "name": "JEM 580/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.542839+05
Jem 600/20	JEM 600/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 600/20", "name": "JEM 600/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.543157+05
Jem 645/20	JEM 645/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 645/20", "name": "JEM 645/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.543783+05
MAT 485/20	MAT 485/20	Kg		homashyo	{"uom": "Kg", "code": "MAT 485/20", "name": "MAT 485/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.563348+05
MCP / 25 mikron / 680	MCP 25/680	Kg		homashyo	{"uom": "Kg", "code": "MCP / 25 mikron / 680", "name": "MCP 25/680", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.565048+05
MCP / 25 mikron / 700	MCP 25/700	Kg		homashyo	{"uom": "Kg", "code": "MCP / 25 mikron / 700", "name": "MCP 25/700", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.565155+05
OPP 860/30 Pol pf	OPP 860/30	Kg		homashyo	{"uom": "Kg", "code": "OPP 860/30 Pol pf", "name": "OPP 860/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.590776+05
OPPM 365/30	OPPM 365/30	Kg		homashyo	{"uom": "Kg", "code": "OPPM 365/30", "name": "OPPM 365/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.591665+05
jem 315/30 pol	JEM 315/30	Kg		homashyo	{"uom": "Kg", "code": "jem 315/30 pol", "name": "JEM 315/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.758803+05
jem 335/35 kar	JEM 335/35	Kg		homashyo	{"uom": "Kg", "code": "jem 335/35 kar", "name": "JEM 335/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.758973+05
jem 735/25	JEM 735/25	Kg		homashyo	{"uom": "Kg", "code": "jem 735/25", "name": "JEM 735/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.767516+05
opp 460/20 kar	OPP 460/20	Kg		homashyo	{"uom": "Kg", "code": "opp 460/20 kar", "name": "OPP 460/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.867472+05
opp 465/20 kar 3	OPP 465/20	Kg		homashyo	{"uom": "Kg", "code": "opp 465/20 kar 3", "name": "OPP 465/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.868113+05
opp 468/18 kar	OPP 468/18	Kg		homashyo	{"uom": "Kg", "code": "opp 468/18 kar", "name": "OPP 468/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.86857+05
opp 475/18 kar	OPP 475/18	Kg		homashyo	{"uom": "Kg", "code": "opp 475/18 kar", "name": "OPP 475/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.869493+05
opp 495/18 kar	OPP 495/18	Kg		homashyo	{"uom": "Kg", "code": "opp 495/18 kar", "name": "OPP 495/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.871729+05
opp 495/30	OPP 495/30	Kg		homashyo	{"uom": "Kg", "code": "opp 495/30", "name": "OPP 495/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.872045+05
opp 510/18 PF kar	OPP 510/18	Kg		homashyo	{"uom": "Kg", "code": "opp 510/18 PF kar", "name": "OPP 510/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.872982+05
opp 560/30 pf kar	OPP 560/30	Kg		homashyo	{"uom": "Kg", "code": "opp 560/30 pf kar", "name": "OPP 560/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.879322+05
opp 840/30 PF kar	OPP 840/30	Kg		homashyo	{"uom": "Kg", "code": "opp 840/30 PF kar", "name": "OPP 840/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.90315+05
oppm 700/20 kar	OPPM 700/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 700/20 kar", "name": "OPPM 700/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.919403+05
pet 520/12 pol	PET 520/12	Kg		homashyo	{"uom": "Kg", "code": "pet 520/12 pol", "name": "PET 520/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.942355+05
pet 600/12 pol	PET 600/12	Kg		homashyo	{"uom": "Kg", "code": "pet 600/12 pol", "name": "PET 600/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.944002+05
pet 920/12	PET 920/12	Kg		homashyo	{"uom": "Kg", "code": "pet 920/12", "name": "PET 920/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.949322+05
510/45 pe pr toza	PE PR 510/45	Kg		homashyo	{"uom": "Kg", "code": "510/45 pe pr toza", "name": "PE PR 510/45", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.447408+05
510/90 pe pr vakuum	PE PR 510/90	Kg		homashyo	{"uom": "Kg", "code": "510/90 pe pr vakuum", "name": "PE PR 510/90", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.447812+05
615/70 pe pr oq	PE PR 615/70	Kg		homashyo	{"uom": "Kg", "code": "615/70 pe pr oq", "name": "PE PR 615/70", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.455699+05
645/90 pe pr toza	PE PR 645/90	Kg		homashyo	{"uom": "Kg", "code": "645/90 pe pr toza", "name": "PE PR 645/90", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.456952+05
660/90 pe pr toza	PE PR 660/90	Kg		homashyo	{"uom": "Kg", "code": "660/90 pe pr toza", "name": "PE PR 660/90", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.457467+05
665/40 pe pr toza	PE PR 665/40	Kg		homashyo	{"uom": "Kg", "code": "665/40 pe pr toza", "name": "PE PR 665/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.457705+05
670/85 pe pr oq	PE PR 670/85	Kg		homashyo	{"uom": "Kg", "code": "670/85 pe pr oq", "name": "PE PR 670/85", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.458496+05
670/90 pe pr toza	PE PR 670/90	Kg		homashyo	{"uom": "Kg", "code": "670/90 pe pr toza", "name": "PE PR 670/90", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.45891+05
675/60 pe pr oq	PE PR 675/60	Kg		homashyo	{"uom": "Kg", "code": "675/60 pe pr oq", "name": "PE PR 675/60", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.459427+05
680/30 pe pr toza	PE PR 680/30	Kg		homashyo	{"uom": "Kg", "code": "680/30 pe pr toza", "name": "PE PR 680/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.459789+05
680/65 pe pr toza	PE PR 680/65	Kg		homashyo	{"uom": "Kg", "code": "680/65 pe pr toza", "name": "PE PR 680/65", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.460711+05
685/95 pe pr oq	PE PR 685/95	Kg		homashyo	{"uom": "Kg", "code": "685/95 pe pr oq", "name": "PE PR 685/95", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.461142+05
710/45 pe pr toza	PE PR 710/45	Kg		homashyo	{"uom": "Kg", "code": "710/45 pe pr toza", "name": "PE PR 710/45", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.462149+05
735/60 pe pr oq	PE PR 735/60	Kg		homashyo	{"uom": "Kg", "code": "735/60 pe pr oq", "name": "PE PR 735/60", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.4624+05
750/45 pe pr toza	PE PR 750/45	Kg		homashyo	{"uom": "Kg", "code": "750/45 pe pr toza", "name": "PE PR 750/45", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.463806+05
755/55 pe pr oq	PE PR 755/55	Kg		homashyo	{"uom": "Kg", "code": "755/55 pe pr oq", "name": "PE PR 755/55", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.464123+05
755/60 pe pr oq	PE PR 755/60	Kg		homashyo	{"uom": "Kg", "code": "755/60 pe pr oq", "name": "PE PR 755/60", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.464387+05
765/40 pe pr toza	PE PR 765/40	Kg		homashyo	{"uom": "Kg", "code": "765/40 pe pr toza", "name": "PE PR 765/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.464668+05
775/55 pe pr oq	PE PR 775/55	Kg		homashyo	{"uom": "Kg", "code": "775/55 pe pr oq", "name": "PE PR 775/55", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.464929+05
775/65 pe pr oq	PE PR 775/65	Kg		homashyo	{"uom": "Kg", "code": "775/65 pe pr oq", "name": "PE PR 775/65", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.465185+05
780/20 pff kar	PFF 780/20	Kg		homashyo	{"uom": "Kg", "code": "780/20 pff kar", "name": "PFF 780/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.465446+05
790/60 pe pr toza	PE PR 790/60	Kg		homashyo	{"uom": "Kg", "code": "790/60 pe pr toza", "name": "PE PR 790/60", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.465707+05
MCP / 25 mikron / 720	MCP 25/720	Kg		homashyo	{"uom": "Kg", "code": "MCP / 25 mikron / 720", "name": "MCP 25/720", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.565257+05
MCP / 30 mikron / 500	MCP 30/500	Kg		homashyo	{"uom": "Kg", "code": "MCP / 30 mikron / 500", "name": "MCP 30/500", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.565354+05
OPP 1350/25	OPP 1350/25	Kg		homashyo	{"uom": "Kg", "code": "OPP 1350/25", "name": "OPP 1350/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.586163+05
OPP 615/30 Pol pff	OPP POL PFF 615/30	Kg		homashyo	{"uom": "Kg", "code": "OPP 615/30 Pol pff", "name": "OPP POL PFF 615/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.588683+05
jem 1200/35 kar	JEM 1200/35	Kg		homashyo	{"uom": "Kg", "code": "jem 1200/35 kar", "name": "JEM 1200/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.757839+05
opp 435/18 kar	OPP 435/18	Kg		homashyo	{"uom": "Kg", "code": "opp 435/18 kar", "name": "OPP 435/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.865776+05
opp 440/20 kar	OPP 440/20	Kg		homashyo	{"uom": "Kg", "code": "opp 440/20 kar", "name": "OPP 440/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.866193+05
opp 455/18 pf kar	OPP 455/18	Kg		homashyo	{"uom": "Kg", "code": "opp 455/18 pf kar", "name": "OPP 455/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.866934+05
opp 660/40 kar	OPP 660/40	Kg		homashyo	{"uom": "Kg", "code": "opp 660/40 kar", "name": "OPP 660/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.887675+05
opp 665/18 kar	OPP 665/18	Kg		homashyo	{"uom": "Kg", "code": "opp 665/18 kar", "name": "OPP 665/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.887815+05
opp 740/20 pf kar	OPP 740/20	Kg		homashyo	{"uom": "Kg", "code": "opp 740/20 pf kar", "name": "OPP 740/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.895527+05
opp 825/30 pf kar	OPP 825/30	Kg		homashyo	{"uom": "Kg", "code": "opp 825/30 pf kar", "name": "OPP 825/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.901137+05
opp 840/25 kar	OPP 840/25	Kg		homashyo	{"uom": "Kg", "code": "opp 840/25 kar", "name": "OPP 840/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.902992+05
oppm 635/20 pol	OPPM 635/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 635/20 pol", "name": "OPPM 635/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.916579+05
oppm 885/30 pol	OPPM 885/30	Kg		homashyo	{"uom": "Kg", "code": "oppm 885/30 pol", "name": "OPPM 885/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.923881+05
st01 1230/25	ST01 1230/25	Kg		homashyo	{"uom": "Kg", "code": "st01 1230/25", "name": "ST01 1230/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.00541+05
610/90 pe pr toza	PE PR 610/90	Kg		homashyo	{"uom": "Kg", "code": "610/90 pe pr toza", "name": "PE PR 610/90", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.454908+05
615/65 pe pr oq	PE PR 615/65	Kg		homashyo	{"uom": "Kg", "code": "615/65 pe pr oq", "name": "PE PR 615/65", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.45542+05
Jem 435/18	JEM 435/18	Kg		homashyo	{"uom": "Kg", "code": "Jem 435/18", "name": "JEM 435/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.54105+05
Jem 570/20	JEM 570/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 570/20", "name": "JEM 570/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.542611+05
Jem 695/20	JEM 695/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 695/20", "name": "JEM 695/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.544205+05
cpp 1030/25	CPP 1030/25	Kg		homashyo	{"uom": "Kg", "code": "cpp 1030/25", "name": "CPP 1030/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.702206+05
cpp 455/30	CPP 455/30	Kg		homashyo	{"uom": "Kg", "code": "cpp 455/30", "name": "CPP 455/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.70253+05
cpp 455/40	CPP 455/40	Kg		homashyo	{"uom": "Kg", "code": "cpp 455/40", "name": "CPP 455/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.702663+05
cpp 475/50	CPP 475/50	Kg		homashyo	{"uom": "Kg", "code": "cpp 475/50", "name": "CPP 475/50", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.702818+05
cpp 595/60	CPP 595/60	Kg		homashyo	{"uom": "Kg", "code": "cpp 595/60", "name": "CPP 595/60", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.703416+05
cpp 615/55	CPP 615/55	Kg		homashyo	{"uom": "Kg", "code": "cpp 615/55", "name": "CPP 615/55", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.703564+05
cpp 675/30	CPP 675/30	Kg		homashyo	{"uom": "Kg", "code": "cpp 675/30", "name": "CPP 675/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.703835+05
cpp 695/55	CPP 695/55	Kg		homashyo	{"uom": "Kg", "code": "cpp 695/55", "name": "CPP 695/55", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.704003+05
cpp 750/30	CPP 750/30	Kg		homashyo	{"uom": "Kg", "code": "cpp 750/30", "name": "CPP 750/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.704167+05
cpp 780/35	CPP 780/35	Kg		homashyo	{"uom": "Kg", "code": "cpp 780/35", "name": "CPP 780/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.704327+05
cpp 795/30	CPP 795/30	Kg		homashyo	{"uom": "Kg", "code": "cpp 795/30", "name": "CPP 795/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.704472+05
cpp 985/40	CPP 985/40	Kg		homashyo	{"uom": "Kg", "code": "cpp 985/40", "name": "CPP 985/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.704908+05
cpp 990/35	CPP 990/35	Kg		homashyo	{"uom": "Kg", "code": "cpp 990/35", "name": "CPP 990/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.705053+05
mat 1500/20	MAT 1500/20	Kg		homashyo	{"uom": "Kg", "code": "mat 1500/20", "name": "MAT 1500/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.802973+05
mat 405/20 kar	MAT 405/20	Kg		homashyo	{"uom": "Kg", "code": "mat 405/20 kar", "name": "MAT 405/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.80312+05
mat 410/20 pol	MAT 410/20	Kg		homashyo	{"uom": "Kg", "code": "mat 410/20 pol", "name": "MAT 410/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.803321+05
mat 415/20 kar	MAT 415/20	Kg		homashyo	{"uom": "Kg", "code": "mat 415/20 kar", "name": "MAT 415/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.803481+05
mat 440/20 pol	MAT 440/20	Kg		homashyo	{"uom": "Kg", "code": "mat 440/20 pol", "name": "MAT 440/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.804086+05
mat 455/20 kar	MAT 455/20	Kg		homashyo	{"uom": "Kg", "code": "mat 455/20 kar", "name": "MAT 455/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.804237+05
mat 455/20 kar 3	MAT 455/20	Kg		homashyo	{"uom": "Kg", "code": "mat 455/20 kar 3", "name": "MAT 455/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.804393+05
mat 460/20 kar	MAT 460/20	Kg		homashyo	{"uom": "Kg", "code": "mat 460/20 kar", "name": "MAT 460/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.804544+05
mat 485/20 kar	MAT 485/20	Kg		homashyo	{"uom": "Kg", "code": "mat 485/20 kar", "name": "MAT 485/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.805122+05
mat 495/20 kar	MAT 495/20	Kg		homashyo	{"uom": "Kg", "code": "mat 495/20 kar", "name": "MAT 495/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.805274+05
mat 500/20 kar	MAT 500/20	Kg		homashyo	{"uom": "Kg", "code": "mat 500/20 kar", "name": "MAT 500/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.805404+05
mat 505/20 kar	MAT 505/20	Kg		homashyo	{"uom": "Kg", "code": "mat 505/20 kar", "name": "MAT 505/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.805552+05
mat 510/20 kar	MAT 510/20	Kg		homashyo	{"uom": "Kg", "code": "mat 510/20 kar", "name": "MAT 510/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.805704+05
mat 515/20 kar	MAT 515/20	Kg		homashyo	{"uom": "Kg", "code": "mat 515/20 kar", "name": "MAT 515/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.80584+05
mat 540/20 kar	MAT 540/20	Kg		homashyo	{"uom": "Kg", "code": "mat 540/20 kar", "name": "MAT 540/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.805973+05
opp 475/18 pol	OPP 475/18	Kg		homashyo	{"uom": "Kg", "code": "opp 475/18 pol", "name": "OPP 475/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.869655+05
opp 500/20 pol	OPP 500/20	Kg		homashyo	{"uom": "Kg", "code": "opp 500/20 pol", "name": "OPP 500/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.872348+05
opp 560/30 kar	OPP 560/30	Kg		homashyo	{"uom": "Kg", "code": "opp 560/30 kar", "name": "OPP 560/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.879163+05
opp 730/20 pff kar	OPP PFF 730/20	Kg		homashyo	{"uom": "Kg", "code": "opp 730/20 pff kar", "name": "OPP PFF 730/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.894966+05
oppm 670/23	OPPM 670/23	Kg		homashyo	{"uom": "Kg", "code": "oppm 670/23", "name": "OPPM 670/23", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.91849+05
pet 1000/12	PET 1000/12	Kg		homashyo	{"uom": "Kg", "code": "pet 1000/12", "name": "PET 1000/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.939416+05
795/70 pe pr oq	PE PR 795/70	Kg		homashyo	{"uom": "Kg", "code": "795/70 pe pr oq", "name": "PE PR 795/70", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.466465+05
815/60 pe pr oq	PE PR 815/60	Kg		homashyo	{"uom": "Kg", "code": "815/60 pe pr oq", "name": "PE PR 815/60", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.467941+05
mat 545/20	MAT 545/20	Kg		homashyo	{"uom": "Kg", "code": "mat 545/20", "name": "MAT 545/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.806114+05
mat 570/20 kar	MAT 570/20	Kg		homashyo	{"uom": "Kg", "code": "mat 570/20 kar", "name": "MAT 570/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.806393+05
mat 575/20 kar	MAT 575/20	Kg		homashyo	{"uom": "Kg", "code": "mat 575/20 kar", "name": "MAT 575/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.806535+05
mat 580/20 kar	MAT 580/20	Kg		homashyo	{"uom": "Kg", "code": "mat 580/20 kar", "name": "MAT 580/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.806671+05
mat 585/18 kar	MAT 585/18	Kg		homashyo	{"uom": "Kg", "code": "mat 585/18 kar", "name": "MAT 585/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.806809+05
mat 585/20 pol	MAT 585/20	Kg		homashyo	{"uom": "Kg", "code": "mat 585/20 pol", "name": "MAT 585/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.806961+05
mat 595/20 pol	MAT 595/20	Kg		homashyo	{"uom": "Kg", "code": "mat 595/20 pol", "name": "MAT 595/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.807291+05
mat 620/20	MAT 620/20	Kg		homashyo	{"uom": "Kg", "code": "mat 620/20", "name": "MAT 620/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.807771+05
mat 625/20 kar	MAT 625/20	Kg		homashyo	{"uom": "Kg", "code": "mat 625/20 kar", "name": "MAT 625/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.808077+05
mat 635/20 kar	MAT 635/20	Kg		homashyo	{"uom": "Kg", "code": "mat 635/20 kar", "name": "MAT 635/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.808255+05
mat 635/20 pol	MAT 635/20	Kg		homashyo	{"uom": "Kg", "code": "mat 635/20 pol", "name": "MAT 635/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.808416+05
mat 645/20 kar	MAT 645/20	Kg		homashyo	{"uom": "Kg", "code": "mat 645/20 kar", "name": "MAT 645/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.808593+05
mat 655/20 pol	MAT 655/20	Kg		homashyo	{"uom": "Kg", "code": "mat 655/20 pol", "name": "MAT 655/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.808731+05
mat 665/20	MAT 665/20	Kg		homashyo	{"uom": "Kg", "code": "mat 665/20", "name": "MAT 665/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.808883+05
mat 665/20 pol	MAT 665/20	Kg		homashyo	{"uom": "Kg", "code": "mat 665/20 pol", "name": "MAT 665/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.809035+05
mat 675/20 kar	MAT 675/20	Kg		homashyo	{"uom": "Kg", "code": "mat 675/20 kar", "name": "MAT 675/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.809219+05
mat 685/20	MAT 685/20	Kg		homashyo	{"uom": "Kg", "code": "mat 685/20", "name": "MAT 685/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.80974+05
mat 695/20 kar	MAT 695/20	Kg		homashyo	{"uom": "Kg", "code": "mat 695/20 kar", "name": "MAT 695/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.810063+05
mat 695/20 pol	MAT 695/20	Kg		homashyo	{"uom": "Kg", "code": "mat 695/20 pol", "name": "MAT 695/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.810299+05
mat 705/20	MAT 705/20	Kg		homashyo	{"uom": "Kg", "code": "mat 705/20", "name": "MAT 705/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.810461+05
mat 710/20 kar	MAT 710/20	Kg		homashyo	{"uom": "Kg", "code": "mat 710/20 kar", "name": "MAT 710/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.810642+05
mat 765/20	MAT 765/20	Kg		homashyo	{"uom": "Kg", "code": "mat 765/20", "name": "MAT 765/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.81142+05
mat 780/20 kar	MAT 780/20	Kg		homashyo	{"uom": "Kg", "code": "mat 780/20 kar", "name": "MAT 780/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.811728+05
mat 830/20	MAT 830/20	Kg		homashyo	{"uom": "Kg", "code": "mat 830/20", "name": "MAT 830/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.812386+05
mat 885/20 kar	MAT 885/20	Kg		homashyo	{"uom": "Kg", "code": "mat 885/20 kar", "name": "MAT 885/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.812563+05
mat 895/20	MAT 895/20	Kg		homashyo	{"uom": "Kg", "code": "mat 895/20", "name": "MAT 895/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.812761+05
mat 955/20 pol	MAT 955/20	Kg		homashyo	{"uom": "Kg", "code": "mat 955/20 pol", "name": "MAT 955/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.812944+05
mpet 735/12 pol	MPET 735/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 735/12 pol", "name": "MPET 735/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.837824+05
mpet 830/12 pol	MPET 830/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 830/12 pol", "name": "MPET 830/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.838729+05
mpet 940/12	MPET 940/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 940/12", "name": "MPET 940/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.839176+05
mpet 940/12 pol	MPET 940/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 940/12 pol", "name": "MPET 940/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.839328+05
opp  585/18	OPP 585/18	Kg		homashyo	{"uom": "Kg", "code": "opp  585/18", "name": "OPP 585/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.852224+05
opp  795/18	OPP 795/18	Kg		homashyo	{"uom": "Kg", "code": "opp  795/18", "name": "OPP 795/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.852532+05
opp 685/20 pfg	OPP 685/20	Kg		homashyo	{"uom": "Kg", "code": "opp 685/20 pfg", "name": "OPP 685/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.890287+05
oppm 660/20	OPPM 660/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 660/20", "name": "OPPM 660/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.917537+05
oppm 875/20 pol	OPPM 875/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 875/20 pol", "name": "OPPM 875/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.923079+05
pet 590/12 pol	PET 590/12	Kg		homashyo	{"uom": "Kg", "code": "pet 590/12 pol", "name": "PET 590/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.943686+05
tvist m 675/23 pol	TVIST m 675/23	Kg		homashyo	{"uom": "Kg", "code": "tvist m 675/23 pol", "name": "TVIST m 675/23", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.026105+05
jem 615/25 kar	JEM 615/25	Kg		homashyo	{"uom": "Kg", "code": "jem 615/25 kar", "name": "JEM 615/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.765042+05
jem 620/35 kar	JEM 620/35	Kg		homashyo	{"uom": "Kg", "code": "jem 620/35 kar", "name": "JEM 620/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.765373+05
opp 1015/40 kar	OPP 1015/40	Kg		homashyo	{"uom": "Kg", "code": "opp 1015/40 kar", "name": "OPP 1015/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.853829+05
opp 1100/30 pol	OPP 1100/30	Kg		homashyo	{"uom": "Kg", "code": "opp 1100/30 pol", "name": "OPP 1100/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.854501+05
opp 1100/30 spp	OPP 1100/30	Kg		homashyo	{"uom": "Kg", "code": "opp 1100/30 spp", "name": "OPP 1100/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.854631+05
opp 1140/25 pol	OPP 1140/25	Kg		homashyo	{"uom": "Kg", "code": "opp 1140/25 pol", "name": "OPP 1140/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.85479+05
opp 1220/35	OPP 1220/35	Kg		homashyo	{"uom": "Kg", "code": "opp 1220/35", "name": "OPP 1220/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.855098+05
opp 1220/35 pol	OPP 1220/35	Kg		homashyo	{"uom": "Kg", "code": "opp 1220/35 pol", "name": "OPP 1220/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.855241+05
opp 1230/30	OPP 1230/30	Kg		homashyo	{"uom": "Kg", "code": "opp 1230/30", "name": "OPP 1230/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.855377+05
opp 1230/35	OPP 1230/35	Kg		homashyo	{"uom": "Kg", "code": "opp 1230/35", "name": "OPP 1230/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.855518+05
opp 1280/20	OPP 1280/20	Kg		homashyo	{"uom": "Kg", "code": "opp 1280/20", "name": "OPP 1280/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.855658+05
opp 1280/25	OPP 1280/25	Kg		homashyo	{"uom": "Kg", "code": "opp 1280/25", "name": "OPP 1280/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.855803+05
opp 1280/30	OPP 1280/30	Kg		homashyo	{"uom": "Kg", "code": "opp 1280/30", "name": "OPP 1280/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.855939+05
opp 1300/35	OPP 1300/35	Kg		homashyo	{"uom": "Kg", "code": "opp 1300/35", "name": "OPP 1300/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.856216+05
opp 1340/35 pol	OPP 1340/35	Kg		homashyo	{"uom": "Kg", "code": "opp 1340/35 pol", "name": "OPP 1340/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.856353+05
opp 1350/18 pol	OPP 1350/18	Kg		homashyo	{"uom": "Kg", "code": "opp 1350/18 pol", "name": "OPP 1350/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.85649+05
opp 1350/20 pol	OPP 1350/20	Kg		homashyo	{"uom": "Kg", "code": "opp 1350/20 pol", "name": "OPP 1350/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.856905+05
opp 1350/25 pol	OPP 1350/25	Kg		homashyo	{"uom": "Kg", "code": "opp 1350/25 pol", "name": "OPP 1350/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.857059+05
opp 1360/20	OPP 1360/20	Kg		homashyo	{"uom": "Kg", "code": "opp 1360/20", "name": "OPP 1360/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.857333+05
opp 1400/25	OPP 1400/25	Kg		homashyo	{"uom": "Kg", "code": "opp 1400/25", "name": "OPP 1400/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.857465+05
opp 1400/30 kar	OPP 1400/30	Kg		homashyo	{"uom": "Kg", "code": "opp 1400/30 kar", "name": "OPP 1400/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.857752+05
opp 1400/30 pff kar	OPP PFF 1400/30	Kg		homashyo	{"uom": "Kg", "code": "opp 1400/30 pff kar", "name": "OPP PFF 1400/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.857907+05
opp 1410/25	OPP 1410/25	Kg		homashyo	{"uom": "Kg", "code": "opp 1410/25", "name": "OPP 1410/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.858037+05
opp 1440/30	OPP 1440/30	Kg		homashyo	{"uom": "Kg", "code": "opp 1440/30", "name": "OPP 1440/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.858181+05
opp 1470/18	OPP 1470/18	Kg		homashyo	{"uom": "Kg", "code": "opp 1470/18", "name": "OPP 1470/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.858332+05
opp 1470/30 pol	OPP 1470/30	Kg		homashyo	{"uom": "Kg", "code": "opp 1470/30 pol", "name": "OPP 1470/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.858509+05
opp 1520/20 pff msp	OPP PFF MSP 1520/20	Kg		homashyo	{"uom": "Kg", "code": "opp 1520/20 pff msp", "name": "OPP PFF MSP 1520/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.859019+05
opp 1630/20	OPP 1630/20	Kg		homashyo	{"uom": "Kg", "code": "opp 1630/20", "name": "OPP 1630/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.859333+05
opp 1680/20	OPP 1680/20	Kg		homashyo	{"uom": "Kg", "code": "opp 1680/20", "name": "OPP 1680/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.859546+05
opp 1770/18	OPP 1770/18	Kg		homashyo	{"uom": "Kg", "code": "opp 1770/18", "name": "OPP 1770/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.860187+05
opp 260/35	OPP 260/35	Kg		homashyo	{"uom": "Kg", "code": "opp 260/35", "name": "OPP 260/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.860944+05
opp 365/35 99 kg	OPP 365/35	Kg		homashyo	{"uom": "Kg", "code": "opp 365/35 99 kg", "name": "OPP 365/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.862057+05
opp 370/20 pff pol	OPP PFF 370/20	Kg		homashyo	{"uom": "Kg", "code": "opp 370/20 pff pol", "name": "OPP PFF 370/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.862208+05
opp 375/20	OPP 375/20	Kg		homashyo	{"uom": "Kg", "code": "opp 375/20", "name": "OPP 375/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.862359+05
opp 380/2 pff	OPP PFF 380/2	Kg		homashyo	{"uom": "Kg", "code": "opp 380/2 pff", "name": "OPP PFF 380/2", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.862521+05
opp 765/30 kar	OPP 765/30	Kg		homashyo	{"uom": "Kg", "code": "opp 765/30 kar", "name": "OPP 765/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.897353+05
opp 815/18 pol	OPP 815/18	Kg		homashyo	{"uom": "Kg", "code": "opp 815/18 pol", "name": "OPP 815/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.900339+05
opp 875/30 pol	OPP 875/30	Kg		homashyo	{"uom": "Kg", "code": "opp 875/30 pol", "name": "OPP 875/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.904864+05
oppm 670/25 kar	OPPM 670/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 670/25 kar", "name": "OPPM 670/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.918643+05
pet 230/12 pol	PET 230/12	Kg		homashyo	{"uom": "Kg", "code": "pet 230/12 pol", "name": "PET 230/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.940083+05
opp 1010/30 pf	OPP 1010/30	Kg		homashyo	{"uom": "Kg", "code": "opp 1010/30 pf", "name": "OPP 1010/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.8533+05
opp 1015/18 kar	OPP 1015/18	Kg		homashyo	{"uom": "Kg", "code": "opp 1015/18 kar", "name": "OPP 1015/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.853671+05
opp 350/20 kar	OPP 350/20	Kg		homashyo	{"uom": "Kg", "code": "opp 350/20 kar", "name": "OPP 350/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.861414+05
opp 365/35  pol	OPP 365/35	Kg		homashyo	{"uom": "Kg", "code": "opp 365/35  pol", "name": "OPP 365/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.861731+05
opp 380/20 pol	OPP 380/20	Kg		homashyo	{"uom": "Kg", "code": "opp 380/20 pol", "name": "OPP 380/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.862833+05
opp 400/18 kar	OPP 400/18	Kg		homashyo	{"uom": "Kg", "code": "opp 400/18 kar", "name": "OPP 400/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.863189+05
opp 400/18 pol	OPP 400/18	Kg		homashyo	{"uom": "Kg", "code": "opp 400/18 pol", "name": "OPP 400/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.863346+05
opp 405/25 pol	OPP 405/25	Kg		homashyo	{"uom": "Kg", "code": "opp 405/25 pol", "name": "OPP 405/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.863646+05
opp 41/18 kar	OPP 41/18	Kg		homashyo	{"uom": "Kg", "code": "opp 41/18 kar", "name": "OPP 41/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.863802+05
opp 415/18	OPP 415/18	Kg		homashyo	{"uom": "Kg", "code": "opp 415/18", "name": "OPP 415/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.86399+05
opp 415/20 pol	OPP 415/20	Kg		homashyo	{"uom": "Kg", "code": "opp 415/20 pol", "name": "OPP 415/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.86415+05
opp 420/20 pff	OPP PFF 420/20	Kg		homashyo	{"uom": "Kg", "code": "opp 420/20 pff", "name": "OPP PFF 420/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.864301+05
opp 420/20 pff kar	OPP PFF 420/20	Kg		homashyo	{"uom": "Kg", "code": "opp 420/20 pff kar", "name": "OPP PFF 420/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.864446+05
opp 425/20 kar	OPP 425/20	Kg		homashyo	{"uom": "Kg", "code": "opp 425/20 kar", "name": "OPP 425/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.865042+05
opp 430/18 kar	OPP 430/18	Kg		homashyo	{"uom": "Kg", "code": "opp 430/18 kar", "name": "OPP 430/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.865179+05
opp 430/20	OPP 430/20	Kg		homashyo	{"uom": "Kg", "code": "opp 430/20", "name": "OPP 430/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.86533+05
opp 485/30 pff	OPP PFF 485/30	Kg		homashyo	{"uom": "Kg", "code": "opp 485/30 pff", "name": "OPP PFF 485/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.871043+05
opp 505/18 kar	OPP 505/18	Kg		homashyo	{"uom": "Kg", "code": "opp 505/18 kar", "name": "OPP 505/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.872661+05
opp 510/18 pf pol	OPP 510/18	Kg		homashyo	{"uom": "Kg", "code": "opp 510/18 pf pol", "name": "OPP 510/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.87335+05
opp 515//18	OPP 515//18	Kg		homashyo	{"uom": "Kg", "code": "opp 515//18", "name": "OPP 515//18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.873552+05
opp 515/18 kar	OPP 515/18	Kg		homashyo	{"uom": "Kg", "code": "opp 515/18 kar", "name": "OPP 515/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.873714+05
opp 515/20 pol	OPP 515/20	Kg		homashyo	{"uom": "Kg", "code": "opp 515/20 pol", "name": "OPP 515/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.874151+05
opp 525/20 kar	OPP 525/20	Kg		homashyo	{"uom": "Kg", "code": "opp 525/20 kar", "name": "OPP 525/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.875723+05
opp 525/20 pff	OPP PFF 525/20	Kg		homashyo	{"uom": "Kg", "code": "opp 525/20 pff", "name": "OPP PFF 525/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.875872+05
opp 525/30 pf	OPP 525/30	Kg		homashyo	{"uom": "Kg", "code": "opp 525/30 pf", "name": "OPP 525/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.876011+05
opp 525/35 kar	OPP 525/35	Kg		homashyo	{"uom": "Kg", "code": "opp 525/35 kar", "name": "OPP 525/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.876162+05
opp 535/25 kar	OPP 535/25	Kg		homashyo	{"uom": "Kg", "code": "opp 535/25 kar", "name": "OPP 535/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.876736+05
opp 540/18 pol	OPP 540/18	Kg		homashyo	{"uom": "Kg", "code": "opp 540/18 pol", "name": "OPP 540/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.876868+05
opp 540/25	OPP 540/25	Kg		homashyo	{"uom": "Kg", "code": "opp 540/25", "name": "OPP 540/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.87701+05
opp 540/25 pol	OPP 540/25	Kg		homashyo	{"uom": "Kg", "code": "opp 540/25 pol", "name": "OPP 540/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.877154+05
opp 540/30 kar	OPP 540/30	Kg		homashyo	{"uom": "Kg", "code": "opp 540/30 kar", "name": "OPP 540/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.877569+05
opp 540/30 pf	OPP 540/30	Kg		homashyo	{"uom": "Kg", "code": "opp 540/30 pf", "name": "OPP 540/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.877719+05
opp 540/30 pol	OPP 540/30	Kg		homashyo	{"uom": "Kg", "code": "opp 540/30 pol", "name": "OPP 540/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.877846+05
opp 545/20 pol	OPP 545/20	Kg		homashyo	{"uom": "Kg", "code": "opp 545/20 pol", "name": "OPP 545/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.878157+05
opp 555/30 kar	OPP 555/30	Kg		homashyo	{"uom": "Kg", "code": "opp 555/30 kar", "name": "OPP 555/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.879+05
opp 575/18 kar	OPP 575/18	Kg		homashyo	{"uom": "Kg", "code": "opp 575/18 kar", "name": "OPP 575/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.879918+05
oppm 975/20 pol	OPPM 975/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 975/20 pol", "name": "OPPM 975/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.925169+05
pet 615/12	PET 615/12	Kg		homashyo	{"uom": "Kg", "code": "pet 615/12", "name": "PET 615/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.944159+05
pet 635/12 pol	PET 635/12	Kg		homashyo	{"uom": "Kg", "code": "pet 635/12 pol", "name": "PET 635/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.944475+05
mat 720/20	MAT 720/20	Kg		homashyo	{"uom": "Kg", "code": "mat 720/20", "name": "MAT 720/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.810789+05
mpet 850/12	MPET 850/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 850/12", "name": "MPET 850/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.838869+05
opp 575/40 kar	OPP 575/40	Kg		homashyo	{"uom": "Kg", "code": "opp 575/40 kar", "name": "OPP 575/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.880695+05
opp 580/18 pol	OPP 580/18	Kg		homashyo	{"uom": "Kg", "code": "opp 580/18 pol", "name": "OPP 580/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.88085+05
opp 585/18	OPP 585/18	Kg		homashyo	{"uom": "Kg", "code": "opp 585/18", "name": "OPP 585/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.881605+05
opp 585/18 kar	OPP 585/18	Kg		homashyo	{"uom": "Kg", "code": "opp 585/18 kar", "name": "OPP 585/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.881759+05
opp 585/20 kar	OPP 585/20	Kg		homashyo	{"uom": "Kg", "code": "opp 585/20 kar", "name": "OPP 585/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.881917+05
opp 585/20 pol	OPP 585/20	Kg		homashyo	{"uom": "Kg", "code": "opp 585/20 pol", "name": "OPP 585/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.882079+05
opp 590/18	OPP 590/18	Kg		homashyo	{"uom": "Kg", "code": "opp 590/18", "name": "OPP 590/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.882242+05
opp 590/25	OPP 590/25	Kg		homashyo	{"uom": "Kg", "code": "opp 590/25", "name": "OPP 590/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.882397+05
opp 590/30 pol	OPP 590/30	Kg		homashyo	{"uom": "Kg", "code": "opp 590/30 pol", "name": "OPP 590/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.882552+05
opp 595/18	OPP 595/18	Kg		homashyo	{"uom": "Kg", "code": "opp 595/18", "name": "OPP 595/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.882719+05
opp 600/20	OPP 600/20	Kg		homashyo	{"uom": "Kg", "code": "opp 600/20", "name": "OPP 600/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.883069+05
opp 600/25 kar	OPP 600/25	Kg		homashyo	{"uom": "Kg", "code": "opp 600/25 kar", "name": "OPP 600/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.883405+05
opp 600/30	OPP 600/30	Kg		homashyo	{"uom": "Kg", "code": "opp 600/30", "name": "OPP 600/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.883579+05
opp 605/20 kar	OPP 605/20	Kg		homashyo	{"uom": "Kg", "code": "opp 605/20 kar", "name": "OPP 605/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.883893+05
opp 610/20 kar	OPP 610/20	Kg		homashyo	{"uom": "Kg", "code": "opp 610/20 kar", "name": "OPP 610/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.884035+05
opp 615/18	OPP 615/18	Kg		homashyo	{"uom": "Kg", "code": "opp 615/18", "name": "OPP 615/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.884365+05
opp 615/20	OPP 615/20	Kg		homashyo	{"uom": "Kg", "code": "opp 615/20", "name": "OPP 615/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.884521+05
opp 615/25 kar	OPP 615/25	Kg		homashyo	{"uom": "Kg", "code": "opp 615/25 kar", "name": "OPP 615/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.884837+05
opp 620/18	OPP 620/18	Kg		homashyo	{"uom": "Kg", "code": "opp 620/18", "name": "OPP 620/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.884997+05
opp 620/20	OPP 620/20	Kg		homashyo	{"uom": "Kg", "code": "opp 620/20", "name": "OPP 620/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.885143+05
opp 630/25	OPP 630/25	Kg		homashyo	{"uom": "Kg", "code": "opp 630/25", "name": "OPP 630/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.8857+05
opp 635/18 kar	OPP 635/18	Kg		homashyo	{"uom": "Kg", "code": "opp 635/18 kar", "name": "OPP 635/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.885849+05
opp 640/20 pff spp	OPP 640/20	Kg		homashyo	{"uom": "Kg", "code": "opp 640/20 pff spp", "name": "OPP 640/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.885991+05
opp 640/25 kar 3	OPP 640/25	Kg		homashyo	{"uom": "Kg", "code": "opp 640/25 kar 3", "name": "OPP 640/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.886271+05
opp 645/25 kar	OPP 645/25	Kg		homashyo	{"uom": "Kg", "code": "opp 645/25 kar", "name": "OPP 645/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.886822+05
opp 655/18 kar	OPP 655/18	Kg		homashyo	{"uom": "Kg", "code": "opp 655/18 kar", "name": "OPP 655/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.887114+05
opp 660/30 pf kar	OPP 660/30	Kg		homashyo	{"uom": "Kg", "code": "opp 660/30 pf kar", "name": "OPP 660/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.887399+05
opp 675/25	OPP 675/25	Kg		homashyo	{"uom": "Kg", "code": "opp 675/25", "name": "OPP 675/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.888882+05
opp 675/30	OPP 675/30	Kg		homashyo	{"uom": "Kg", "code": "opp 675/30", "name": "OPP 675/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.889201+05
opp 680/18	OPP 680/18	Kg		homashyo	{"uom": "Kg", "code": "opp 680/18", "name": "OPP 680/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.8895+05
opp 680/18 kar	OPP 680/18	Kg		homashyo	{"uom": "Kg", "code": "opp 680/18 kar", "name": "OPP 680/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.889663+05
opp 685/20 kar	OPP 685/20	Kg		homashyo	{"uom": "Kg", "code": "opp 685/20 kar", "name": "OPP 685/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.889949+05
opp 685/25 kar	OPP 685/25	Kg		homashyo	{"uom": "Kg", "code": "opp 685/25 kar", "name": "OPP 685/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.890462+05
opp 695/20 kar	OPP 695/20	Kg		homashyo	{"uom": "Kg", "code": "opp 695/20 kar", "name": "OPP 695/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.890687+05
opp 700/30 PF kar	OPP 700/30	Kg		homashyo	{"uom": "Kg", "code": "opp 700/30 PF kar", "name": "OPP 700/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.890979+05
opp 700/30 kar	OPP 700/30	Kg		homashyo	{"uom": "Kg", "code": "opp 700/30 kar", "name": "OPP 700/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.891117+05
opp 715/25 kar	OPP 715/25	Kg		homashyo	{"uom": "Kg", "code": "opp 715/25 kar", "name": "OPP 715/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.892383+05
opp 715/25 pol	OPP 715/25	Kg		homashyo	{"uom": "Kg", "code": "opp 715/25 pol", "name": "OPP 715/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.89254+05
opp 575/20 kar	OPP 575/20	Kg		homashyo	{"uom": "Kg", "code": "opp 575/20 kar", "name": "OPP 575/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.880231+05
opp 575/25 kar	OPP 575/25	Kg		homashyo	{"uom": "Kg", "code": "opp 575/25 kar", "name": "OPP 575/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.880432+05
opp 715/18 spp	OPP 715/18	Kg		homashyo	{"uom": "Kg", "code": "opp 715/18 spp", "name": "OPP 715/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.892207+05
opp 715/30 pf	OPP 715/30	Kg		homashyo	{"uom": "Kg", "code": "opp 715/30 pf", "name": "OPP 715/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.892876+05
opp 720/18	OPP 720/18	Kg		homashyo	{"uom": "Kg", "code": "opp 720/18", "name": "OPP 720/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.893064+05
opp 720/20 pf	OPP 720/20	Kg		homashyo	{"uom": "Kg", "code": "opp 720/20 pf", "name": "OPP 720/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.893233+05
opp 720/25 pol	OPP 720/25	Kg		homashyo	{"uom": "Kg", "code": "opp 720/25 pol", "name": "OPP 720/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.893394+05
opp 720/30 PFF pol	OPP PFF 720/30	Kg		homashyo	{"uom": "Kg", "code": "opp 720/30 PFF pol", "name": "OPP PFF 720/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.8937+05
opp 720/30 kar	OPP 720/30	Kg		homashyo	{"uom": "Kg", "code": "opp 720/30 kar", "name": "OPP 720/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.8939+05
opp 720/30 pff	OPP PFF 720/30	Kg		homashyo	{"uom": "Kg", "code": "opp 720/30 pff", "name": "OPP PFF 720/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.894208+05
opp 720/30 pol	OPP 720/30	Kg		homashyo	{"uom": "Kg", "code": "opp 720/30 pol", "name": "OPP 720/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.894359+05
opp 725/18	OPP 725/18	Kg		homashyo	{"uom": "Kg", "code": "opp 725/18", "name": "OPP 725/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.894491+05
opp 725/25 pol	OPP 725/25	Kg		homashyo	{"uom": "Kg", "code": "opp 725/25 pol", "name": "OPP 725/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.894794+05
opp 735/18 kar	OPP 735/18	Kg		homashyo	{"uom": "Kg", "code": "opp 735/18 kar", "name": "OPP 735/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.895118+05
opp 740/18	OPP 740/18	Kg		homashyo	{"uom": "Kg", "code": "opp 740/18", "name": "OPP 740/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.895261+05
opp 740/20 kar	OPP 740/20	Kg		homashyo	{"uom": "Kg", "code": "opp 740/20 kar", "name": "OPP 740/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.895402+05
opp 740/30 pf	OPP 740/30	Kg		homashyo	{"uom": "Kg", "code": "opp 740/30 pf", "name": "OPP 740/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.895958+05
opp 745/25 pol	OPP 745/25	Kg		homashyo	{"uom": "Kg", "code": "opp 745/25 pol", "name": "OPP 745/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.896103+05
opp 745/30	OPP 745/30	Kg		homashyo	{"uom": "Kg", "code": "opp 745/30", "name": "OPP 745/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.896241+05
opp 745/30 pf	OPP 745/30	Kg		homashyo	{"uom": "Kg", "code": "opp 745/30 pf", "name": "OPP 745/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.896368+05
opp 750/18 kar	OPP 750/18	Kg		homashyo	{"uom": "Kg", "code": "opp 750/18 kar", "name": "OPP 750/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.89651+05
opp 750/20	OPP 750/20	Kg		homashyo	{"uom": "Kg", "code": "opp 750/20", "name": "OPP 750/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.896653+05
opp 750/20 kar	OPP 750/20	Kg		homashyo	{"uom": "Kg", "code": "opp 750/20 kar", "name": "OPP 750/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.896913+05
opp 765/25 kar	OPP 765/25	Kg		homashyo	{"uom": "Kg", "code": "opp 765/25 kar", "name": "OPP 765/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.897066+05
opp 765/35 kar	OPP 765/35	Kg		homashyo	{"uom": "Kg", "code": "opp 765/35 kar", "name": "OPP 765/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.897669+05
opp 770/18	OPP 770/18	Kg		homashyo	{"uom": "Kg", "code": "opp 770/18", "name": "OPP 770/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.897819+05
opp 770/25 kar	OPP 770/25	Kg		homashyo	{"uom": "Kg", "code": "opp 770/25 kar", "name": "OPP 770/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.897993+05
opp 775/18 kar	OPP 775/18	Kg		homashyo	{"uom": "Kg", "code": "opp 775/18 kar", "name": "OPP 775/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.898142+05
opp 775/30 PF kar	OPP 775/30	Kg		homashyo	{"uom": "Kg", "code": "opp 775/30 PF kar", "name": "OPP 775/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.898303+05
opp 780/20 pol	OPP 780/20	Kg		homashyo	{"uom": "Kg", "code": "opp 780/20 pol", "name": "OPP 780/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.898672+05
opp 790/25	OPP 790/25	Kg		homashyo	{"uom": "Kg", "code": "opp 790/25", "name": "OPP 790/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.899014+05
opp 795/18 pol	OPP 795/18	Kg		homashyo	{"uom": "Kg", "code": "opp 795/18 pol", "name": "OPP 795/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.89932+05
opp 800/18 kar	OPP 800/18	Kg		homashyo	{"uom": "Kg", "code": "opp 800/18 kar", "name": "OPP 800/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.899488+05
opp 800/20 kar	OPP 800/20	Kg		homashyo	{"uom": "Kg", "code": "opp 800/20 kar", "name": "OPP 800/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.899632+05
opp 815/18 kar	OPP 815/18	Kg		homashyo	{"uom": "Kg", "code": "opp 815/18 kar", "name": "OPP 815/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.900077+05
opp 815/25 kar	OPP 815/25	Kg		homashyo	{"uom": "Kg", "code": "opp 815/25 kar", "name": "OPP 815/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.900526+05
opp 820/18 kar	OPP 820/18	Kg		homashyo	{"uom": "Kg", "code": "opp 820/18 kar", "name": "OPP 820/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.900828+05
opp 820/30 pol	OPP 820/30	Kg		homashyo	{"uom": "Kg", "code": "opp 820/30 pol", "name": "OPP 820/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.900985+05
opp 830/18	OPP 830/18	Kg		homashyo	{"uom": "Kg", "code": "opp 830/18", "name": "OPP 830/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.90144+05
opp 715/18 kar	OPP 715/18	Kg		homashyo	{"uom": "Kg", "code": "opp 715/18 kar", "name": "OPP 715/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.891853+05
opp 715/18 pol	OPP 715/18	Kg		homashyo	{"uom": "Kg", "code": "opp 715/18 pol", "name": "OPP 715/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.892061+05
opp 830/35 pol	OPP 830/35	Kg		homashyo	{"uom": "Kg", "code": "opp 830/35 pol", "name": "OPP 830/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.902328+05
opp 840/18 pff kar	OPP PFF 840/18	Kg		homashyo	{"uom": "Kg", "code": "opp 840/18 pff kar", "name": "OPP PFF 840/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.902475+05
opp 840/20 kar	OPP 840/20	Kg		homashyo	{"uom": "Kg", "code": "opp 840/20 kar", "name": "OPP 840/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.902693+05
opp 845/30 pff kar	OPP PFF 845/30	Kg		homashyo	{"uom": "Kg", "code": "opp 845/30 pff kar", "name": "OPP PFF 845/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.903463+05
opp 850/30	OPP 850/30	Kg		homashyo	{"uom": "Kg", "code": "opp 850/30", "name": "OPP 850/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.903751+05
opp 855/18 kar	OPP 855/18	Kg		homashyo	{"uom": "Kg", "code": "opp 855/18 kar", "name": "OPP 855/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.904115+05
opp 855/18 pol	OPP 855/18	Kg		homashyo	{"uom": "Kg", "code": "opp 855/18 pol", "name": "OPP 855/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.904259+05
opp 855/45 kar	OPP 855/45	Kg		homashyo	{"uom": "Kg", "code": "opp 855/45 kar", "name": "OPP 855/45", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.904409+05
opp 865/30 kar	OPP 865/30	Kg		homashyo	{"uom": "Kg", "code": "opp 865/30 kar", "name": "OPP 865/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.904708+05
opp 885/18 kar	OPP 885/18	Kg		homashyo	{"uom": "Kg", "code": "opp 885/18 kar", "name": "OPP 885/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.905153+05
opp 895/35pff kor	OPP 895/35pff kor	Kg		homashyo	{"uom": "Kg", "code": "opp 895/35pff kor", "name": "OPP 895/35pff kor", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.905775+05
opp 915/18	OPP 915/18	Kg		homashyo	{"uom": "Kg", "code": "opp 915/18", "name": "OPP 915/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.906048+05
opp 915/18 kar	OPP 915/18	Kg		homashyo	{"uom": "Kg", "code": "opp 915/18 kar", "name": "OPP 915/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.906191+05
opp 940/30 kar	OPP 940/30	Kg		homashyo	{"uom": "Kg", "code": "opp 940/30 kar", "name": "OPP 940/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.906492+05
opp 945/30 PFF kar	OPP PFF 945/30	Kg		homashyo	{"uom": "Kg", "code": "opp 945/30 PFF kar", "name": "OPP PFF 945/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.906644+05
opp 950/18 kar	OPP 950/18	Kg		homashyo	{"uom": "Kg", "code": "opp 950/18 kar", "name": "OPP 950/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.906785+05
opp 960/30 kar	OPP 960/30	Kg		homashyo	{"uom": "Kg", "code": "opp 960/30 kar", "name": "OPP 960/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.906924+05
opp 970/18	OPP 970/18	Kg		homashyo	{"uom": "Kg", "code": "opp 970/18", "name": "OPP 970/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.907067+05
opp 975/30 pf	OPP 975/30	Kg		homashyo	{"uom": "Kg", "code": "opp 975/30 pf", "name": "OPP 975/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.907346+05
opp 980/30	OPP 980/30	Kg		homashyo	{"uom": "Kg", "code": "opp 980/30", "name": "OPP 980/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.907484+05
opp 980/30 pf	OPP 980/30	Kg		homashyo	{"uom": "Kg", "code": "opp 980/30 pf", "name": "OPP 980/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.907623+05
opp 990/18 kar	OPP 990/18	Kg		homashyo	{"uom": "Kg", "code": "opp 990/18 kar", "name": "OPP 990/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.907788+05
opp Pol 725/20	OPP Pol 725/20	Kg		homashyo	{"uom": "Kg", "code": "opp Pol 725/20", "name": "OPP Pol 725/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.907932+05
opp475/20 pol	OPP 475/20	Kg		homashyo	{"uom": "Kg", "code": "opp475/20 pol", "name": "OPP 475/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.910185+05
opp720/20 pff pol	OPP PFF 720/20	Kg		homashyo	{"uom": "Kg", "code": "opp720/20 pff pol", "name": "OPP PFF 720/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.910333+05
opp720/20 pff spp	OPP 720/20	Kg		homashyo	{"uom": "Kg", "code": "opp720/20 pff spp", "name": "OPP 720/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.910486+05
oppm 1020/20	OPPM 1020/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 1020/20", "name": "OPPM 1020/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.910624+05
oppm 1020/20 pol	OPPM 1020/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 1020/20 pol", "name": "OPPM 1020/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.910907+05
oppm 1020/25	OPPM 1020/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 1020/25", "name": "OPPM 1020/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.911077+05
oppm 1040/20	OPPM 1040/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 1040/20", "name": "OPPM 1040/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.911223+05
oppm 325/20 pol	OPPM 325/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 325/20 pol", "name": "OPPM 325/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.911523+05
oppm 330/20	OPPM 330/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 330/20", "name": "OPPM 330/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.911698+05
oppm 332/20	OPPM 332/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 332/20", "name": "OPPM 332/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.912023+05
oppm 380/20	OPPM 380/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 380/20", "name": "OPPM 380/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.913281+05
oppm 475/25	OPPM 475/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 475/25", "name": "OPPM 475/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.914307+05
oppm 485/25 kar	OPPM 485/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 485/25 kar", "name": "OPPM 485/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.914671+05
opp 830/30 kar	OPP 830/30	Kg		homashyo	{"uom": "Kg", "code": "opp 830/30 kar", "name": "OPP 830/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.902023+05
opp 830/30 pf	OPP 830/30	Kg		homashyo	{"uom": "Kg", "code": "opp 830/30 pf", "name": "OPP 830/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.902178+05
oppm 400/20 pol	OPPM 400/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 400/20 pol", "name": "OPPM 400/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.913632+05
oppm 445/20	OPPM 445/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 445/20", "name": "OPPM 445/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.913807+05
oppm 465/20	OPPM 465/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 465/20", "name": "OPPM 465/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.913973+05
oppm 495/20 pol	OPPM 495/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 495/20 pol", "name": "OPPM 495/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.914814+05
oppm 510/20	OPPM 510/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 510/20", "name": "OPPM 510/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.914972+05
oppm 535/20	OPPM 535/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 535/20", "name": "OPPM 535/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.91513+05
oppm 535/25 kar	OPPM 535/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 535/25 kar", "name": "OPPM 535/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.915288+05
oppm 565/25 jar	OPPM 565/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 565/25 jar", "name": "OPPM 565/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.915446+05
oppm 570/25 pol	OPPM 570/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 570/25 pol", "name": "OPPM 570/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.915626+05
oppm 595/20	OPPM 595/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 595/20", "name": "OPPM 595/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.915779+05
oppm 615/20	OPPM 615/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 615/20", "name": "OPPM 615/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.915964+05
oppm 615/25 kar	OPPM 615/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 615/25 kar", "name": "OPPM 615/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.916115+05
oppm 615/25 pol	OPPM 615/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 615/25 pol", "name": "OPPM 615/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.916261+05
oppm 620/20	OPPM 620/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 620/20", "name": "OPPM 620/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.916421+05
oppm 645/20	OPPM 645/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 645/20", "name": "OPPM 645/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.916752+05
oppm 650/20	OPPM 650/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 650/20", "name": "OPPM 650/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.916907+05
oppm 655/20	OPPM 655/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 655/20", "name": "OPPM 655/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.917067+05
oppm 660/20 pol	OPPM 660/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 660/20 pol", "name": "OPPM 660/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.917709+05
oppm 665/20 kar	OPPM 665/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 665/20 kar", "name": "OPPM 665/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.917871+05
oppm 665/20 pol	OPPM 665/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 665/20 pol", "name": "OPPM 665/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.918021+05
oppm 665/25 kar	OPPM 665/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 665/25 kar", "name": "OPPM 665/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.91817+05
oppm 670/20	OPPM 670/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 670/20", "name": "OPPM 670/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.918325+05
oppm 690/20	OPPM 690/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 690/20", "name": "OPPM 690/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.919248+05
oppm 705/25	OPPM 705/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 705/25", "name": "OPPM 705/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.919557+05
oppm 715/20	OPPM 715/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 715/20", "name": "OPPM 715/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.919871+05
oppm 780/18 pol	OPPM 780/18	Kg		homashyo	{"uom": "Kg", "code": "oppm 780/18 pol", "name": "OPPM 780/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.920977+05
oppm 815/20	OPPM 815/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 815/20", "name": "OPPM 815/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.921829+05
oppm 820/20	OPPM 820/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 820/20", "name": "OPPM 820/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.92197+05
oppm 820/25 kar	OPPM 820/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 820/25 kar", "name": "OPPM 820/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.922131+05
oppm 820/25 pol	OPPM 820/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 820/25 pol", "name": "OPPM 820/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.92228+05
oppm 830/25	OPPM 830/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 830/25", "name": "OPPM 830/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.922484+05
oppm 835/20	OPPM 835/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 835/20", "name": "OPPM 835/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.92264+05
oppm 855/30	OPPM 855/30	Kg		homashyo	{"uom": "Kg", "code": "oppm 855/30", "name": "OPPM 855/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.922804+05
oppm 875/25	OPPM 875/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 875/25", "name": "OPPM 875/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.923228+05
oppm 875/30 pol	OPPM 875/30	Kg		homashyo	{"uom": "Kg", "code": "oppm 875/30 pol", "name": "OPPM 875/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.923394+05
oppm 880/20 pol	OPPM 880/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 880/20 pol", "name": "OPPM 880/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.923554+05
oppm 885/30	OPPM 885/30	Kg		homashyo	{"uom": "Kg", "code": "oppm 885/30", "name": "OPPM 885/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.923734+05
oppm 905/20 pol	OPPM 905/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 905/20 pol", "name": "OPPM 905/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.924027+05
oppm 375/20 kar	OPPM 375/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 375/20 kar", "name": "OPPM 375/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.913051+05
oppm 397/30	OPPM 397/30	Kg		homashyo	{"uom": "Kg", "code": "oppm 397/30", "name": "OPPM 397/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.913461+05
oppm 775/20	OPPM 775/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 775/20", "name": "OPPM 775/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.920789+05
oppm 790/25	OPPM 790/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 790/25", "name": "OPPM 790/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.921151+05
oppm 795/25 kar	OPPM 795/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 795/25 kar", "name": "OPPM 795/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.921346+05
oppm 935/30	OPPM 935/30	Kg		homashyo	{"uom": "Kg", "code": "oppm 935/30", "name": "OPPM 935/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.924198+05
oppm 940/20	OPPM 940/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 940/20", "name": "OPPM 940/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.924357+05
oppm 945/20	OPPM 945/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 945/20", "name": "OPPM 945/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.924527+05
oppm 963/20	OPPM 963/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 963/20", "name": "OPPM 963/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.924719+05
oppm 970/25 kar	OPPM 970/25	Kg		homashyo	{"uom": "Kg", "code": "oppm 970/25 kar", "name": "OPPM 970/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.924873+05
oppm 975/20	OPPM 975/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 975/20", "name": "OPPM 975/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.92503+05
oppm 990/20	OPPM 990/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 990/20", "name": "OPPM 990/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.925314+05
pe pr 510/6p	PE pr 510/6p	Kg		homashyo	{"uom": "Kg", "code": "pe pr 510/6p", "name": "PE pr 510/6p", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.936453+05
pet 240/12 pol	PET 240/12	Kg		homashyo	{"uom": "Kg", "code": "pet 240/12 pol", "name": "PET 240/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.940251+05
pet 300/12	PET 300/12	Kg		homashyo	{"uom": "Kg", "code": "pet 300/12", "name": "PET 300/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.940395+05
pet 410/12	PET 410/12	Kg		homashyo	{"uom": "Kg", "code": "pet 410/12", "name": "PET 410/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.941149+05
pet 490/12	PET 490/12	Kg		homashyo	{"uom": "Kg", "code": "pet 490/12", "name": "PET 490/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.941599+05
pet 510/12	PET 510/12	Kg		homashyo	{"uom": "Kg", "code": "pet 510/12", "name": "PET 510/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.941903+05
pet 510/12 pol	PET 510/12	Kg		homashyo	{"uom": "Kg", "code": "pet 510/12 pol", "name": "PET 510/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.942044+05
pet 535/12 pol	PET 535/12	Kg		homashyo	{"uom": "Kg", "code": "pet 535/12 pol", "name": "PET 535/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.942517+05
pet 540/12	PET 540/12	Kg		homashyo	{"uom": "Kg", "code": "pet 540/12", "name": "PET 540/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.942686+05
pet 555/12 pol	PET 555/12	Kg		homashyo	{"uom": "Kg", "code": "pet 555/12 pol", "name": "PET 555/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.94286+05
pet 585/12 pol	PET 585/12	Kg		homashyo	{"uom": "Kg", "code": "pet 585/12 pol", "name": "PET 585/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.943372+05
pet 590/12	PET 590/12	Kg		homashyo	{"uom": "Kg", "code": "pet 590/12", "name": "PET 590/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.943528+05
pet 660/12	PET 660/12	Kg		homashyo	{"uom": "Kg", "code": "pet 660/12", "name": "PET 660/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.94496+05
pet 660/12 kar	PET 660/12	Kg		homashyo	{"uom": "Kg", "code": "pet 660/12 kar", "name": "PET 660/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.945118+05
pet 665/12 pol	PET 665/12	Kg		homashyo	{"uom": "Kg", "code": "pet 665/12 pol", "name": "PET 665/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.945274+05
pet 675/12 pol	PET 675/12	Kg		homashyo	{"uom": "Kg", "code": "pet 675/12 pol", "name": "PET 675/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.945751+05
pet 680/12	PET 680/12	Kg		homashyo	{"uom": "Kg", "code": "pet 680/12", "name": "PET 680/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.945899+05
pet 680/12 pol	PET 680/12	Kg		homashyo	{"uom": "Kg", "code": "pet 680/12 pol", "name": "PET 680/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.946061+05
pet 700/12 pol	PET 700/12	Kg		homashyo	{"uom": "Kg", "code": "pet 700/12 pol", "name": "PET 700/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.946223+05
pet 705/12	PET 705/12	Kg		homashyo	{"uom": "Kg", "code": "pet 705/12", "name": "PET 705/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.946388+05
pet 718/12	PET 718/12	Kg		homashyo	{"uom": "Kg", "code": "pet 718/12", "name": "PET 718/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.946711+05
pet 718/12 pol	PET 718/12	Kg		homashyo	{"uom": "Kg", "code": "pet 718/12 pol", "name": "PET 718/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.94687+05
pet 725/12	PET 725/12	Kg		homashyo	{"uom": "Kg", "code": "pet 725/12", "name": "PET 725/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.947021+05
pet 730/12	PET 730/12	Kg		homashyo	{"uom": "Kg", "code": "pet 730/12", "name": "PET 730/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.947178+05
pet 735/12 pol	PET 735/12	Kg		homashyo	{"uom": "Kg", "code": "pet 735/12 pol", "name": "PET 735/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.94733+05
pet 740/12	PET 740/12	Kg		homashyo	{"uom": "Kg", "code": "pet 740/12", "name": "PET 740/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.94748+05
pet 755/12 pol	PET 755/12	Kg		homashyo	{"uom": "Kg", "code": "pet 755/12 pol", "name": "PET 755/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.947819+05
pet 760/12 pol	PET 760/12	Kg		homashyo	{"uom": "Kg", "code": "pet 760/12 pol", "name": "PET 760/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.947973+05
oppm 745/20	OPPM 745/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 745/20", "name": "OPPM 745/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.920344+05
oppm 765/18 pol	OPPM 765/18	Kg		homashyo	{"uom": "Kg", "code": "oppm 765/18 pol", "name": "OPPM 765/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.920512+05
mat 435/20 kar	MAT 435/20	Kg		homashyo	{"uom": "Kg", "code": "mat 435/20 kar", "name": "MAT 435/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.803921+05
opp 1600/35	OPP 1600/35	Kg		homashyo	{"uom": "Kg", "code": "opp 1600/35", "name": "OPP 1600/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.859159+05
opp 695/20 pf kar	OPP 695/20	Kg		homashyo	{"uom": "Kg", "code": "opp 695/20 pf kar", "name": "OPP 695/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.890821+05
opp 790/30	OPP 790/30	Kg		homashyo	{"uom": "Kg", "code": "opp 790/30", "name": "OPP 790/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.899158+05
opp 940/20 PFF kar	OPP PFF 940/20	Kg		homashyo	{"uom": "Kg", "code": "opp 940/20 PFF kar", "name": "OPP PFF 940/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.90635+05
oppm 370/20	OPPM 370/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 370/20", "name": "OPPM 370/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.91271+05
oppm 655/20 pol	OPPM 655/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 655/20 pol", "name": "OPPM 655/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.917344+05
pet 365/12	PET 365/12	Kg		homashyo	{"uom": "Kg", "code": "pet 365/12", "name": "PET 365/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.940542+05
pet 375/12 pol	PET 375/12	Kg		homashyo	{"uom": "Kg", "code": "pet 375/12 pol", "name": "PET 375/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.940971+05
pet 460/12	PET 460/12	Kg		homashyo	{"uom": "Kg", "code": "pet 460/12", "name": "PET 460/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.941302+05
pet 460/12 pol	PET 460/12	Kg		homashyo	{"uom": "Kg", "code": "pet 460/12 pol", "name": "PET 460/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.941452+05
pet 500/12 pol	PET 500/12	Kg		homashyo	{"uom": "Kg", "code": "pet 500/12 pol", "name": "PET 500/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.941753+05
pet 775/12 pol	PET 775/12	Kg		homashyo	{"uom": "Kg", "code": "pet 775/12 pol", "name": "PET 775/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.948121+05
pet 7790/12	PET 7790/12	Kg		homashyo	{"uom": "Kg", "code": "pet 7790/12", "name": "PET 7790/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.94826+05
pet 790/12 pol	PET 790/12	Kg		homashyo	{"uom": "Kg", "code": "pet 790/12 pol", "name": "PET 790/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.9484+05
pet 795/12 pol	PET 795/12	Kg		homashyo	{"uom": "Kg", "code": "pet 795/12 pol", "name": "PET 795/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.948568+05
pet 800/12 pol	PET 800/12	Kg		homashyo	{"uom": "Kg", "code": "pet 800/12 pol", "name": "PET 800/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.9487+05
pet 805/12 pol	PET 805/12	Kg		homashyo	{"uom": "Kg", "code": "pet 805/12 pol", "name": "PET 805/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.948872+05
pet 860/12 pol	PET 860/12	Kg		homashyo	{"uom": "Kg", "code": "pet 860/12 pol", "name": "PET 860/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.949016+05
pet 875/12	PET 875/12	Kg		homashyo	{"uom": "Kg", "code": "pet 875/12", "name": "PET 875/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.949174+05
pet 940/12 pol	PET 940/12	Kg		homashyo	{"uom": "Kg", "code": "pet 940/12 pol", "name": "PET 940/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.949462+05
pet 980/12	PET 980/12	Kg		homashyo	{"uom": "Kg", "code": "pet 980/12", "name": "PET 980/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.949609+05
st01 1100/30	ST01 1100/30	Kg		homashyo	{"uom": "Kg", "code": "st01 1100/30", "name": "ST01 1100/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.004954+05
st01 1155/25	ST01 1155/25	Kg		homashyo	{"uom": "Kg", "code": "st01 1155/25", "name": "ST01 1155/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.005107+05
st01 1280/25	ST01 1280/25	Kg		homashyo	{"uom": "Kg", "code": "st01 1280/25", "name": "ST01 1280/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.005713+05
st01 1280/30	ST01 1280/30	Kg		homashyo	{"uom": "Kg", "code": "st01 1280/30", "name": "ST01 1280/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.005886+05
st01 1360/20	ST01 1360/20	Kg		homashyo	{"uom": "Kg", "code": "st01 1360/20", "name": "ST01 1360/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.006065+05
st01 1400/25	ST01 1400/25	Kg		homashyo	{"uom": "Kg", "code": "st01 1400/25", "name": "ST01 1400/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.006232+05
st01 1410/25	ST01 1410/25	Kg		homashyo	{"uom": "Kg", "code": "st01 1410/25", "name": "ST01 1410/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.006408+05
st01 1479/20	ST01 1479/20	Kg		homashyo	{"uom": "Kg", "code": "st01 1479/20", "name": "ST01 1479/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.006567+05
st01 445/30	ST01 445/30	Kg		homashyo	{"uom": "Kg", "code": "st01 445/30", "name": "ST01 445/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.00671+05
st01 615/25	ST01 615/25	Kg		homashyo	{"uom": "Kg", "code": "st01 615/25", "name": "ST01 615/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.006871+05
st01 630/20	ST01 630/20	Kg		homashyo	{"uom": "Kg", "code": "st01 630/20", "name": "ST01 630/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.00704+05
st01 710/25	ST01 710/25	Kg		homashyo	{"uom": "Kg", "code": "st01 710/25", "name": "ST01 710/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.007193+05
st01 715/25	ST01 715/25	Kg		homashyo	{"uom": "Kg", "code": "st01 715/25", "name": "ST01 715/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.007338+05
st01 895/25	ST01 895/25	Kg		homashyo	{"uom": "Kg", "code": "st01 895/25", "name": "ST01 895/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.007503+05
st01 945/30	ST01 945/30	Kg		homashyo	{"uom": "Kg", "code": "st01 945/30", "name": "ST01 945/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.007684+05
st01 990/20	ST01 990/20	Kg		homashyo	{"uom": "Kg", "code": "st01 990/20", "name": "ST01 990/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.007874+05
jem 465/35 pol	JEM 465/35	Kg		homashyo	{"uom": "Kg", "code": "jem 465/35 pol", "name": "JEM 465/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.761225+05
jem 505/25	JEM 505/25	Kg		homashyo	{"uom": "Kg", "code": "jem 505/25", "name": "JEM 505/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.762329+05
OPP 655/18	OPP 655/18	Kg		homashyo	{"uom": "Kg", "code": "OPP 655/18", "name": "OPP 655/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.589894+05
OPP 715/18	OPP 715/18	Kg		homashyo	{"uom": "Kg", "code": "OPP 715/18", "name": "OPP 715/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.590226+05
jem 410/25 pol	JEM 410/25	Kg		homashyo	{"uom": "Kg", "code": "jem 410/25 pol", "name": "JEM 410/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.759786+05
jem 430/25	JEM 430/25	Kg		homashyo	{"uom": "Kg", "code": "jem 430/25", "name": "JEM 430/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.759974+05
jem 465/25 pol	JEM 465/25	Kg		homashyo	{"uom": "Kg", "code": "jem 465/25 pol", "name": "JEM 465/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.761085+05
jem 495/25 kar	JEM 495/25	Kg		homashyo	{"uom": "Kg", "code": "jem 495/25 kar", "name": "JEM 495/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.761646+05
jem 500/20 kar	JEM 500/20	Kg		homashyo	{"uom": "Kg", "code": "jem 500/20 kar", "name": "JEM 500/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.761802+05
jem 500/25 pol	JEM 500/25	Kg		homashyo	{"uom": "Kg", "code": "jem 500/25 pol", "name": "JEM 500/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.761948+05
jem 510/30 kar	JEM 510/30	Kg		homashyo	{"uom": "Kg", "code": "jem 510/30 kar", "name": "JEM 510/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.762488+05
jem 510/35	JEM 510/35	Kg		homashyo	{"uom": "Kg", "code": "jem 510/35", "name": "JEM 510/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.762652+05
jem 515/25 kar	JEM 515/25	Kg		homashyo	{"uom": "Kg", "code": "jem 515/25 kar", "name": "JEM 515/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.762826+05
jem 520/25 pol	JEM 520/25	Kg		homashyo	{"uom": "Kg", "code": "jem 520/25 pol", "name": "JEM 520/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.763+05
jem 530/25 pol	JEM 530/25	Kg		homashyo	{"uom": "Kg", "code": "jem 530/25 pol", "name": "JEM 530/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.763516+05
jem 545/25 kar	JEM 545/25	Kg		homashyo	{"uom": "Kg", "code": "jem 545/25 kar", "name": "JEM 545/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.763826+05
jem 545/35	JEM 545/35	Kg		homashyo	{"uom": "Kg", "code": "jem 545/35", "name": "JEM 545/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.763967+05
jem 615/20 kar	JEM 615/20	Kg		homashyo	{"uom": "Kg", "code": "jem 615/20 kar", "name": "JEM 615/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.764885+05
jem 620/30 kar	JEM 620/30	Kg		homashyo	{"uom": "Kg", "code": "jem 620/30 kar", "name": "JEM 620/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.765232+05
jem 675/30 kar	JEM 675/30	Kg		homashyo	{"uom": "Kg", "code": "jem 675/30 kar", "name": "JEM 675/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.765938+05
jem 720/25 pol	JEM 720/25	Kg		homashyo	{"uom": "Kg", "code": "jem 720/25 pol", "name": "JEM 720/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.767353+05
jem 780/40 kar	JEM 780/40	Kg		homashyo	{"uom": "Kg", "code": "jem 780/40 kar", "name": "JEM 780/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.769001+05
jem 790/25 kar	JEM 790/25	Kg		homashyo	{"uom": "Kg", "code": "jem 790/25 kar", "name": "JEM 790/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.76945+05
jem 840/38 LOBO	JEM 840/38	Kg		homashyo	{"uom": "Kg", "code": "jem 840/38 LOBO", "name": "JEM 840/38", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.769949+05
jem 855/45 kar	JEM 855/45	Kg		homashyo	{"uom": "Kg", "code": "jem 855/45 kar", "name": "JEM 855/45", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.770585+05
mat 785/20 pol	MAT 785/20	Kg		homashyo	{"uom": "Kg", "code": "mat 785/20 pol", "name": "MAT 785/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.811884+05
mpet 715/12 kar	MPET 715/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 715/12 kar", "name": "MPET 715/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.83752+05
opp 435/18	OPP 435/18	Kg		homashyo	{"uom": "Kg", "code": "opp 435/18", "name": "OPP 435/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.86563+05
opp 500/20 kar	OPP 500/20	Kg		homashyo	{"uom": "Kg", "code": "opp 500/20 kar", "name": "OPP 500/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.872208+05
opp 505/20 kar	OPP 505/20	Kg		homashyo	{"uom": "Kg", "code": "opp 505/20 kar", "name": "OPP 505/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.872827+05
tvist M 370/23	TVIST M 370/23	Kg		homashyo	{"uom": "Kg", "code": "tvist M 370/23", "name": "TVIST M 370/23", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.024594+05
tvist m 240/23 pol	TVIST m 240/23	Kg		homashyo	{"uom": "Kg", "code": "tvist m 240/23 pol", "name": "TVIST m 240/23", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.024919+05
tvist m 370/23 pol	TVIST m 370/23	Kg		homashyo	{"uom": "Kg", "code": "tvist m 370/23 pol", "name": "TVIST m 370/23", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.025103+05
tvist m 373/23 pol	TVIST m 373/23	Kg		homashyo	{"uom": "Kg", "code": "tvist m 373/23 pol", "name": "TVIST m 373/23", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.025286+05
tvist m 520/23 pol	TVIST m 520/23	Kg		homashyo	{"uom": "Kg", "code": "tvist m 520/23 pol", "name": "TVIST m 520/23", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.02546+05
tvist m 610/23 pol	TVIST m 610/23	Kg		homashyo	{"uom": "Kg", "code": "tvist m 610/23 pol", "name": "TVIST m 610/23", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.025634+05
tvist m 614/23 pol	TVIST m 614/23	Kg		homashyo	{"uom": "Kg", "code": "tvist m 614/23 pol", "name": "TVIST m 614/23", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.025797+05
tvist m 620/23 pol	TVIST m 620/23	Kg		homashyo	{"uom": "Kg", "code": "tvist m 620/23 pol", "name": "TVIST m 620/23", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.02595+05
опп 475/18	опп 475/18	Kg		homashyo	{"uom": "Kg", "code": "опп 475/18", "name": "опп 475/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.056209+05
OPP 600/20 pf	OPP 600/20	Kg		homashyo	{"uom": "Kg", "code": "OPP 600/20 pf", "name": "OPP 600/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.587872+05
OPP 620/35	OPP 620/35	Kg		homashyo	{"uom": "Kg", "code": "OPP 620/35", "name": "OPP 620/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.588874+05
jem 690/25 kar	JEM 690/25	Kg		homashyo	{"uom": "Kg", "code": "jem 690/25 kar", "name": "JEM 690/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.766219+05
jem 693/25 kar	JEM 693/25	Kg		homashyo	{"uom": "Kg", "code": "jem 693/25 kar", "name": "JEM 693/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.766362+05
jem 705/30	JEM 705/30	Kg		homashyo	{"uom": "Kg", "code": "jem 705/30", "name": "JEM 705/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.76691+05
jem 735/35 kar	JEM 735/35	Kg		homashyo	{"uom": "Kg", "code": "jem 735/35 kar", "name": "JEM 735/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.767845+05
jem 745/30 kar	JEM 745/30	Kg		homashyo	{"uom": "Kg", "code": "jem 745/30 kar", "name": "JEM 745/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.768002+05
jem 755/30 kar	JEM 755/30	Kg		homashyo	{"uom": "Kg", "code": "jem 755/30 kar", "name": "JEM 755/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.768165+05
jem 765/25 kar	JEM 765/25	Kg		homashyo	{"uom": "Kg", "code": "jem 765/25 kar", "name": "JEM 765/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.768333+05
jem 765/35 kar	JEM 765/35	Kg		homashyo	{"uom": "Kg", "code": "jem 765/35 kar", "name": "JEM 765/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.768511+05
jem 770/25 kar	JEM 770/25	Kg		homashyo	{"uom": "Kg", "code": "jem 770/25 kar", "name": "JEM 770/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.768665+05
jem 775/35 pol	JEM 775/35	Kg		homashyo	{"uom": "Kg", "code": "jem 775/35 pol", "name": "JEM 775/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.768829+05
jem 785/25 pol	JEM 785/25	Kg		homashyo	{"uom": "Kg", "code": "jem 785/25 pol", "name": "JEM 785/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.769156+05
jem 790/20 kar	JEM 790/20	Kg		homashyo	{"uom": "Kg", "code": "jem 790/20 kar", "name": "JEM 790/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.76931+05
jem 830/30 kar	JEM 830/30	Kg		homashyo	{"uom": "Kg", "code": "jem 830/30 kar", "name": "JEM 830/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.769783+05
jem 855/30 kar	JEM 855/30	Kg		homashyo	{"uom": "Kg", "code": "jem 855/30 kar", "name": "JEM 855/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.770223+05
jem 855/35 kar	JEM 855/35	Kg		homashyo	{"uom": "Kg", "code": "jem 855/35 kar", "name": "JEM 855/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.770415+05
jem 960/30 kar	JEM 960/30	Kg		homashyo	{"uom": "Kg", "code": "jem 960/30 kar", "name": "JEM 960/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.770869+05
mpet 365/12 pol	MPET 365/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 365/12 pol", "name": "MPET 365/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.834979+05
mpet 425/12 pol	MPET 425/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 425/12 pol", "name": "MPET 425/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.835125+05
mpet 430/12	MPET 430/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 430/12", "name": "MPET 430/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.835278+05
mpet 430/12 pol	MPET 430/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 430/12 pol", "name": "MPET 430/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.83544+05
mpet 435/12 pol	MPET 435/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 435/12 pol", "name": "MPET 435/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.835613+05
mpet 460/12 pol	MPET 460/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 460/12 pol", "name": "MPET 460/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.835762+05
mpet 470/12	MPET 470/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 470/12", "name": "MPET 470/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.835906+05
mpet 470/12 pol	MPET 470/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 470/12 pol", "name": "MPET 470/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.836045+05
mpet 510/12 pol	MPET 510/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 510/12 pol", "name": "MPET 510/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.836182+05
mpet 565/12	MPET 565/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 565/12", "name": "MPET 565/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.836324+05
mpet 580/12	MPET 580/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 580/12", "name": "MPET 580/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.836466+05
mpet 610/12	MPET 610/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 610/12", "name": "MPET 610/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.836624+05
mpet 615/12	MPET 615/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 615/12", "name": "MPET 615/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.836773+05
mpet 615/12 pol	MPET 615/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 615/12 pol", "name": "MPET 615/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.836929+05
mpet 625/12 pol	MPET 625/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 625/12 pol", "name": "MPET 625/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.837086+05
mpet 650/12 pol	MPET 650/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 650/12 pol", "name": "MPET 650/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.837217+05
opp 565/30 kar	OPP 565/30	Kg		homashyo	{"uom": "Kg", "code": "opp 565/30 kar", "name": "OPP 565/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.879617+05
opp 640/25 kar	OPP 640/25	Kg		homashyo	{"uom": "Kg", "code": "opp 640/25 kar", "name": "OPP 640/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.886132+05
opp 715/25 spp	OPP 715/25	Kg		homashyo	{"uom": "Kg", "code": "opp 715/25 spp", "name": "OPP 715/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.89269+05
opp 725/20 polietilen	OPP 725/20	Kg		homashyo	{"uom": "Kg", "code": "opp 725/20 polietilen", "name": "OPP 725/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.89465+05
oppm 705/30	OPPM 705/30	Kg		homashyo	{"uom": "Kg", "code": "oppm 705/30", "name": "OPPM 705/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.919709+05
pet 670/12 pol	PET 670/12	Kg		homashyo	{"uom": "Kg", "code": "pet 670/12 pol", "name": "PET 670/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.94559+05
OPP 775/20	OPP 775/20	Kg		homashyo	{"uom": "Kg", "code": "OPP 775/20", "name": "OPP 775/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.590442+05
jem 1280/25	JEM 1280/25	Kg		homashyo	{"uom": "Kg", "code": "jem 1280/25", "name": "JEM 1280/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.758151+05
680/50 pe pr oq	PE PR 680/50	Kg		homashyo	{"uom": "Kg", "code": "680/50 pe pr oq", "name": "PE PR 680/50", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.460247+05
740/50 pe pr oq	PE PR 740/50	Kg		homashyo	{"uom": "Kg", "code": "740/50 pe pr oq", "name": "PE PR 740/50", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.46342+05
800/60 pe pr toza	PE PR 800/60	Kg		homashyo	{"uom": "Kg", "code": "800/60 pe pr toza", "name": "PE PR 800/60", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.466771+05
805/40 pe pr oq	PE PR 805/40	Kg		homashyo	{"uom": "Kg", "code": "805/40 pe pr oq", "name": "PE PR 805/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.467361+05
820/60 pe pr toza	PE PR 820/60	Kg		homashyo	{"uom": "Kg", "code": "820/60 pe pr toza", "name": "PE PR 820/60", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.468292+05
980/85 pe pr toza	PE PR 980/85	Kg		homashyo	{"uom": "Kg", "code": "980/85 pe pr toza", "name": "PE PR 980/85", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.470274+05
995/45 pe pr toza	PE PR 995/45	Kg		homashyo	{"uom": "Kg", "code": "995/45 pe pr toza", "name": "PE PR 995/45", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.470909+05
Jem 540/20	JEM 540/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 540/20", "name": "JEM 540/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.541939+05
Jem 585/18	JEM 585/18	Kg		homashyo	{"uom": "Kg", "code": "Jem 585/18", "name": "JEM 585/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.543027+05
Jem 750/18	JEM 750/18	Kg		homashyo	{"uom": "Kg", "code": "Jem 750/18", "name": "JEM 750/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.54435+05
Jem 780/20	JEM 780/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 780/20", "name": "JEM 780/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.544563+05
Jem 895/20	JEM 895/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 895/20", "name": "JEM 895/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.544901+05
OPP 360/35	OPP 360/35	Kg		homashyo	{"uom": "Kg", "code": "OPP 360/35", "name": "OPP 360/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.586276+05
OPP 425/25	OPP 425/25	Kg		homashyo	{"uom": "Kg", "code": "OPP 425/25", "name": "OPP 425/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.58638+05
jem 1230/35 kat	JEM 1230/35	Kg		homashyo	{"uom": "Kg", "code": "jem 1230/35 kat", "name": "JEM 1230/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.757998+05
jem 1300/30 kat	JEM 1300/30	Kg		homashyo	{"uom": "Kg", "code": "jem 1300/30 kat", "name": "JEM 1300/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.758305+05
jem 435/35 pol	JEM 435/35	Kg		homashyo	{"uom": "Kg", "code": "jem 435/35 pol", "name": "JEM 435/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.760269+05
jem 465/25 kar	JEM 465/25	Kg		homashyo	{"uom": "Kg", "code": "jem 465/25 kar", "name": "JEM 465/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.760932+05
jem 490/25 kar	JEM 490/25	Kg		homashyo	{"uom": "Kg", "code": "jem 490/25 kar", "name": "JEM 490/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.761375+05
jem 535/35 kar	JEM 535/35	Kg		homashyo	{"uom": "Kg", "code": "jem 535/35 kar", "name": "JEM 535/35", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.763683+05
jem 700/25 kar	JEM 700/25	Kg		homashyo	{"uom": "Kg", "code": "jem 700/25 kar", "name": "JEM 700/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.766624+05
jem 705/25 kar	JEM 705/25	Kg		homashyo	{"uom": "Kg", "code": "jem 705/25 kar", "name": "JEM 705/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.766765+05
jem 710/25 pol	JEM 710/25	Kg		homashyo	{"uom": "Kg", "code": "jem 710/25 pol", "name": "JEM 710/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.767055+05
jem 720/25 kar	JEM 720/25	Kg		homashyo	{"uom": "Kg", "code": "jem 720/25 kar", "name": "JEM 720/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.767212+05
mat 675/20 kar 3	MAT 675/20	Kg		homashyo	{"uom": "Kg", "code": "mat 675/20 kar 3", "name": "MAT 675/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.809397+05
mpet 675/12 pol	MPET 675/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 675/12 pol", "name": "MPET 675/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.837377+05
mpet 800/12	MPET 800/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 800/12", "name": "MPET 800/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.837971+05
mpet 820/12	MPET 820/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 820/12", "name": "MPET 820/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.838272+05
mpet 820/12 pol	MPET 820/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 820/12 pol", "name": "MPET 820/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.838433+05
mpet 825/12 pol	MPET 825/12	Kg		homashyo	{"uom": "Kg", "code": "mpet 825/12 pol", "name": "MPET 825/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.838588+05
opp 430/30 pol	OPP 430/30	Kg		homashyo	{"uom": "Kg", "code": "opp 430/30 pol", "name": "OPP 430/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.865479+05
opp 435/20 kar	OPP 435/20	Kg		homashyo	{"uom": "Kg", "code": "opp 435/20 kar", "name": "OPP 435/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.865912+05
opp 455/20 kar	OPP 455/20	Kg		homashyo	{"uom": "Kg", "code": "opp 455/20 kar", "name": "OPP 455/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.867203+05
opp 465/20 pol	OPP 465/20	Kg		homashyo	{"uom": "Kg", "code": "opp 465/20 pol", "name": "OPP 465/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.868263+05
opp 480/20	OPP 480/20	Kg		homashyo	{"uom": "Kg", "code": "opp 480/20", "name": "OPP 480/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.870279+05
opp 815/30 pf kar	OPP 815/30	Kg		homashyo	{"uom": "Kg", "code": "opp 815/30 pf kar", "name": "OPP 815/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.900682+05
oppm 330/20 kar	OPPM 330/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 330/20 kar", "name": "OPPM 330/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.911858+05
410/40 pe oq	PE OQ 410/40	Kg		homashyo	{"uom": "Kg", "code": "410/40 pe oq", "name": "PE OQ 410/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.445681+05
670/65 pe pr oq	PE PR 670/65	Kg		homashyo	{"uom": "Kg", "code": "670/65 pe pr oq", "name": "PE PR 670/65", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.458249+05
535/100 pe pr toza	PE PR 535/100	Kg		homashyo	{"uom": "Kg", "code": "535/100 pe pr toza", "name": "PE PR 535/100", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.450644+05
570/60 pe pr oq	PE PR 570/60	Kg		homashyo	{"uom": "Kg", "code": "570/60 pe pr oq", "name": "PE PR 570/60", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.452107+05
595/100 pe pr toza	PE PR 595/100	Kg		homashyo	{"uom": "Kg", "code": "595/100 pe pr toza", "name": "PE PR 595/100", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.453853+05
600/55 pe pr oq	PE PR 600/55	Kg		homashyo	{"uom": "Kg", "code": "600/55 pe pr oq", "name": "PE PR 600/55", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.454105+05
620/50 pe pr vakuum	PE PR 620/50	Kg		homashyo	{"uom": "Kg", "code": "620/50 pe pr vakuum", "name": "PE PR 620/50", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.455947+05
650/40 pe pr oq	PE PR 650/40	Kg		homashyo	{"uom": "Kg", "code": "650/40 pe pr oq", "name": "PE PR 650/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.457218+05
690/50 pe pr toza	PE PR 690/50	Kg		homashyo	{"uom": "Kg", "code": "690/50 pe pr toza", "name": "PE PR 690/50", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.461583+05
690/60  pe pr oq	PE PR 690/60	Kg		homashyo	{"uom": "Kg", "code": "690/60  pe pr oq", "name": "PE PR 690/60", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.46189+05
815/30 pe pr toza	PE PR 815/30	Kg		homashyo	{"uom": "Kg", "code": "815/30 pe pr toza", "name": "PE PR 815/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.467638+05
845/80 pe pr oq	PE PR 845/80	Kg		homashyo	{"uom": "Kg", "code": "845/80 pe pr oq", "name": "PE PR 845/80", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.468635+05
990/40 pe pr toza	PE PR 990/40	Kg		homashyo	{"uom": "Kg", "code": "990/40 pe pr toza", "name": "PE PR 990/40", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.470598+05
JEM 695/25	JEM 695/25	Kg		homashyo	{"uom": "Kg", "code": "JEM 695/25", "name": "JEM 695/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.536491+05
Jem 405/20	JEM 405/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 405/20", "name": "JEM 405/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.537009+05
Jem 545/20	JEM 545/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 545/20", "name": "JEM 545/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.542085+05
Jem 755/20	JEM 755/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 755/20", "name": "JEM 755/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.544453+05
Jem 815/20	JEM 815/20	Kg		homashyo	{"uom": "Kg", "code": "Jem 815/20", "name": "JEM 815/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.544678+05
MAT 615/20	MAT 615/20	Kg		homashyo	{"uom": "Kg", "code": "MAT 615/20", "name": "MAT 615/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.564138+05
MCP / 20 mikron / 680	MCP 20/680	Kg		homashyo	{"uom": "Kg", "code": "MCP / 20 mikron / 680", "name": "MCP 20/680", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.564352+05
OPP 480/30 Pol	OPP 480/30	Kg		homashyo	{"uom": "Kg", "code": "OPP 480/30 Pol", "name": "OPP 480/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.586509+05
OPP 535/20 pol	OPP 535/20	Kg		homashyo	{"uom": "Kg", "code": "OPP 535/20 pol", "name": "OPP 535/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.586717+05
OPP 575/20 pol	OPP 575/20	Kg		homashyo	{"uom": "Kg", "code": "OPP 575/20 pol", "name": "OPP 575/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.587419+05
OPP 615/30 pol	OPP 615/30	Kg		homashyo	{"uom": "Kg", "code": "OPP 615/30 pol", "name": "OPP 615/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.588781+05
cpp 585/25	CPP 585/25	Kg		homashyo	{"uom": "Kg", "code": "cpp 585/25", "name": "CPP 585/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.703142+05
jem 1420/25 msp	JEM MSP 1420/25	Kg		homashyo	{"uom": "Kg", "code": "jem 1420/25 msp", "name": "JEM MSP 1420/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.758637+05
jem 440/25 kar	JEM 440/25	Kg		homashyo	{"uom": "Kg", "code": "jem 440/25 kar", "name": "JEM 440/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.760438+05
jem 685/25 kar	JEM 685/25	Kg		homashyo	{"uom": "Kg", "code": "jem 685/25 kar", "name": "JEM 685/25", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.766077+05
mat 555/20 kar	MAT 555/20	Kg		homashyo	{"uom": "Kg", "code": "mat 555/20 kar", "name": "MAT 555/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.806251+05
opp 460/20	OPP 460/20	Kg		homashyo	{"uom": "Kg", "code": "opp 460/20", "name": "OPP 460/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.867331+05
opp 470/20 pol	OPP 470/20	Kg		homashyo	{"uom": "Kg", "code": "opp 470/20 pol", "name": "OPP 470/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.869356+05
opp 475/20 pol	OPP 475/20	Kg		homashyo	{"uom": "Kg", "code": "opp 475/20 pol", "name": "OPP 475/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.869801+05
opp 475/30 pf kar	OPP 475/30	Kg		homashyo	{"uom": "Kg", "code": "opp 475/30 pf kar", "name": "OPP 475/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.869965+05
opp 495/20	OPP 495/20	Kg		homashyo	{"uom": "Kg", "code": "opp 495/20", "name": "OPP 495/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.871883+05
opp 685/20 pff pol	OPP PFF 685/20	Kg		homashyo	{"uom": "Kg", "code": "opp 685/20 pff pol", "name": "OPP PFF 685/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.890102+05
opp 705/18	OPP 705/18	Kg		homashyo	{"uom": "Kg", "code": "opp 705/18", "name": "OPP 705/18", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.891255+05
opp 840/30 pf	OPP 840/30	Kg		homashyo	{"uom": "Kg", "code": "opp 840/30 pf", "name": "OPP 840/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.903324+05
oppm 675/20	OPPM 675/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 675/20", "name": "OPPM 675/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.918807+05
pet 1000/12 pol	PET 1000/12	Kg		homashyo	{"uom": "Kg", "code": "pet 1000/12 pol", "name": "PET 1000/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.939594+05
510/99 pe pr toza	PE PR 510/99	Kg		homashyo	{"uom": "Kg", "code": "510/99 pe pr toza", "name": "PE PR 510/99", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.448291+05
530/30 pe pr toza	PE PR 530/30	Kg		homashyo	{"uom": "Kg", "code": "530/30 pe pr toza", "name": "PE PR 530/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.450165+05
oppm 340/20	OPPM 340/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 340/20", "name": "OPPM 340/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.91218+05
oppm 675/20 pol	OPPM 675/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 675/20 pol", "name": "OPPM 675/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.918953+05
oppm 735/20 kar	OPPM 735/20	Kg		homashyo	{"uom": "Kg", "code": "oppm 735/20 kar", "name": "OPPM 735/20", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.920024+05
pet 365/12 pol	PET 365/12	Kg		homashyo	{"uom": "Kg", "code": "pet 365/12 pol", "name": "PET 365/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.940748+05
pet 640/12 pol	PET 640/12	Kg		homashyo	{"uom": "Kg", "code": "pet 640/12 pol", "name": "PET 640/12", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:55.94463+05
st01 1160/30	ST01 1160/30	Kg		homashyo	{"uom": "Kg", "code": "st01 1160/30", "name": "ST01 1160/30", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.005268+05
tvist M 373/23	TVIST M 373/23	Kg		homashyo	{"uom": "Kg", "code": "tvist M 373/23", "name": "TVIST M 373/23", "warehouse": "", "item_group": "homashyo"}	2026-06-17 09:15:12.147471+05	2026-06-25 17:48:56.024768+05
\.


--
-- Data for Name: mini_order_products; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_order_products (id, order_id, item_code, product_name, material_display, color, first_layer_material, first_layer_micron, second_layer_material, second_layer_micron, third_layer_material, third_layer_micron, note) FROM stdin;
zakaz-2344:product	zakaz-2344	Dide	Dide			pet	12	pe oq	30			
\.


--
-- Data for Name: mini_order_progress_events; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_order_progress_events (id, event_id, session_id, batch_id, apparatus, order_id, action, produced_qty, uom, worker_role, worker_ref, worker_display_name, qr_payload, payload_json, created_at, return_ink_kg, lamination_print_leftover_rolls, lamination_film_leftover_rolls, rezka_bosma_waste, rezka_lamination_waste, rezka_edge_waste, total_waste, finished_goods_kg, finished_goods_meter, description) FROM stdin;
\.


--
-- Data for Name: mini_order_run_sessions; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_order_run_sessions (session_id, apparatus, order_id, status, worker_role, worker_ref, worker_display_name, started_at, updated_at, payload_json) FROM stdin;
\.


--
-- Data for Name: mini_orders; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_orders (id, code, order_number, customer_ref, customer_name, product_code, product_name, status, kg, width_mm, roll_count, created_at, updated_at) FROM stdin;
zakaz-2344	2344	2344	CUST-002	Taze Ay	Dide	Dide	rulon	600.000	765.000	7.000	2026-07-03 11:24:41.14589+05	2026-07-03 11:24:41.14589+05
\.


--
-- Data for Name: mini_production_map_edges; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_production_map_edges (map_id, edge_index, from_node_id, to_node_id, branch, payload_json) FROM stdin;
template-zakaz-2344	0	start	order		{"to": "order", "from": "start"}
template-zakaz-2344	1	order	apparatus_1		{"to": "apparatus_1", "from": "order"}
template-zakaz-2344	2	order	apparatus_2		{"to": "apparatus_2", "from": "order"}
template-zakaz-2344	3	order	apparatus_3		{"to": "apparatus_3", "from": "order"}
template-zakaz-2344	4	apparatus_1	apparatus_4		{"to": "apparatus_4", "from": "apparatus_1"}
template-zakaz-2344	5	apparatus_2	apparatus_4		{"to": "apparatus_4", "from": "apparatus_2"}
template-zakaz-2344	6	apparatus_3	apparatus_4		{"to": "apparatus_4", "from": "apparatus_3"}
template-zakaz-2344	7	apparatus_1	apparatus_5		{"to": "apparatus_5", "from": "apparatus_1"}
template-zakaz-2344	8	apparatus_2	apparatus_5		{"to": "apparatus_5", "from": "apparatus_2"}
template-zakaz-2344	9	apparatus_3	apparatus_5		{"to": "apparatus_5", "from": "apparatus_3"}
template-zakaz-2344	10	apparatus_4	apparatus_6		{"to": "apparatus_6", "from": "apparatus_4"}
template-zakaz-2344	11	apparatus_5	apparatus_6		{"to": "apparatus_6", "from": "apparatus_5"}
template-zakaz-2344	12	apparatus_6	end		{"to": "end", "from": "apparatus_6"}
zakaz-2344	0	start	order		{"to": "order", "from": "start"}
zakaz-2344	1	order	apparatus_1		{"to": "apparatus_1", "from": "order"}
zakaz-2344	2	order	apparatus_2		{"to": "apparatus_2", "from": "order"}
zakaz-2344	3	order	apparatus_3		{"to": "apparatus_3", "from": "order"}
zakaz-2344	4	apparatus_1	apparatus_4		{"to": "apparatus_4", "from": "apparatus_1"}
zakaz-2344	5	apparatus_2	apparatus_4		{"to": "apparatus_4", "from": "apparatus_2"}
zakaz-2344	6	apparatus_3	apparatus_4		{"to": "apparatus_4", "from": "apparatus_3"}
zakaz-2344	7	apparatus_1	apparatus_5		{"to": "apparatus_5", "from": "apparatus_1"}
zakaz-2344	8	apparatus_2	apparatus_5		{"to": "apparatus_5", "from": "apparatus_2"}
zakaz-2344	9	apparatus_3	apparatus_5		{"to": "apparatus_5", "from": "apparatus_3"}
zakaz-2344	10	apparatus_4	apparatus_6		{"to": "apparatus_6", "from": "apparatus_4"}
zakaz-2344	11	apparatus_5	apparatus_6		{"to": "apparatus_6", "from": "apparatus_5"}
zakaz-2344	12	apparatus_6	end		{"to": "end", "from": "apparatus_6"}
\.


--
-- Data for Name: mini_production_map_nodes; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_production_map_nodes (map_id, node_id, kind, title, payload_json) FROM stdin;
template-zakaz-2344	start	start	Start	{"x": 420.0, "y": 32.0, "id": "start", "kind": "start", "title": "Start", "formula": null, "item_code": "", "role_code": ""}
template-zakaz-2344	order	task	Dide	{"x": 420.0, "y": 164.0, "id": "order", "kind": "task", "title": "Dide", "formula": null, "item_code": "", "role_code": "zakaz"}
template-zakaz-2344	apparatus_1	apparatus	7 ta rangli pechat	{"x": 160.0, "y": 296.0, "id": "apparatus_1", "kind": "apparatus", "title": "7 ta rangli pechat", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_bosma aparat_1", "alternative_group_label": "Bosma aparat"}
template-zakaz-2344	apparatus_2	apparatus	8 ta rangli pechat	{"x": 420.0, "y": 296.0, "id": "apparatus_2", "kind": "apparatus", "title": "8 ta rangli pechat", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_bosma aparat_1", "alternative_group_label": "Bosma aparat"}
template-zakaz-2344	apparatus_3	apparatus	9 ta rangli pechat	{"x": 680.0, "y": 296.0, "id": "apparatus_3", "kind": "apparatus", "title": "9 ta rangli pechat", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_bosma aparat_1", "alternative_group_label": "Bosma aparat"}
template-zakaz-2344	apparatus_4	apparatus	Laminatsiya 1	{"x": 290.0, "y": 428.0, "id": "apparatus_4", "kind": "apparatus", "title": "Laminatsiya 1", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_laminatsiya_4", "alternative_group_label": "Laminatsiya"}
template-zakaz-2344	apparatus_5	apparatus	Laminatsiya 2	{"x": 550.0, "y": 428.0, "id": "apparatus_5", "kind": "apparatus", "title": "Laminatsiya 2", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_laminatsiya_4", "alternative_group_label": "Laminatsiya"}
template-zakaz-2344	apparatus_6	apparatus	Rezka	{"x": 420.0, "y": 560.0, "id": "apparatus_6", "kind": "apparatus", "title": "Rezka", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_rezka_6", "alternative_group_label": "Rezka"}
template-zakaz-2344	end	end	Dide	{"x": 420.0, "y": 692.0, "id": "end", "kind": "end", "title": "Dide", "formula": null, "item_code": "Dide", "role_code": ""}
zakaz-2344	start	start	Start	{"x": 420.0, "y": 32.0, "id": "start", "kind": "start", "title": "Start", "formula": null, "item_code": "", "role_code": ""}
zakaz-2344	order	task	Dide	{"x": 420.0, "y": 164.0, "id": "order", "kind": "task", "title": "Dide", "formula": null, "item_code": "", "role_code": "zakaz"}
zakaz-2344	apparatus_1	apparatus	7 ta rangli pechat	{"x": 160.0, "y": 296.0, "id": "apparatus_1", "kind": "apparatus", "title": "7 ta rangli pechat", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_bosma aparat_1", "alternative_group_label": "Bosma aparat", "alternative_assigned_title": "9 ta rangli pechat"}
zakaz-2344	apparatus_2	apparatus	8 ta rangli pechat	{"x": 420.0, "y": 296.0, "id": "apparatus_2", "kind": "apparatus", "title": "8 ta rangli pechat", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_bosma aparat_1", "alternative_group_label": "Bosma aparat", "alternative_assigned_title": "9 ta rangli pechat"}
zakaz-2344	apparatus_3	apparatus	9 ta rangli pechat	{"x": 680.0, "y": 296.0, "id": "apparatus_3", "kind": "apparatus", "title": "9 ta rangli pechat", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_bosma aparat_1", "alternative_group_label": "Bosma aparat", "alternative_assigned_title": "9 ta rangli pechat"}
zakaz-2344	apparatus_4	apparatus	Laminatsiya 1	{"x": 290.0, "y": 428.0, "id": "apparatus_4", "kind": "apparatus", "title": "Laminatsiya 1", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_laminatsiya_4", "alternative_group_label": "Laminatsiya"}
zakaz-2344	apparatus_5	apparatus	Laminatsiya 2	{"x": 550.0, "y": 428.0, "id": "apparatus_5", "kind": "apparatus", "title": "Laminatsiya 2", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_laminatsiya_4", "alternative_group_label": "Laminatsiya"}
zakaz-2344	apparatus_6	apparatus	Rezka	{"x": 420.0, "y": 560.0, "id": "apparatus_6", "kind": "apparatus", "title": "Rezka", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_rezka_6", "alternative_group_label": "Rezka"}
zakaz-2344	end	end	Dide	{"x": 420.0, "y": 692.0, "id": "end", "kind": "end", "title": "Dide", "formula": null, "item_code": "Dide", "role_code": ""}
\.


--
-- Data for Name: mini_production_maps; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_production_maps (id, order_id, product_code, title, code, order_number, roll_count, width_mm, map_json, created_at, updated_at) FROM stdin;
template-zakaz-2344	\N	Dide	Dide			7.000	765.000	{"id": "template-zakaz-2344", "edges": [{"to": "order", "from": "start"}, {"to": "apparatus_1", "from": "order"}, {"to": "apparatus_2", "from": "order"}, {"to": "apparatus_3", "from": "order"}, {"to": "apparatus_4", "from": "apparatus_1"}, {"to": "apparatus_4", "from": "apparatus_2"}, {"to": "apparatus_4", "from": "apparatus_3"}, {"to": "apparatus_5", "from": "apparatus_1"}, {"to": "apparatus_5", "from": "apparatus_2"}, {"to": "apparatus_5", "from": "apparatus_3"}, {"to": "apparatus_6", "from": "apparatus_4"}, {"to": "apparatus_6", "from": "apparatus_5"}, {"to": "end", "from": "apparatus_6"}], "nodes": [{"x": 420.0, "y": 32.0, "id": "start", "kind": "start", "title": "Start", "formula": null, "item_code": "", "role_code": ""}, {"x": 420.0, "y": 164.0, "id": "order", "kind": "task", "title": "Dide", "formula": null, "item_code": "", "role_code": "zakaz"}, {"x": 160.0, "y": 296.0, "id": "apparatus_1", "kind": "apparatus", "title": "7 ta rangli pechat", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_bosma aparat_1", "alternative_group_label": "Bosma aparat"}, {"x": 420.0, "y": 296.0, "id": "apparatus_2", "kind": "apparatus", "title": "8 ta rangli pechat", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_bosma aparat_1", "alternative_group_label": "Bosma aparat"}, {"x": 680.0, "y": 296.0, "id": "apparatus_3", "kind": "apparatus", "title": "9 ta rangli pechat", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_bosma aparat_1", "alternative_group_label": "Bosma aparat"}, {"x": 290.0, "y": 428.0, "id": "apparatus_4", "kind": "apparatus", "title": "Laminatsiya 1", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_laminatsiya_4", "alternative_group_label": "Laminatsiya"}, {"x": 550.0, "y": 428.0, "id": "apparatus_5", "kind": "apparatus", "title": "Laminatsiya 2", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_laminatsiya_4", "alternative_group_label": "Laminatsiya"}, {"x": 420.0, "y": 560.0, "id": "apparatus_6", "kind": "apparatus", "title": "Rezka", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_rezka_6", "alternative_group_label": "Rezka"}, {"x": 420.0, "y": 692.0, "id": "end", "kind": "end", "title": "Dide", "formula": null, "item_code": "Dide", "role_code": ""}], "title": "Dide", "width_mm": 765.0, "roll_count": 7.0, "product_code": "Dide"}	2026-07-03 11:24:41.11604+05	2026-07-03 11:24:41.11604+05
zakaz-2344	\N	Dide	Dide	2344	2344	7.000	765.000	{"id": "zakaz-2344", "code": "2344", "edges": [{"to": "order", "from": "start"}, {"to": "apparatus_1", "from": "order"}, {"to": "apparatus_2", "from": "order"}, {"to": "apparatus_3", "from": "order"}, {"to": "apparatus_4", "from": "apparatus_1"}, {"to": "apparatus_4", "from": "apparatus_2"}, {"to": "apparatus_4", "from": "apparatus_3"}, {"to": "apparatus_5", "from": "apparatus_1"}, {"to": "apparatus_5", "from": "apparatus_2"}, {"to": "apparatus_5", "from": "apparatus_3"}, {"to": "apparatus_6", "from": "apparatus_4"}, {"to": "apparatus_6", "from": "apparatus_5"}, {"to": "end", "from": "apparatus_6"}], "nodes": [{"x": 420.0, "y": 32.0, "id": "start", "kind": "start", "title": "Start", "formula": null, "item_code": "", "role_code": ""}, {"x": 420.0, "y": 164.0, "id": "order", "kind": "task", "title": "Dide", "formula": null, "item_code": "", "role_code": "zakaz"}, {"x": 160.0, "y": 296.0, "id": "apparatus_1", "kind": "apparatus", "title": "7 ta rangli pechat", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_bosma aparat_1", "alternative_group_label": "Bosma aparat", "alternative_assigned_title": "9 ta rangli pechat"}, {"x": 420.0, "y": 296.0, "id": "apparatus_2", "kind": "apparatus", "title": "8 ta rangli pechat", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_bosma aparat_1", "alternative_group_label": "Bosma aparat", "alternative_assigned_title": "9 ta rangli pechat"}, {"x": 680.0, "y": 296.0, "id": "apparatus_3", "kind": "apparatus", "title": "9 ta rangli pechat", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_bosma aparat_1", "alternative_group_label": "Bosma aparat", "alternative_assigned_title": "9 ta rangli pechat"}, {"x": 290.0, "y": 428.0, "id": "apparatus_4", "kind": "apparatus", "title": "Laminatsiya 1", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_laminatsiya_4", "alternative_group_label": "Laminatsiya"}, {"x": 550.0, "y": 428.0, "id": "apparatus_5", "kind": "apparatus", "title": "Laminatsiya 2", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_laminatsiya_4", "alternative_group_label": "Laminatsiya"}, {"x": 420.0, "y": 560.0, "id": "apparatus_6", "kind": "apparatus", "title": "Rezka", "formula": null, "item_code": "", "role_code": "", "alternative_group_id": "alt_rezka_6", "alternative_group_label": "Rezka"}, {"x": 420.0, "y": 692.0, "id": "end", "kind": "end", "title": "Dide", "formula": null, "item_code": "Dide", "role_code": ""}], "title": "Dide", "order_kg": 600.0, "width_mm": 765.0, "roll_count": 7.0, "base_length": 15686.27450980392, "order_number": "2344", "product_code": "Dide"}	2026-07-03 11:24:41.11604+05	2026-07-03 11:26:04.941714+05
\.


--
-- Data for Name: mini_progress_batches; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_progress_batches (batch_id, session_id, apparatus, order_id, action, status, produced_qty, uom, qr_payload, label_item_code, label_item_name, executor_name, worker_role, worker_ref, worker_display_name, payload_json, created_at, updated_at, return_ink_kg, lamination_print_leftover_rolls, lamination_film_leftover_rolls, rezka_bosma_waste, rezka_lamination_waste, rezka_edge_waste, total_waste, finished_goods_kg, finished_goods_meter, description, wip_status, current_apparatus, current_apparatus_key, current_location, next_apparatus, parent_batch_id, used_by_session_id, used_by_apparatus, processed_by_session_id, processed_by_apparatus) FROM stdin;
\.


--
-- Data for Name: mini_push_tokens; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_push_tokens (token, owner_key, platform, updated_at) FROM stdin;
\.


--
-- Data for Name: mini_qolip_cell_qrs; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_qolip_cell_qrs (id, block, warehouse, row_letter, column_number, location_label, qr_payload, created_by_role, created_by_ref, created_by_name, payload_json, created_at, updated_at) FROM stdin;
qolip-cell:qolip_ombor:a_blok:a:1	A blok	Qolip ombor	A	1	A1	40029A399E63E756199C199C	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"id": "qolip-cell:qolip_ombor:a_blok:a:1", "block": "A blok", "warehouse": "Qolip ombor", "qr_payload": "40029A399E63E756199C199C", "row_letter": "A", "column_number": 1, "created_by_ref": "worker_qolipchi_cust_001", "location_label": "A1", "created_by_name": "Jumaniyoz qolipchi", "created_by_role": "qolipchi"}	2026-06-21 11:37:38.159981+05	2026-06-22 12:53:46.463345+05
qolip-cell:qolip_ombor:a_blok:b:1	A blok	Qolip ombor	B	1	B1	400280E02F63D92882518251	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"id": "qolip-cell:qolip_ombor:a_blok:b:1", "block": "A blok", "warehouse": "Qolip ombor", "qr_payload": "400280E02F63D92882518251", "row_letter": "B", "column_number": 1, "created_by_ref": "worker_qolipchi_cust_001", "location_label": "B1", "created_by_name": "Jumaniyoz qolipchi", "created_by_role": "qolipchi"}	2026-06-21 11:40:04.615505+05	2026-06-22 13:02:29.695617+05
qolip-cell:qolip_ombor:a_blok:a:3	A blok	Qolip ombor	A	3	A3	40029A39A063E7561D021D02	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"id": "qolip-cell:qolip_ombor:a_blok:a:3", "block": "A blok", "warehouse": "Qolip ombor", "qr_payload": "40029A39A063E7561D021D02", "row_letter": "A", "column_number": 3, "created_by_ref": "worker_qolipchi_cust_001", "location_label": "A3", "created_by_name": "Jumaniyoz qolipchi", "created_by_role": "qolipchi"}	2026-06-22 12:37:02.798811+05	2026-06-22 12:37:02.798811+05
qolip-cell:qolip_ombor:a_blok:a:2	A blok	Qolip ombor	A	2	A2	40029A39A163E7561EB51EB5	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"id": "qolip-cell:qolip_ombor:a_blok:a:2", "block": "A blok", "warehouse": "Qolip ombor", "qr_payload": "40029A39A163E7561EB51EB5", "row_letter": "A", "column_number": 2, "created_by_ref": "worker_qolipchi_cust_001", "location_label": "A2", "created_by_name": "Jumaniyoz qolipchi", "created_by_role": "qolipchi"}	2026-06-20 18:51:49.456627+05	2026-06-22 12:35:43.378934+05
\.


--
-- Data for Name: mini_qolip_checkouts; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_qolip_checkouts (id, location_id, block, warehouse, item_code, item_name, qolip_code, size, quantity, row_letter, column_number, location_label, issued_to_ref, issued_to_name, status, issued_by_role, issued_by_ref, issued_by_name, payload_json, issued_at, created_at, updated_at) FROM stdin;
qolip-checkout-8335b3b5487c64fad973938a	qolip:a_blok:abcd_family:234:342:a:1	A blok	Qolip ombor	ABCD Family	ABCD Family	234	342	1	A	1	A1	worker_5d55608d8750f09f461c4127	Abdulloh	open	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"id": "qolip-checkout-8335b3b5487c64fad973938a", "size": 342, "block": "A blok", "status": "open", "quantity": 1, "issued_at": "", "item_code": "ABCD Family", "item_name": "ABCD Family", "warehouse": "Qolip ombor", "qolip_code": "234", "row_letter": "A", "location_id": "qolip:a_blok:abcd_family:234:342:a:1", "column_number": 1, "issued_by_ref": "worker_qolipchi_cust_001", "issued_to_ref": "worker_5d55608d8750f09f461c4127", "issued_by_name": "Jumaniyoz qolipchi", "issued_by_role": "qolipchi", "issued_to_name": "Abdulloh", "location_label": "A1"}	2026-06-21 13:13:22.062544+05	2026-06-21 13:13:22.062544+05	2026-06-21 13:13:22.062544+05
qolip-checkout-98683daad3740aaa605430ca	qolip:a_blok:abrazes:4:453:a:1	A blok	Qolip ombor	abrazes	Abrazes	4	453	4	A	1	A1	worker_7859dc248c2b0e960efc6d37	ibrohim	open	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"id": "qolip-checkout-98683daad3740aaa605430ca", "size": 453, "block": "A blok", "status": "open", "quantity": 4, "issued_at": "", "item_code": "abrazes", "item_name": "Abrazes", "warehouse": "Qolip ombor", "qolip_code": "4", "row_letter": "A", "location_id": "qolip:a_blok:abrazes:4:453:a:1", "column_number": 1, "issued_by_ref": "worker_qolipchi_cust_001", "issued_to_ref": "worker_7859dc248c2b0e960efc6d37", "issued_by_name": "Jumaniyoz qolipchi", "issued_by_role": "qolipchi", "issued_to_name": "ibrohim", "location_label": "A1"}	2026-06-21 13:13:35.64744+05	2026-06-21 13:13:35.64744+05	2026-06-21 13:13:35.64744+05
qolip-checkout-2ea053b27094ba719d5012e6	qolip:a_blok:abrazesla:345453:453:a:1	A blok	Qolip ombor	Abrazesla	Abrazesla	345453	453	1	A	1	A1	worker_e85528d43e0c1137e890d61b	falonchi	open	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"id": "qolip-checkout-2ea053b27094ba719d5012e6", "size": 453, "block": "A blok", "status": "open", "quantity": 1, "issued_at": "", "item_code": "Abrazesla", "item_name": "Abrazesla", "warehouse": "Qolip ombor", "qolip_code": "345453", "row_letter": "A", "location_id": "qolip:a_blok:abrazesla:345453:453:a:1", "column_number": 1, "issued_by_ref": "worker_qolipchi_cust_001", "issued_to_ref": "worker_e85528d43e0c1137e890d61b", "issued_by_name": "Jumaniyoz qolipchi", "issued_by_role": "qolipchi", "issued_to_name": "falonchi", "location_label": "A1"}	2026-06-22 14:39:29.630127+05	2026-06-22 14:39:29.630127+05	2026-06-22 14:39:29.630127+05
\.


--
-- Data for Name: mini_qolip_locations; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_qolip_locations (id, block, warehouse, item_code, item_name, qolip_code, size, quantity, row_letter, column_number, location_label, created_by_role, created_by_ref, created_by_name, payload_json, created_at, updated_at) FROM stdin;
qolip:qolip_ombor:abcd_family:42342:324:a:1	Qolip ombor	Qolip ombor	ABCD Family	ABCD Family	42342	324	21	A	1	A1	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"id": "qolip:qolip_ombor:abcd_family:42342:324:a:1", "size": 324, "block": "Qolip ombor", "quantity": 21, "item_code": "ABCD Family", "item_name": "ABCD Family", "warehouse": "Qolip ombor", "qolip_code": "42342", "row_letter": "A", "column_number": 1, "created_by_ref": "worker_qolipchi_cust_001", "location_label": "A1", "created_by_name": "Jumaniyoz qolipchi", "created_by_role": "qolipchi"}	2026-06-20 16:12:12.889797+05	2026-06-20 16:12:12.889797+05
qolip:a_blok:843589437859:843589437859:985::0	A blok	Qolip ombor	843589437859	843589437859	843589437859	985	3		\N		qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"id": "qolip:a_blok:843589437859:843589437859:985::0", "size": 985, "block": "A blok", "quantity": 3, "item_code": "843589437859", "item_name": "843589437859", "warehouse": "Qolip ombor", "qolip_code": "843589437859", "row_letter": "", "column_number": null, "created_by_ref": "worker_qolipchi_cust_001", "location_label": "", "created_by_name": "Jumaniyoz qolipchi", "created_by_role": "qolipchi"}	2026-06-20 17:15:46.699443+05	2026-06-20 17:15:46.699443+05
qolip:a_blok:abcd_family:234:342:b:1	A blok	Qolip ombor	ABCD Family	ABCD Family	234	342	2	B	1	B1	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"id": "qolip:a_blok:abcd_family:234:342:b:1", "size": 342, "block": "A blok", "quantity": 2, "item_code": "ABCD Family", "item_name": "ABCD Family", "warehouse": "Qolip ombor", "qolip_code": "234", "row_letter": "B", "column_number": 1, "created_by_ref": "worker_qolipchi_cust_001", "location_label": "B1", "created_by_name": "Jumaniyoz qolipchi", "created_by_role": "qolipchi"}	2026-06-22 11:37:17.948366+05	2026-06-22 11:37:17.948366+05
qolip:a_blok:ali_baba_chempion_paket:26262:12:a:1	A blok	Qolip ombor	ali baba chempion paket	Ali Baba Chempion Paket	26262	12	2	A	1	A1	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"id": "qolip:a_blok:ali_baba_chempion_paket:26262:12:a:1", "size": 12, "block": "A blok", "quantity": 2, "item_code": "ali baba chempion paket", "item_name": "Ali Baba Chempion Paket", "warehouse": "Qolip ombor", "qolip_code": "26262", "row_letter": "A", "column_number": 1, "created_by_ref": "worker_qolipchi_cust_001", "location_label": "A1", "created_by_name": "Jumaniyoz qolipchi", "created_by_role": "qolipchi"}	2026-06-22 14:39:11.413456+05	2026-06-22 14:39:11.413456+05
qolip:a_blok:abrazesla:345453:453:a:1	A blok	Qolip ombor	Abrazesla	Abrazesla	345453	453	11	A	1	A1	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"id": "qolip:a_blok:abrazesla:345453:453:a:1", "size": 453, "block": "A blok", "quantity": 9, "item_code": "Abrazesla", "item_name": "Abrazesla", "warehouse": "Qolip ombor", "qolip_code": "345453", "row_letter": "A", "column_number": 1, "created_by_ref": "worker_qolipchi_cust_001", "location_label": "A1", "created_by_name": "Jumaniyoz qolipchi", "created_by_role": "qolipchi"}	2026-06-22 11:37:01.362587+05	2026-06-22 14:39:29.630127+05
\.


--
-- Data for Name: mini_qolip_product_specs; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_qolip_product_specs (item_code, item_name, item_group, qolip_code, size, created_by_role, created_by_ref, created_by_name, payload_json, created_at, updated_at) FROM stdin;
Abrazesla	Abrazesla	tayyor mahsulot	345453	453	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"size": 453, "item_code": "Abrazesla", "item_name": "Abrazesla", "item_group": "tayyor mahsulot", "qolip_code": "345453", "created_by_ref": "worker_qolipchi_cust_001", "created_by_name": "Jumaniyoz qolipchi", "created_by_role": "qolipchi"}	2026-06-20 18:34:07.627735+05	2026-06-20 18:34:07.627735+05
ali baba chempion paket	Ali Baba Chempion Paket	tayyor mahsulot	26262	12	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"size": 12, "item_code": "ali baba chempion paket", "item_name": "Ali Baba Chempion Paket", "item_group": "tayyor mahsulot", "qolip_code": "26262", "created_by_ref": "worker_qolipchi_cust_001", "created_by_name": "Jumaniyoz qolipchi", "created_by_role": "qolipchi"}	2026-06-22 14:38:59.409687+05	2026-06-22 14:38:59.409687+05
ABCD Family	ABCD Family	tayyor mahsulot	45	2	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"size": 2, "item_code": "ABCD Family", "item_name": "ABCD Family", "item_group": "tayyor mahsulot", "qolip_code": "45", "created_by_ref": "worker_qolipchi_cust_001", "created_by_name": "Jumaniyoz qolipchi", "created_by_role": "qolipchi"}	2026-06-30 16:02:50.385554+05	2026-06-30 16:02:50.385554+05
ABCD Family	ABCD Family	tayyor mahsulot	453	46	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"size": 46, "item_code": "ABCD Family", "item_name": "ABCD Family", "item_group": "tayyor mahsulot", "qolip_code": "453", "created_by_ref": "worker_qolipchi_cust_001", "created_by_name": "Jumaniyoz qolipchi", "created_by_role": "qolipchi"}	2026-06-30 16:05:28.034722+05	2026-06-30 16:05:28.034722+05
\.


--
-- Data for Name: mini_queue_action_events; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_queue_action_events (id, event_id, apparatus, order_id, action, from_state, to_state, policy, actor_role, actor_ref, actor_display_name, assigned_apparatus, payload_json, created_at) FROM stdin;
\.


--
-- Data for Name: mini_queue_sequences; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_queue_sequences (apparatus, order_ids, updated_at) FROM stdin;
\.


--
-- Data for Name: mini_queue_states; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_queue_states (apparatus, order_id, state, updated_at) FROM stdin;
\.


--
-- Data for Name: mini_quick_order_images; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_quick_order_images (owner_key, image_id, image_name, image_mime, image_size_bytes, body, created_at) FROM stdin;
\.


--
-- Data for Name: mini_quick_order_templates; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_quick_order_templates (id, owner_key, code, name, item_code, product_name, customer_ref, customer_name, payload_json, quick_key, saved_at) FROM stdin;
1783059881134703	admin:admin	Z-1783059881129516	Dide	Dide	Dide	CUST-002	Taze Ay	{"id": "1783059881134703", "kg": 600.0, "code": "Z-1783059881129516", "name": "Dide", "note": "", "color": "", "status": "rulon", "product": "Dide", "customer": "Taze Ay", "image_id": "", "saved_at": "1783059881134705", "width_mm": 765.0, "image_url": "", "item_code": "Dide", "image_mime": "", "image_name": "", "roll_count": 7.0, "frame_count": 3.0, "customer_ref": "CUST-002", "order_number": "", "source_map_id": "template-zakaz-2344", "waste_percent": 2.0, "image_size_bytes": 0, "material_display": "", "edge_allowance_mm": 15.0, "first_layer_micron": "12", "third_layer_micron": "", "second_layer_micron": "30", "first_layer_material": "pet", "third_layer_material": "", "frame_product_size_mm": 250.0, "second_layer_material": "pe oq"}	quick|cust-002|taze ay|dide|rulon|||250.000|3.000|15.000|2.000|7.000|pet|12|pe oq|30|||	2026-07-03 11:24:41.138832+05
\.


--
-- Data for Name: mini_quick_order_templates_backup_frame_fields_20260620_131158; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_quick_order_templates_backup_frame_fields_20260620_131158 (id, owner_key, code, name, item_code, product_name, customer_ref, customer_name, payload_json, quick_key, saved_at) FROM stdin;
1781496811810977	admin:admin	Z-1781496811803421	ABCD Family	ABCD Family	ABCD Family	ABCD Family	ABCD Family	{"id": "1781496811810977", "kg": 4435.0, "code": "Z-1781496811803421", "name": "ABCD Family", "note": "", "color": "", "status": "rulon", "product": "ABCD Family", "customer": "ABCD Family", "image_id": "", "saved_at": "1781499857192830", "width_mm": 630.0, "image_url": "", "item_code": "ABCD Family", "image_mime": "", "image_name": "", "roll_count": 7.0, "customer_ref": "ABCD Family", "order_number": "", "source_map_id": "zakaz-7656", "waste_percent": 5.0, "image_size_bytes": 0, "material_display": "", "first_layer_micron": "12", "third_layer_micron": "", "second_layer_micron": "50", "first_layer_material": "pet", "third_layer_material": "", "second_layer_material": "pe oq"}	quick|abcd family|abcd family|abcd family|rulon|||630.000|5.000|7.000|pet|12|pe oq|50|||	2026-06-15 10:04:17.20194+05
1781500139408593	admin:admin	Z-1781500139407698	Imperator salyami	Imperator salyami	Imperator salyami	Abdulatifaka Imperator	Abdulatifaka Imperator	{"id": "1781500139408593", "kg": 1000.0, "code": "Z-1781500139407698", "name": "Imperator salyami", "note": "", "color": "", "status": "rulon", "product": "Imperator salyami", "customer": "Abdulatifaka Imperator", "image_id": "", "saved_at": "1781500139408596", "width_mm": 630.0, "image_url": "", "item_code": "Imperator salyami", "image_mime": "", "image_name": "", "roll_count": 7.0, "customer_ref": "Abdulatifaka Imperator", "order_number": "", "source_map_id": "zakaz-1233", "waste_percent": 5.0, "image_size_bytes": 0, "material_display": "", "first_layer_micron": "12", "third_layer_micron": "", "second_layer_micron": "40", "first_layer_material": "pet", "third_layer_material": "", "second_layer_material": "pe oq"}	quick|abdulatifaka imperator|abdulatifaka imperator|imperator salyami|rulon|||630.000|5.000|7.000|pet|12|pe oq|40|||	2026-06-15 10:08:59.408877+05
1781607187519920	admin:admin	Z-1781607187515465	Vesta kotta	Vesta kotta	Vesta kotta	Akfa vesta	Akfa vesta	{"id": "1781607187519920", "kg": 245.0, "code": "Z-1781607187515465", "name": "Vesta kotta", "note": "", "color": "", "status": "rulon", "product": "Vesta kotta", "customer": "Akfa vesta", "image_id": "", "saved_at": "1781794266697330", "width_mm": 630.0, "image_url": "", "item_code": "Vesta kotta", "image_mime": "", "image_name": "", "roll_count": 7.0, "customer_ref": "Akfa vesta", "order_number": "", "source_map_id": "zakaz-5678", "waste_percent": 5.0, "image_size_bytes": 0, "material_display": "", "first_layer_micron": "12", "third_layer_micron": "", "second_layer_micron": "40", "first_layer_material": "pet", "third_layer_material": "", "second_layer_material": "pe oq"}	quick|akfa vesta|akfa vesta|vesta kotta|rulon|||630.000|5.000|7.000|pet|12|pe oq|40|||	2026-06-18 19:51:06.697834+05
1781862659501435	admin:admin	Z-1781862659501001	Amir boss shef 65 gr	Amir boss shef 65 gr	Amir boss shef 65 gr	Ahror aka vodiy maro'jniy	Ahror aka vodiy maro'jniy	{"id": "1781862659501435", "kg": 1000.0, "code": "Z-1781862659501001", "name": "Amir boss shef 65 gr", "note": "", "color": "", "status": "rulon", "product": "Amir boss shef 65 gr", "customer": "Ahror aka vodiy maro'jniy", "image_id": "img1781862478596493", "saved_at": "1781862659501437", "width_mm": 800.0, "image_url": "/v1/mobile/calculate/orders/image/view?id=img1781862478596493", "item_code": "Amir boss shef 65 gr", "image_mime": "image/jpeg", "image_name": "image_picker_A019BE7A-1FA2-420F-9842-0454ED849E44-8242-00000168F1124886.png", "roll_count": 7.0, "customer_ref": "Ahror aka vodiy maro'jniy", "order_number": "", "source_map_id": "zakaz-9134", "waste_percent": 5.0, "image_size_bytes": 197694, "material_display": "", "first_layer_micron": "12", "third_layer_micron": "", "second_layer_micron": "30", "first_layer_material": "pet", "third_layer_material": "", "second_layer_material": "pe oq"}	quick|ahror aka vodiy maro'jniy|ahror aka vodiy maro'jniy|amir boss shef 65 gr|rulon|||800.000|5.000|7.000|pet|12|pe oq|30|||	2026-06-19 14:50:59.505722+05
1781866311108673	admin:admin	Z-1781866311107171	Paynet	Paynet	Paynet	Akmalaka Karvon	Akmalaka Karvon	{"id": "1781866311108673", "kg": 1000.0, "code": "Z-1781866311107171", "name": "Paynet", "note": "", "color": "", "status": "rulon", "product": "Paynet", "customer": "Akmalaka Karvon", "image_id": "img1781866016035286", "saved_at": "1781866311108677", "width_mm": 765.0, "image_url": "/v1/mobile/calculate/orders/image/view?id=img1781866016035286", "item_code": "Paynet", "image_mime": "image/jpeg", "image_name": "image_picker_A79CF358-F944-4F20-A4BC-E6E73FE16AA8-8242-00000172DA092784.png", "roll_count": 7.0, "customer_ref": "Akmalaka Karvon", "order_number": "", "source_map_id": "zakaz-1953", "waste_percent": 5.0, "image_size_bytes": 197694, "material_display": "", "first_layer_micron": "12", "third_layer_micron": "", "second_layer_micron": "30", "first_layer_material": "pet", "third_layer_material": "", "second_layer_material": "pe oq"}	quick|akmalaka karvon|akmalaka karvon|paynet|rulon|||765.000|5.000|7.000|pet|12|pe oq|30|||	2026-06-19 15:51:51.109096+05
\.


--
-- Data for Name: mini_raw_material_assignments; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_raw_material_assignments (barcode, order_id, apparatus, item_code, item_group, payload_json, updated_at) FROM stdin;
\.


--
-- Data for Name: mini_raw_material_stock; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_raw_material_stock (id, warehouse, item_code, item_name, barcode, qty, uom, status, reserved_order_id, source_receipt_id, payload_json, created_at, updated_at) FROM stdin;
\.


--
-- Data for Name: mini_rps_batches; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_rps_batches (owner_key, batch_id, active, owner_role, owner_ref, item_code, warehouse, payload_json, updated_at) FROM stdin;
\.


--
-- Data for Name: mini_warehouse_assignments; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_warehouse_assignments (warehouse, principal_role, principal_ref, display_name, payload_json, updated_at) FROM stdin;
Qolip ombor	qolipchi	worker_qolipchi_cust_001	Jumaniyoz qolipchi	{"warehouse": "Qolip ombor", "display_name": "Jumaniyoz qolipchi", "principal_ref": "worker_qolipchi_cust_001", "principal_role": "qolipchi"}	2026-06-20 16:10:34.532377+05
\.


--
-- Data for Name: mini_warehouses; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_warehouses (id, name, company, is_group, parent_warehouse, payload_json, updated_at) FROM stdin;
warehouse:kalidor	Kalidor		f		{"warehouse": "Kalidor"}	2026-06-17 07:40:13.84395+05
warehouse:ombor	Ombor		f		{"warehouse": "Ombor"}	2026-06-17 07:40:13.84395+05
warehouse:qolip ombor	Qolip ombor		f		{"company": "", "is_group": false, "warehouse": "Qolip ombor", "parent_warehouse": ""}	2026-06-20 16:10:34.182146+05
warehouse:a blok	A blok		f	Qolip ombor	{"company": "", "is_group": false, "warehouse": "A blok", "parent_warehouse": "Qolip ombor"}	2026-06-20 16:33:21.203136+05
warehouse:b blok	b blok		f	Qolip ombor	{"company": "", "is_group": false, "warehouse": "b blok", "parent_warehouse": "Qolip ombor"}	2026-06-20 18:40:10.595702+05
warehouse:d blok	d blok		f	Qolip ombor	{"company": "", "is_group": false, "warehouse": "d blok", "parent_warehouse": "Qolip ombor"}	2026-06-20 18:52:08.711786+05
warehouse:real api tayyor ombor 20260626151725	Real API Tayyor Ombor 20260626151725	Real API Test	f		{"company": "Real API Test", "is_group": false, "warehouse": "Real API Tayyor Ombor 20260626151725", "parent_warehouse": ""}	2026-06-26 15:17:25.642136+05
warehouse:real api tayyor ombor 20260626151828	Real API Tayyor Ombor 20260626151828	Real API Test	f		{"company": "Real API Test", "is_group": false, "warehouse": "Real API Tayyor Ombor 20260626151828", "parent_warehouse": ""}	2026-06-26 15:18:28.716737+05
warehouse:real api tayyor ombor 20260626152003	Real API Tayyor Ombor 20260626152003	Real API Test	f		{"company": "Real API Test", "is_group": false, "warehouse": "Real API Tayyor Ombor 20260626152003", "parent_warehouse": ""}	2026-06-26 15:20:03.107868+05
warehouse:real api tayyor ombor 20260626152011	Real API Tayyor Ombor 20260626152011	Real API Test	f		{"company": "Real API Test", "is_group": false, "warehouse": "Real API Tayyor Ombor 20260626152011", "parent_warehouse": ""}	2026-06-26 15:20:11.780786+05
warehouse:real api tayyor ombor 20260626152031	Real API Tayyor Ombor 20260626152031	Real API Test	f		{"company": "Real API Test", "is_group": false, "warehouse": "Real API Tayyor Ombor 20260626152031", "parent_warehouse": ""}	2026-06-26 15:20:31.913927+05
warehouse:real api tayyor ombor 20260626152054	Real API Tayyor Ombor 20260626152054	Real API Test	f		{"company": "Real API Test", "is_group": false, "warehouse": "Real API Tayyor Ombor 20260626152054", "parent_warehouse": ""}	2026-06-26 15:20:55.283765+05
warehouse:real api tayyor ombor 20260626152118	Real API Tayyor Ombor 20260626152118	Real API Test	f		{"company": "Real API Test", "is_group": false, "warehouse": "Real API Tayyor Ombor 20260626152118", "parent_warehouse": ""}	2026-06-26 15:21:19.189885+05
\.


--
-- Data for Name: mini_worker_groups; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_worker_groups (apparatus, group_code, shift, worker_ids, payload_json, created_at, updated_at, start_time, end_time, work_days_per_week, start_day, accounting_enabled) FROM stdin;
7 ta rangli pechat	A	kunduz	["worker_b61269bcf7b8f62a5f7c3f36", "worker_7859dc248c2b0e960efc6d37", "worker_22c83e9cd6d311607f7b58b7", "worker_e85528d43e0c1137e890d61b"]	{"shift": "kunduz", "end_time": "20:00", "apparatus": "7 ta rangli pechat", "start_day": "monday", "group_code": "A", "start_time": "08:00", "worker_ids": ["worker_b61269bcf7b8f62a5f7c3f36", "worker_7859dc248c2b0e960efc6d37", "worker_22c83e9cd6d311607f7b58b7", "worker_e85528d43e0c1137e890d61b"], "accounting_enabled": false, "work_days_per_week": 6}	2026-06-18 14:56:21.850721+05	2026-06-18 14:56:21.850721+05	08:00	20:00	6	monday	f
worker-settings	B GURUH	obet	["worker_5d55608d8750f09f461c4127"]	{"shift": "obet", "end_time": "20:00", "apparatus": "worker-settings", "start_day": "monday", "group_code": "B GURUH", "start_time": "22:15", "worker_ids": ["worker_5d55608d8750f09f461c4127"], "accounting_enabled": true, "work_days_per_week": 6}	2026-06-18 14:57:01.281147+05	2026-06-18 14:57:01.281147+05	22:15	20:00	6	monday	t
worker-settings	E GURUH	kunduz	[]	{"shift": "kunduz", "end_time": "20:00", "apparatus": "worker-settings", "start_day": "monday", "group_code": "E GURUH", "start_time": "08:00", "worker_ids": [], "accounting_enabled": false, "work_days_per_week": 6}	2026-06-18 14:57:01.281147+05	2026-06-18 14:57:01.281147+05	08:00	20:00	6	monday	f
Laminatsiya 1	D GURUH	kunduz	[]	{"shift": "kunduz", "end_time": "20:00", "apparatus": "Laminatsiya 1", "start_day": "monday", "group_code": "D GURUH", "start_time": "08:00", "worker_ids": [], "accounting_enabled": false, "work_days_per_week": 6}	2026-06-18 14:57:01.283475+05	2026-06-18 14:57:01.283475+05	08:00	20:00	6	monday	f
Laminatsiya 1	L	kunduz	["worker_7a060b5d3f88989e5d8e560c"]	{"shift": "kunduz", "end_time": "20:00", "apparatus": "Laminatsiya 1", "start_day": "monday", "group_code": "L", "start_time": "08:00", "worker_ids": ["worker_7a060b5d3f88989e5d8e560c"], "accounting_enabled": false, "work_days_per_week": 6}	2026-06-18 14:57:01.283475+05	2026-06-18 14:57:01.283475+05	08:00	20:00	6	monday	f
\.


--
-- Data for Name: mini_workers; Type: TABLE DATA; Schema: public; Owner: -
--

COPY public.mini_workers (id, name, level, payload_json, created_at, updated_at, phone) FROM stdin;
worker_b61269bcf7b8f62a5f7c3f36	Jamoliddin	Brigader	{"id": "worker_b61269bcf7b8f62a5f7c3f36", "name": "Jamoliddin", "level": "Brigader"}	2026-06-15 11:21:52.356552+05	2026-06-15 11:21:52.356552+05	
worker_5d55608d8750f09f461c4127	Abdulloh	Brigader	{"id": "worker_5d55608d8750f09f461c4127", "name": "Abdulloh", "level": "Brigader"}	2026-06-15 11:22:44.49219+05	2026-06-15 11:22:58.917227+05	
worker_22c83e9cd6d311607f7b58b7	Jasur aka	1 - darajali	{"id": "worker_22c83e9cd6d311607f7b58b7", "name": "Jasur aka", "level": "1 - darajali"}	2026-06-16 09:16:19.580968+05	2026-06-16 09:16:19.580968+05	
worker_7859dc248c2b0e960efc6d37	ibrohim	Master	{"id": "worker_7859dc248c2b0e960efc6d37", "name": "ibrohim", "level": "Master"}	2026-06-16 09:21:05.460071+05	2026-06-16 09:21:05.460071+05	
worker_e85528d43e0c1137e890d61b	falonchi	2 - darajali	{"id": "worker_e85528d43e0c1137e890d61b", "name": "falonchi", "level": "2 - darajali", "phone": "+998550000055"}	2026-06-16 13:23:25.977139+05	2026-06-16 13:32:56.896688+05	+998550000055
worker_7a060b5d3f88989e5d8e560c	laminatchik	1 - darajali	{"id": "worker_7a060b5d3f88989e5d8e560c", "name": "laminatchik", "level": "1 - darajali", "phone": "660000066"}	2026-06-18 14:55:38.798867+05	2026-06-18 14:57:23.12999+05	660000066
worker_qolipchi_cust_001	Jumaniyoz qolipchi	Brigader	{"id": "worker_qolipchi_cust_001", "name": "Jumaniyoz qolipchi", "level": "Brigader", "phone": "+998110000011"}	2026-06-20 15:49:40.264013+05	2026-06-20 15:49:40.264013+05	+998110000011
\.


--
-- Name: mini_engine_events_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.mini_engine_events_id_seq', 1, false);


--
-- Name: mini_order_progress_events_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.mini_order_progress_events_id_seq', 1, false);


--
-- Name: mini_queue_action_events_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.mini_queue_action_events_id_seq', 1, false);


--
-- Name: mini_apparatus_groups mini_apparatus_groups_name_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_apparatus_groups
    ADD CONSTRAINT mini_apparatus_groups_name_unique UNIQUE (name);


--
-- Name: mini_apparatus_groups mini_apparatus_groups_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_apparatus_groups
    ADD CONSTRAINT mini_apparatus_groups_pkey PRIMARY KEY (id);


--
-- Name: mini_apparatus_material_rules mini_apparatus_material_rules_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_apparatus_material_rules
    ADD CONSTRAINT mini_apparatus_material_rules_pkey PRIMARY KEY (apparatus);


--
-- Name: mini_apparatus mini_apparatus_name_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_apparatus
    ADD CONSTRAINT mini_apparatus_name_unique UNIQUE (name);


--
-- Name: mini_apparatus mini_apparatus_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_apparatus
    ADD CONSTRAINT mini_apparatus_pkey PRIMARY KEY (id);


--
-- Name: mini_apparatus_queue_policies mini_apparatus_queue_policies_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_apparatus_queue_policies
    ADD CONSTRAINT mini_apparatus_queue_policies_pkey PRIMARY KEY (apparatus);


--
-- Name: mini_daily_apparatus_sequences mini_daily_apparatus_sequences_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_daily_apparatus_sequences
    ADD CONSTRAINT mini_daily_apparatus_sequences_pkey PRIMARY KEY (work_date, apparatus);


--
-- Name: mini_daily_work_sequences mini_daily_work_sequences_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_daily_work_sequences
    ADD CONSTRAINT mini_daily_work_sequences_pkey PRIMARY KEY (work_date);


--
-- Name: mini_engine_events mini_engine_events_event_id_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_engine_events
    ADD CONSTRAINT mini_engine_events_event_id_unique UNIQUE (event_id);


--
-- Name: mini_engine_events mini_engine_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_engine_events
    ADD CONSTRAINT mini_engine_events_pkey PRIMARY KEY (id);


--
-- Name: mini_finished_goods_stock mini_finished_goods_stock_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_finished_goods_stock
    ADD CONSTRAINT mini_finished_goods_stock_pkey PRIMARY KEY (id);


--
-- Name: mini_gscale_receipts mini_gscale_receipts_barcode_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_gscale_receipts
    ADD CONSTRAINT mini_gscale_receipts_barcode_unique UNIQUE (barcode);


--
-- Name: mini_gscale_receipts mini_gscale_receipts_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_gscale_receipts
    ADD CONSTRAINT mini_gscale_receipts_pkey PRIMARY KEY (name);


--
-- Name: mini_idempotency_keys mini_idempotency_keys_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_idempotency_keys
    ADD CONSTRAINT mini_idempotency_keys_pkey PRIMARY KEY (key);


--
-- Name: mini_item_groups mini_item_groups_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_item_groups
    ADD CONSTRAINT mini_item_groups_pkey PRIMARY KEY (name);


--
-- Name: mini_items mini_items_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_items
    ADD CONSTRAINT mini_items_pkey PRIMARY KEY (code);


--
-- Name: mini_order_products mini_order_products_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_order_products
    ADD CONSTRAINT mini_order_products_pkey PRIMARY KEY (id);


--
-- Name: mini_order_progress_events mini_order_progress_events_event_id_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_order_progress_events
    ADD CONSTRAINT mini_order_progress_events_event_id_unique UNIQUE (event_id);


--
-- Name: mini_order_progress_events mini_order_progress_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_order_progress_events
    ADD CONSTRAINT mini_order_progress_events_pkey PRIMARY KEY (id);


--
-- Name: mini_order_run_sessions mini_order_run_sessions_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_order_run_sessions
    ADD CONSTRAINT mini_order_run_sessions_pkey PRIMARY KEY (session_id);


--
-- Name: mini_orders mini_orders_code_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_orders
    ADD CONSTRAINT mini_orders_code_unique UNIQUE (code);


--
-- Name: mini_orders mini_orders_order_number_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_orders
    ADD CONSTRAINT mini_orders_order_number_unique UNIQUE (order_number);


--
-- Name: mini_orders mini_orders_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_orders
    ADD CONSTRAINT mini_orders_pkey PRIMARY KEY (id);


--
-- Name: mini_production_map_edges mini_production_map_edges_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_production_map_edges
    ADD CONSTRAINT mini_production_map_edges_pkey PRIMARY KEY (map_id, edge_index);


--
-- Name: mini_production_map_nodes mini_production_map_nodes_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_production_map_nodes
    ADD CONSTRAINT mini_production_map_nodes_pkey PRIMARY KEY (map_id, node_id);


--
-- Name: mini_production_maps mini_production_maps_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_production_maps
    ADD CONSTRAINT mini_production_maps_pkey PRIMARY KEY (id);


--
-- Name: mini_progress_batches mini_progress_batches_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_progress_batches
    ADD CONSTRAINT mini_progress_batches_pkey PRIMARY KEY (batch_id);


--
-- Name: mini_progress_batches mini_progress_batches_qr_payload_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_progress_batches
    ADD CONSTRAINT mini_progress_batches_qr_payload_unique UNIQUE (qr_payload);


--
-- Name: mini_push_tokens mini_push_tokens_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_push_tokens
    ADD CONSTRAINT mini_push_tokens_pkey PRIMARY KEY (token);


--
-- Name: mini_qolip_cell_qrs mini_qolip_cell_qrs_cell_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_qolip_cell_qrs
    ADD CONSTRAINT mini_qolip_cell_qrs_cell_unique UNIQUE (warehouse, block, row_letter, column_number);


--
-- Name: mini_qolip_cell_qrs mini_qolip_cell_qrs_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_qolip_cell_qrs
    ADD CONSTRAINT mini_qolip_cell_qrs_pkey PRIMARY KEY (id);


--
-- Name: mini_qolip_cell_qrs mini_qolip_cell_qrs_qr_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_qolip_cell_qrs
    ADD CONSTRAINT mini_qolip_cell_qrs_qr_unique UNIQUE (qr_payload);


--
-- Name: mini_qolip_checkouts mini_qolip_checkouts_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_qolip_checkouts
    ADD CONSTRAINT mini_qolip_checkouts_pkey PRIMARY KEY (id);


--
-- Name: mini_qolip_locations mini_qolip_locations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_qolip_locations
    ADD CONSTRAINT mini_qolip_locations_pkey PRIMARY KEY (id);


--
-- Name: mini_queue_action_events mini_queue_action_events_event_id_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_queue_action_events
    ADD CONSTRAINT mini_queue_action_events_event_id_unique UNIQUE (event_id);


--
-- Name: mini_queue_action_events mini_queue_action_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_queue_action_events
    ADD CONSTRAINT mini_queue_action_events_pkey PRIMARY KEY (id);


--
-- Name: mini_queue_sequences mini_queue_sequences_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_queue_sequences
    ADD CONSTRAINT mini_queue_sequences_pkey PRIMARY KEY (apparatus);


--
-- Name: mini_queue_states mini_queue_states_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_queue_states
    ADD CONSTRAINT mini_queue_states_pkey PRIMARY KEY (apparatus, order_id);


--
-- Name: mini_quick_order_images mini_quick_order_images_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_quick_order_images
    ADD CONSTRAINT mini_quick_order_images_pkey PRIMARY KEY (owner_key, image_id);


--
-- Name: mini_quick_order_templates mini_quick_order_templates_owner_code_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_quick_order_templates
    ADD CONSTRAINT mini_quick_order_templates_owner_code_unique UNIQUE (owner_key, code);


--
-- Name: mini_quick_order_templates mini_quick_order_templates_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_quick_order_templates
    ADD CONSTRAINT mini_quick_order_templates_pkey PRIMARY KEY (id);


--
-- Name: mini_raw_material_assignments mini_raw_material_assignments_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_raw_material_assignments
    ADD CONSTRAINT mini_raw_material_assignments_pkey PRIMARY KEY (barcode);


--
-- Name: mini_raw_material_stock mini_raw_material_stock_barcode_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_raw_material_stock
    ADD CONSTRAINT mini_raw_material_stock_barcode_unique UNIQUE (barcode);


--
-- Name: mini_raw_material_stock mini_raw_material_stock_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_raw_material_stock
    ADD CONSTRAINT mini_raw_material_stock_pkey PRIMARY KEY (id);


--
-- Name: mini_rps_batches mini_rps_batches_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_rps_batches
    ADD CONSTRAINT mini_rps_batches_pkey PRIMARY KEY (owner_key);


--
-- Name: mini_warehouse_assignments mini_warehouse_assignments_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_warehouse_assignments
    ADD CONSTRAINT mini_warehouse_assignments_pkey PRIMARY KEY (warehouse, principal_role, principal_ref);


--
-- Name: mini_warehouses mini_warehouses_name_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_warehouses
    ADD CONSTRAINT mini_warehouses_name_unique UNIQUE (name);


--
-- Name: mini_warehouses mini_warehouses_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_warehouses
    ADD CONSTRAINT mini_warehouses_pkey PRIMARY KEY (id);


--
-- Name: mini_worker_groups mini_worker_groups_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_worker_groups
    ADD CONSTRAINT mini_worker_groups_pkey PRIMARY KEY (apparatus, group_code);


--
-- Name: mini_workers mini_workers_name_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_workers
    ADD CONSTRAINT mini_workers_name_unique UNIQUE (name);


--
-- Name: mini_workers mini_workers_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_workers
    ADD CONSTRAINT mini_workers_pkey PRIMARY KEY (id);


--
-- Name: idx_mini_apparatus_groups_lower_name; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX idx_mini_apparatus_groups_lower_name ON public.mini_apparatus_groups USING btree (lower(name));


--
-- Name: idx_mini_apparatus_lower_name; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX idx_mini_apparatus_lower_name ON public.mini_apparatus USING btree (lower(name));


--
-- Name: idx_mini_engine_events_entity; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_engine_events_entity ON public.mini_engine_events USING btree (domain, entity_id, created_at DESC);


--
-- Name: idx_mini_gscale_receipts_item_updated; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_gscale_receipts_item_updated ON public.mini_gscale_receipts USING btree (lower(item_code), updated_at DESC);


--
-- Name: idx_mini_gscale_receipts_status_updated; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_gscale_receipts_status_updated ON public.mini_gscale_receipts USING btree (status, updated_at DESC);


--
-- Name: idx_mini_item_groups_lower_name; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_item_groups_lower_name ON public.mini_item_groups USING btree (lower(name));


--
-- Name: idx_mini_item_groups_parent; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_item_groups_parent ON public.mini_item_groups USING btree (lower(parent_item_group));


--
-- Name: idx_mini_items_lower_code; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_items_lower_code ON public.mini_items USING btree (lower(code));


--
-- Name: idx_mini_items_lower_group; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_items_lower_group ON public.mini_items USING btree (lower(item_group));


--
-- Name: idx_mini_items_lower_name; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_items_lower_name ON public.mini_items USING btree (lower(name));


--
-- Name: idx_mini_order_progress_events_order_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_order_progress_events_order_created ON public.mini_order_progress_events USING btree (order_id, created_at DESC);


--
-- Name: idx_mini_order_run_sessions_apparatus_order; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_order_run_sessions_apparatus_order ON public.mini_order_run_sessions USING btree (lower(apparatus), order_id, updated_at DESC);


--
-- Name: idx_mini_order_run_sessions_one_open; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX idx_mini_order_run_sessions_one_open ON public.mini_order_run_sessions USING btree (lower(apparatus), order_id) WHERE (status = ANY (ARRAY['active'::text, 'paused'::text]));


--
-- Name: idx_mini_order_run_sessions_order_status; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_order_run_sessions_order_status ON public.mini_order_run_sessions USING btree (order_id, status, updated_at DESC);


--
-- Name: idx_mini_orders_customer_ref; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_orders_customer_ref ON public.mini_orders USING btree (customer_ref);


--
-- Name: idx_mini_orders_status; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_orders_status ON public.mini_orders USING btree (status);


--
-- Name: idx_mini_production_map_edges_from; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_production_map_edges_from ON public.mini_production_map_edges USING btree (from_node_id);


--
-- Name: idx_mini_production_map_edges_to; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_production_map_edges_to ON public.mini_production_map_edges USING btree (to_node_id);


--
-- Name: idx_mini_production_map_nodes_kind_title; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_production_map_nodes_kind_title ON public.mini_production_map_nodes USING btree (kind, lower(title));


--
-- Name: idx_mini_production_map_nodes_title; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_production_map_nodes_title ON public.mini_production_map_nodes USING btree (lower(title));


--
-- Name: idx_mini_production_maps_order_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_production_maps_order_id ON public.mini_production_maps USING btree (order_id);


--
-- Name: idx_mini_production_maps_order_number; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_production_maps_order_number ON public.mini_production_maps USING btree (order_number) WHERE (btrim(order_number) <> ''::text);


--
-- Name: idx_mini_progress_batches_order_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_progress_batches_order_created ON public.mini_progress_batches USING btree (order_id, created_at DESC);


--
-- Name: idx_mini_progress_batches_qr; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_progress_batches_qr ON public.mini_progress_batches USING btree (lower(qr_payload));


--
-- Name: idx_mini_progress_batches_wip_status_apparatus; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_progress_batches_wip_status_apparatus ON public.mini_progress_batches USING btree (wip_status, lower(current_apparatus), updated_at DESC);


--
-- Name: idx_mini_progress_batches_wip_status_apparatus_key; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_progress_batches_wip_status_apparatus_key ON public.mini_progress_batches USING btree (wip_status, current_apparatus_key, updated_at DESC);


--
-- Name: idx_mini_push_tokens_owner; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_push_tokens_owner ON public.mini_push_tokens USING btree (owner_key);


--
-- Name: idx_mini_push_tokens_updated; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_push_tokens_updated ON public.mini_push_tokens USING btree (updated_at DESC);


--
-- Name: idx_mini_qolip_cell_qrs_cell; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_qolip_cell_qrs_cell ON public.mini_qolip_cell_qrs USING btree (lower(block), row_letter, column_number);


--
-- Name: idx_mini_qolip_checkouts_block; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_qolip_checkouts_block ON public.mini_qolip_checkouts USING btree (lower(block), issued_at DESC);


--
-- Name: idx_mini_qolip_checkouts_status_issued; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_qolip_checkouts_status_issued ON public.mini_qolip_checkouts USING btree (status, issued_at DESC);


--
-- Name: idx_mini_qolip_checkouts_worker; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_qolip_checkouts_worker ON public.mini_qolip_checkouts USING btree (lower(issued_to_ref), status, issued_at DESC);


--
-- Name: idx_mini_qolip_locations_block; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_qolip_locations_block ON public.mini_qolip_locations USING btree (lower(block), row_letter, column_number);


--
-- Name: idx_mini_qolip_locations_item; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_qolip_locations_item ON public.mini_qolip_locations USING btree (lower(item_code), lower(item_name));


--
-- Name: idx_mini_qolip_product_specs_item; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_qolip_product_specs_item ON public.mini_qolip_product_specs USING btree (lower(item_code), lower(item_name), lower(qolip_code));


--
-- Name: idx_mini_qolip_product_specs_qolip_code_unique; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX idx_mini_qolip_product_specs_qolip_code_unique ON public.mini_qolip_product_specs USING btree (lower(qolip_code));


--
-- Name: idx_mini_queue_action_events_actor_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_queue_action_events_actor_created ON public.mini_queue_action_events USING btree (actor_role, actor_ref, created_at DESC);


--
-- Name: idx_mini_queue_action_events_apparatus_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_queue_action_events_apparatus_created ON public.mini_queue_action_events USING btree (apparatus, created_at DESC);


--
-- Name: idx_mini_queue_action_events_order_created; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_queue_action_events_order_created ON public.mini_queue_action_events USING btree (order_id, created_at DESC);


--
-- Name: idx_mini_queue_states_order_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_queue_states_order_id ON public.mini_queue_states USING btree (order_id);


--
-- Name: idx_mini_quick_order_templates_owner_lower_code; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX idx_mini_quick_order_templates_owner_lower_code ON public.mini_quick_order_templates USING btree (owner_key, lower(code));


--
-- Name: idx_mini_quick_order_templates_owner_quick_key; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_quick_order_templates_owner_quick_key ON public.mini_quick_order_templates USING btree (owner_key, quick_key);


--
-- Name: idx_mini_quick_order_templates_owner_saved; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_quick_order_templates_owner_saved ON public.mini_quick_order_templates USING btree (owner_key, saved_at DESC);


--
-- Name: idx_mini_raw_material_assignments_apparatus; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_raw_material_assignments_apparatus ON public.mini_raw_material_assignments USING btree (lower(apparatus));


--
-- Name: idx_mini_raw_material_assignments_item_group; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_raw_material_assignments_item_group ON public.mini_raw_material_assignments USING btree (lower(item_group));


--
-- Name: idx_mini_raw_material_assignments_order; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_raw_material_assignments_order ON public.mini_raw_material_assignments USING btree (order_id);


--
-- Name: idx_mini_rps_batches_active; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_rps_batches_active ON public.mini_rps_batches USING btree (active) WHERE active;


--
-- Name: idx_mini_warehouses_lower_name; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX idx_mini_warehouses_lower_name ON public.mini_warehouses USING btree (lower(name));


--
-- Name: idx_mini_worker_groups_apparatus; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_worker_groups_apparatus ON public.mini_worker_groups USING btree (lower(apparatus));


--
-- Name: idx_mini_worker_groups_shift; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_worker_groups_shift ON public.mini_worker_groups USING btree (shift);


--
-- Name: idx_mini_workers_level; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_mini_workers_level ON public.mini_workers USING btree (level);


--
-- Name: idx_mini_workers_lower_name; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX idx_mini_workers_lower_name ON public.mini_workers USING btree (lower(name));


--
-- Name: mini_apparatus mini_apparatus_group_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_apparatus
    ADD CONSTRAINT mini_apparatus_group_id_fkey FOREIGN KEY (group_id) REFERENCES public.mini_apparatus_groups(id) ON DELETE SET NULL;


--
-- Name: mini_order_products mini_order_products_order_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_order_products
    ADD CONSTRAINT mini_order_products_order_id_fkey FOREIGN KEY (order_id) REFERENCES public.mini_orders(id) ON DELETE CASCADE;


--
-- Name: mini_production_map_edges mini_production_map_edges_map_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_production_map_edges
    ADD CONSTRAINT mini_production_map_edges_map_id_fkey FOREIGN KEY (map_id) REFERENCES public.mini_production_maps(id) ON DELETE CASCADE;


--
-- Name: mini_production_map_nodes mini_production_map_nodes_map_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_production_map_nodes
    ADD CONSTRAINT mini_production_map_nodes_map_id_fkey FOREIGN KEY (map_id) REFERENCES public.mini_production_maps(id) ON DELETE CASCADE;


--
-- Name: mini_production_maps mini_production_maps_order_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.mini_production_maps
    ADD CONSTRAINT mini_production_maps_order_id_fkey FOREIGN KEY (order_id) REFERENCES public.mini_orders(id) ON DELETE SET NULL;


--
-- PostgreSQL database dump complete
--

\unrestrict AHI20H1RKU00CoIswOW2dtGi9oeK2EJRUf1woPepG35H5jRVogZN6TXEPNEvmZG

