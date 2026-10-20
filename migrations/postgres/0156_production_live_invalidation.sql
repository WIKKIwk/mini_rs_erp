-- NOTIFY is delivered only at COMMIT and duplicate channel/payload pairs are
-- folded within one transaction. No customer data or graph is sent on the bus.
-- LISTEN reconnects must invalidate cached state because notifications are not
-- durable; the application listener explicitly handles that boundary.
CREATE OR REPLACE FUNCTION mini_notify_production_live()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    event_payload text;
BEGIN
    IF TG_TABLE_NAME IN (
        'mini_apparatus',
        'mini_apparatus_queue_policies',
        'mini_apparatus_material_rules',
        'mini_apparatus_capacity_profiles',
        'mini_canonical_apparatus_heads',
        'mini_canonical_apparatus_revisions'
    ) THEN
        event_payload := 'apparatus';
    ELSE
        -- Only dedicated print-preflight store transactions set this local
        -- value. Ordinary queue actions and external SQL remain conservative.
        event_payload := NULLIF(current_setting('mini.production_live', true), '');
        IF event_payload IS NULL OR octet_length(event_payload) > 7000 THEN
            event_payload := 'state';
        END IF;
    END IF;
    PERFORM pg_notify('mini_production_live', event_payload);
    RETURN NULL;
END;
$$;

-- Cover the persisted inputs to order pages, queue permissions, worker
-- histories, training overlays, customer labels and apparatus configuration.
-- This intentionally includes indirect resource dependencies: changing WIP or
-- material/tool availability can alter another order's action permissions.
-- Future migrations introducing another projection input must attach the same
-- trigger. Views are excluded; their underlying domain tables are included.
DO $$
DECLARE
    target record;
BEGIN
    FOR target IN
        SELECT n.nspname, c.relname
        FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = current_schema()
          AND c.relkind IN ('r', 'p')
          AND (
              c.relname ~ '^mini_(production_|queue_|order_|orders$|progress_|print_preflight_|opening_wip_|raw_material_|qolip_|paddon|active_rezka_|apparatus|canonical_apparatus_|training_|worker|customer|item|factory_location|inventory_|finished_goods_|pending_orders$|preparation_|layer_remainder_|bosma_astatka_|laminat|rezka_astatka_|completion_)'
              OR c.relname IN ('mini_quick_order_templates', 'mini_quick_order_images')
          )
          AND c.relname NOT LIKE '%chat_outbox'
          AND c.relname NOT LIKE '%change_outbox'
    LOOP
        EXECUTE format(
            'CREATE TRIGGER mini_production_live_changed AFTER INSERT OR UPDATE OR DELETE OR TRUNCATE ON %I.%I FOR EACH STATEMENT EXECUTE FUNCTION mini_notify_production_live()',
            target.nspname, target.relname
        );
    END LOOP;
END;
$$;
