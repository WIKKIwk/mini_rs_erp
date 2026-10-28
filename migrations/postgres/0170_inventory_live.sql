-- A dedicated bus avoids rebuilding inventory windows for unrelated queue and
-- preflight traffic. Row triggers skip zero-row and unchanged reconciliation
-- writes. PostgreSQL folds identical NOTIFY payloads until transaction COMMIT.
CREATE OR REPLACE FUNCTION mini_notify_inventory_live()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'UPDATE' AND OLD IS NOT DISTINCT FROM NEW THEN
        RETURN NULL;
    END IF;
    PERFORM pg_notify('mini_inventory_live', 'state');
    RETURN NULL;
END;
$$;

DO $$
DECLARE target record;
BEGIN
    FOR target IN
        SELECT n.nspname, c.relname
        FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = current_schema() AND c.relkind IN ('r', 'p')
          AND NOT c.relispartition
          AND c.relname IN (
              'mini_inventory_locations', 'mini_inventory_placements',
              'mini_inventory_transfers', 'mini_inventory_transfer_lines',
              'mini_raw_material_stock', 'mini_finished_goods_stock',
              'mini_qolip_locations', 'mini_raw_material_assignments',
              'mini_order_control_states', 'mini_production_maps',
              'mini_apparatus', 'mini_factory_locations',
              'mini_factory_location_apparatus_links', 'mini_warehouses',
              'mini_warehouse_assignments', 'mini_auth_accounts',
              'mini_auth_builtin_identities', 'mini_items', 'mini_item_groups'
          )
    LOOP
        EXECUTE format(
            'CREATE TRIGGER mini_inventory_live_changed AFTER INSERT OR UPDATE OR DELETE ON %I.%I FOR EACH ROW EXECUTE FUNCTION mini_notify_inventory_live()',
            target.nspname, target.relname);
        EXECUTE format(
            'CREATE TRIGGER mini_inventory_live_truncated AFTER TRUNCATE ON %I.%I FOR EACH STATEMENT EXECUTE FUNCTION mini_notify_inventory_live()',
            target.nspname, target.relname);
    END LOOP;
END;
$$;
