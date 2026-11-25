-- The catalog groups molds by normalized product code, including legacy items.
CREATE TABLE public.mini_qolip_product_information (
    item_code_key TEXT PRIMARY KEY CHECK (item_code_key <> '' AND item_code_key = lower(btrim(item_code_key))),
    details TEXT NOT NULL CHECK (char_length(details) <= 4000),
    attachment_name TEXT,
    attachment_content BYTEA,
    revision BIGINT NOT NULL CHECK (revision > 0),
    updated_by_role TEXT NOT NULL,
    updated_by_ref TEXT NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((attachment_name IS NULL) = (attachment_content IS NULL)),
    CHECK (attachment_name IS NULL OR octet_length(attachment_name) BETWEEN 1 AND 255),
    CHECK (attachment_content IS NULL OR octet_length(attachment_content) BETWEEN 1 AND 20971520)
);

ALTER TABLE public.mini_qolip_product_information OWNER TO mini_rs_erp_owner;
REVOKE ALL ON public.mini_qolip_product_information FROM PUBLIC;
GRANT SELECT, INSERT, UPDATE ON public.mini_qolip_product_information TO mini_rs_erp;
