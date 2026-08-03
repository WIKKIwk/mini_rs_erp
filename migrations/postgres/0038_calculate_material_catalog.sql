CREATE TABLE IF NOT EXISTS mini_calculate_materials (
    id TEXT PRIMARY KEY,
    lower_name TEXT NOT NULL UNIQUE,
    payload_json JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_mini_calculate_materials_name
    ON mini_calculate_materials(lower_name);

DO $$
DECLARE
    table_owner TEXT;
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        RETURN;
    END IF;

    SELECT tableowner
    INTO table_owner
    FROM pg_tables
    WHERE schemaname = 'public' AND tablename = 'mini_calculate_materials';

    IF table_owner = current_user
        OR pg_has_role(current_user, table_owner, 'MEMBER')
    THEN
        GRANT SELECT, INSERT, UPDATE, DELETE
            ON TABLE mini_calculate_materials TO mini_rs_erp;
    ELSIF NOT has_table_privilege(
        'mini_rs_erp',
        'public.mini_calculate_materials',
        'SELECT,INSERT,UPDATE,DELETE'
    ) THEN
        RAISE EXCEPTION
            'migration user % cannot grant calculate material privileges on public.mini_calculate_materials owned by %',
            current_user,
            table_owner;
    END IF;
END;
$$;
