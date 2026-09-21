-- Master-owned catalog changes preserve immutable receipt/command history.
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        GRANT UPDATE, DELETE ON mini_preparation_materials TO mini_rs_erp;
        GRANT DELETE ON mini_preparation_warehouse_history_names TO mini_rs_erp;
    END IF;
END $$;
