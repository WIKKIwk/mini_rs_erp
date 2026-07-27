-- Stable apparatus identities for factory-location assignments. Existing
-- production flows remain name-based. This catalog identity is additive.
INSERT INTO mini_apparatus (id, name, base_name, kind, payload_json)
VALUES
    ('apparatus:default:bosma_7', '7 ta rangli bosma aparat', '7 ta rangli bosma aparat', 'default', '{"sort_order": 0}'::jsonb),
    ('apparatus:default:bosma_8', '8 ta rangli bosma aparat', '8 ta rangli bosma aparat', 'default', '{"sort_order": 1}'::jsonb),
    ('apparatus:default:bosma_9', '9 ta rangli bosma aparat', '9 ta rangli bosma aparat', 'default', '{"sort_order": 2}'::jsonb),
    ('apparatus:default:extruder_laminatsiya', 'Extruder laminatsiya', 'Extruder laminatsiya', 'default', '{"sort_order": 3}'::jsonb),
    ('apparatus:default:flexo_pechat', 'Flexo pechat', 'Flexo pechat', 'default', '{"sort_order": 4}'::jsonb),
    ('apparatus:default:holodniy_kley', 'Holodniy kley aparat', 'Holodniy kley aparat', 'default', '{"sort_order": 5}'::jsonb),
    ('apparatus:default:laminatsiya_1', 'Laminatsiya 1', 'Laminatsiya 1', 'default', '{"sort_order": 6}'::jsonb),
    ('apparatus:default:laminatsiya_2', 'Laminatsiya 2', 'Laminatsiya 2', 'default', '{"sort_order": 7}'::jsonb),
    ('apparatus:default:paket', 'Paket aparat', 'Paket aparat', 'default', '{"sort_order": 8}'::jsonb),
    ('apparatus:default:rezka', 'Rezka', 'Rezka', 'default', '{"sort_order": 9}'::jsonb)
ON CONFLICT ((lower(name))) DO UPDATE SET
    id = excluded.id,
    base_name = excluded.base_name,
    kind = excluded.kind,
    payload_json = excluded.payload_json,
    updated_at = now();

CREATE TABLE IF NOT EXISTS mini_factory_locations (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_factory_locations_id_not_blank CHECK (btrim(id) <> ''),
    CONSTRAINT mini_factory_locations_name_not_blank CHECK (btrim(name) <> '')
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_factory_locations_lower_name
    ON mini_factory_locations (lower(name));

CREATE TABLE IF NOT EXISTS mini_factory_location_apparatus_links (
    location_id TEXT NOT NULL
        REFERENCES mini_factory_locations(id) ON DELETE CASCADE,
    apparatus_id TEXT NOT NULL
        REFERENCES mini_apparatus(id) ON DELETE RESTRICT,
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (location_id, apparatus_id)
);

CREATE INDEX IF NOT EXISTS idx_mini_factory_location_apparatus_links_apparatus
    ON mini_factory_location_apparatus_links (apparatus_id);

-- Migrations may run as an owner role while the application runs as
-- mini_rs_erp. Fail closed if the runtime grants cannot be guaranteed.
DO $$
DECLARE
    table_name TEXT;
    table_owner TEXT;
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mini_rs_erp') THEN
        RAISE EXCEPTION 'required runtime role mini_rs_erp does not exist';
    END IF;

    FOREACH table_name IN ARRAY ARRAY[
        'mini_factory_locations',
        'mini_factory_location_apparatus_links'
    ]
    LOOP
        SELECT tableowner
        INTO table_owner
        FROM pg_tables
        WHERE schemaname = 'public' AND tablename = table_name;

        IF table_owner = current_user
            OR pg_has_role(current_user, table_owner, 'MEMBER')
        THEN
            EXECUTE format(
                'GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.%I TO mini_rs_erp',
                table_name
            );
        ELSIF NOT has_table_privilege(
            'mini_rs_erp',
            format('public.%I', table_name),
            'SELECT,INSERT,UPDATE,DELETE'
        ) THEN
            RAISE EXCEPTION
                'migration user % cannot grant factory location privileges on public.% owned by %; run migrations with MINI_ERP_MIGRATION_DATABASE_URL using the table owner',
                current_user,
                table_name,
                table_owner;
        END IF;
    END LOOP;
END;
$$;
