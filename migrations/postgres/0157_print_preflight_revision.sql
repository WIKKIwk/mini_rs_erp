ALTER TABLE mini_print_preflight_holds
    ADD COLUMN revision BIGINT NOT NULL DEFAULT 0
    CHECK (revision >= 0);
