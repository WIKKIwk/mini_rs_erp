-- Optional names are metadata on the stable set ID, independent of physical
-- composition and rework locks. Unnamed existing sets need no backfill.
CREATE TABLE public.mini_qolip_set_names (
    set_id TEXT PRIMARY KEY REFERENCES public.mini_qolip_sets(set_id)
        ON UPDATE RESTRICT ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (name = btrim(name) AND char_length(name) BETWEEN 1 AND 160),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX mini_qolip_set_names_name_unique
    ON public.mini_qolip_set_names (lower(name));

ALTER TABLE public.mini_qolip_set_names OWNER TO mini_rs_erp_owner;
REVOKE ALL ON public.mini_qolip_set_names FROM PUBLIC, mini_rs_erp;
GRANT SELECT, INSERT ON public.mini_qolip_set_names TO mini_rs_erp;
GRANT UPDATE (name, updated_at) ON public.mini_qolip_set_names TO mini_rs_erp;
