DO $$
DECLARE
    table_name TEXT;
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        RETURN;
    END IF;

    FOREACH table_name IN ARRAY ARRAY[
        'mini_system_users',
        'mini_chat_principals',
        'mini_chat_conversations',
        'mini_chat_conversation_members',
        'mini_chat_messages',
        'mini_chat_device_cursors',
        'mini_chat_outbox_events',
        'mini_returned_paint_requests'
    ]
    LOOP
        IF to_regclass(format('public.%I', table_name)) IS NOT NULL THEN
            EXECUTE format('ALTER TABLE public.%I OWNER TO mini_rs_erp', table_name);
        END IF;
    END LOOP;
END;
$$;
