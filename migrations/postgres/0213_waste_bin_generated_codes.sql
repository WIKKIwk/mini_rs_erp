SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

CREATE SEQUENCE mini_waste_bin_code_seq;
ALTER SEQUENCE mini_waste_bin_code_seq OWNER TO mini_rs_erp_owner;
REVOKE ALL ON SEQUENCE mini_waste_bin_code_seq FROM PUBLIC;
GRANT USAGE ON SEQUENCE mini_waste_bin_code_seq TO mini_rs_erp;

ALTER TABLE mini_waste_bins
    ADD COLUMN registration_request_id TEXT
        CHECK (octet_length(registration_request_id) BETWEEN 1 AND 1024);
CREATE UNIQUE INDEX mini_waste_bin_registration_request
    ON mini_waste_bins (registered_by_ref, registration_request_id)
    WHERE registration_request_id IS NOT NULL;
