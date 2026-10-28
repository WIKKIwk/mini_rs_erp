-- Qolip packages retain the physical mold and home stock; lifecycle never deletes either.
CREATE SEQUENCE public.mini_qolip_paddon_number;
CREATE TABLE public.mini_qolip_paddons (
    code TEXT PRIMARY KEY CHECK (code ~ '^QP-[0-9]{5,}$'),
    warehouse TEXT NOT NULL CHECK (btrim(warehouse) <> ''),
    location TEXT NOT NULL DEFAULT '', note TEXT NOT NULL DEFAULT '',
    locked_at TIMESTAMPTZ, archived_at TIMESTAMPTZ,
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by_role TEXT NOT NULL, created_by_ref TEXT NOT NULL, created_by_name TEXT NOT NULL
);
CREATE TABLE public.mini_qolip_paddon_members (
    id TEXT PRIMARY KEY,
    paddon_code TEXT NOT NULL REFERENCES public.mini_qolip_paddons(code) ON DELETE RESTRICT,
    qolip_code TEXT NOT NULL REFERENCES public.mini_qolip_product_specs(qolip_code) ON UPDATE RESTRICT ON DELETE RESTRICT,
    qolip_code_key TEXT GENERATED ALWAYS AS (lower(btrim(qolip_code))) STORED,
    added_at TIMESTAMPTZ NOT NULL DEFAULT now(), removed_at TIMESTAMPTZ,
    added_by_ref TEXT NOT NULL, added_by_name TEXT NOT NULL,
    removed_by_ref TEXT, removed_by_name TEXT,
    CHECK (removed_at IS NULL OR removed_at >= added_at)
);
CREATE UNIQUE INDEX mini_qolip_paddon_one_membership ON public.mini_qolip_paddon_members(qolip_code_key) WHERE removed_at IS NULL;
CREATE INDEX mini_qolip_paddon_members_current ON public.mini_qolip_paddon_members(paddon_code,added_at) WHERE removed_at IS NULL;
CREATE TABLE public.mini_qolip_archive_entries (
    id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK (kind IN ('paddon','qolip')),
    code TEXT NOT NULL CHECK (btrim(code) <> ''),
    code_key TEXT GENERATED ALWAYS AS (lower(btrim(code))) STORED,
    warehouse TEXT NOT NULL CHECK (btrim(warehouse) <> ''),
    snapshot JSONB NOT NULL, note TEXT NOT NULL DEFAULT '',
    archived_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    archived_by_role TEXT NOT NULL, archived_by_ref TEXT NOT NULL, archived_by_name TEXT NOT NULL,
    restored_at TIMESTAMPTZ, restored_by_role TEXT, restored_by_ref TEXT, restored_by_name TEXT,
    CHECK (restored_at IS NULL OR restored_at >= archived_at)
);
CREATE UNIQUE INDEX mini_qolip_one_open_archive ON public.mini_qolip_archive_entries(kind,code_key) WHERE restored_at IS NULL;
CREATE INDEX mini_qolip_archive_search ON public.mini_qolip_archive_entries(lower(btrim(warehouse)),archived_at DESC,id);
CREATE TABLE public.mini_qolip_paddon_events (
    id TEXT PRIMARY KEY, paddon_code TEXT NOT NULL REFERENCES public.mini_qolip_paddons(code),
    action TEXT NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    actor_ref TEXT NOT NULL, actor_name TEXT NOT NULL, payload JSONB NOT NULL
);
CREATE INDEX mini_qolip_paddon_event_history ON public.mini_qolip_paddon_events(paddon_code,created_at,id);
CREATE TABLE public.mini_qolip_storage_requests (
    actor_key TEXT NOT NULL, request_id TEXT NOT NULL,
    request_json JSONB NOT NULL, response_json JSONB NOT NULL,
    warehouses TEXT[] NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY(actor_key,request_id)
);
ALTER SEQUENCE public.mini_qolip_paddon_number OWNER TO mini_rs_erp_owner;
ALTER TABLE public.mini_qolip_paddons OWNER TO mini_rs_erp_owner;
ALTER TABLE public.mini_qolip_paddon_members OWNER TO mini_rs_erp_owner;
ALTER TABLE public.mini_qolip_archive_entries OWNER TO mini_rs_erp_owner;
ALTER TABLE public.mini_qolip_paddon_events OWNER TO mini_rs_erp_owner;
ALTER TABLE public.mini_qolip_storage_requests OWNER TO mini_rs_erp_owner;
REVOKE ALL ON public.mini_qolip_paddons,public.mini_qolip_paddon_members,public.mini_qolip_archive_entries,public.mini_qolip_paddon_events,public.mini_qolip_storage_requests FROM PUBLIC;
GRANT USAGE,SELECT ON SEQUENCE public.mini_qolip_paddon_number TO mini_rs_erp;
GRANT SELECT,INSERT ON public.mini_qolip_paddons,public.mini_qolip_paddon_members,public.mini_qolip_archive_entries,public.mini_qolip_paddon_events,public.mini_qolip_storage_requests TO mini_rs_erp;
GRANT UPDATE(locked_at,archived_at,revision) ON public.mini_qolip_paddons TO mini_rs_erp;
GRANT UPDATE(removed_at,removed_by_ref,removed_by_name) ON public.mini_qolip_paddon_members TO mini_rs_erp;
GRANT UPDATE(restored_at,restored_by_role,restored_by_ref,restored_by_name) ON public.mini_qolip_archive_entries TO mini_rs_erp;
