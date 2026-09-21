-- Keep authentication hashes and recoverable admin-managed codes separately.
-- Ciphertexts are bound to both the principal and its current credential hash.
CREATE TABLE mini_auth_code_vault (
    principal_ref TEXT PRIMARY KEY REFERENCES mini_auth_accounts(principal_ref) ON DELETE CASCADE,
    credential_hash TEXT NOT NULL,
    encrypted_code TEXT NOT NULL CHECK (encrypted_code LIKE 'v1:%'),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
ALTER TABLE mini_auth_code_vault OWNER TO mini_rs_erp_owner;
REVOKE ALL ON mini_auth_code_vault FROM PUBLIC, mini_rs_erp;
GRANT SELECT, INSERT, UPDATE ON mini_auth_code_vault TO mini_rs_erp;
