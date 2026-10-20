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
    -- Reconciliation reads perform guarded UPSERT/DELETE statements. Statement
    -- triggers fire even when those statements affect zero rows, creating a
    -- read -> invalidation -> read loop. Row triggers below ignore zero-row
    -- writes; this guard also ignores a matched UPDATE with unchanged values.
    IF TG_OP = 'UPDATE' THEN
        IF OLD IS NOT DISTINCT FROM NEW THEN
            RETURN NULL;
        END IF;
    END IF;
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
        SELECT n.nspname, c.relname, c.relispartition AS is_partition
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
        -- Parent row triggers are inherited by partitions automatically.
        IF NOT target.is_partition THEN
            EXECUTE format(
                'CREATE TRIGGER mini_production_live_changed AFTER INSERT OR UPDATE OR DELETE ON %I.%I FOR EACH ROW EXECUTE FUNCTION mini_notify_production_live()',
                target.nspname, target.relname
            );
        END IF;
        EXECUTE format(
            'CREATE TRIGGER mini_production_live_truncated AFTER TRUNCATE ON %I.%I FOR EACH STATEMENT EXECUTE FUNCTION mini_notify_production_live()',
            target.nspname, target.relname
        );
    END LOOP;
END;
$$;
