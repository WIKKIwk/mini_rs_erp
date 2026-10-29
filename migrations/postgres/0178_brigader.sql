ALTER TABLE mini_system_users DROP CONSTRAINT mini_system_users_role_allowed;
ALTER TABLE mini_system_users ADD CONSTRAINT mini_system_users_role_allowed
    CHECK (role IN ('qolipchi', 'boyoqchi', 'tayyorlov_masteri', 'homashyo_rezkachi', 'sales_manager', 'brigader'));
