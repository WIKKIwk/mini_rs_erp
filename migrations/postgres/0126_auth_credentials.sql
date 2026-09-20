-- Authentication secrets live only as salted Argon2id hashes.
CREATE TABLE mini_auth_accounts (
    principal_ref TEXT PRIMARY KEY CHECK (btrim(principal_ref) <> ''),
    credential_hash TEXT,
    access_state JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (credential_hash IS NULL OR credential_hash LIKE '$argon2id$v=19$%'),
    CHECK (jsonb_typeof(access_state) = 'object'),
    CHECK (NOT (access_state ?| ARRAY['custom_code', 'pending_persist_code']))
);

CREATE TABLE mini_auth_builtin_identities (
    principal_ref TEXT PRIMARY KEY REFERENCES mini_auth_accounts(principal_ref) ON DELETE RESTRICT,
    phone TEXT NOT NULL,
    display_name TEXT NOT NULL,
    CHECK (principal_ref IN ('admin', 'werka', 'material_taminotchi'))
);

CREATE TABLE mini_auth_cutovers (
    name TEXT PRIMARY KEY,
    completed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE mini_auth_accounts OWNER TO mini_rs_erp_owner;
ALTER TABLE mini_auth_builtin_identities OWNER TO mini_rs_erp_owner;
ALTER TABLE mini_auth_cutovers OWNER TO mini_rs_erp_owner;
REVOKE ALL ON mini_auth_accounts, mini_auth_builtin_identities, mini_auth_cutovers FROM PUBLIC, mini_rs_erp;
GRANT SELECT, INSERT, UPDATE ON mini_auth_accounts, mini_auth_builtin_identities TO mini_rs_erp;
GRANT SELECT, INSERT ON mini_auth_cutovers TO mini_rs_erp;
