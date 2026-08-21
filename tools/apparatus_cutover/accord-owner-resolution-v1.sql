\set ON_ERROR_STOP on

-- Accord production snapshot owner resolution, approved 2026-08-21.
--
-- This is a one-time, fail-closed data preparation for the audited 0061
-- snapshot. It resolves owner decisions that cannot be inferred safely by
-- schema migrations. It does not write migration history and must run before
-- production migration 0062 and canonical migrations 0063-0068.

BEGIN;

SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';
SELECT pg_advisory_xact_lock(
    hashtextextended('mini-rs-erp:accord-apparatus-owner-resolution:v1', 0)
);

LOCK TABLE
    mini_schema_migrations,
    mini_apparatus,
    mini_apparatus_material_rules,
    mini_worker_groups,
    mini_training_progress_batches,
    mini_production_maps,
    mini_production_map_nodes,
    mini_training_production_maps,
    mini_factory_location_apparatus_links,
    mini_apparatus_capacity_profiles,
    mini_apparatus_downtimes,
    mini_apparatus_schedule_reservations
IN SHARE ROW EXCLUSIVE MODE;

CREATE TEMP TABLE _owner_shadow_master_resolution (
    legacy_id TEXT PRIMARY KEY,
    expected_name TEXT NOT NULL,
    canonical_id TEXT NOT NULL
) ON COMMIT DROP;

INSERT INTO _owner_shadow_master_resolution
    (legacy_id, expected_name, canonical_id)
VALUES
    ('apparatus:7 ta rangli pechat', '7 ta rangli pechat', 'apparatus:default:bosma_7'),
    ('apparatus:8 ta rangli pechat', '8 ta rangli pechat', 'apparatus:default:bosma_8'),
    ('apparatus:9 ta rangli pechat', '9 ta rangli pechat', 'apparatus:default:bosma_9');

CREATE TEMP TABLE _owner_production_node_resolution (
    map_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    expected_title TEXT NOT NULL,
    canonical_id TEXT NOT NULL,
    PRIMARY KEY (map_id, node_id)
) ON COMMIT DROP;

INSERT INTO _owner_production_node_resolution
    (map_id, node_id, expected_title, canonical_id)
SELECT map_id, 'apparatus_1', '8 ta rangli bosma aparat', 'apparatus:default:bosma_8'
FROM (VALUES
    ('template-zakaz-0001'),
    ('template-zakaz-0002'),
    ('zakaz-0001'),
    ('zakaz-0002')
) AS maps(map_id)
UNION ALL
SELECT map_id, 'apparatus_2', 'Laminatsiya 1', 'apparatus:default:asset-007'
FROM (VALUES
    ('template-zakaz-0001'),
    ('template-zakaz-0002'),
    ('zakaz-0001'),
    ('zakaz-0002')
) AS maps(map_id)
UNION ALL
SELECT map_id, 'apparatus_3', 'Rezka', 'apparatus:default:asset-010'
FROM (VALUES
    ('template-zakaz-0001'),
    ('template-zakaz-0002'),
    ('zakaz-0001'),
    ('zakaz-0002')
) AS maps(map_id);

CREATE TEMP TABLE _owner_training_node_resolution (
    map_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    expected_title TEXT NOT NULL,
    canonical_id TEXT NOT NULL,
    PRIMARY KEY (map_id, node_id)
) ON COMMIT DROP;

INSERT INTO _owner_training_node_resolution
    (map_id, node_id, expected_title, canonical_id)
VALUES
    ('training-zakaz-0001', 'apparatus', '7 ta rangli bosma aparat', 'apparatus:default:bosma_7'),
    ('training-zakaz-0002', 'apparatus', '9 ta rangli bosma aparat', 'apparatus:default:bosma_9'),
    ('training-zakaz-0004', 'apparatus', '8 ta rangli bosma aparat', 'apparatus:default:bosma_8'),
    ('training-zakaz-0005', 'apparatus', '7 ta rangli bosma aparat', 'apparatus:default:bosma_7'),
    ('training-zakaz-0006', 'apparatus', '8 ta rangli bosma aparat', 'apparatus:default:bosma_8'),
    ('training-zakaz-0008', 'apparatus', 'Laminatsiya 1', 'apparatus:default:asset-007'),
    ('training-zakaz-0009', 'apparatus', 'Flexo pechat', 'apparatus:default:asset-005'),
    ('training-zakaz-0010', 'apparatus', 'Rezka', 'apparatus:default:asset-010');

CREATE TEMP TABLE _owner_training_input_resolution (
    batch_id TEXT PRIMARY KEY,
    order_id TEXT NOT NULL,
    expected_source_apparatus TEXT NOT NULL,
    target_display_name TEXT NOT NULL,
    canonical_id TEXT NOT NULL
) ON COMMIT DROP;

INSERT INTO _owner_training_input_resolution
    (batch_id, order_id, expected_source_apparatus, target_display_name, canonical_id)
VALUES
    (
        'progress-batch:1786599447914338358:bosma-aparat:training-zakaz-0008:complete',
        'training-zakaz-0008',
        'Bosma aparat',
        'Laminatsiya 1',
        'apparatus:default:asset-007'
    ),
    (
        'progress-batch:1786702853883715642:laminatsiya-aparat:training-zakaz-0010:complete',
        'training-zakaz-0010',
        'Laminatsiya aparat',
        'Rezka',
        'apparatus:default:asset-010'
    );

DO $$
DECLARE
    migration_head TEXT;
    row_count BIGINT;
BEGIN
    SELECT version
    INTO migration_head
    FROM mini_schema_migrations
    ORDER BY substring(version FROM 1 FOR 4)::INTEGER DESC
    LIMIT 1;

    IF migration_head <> '0061_order_reset_append_only_override'
       OR (SELECT count(*) FROM mini_schema_migrations) <> 61
    THEN
        RAISE EXCEPTION
            'owner resolution requires the audited 0061 migration head with 61 entries; found % with % entries',
            migration_head,
            (SELECT count(*) FROM mini_schema_migrations);
    END IF;

    SELECT count(*)
    INTO row_count
    FROM _owner_shadow_master_resolution expected
    JOIN mini_apparatus actual
     ON actual.id = expected.legacy_id
     AND actual.name = expected.expected_name
     AND actual.base_name = ''
     AND actual.payload_json = jsonb_build_object('warehouse', expected.expected_name);
    IF row_count <> 3
       OR (SELECT count(*) FROM mini_apparatus) <> 13
    THEN
        RAISE EXCEPTION
            'shadow apparatus precondition mismatch: expected 3 exact rows inside 13 masters, found % inside %',
            row_count,
            (SELECT count(*) FROM mini_apparatus);
    END IF;

    IF (SELECT count(*)
        FROM mini_apparatus actual
        JOIN _owner_shadow_master_resolution expected
          ON actual.id = expected.canonical_id) <> 3
    THEN
        RAISE EXCEPTION 'canonical bosma master precondition mismatch';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM mini_factory_location_apparatus_links link
        JOIN _owner_shadow_master_resolution expected
          ON expected.legacy_id = link.apparatus_id
        UNION ALL
        SELECT 1
        FROM mini_apparatus_capacity_profiles profile
        JOIN _owner_shadow_master_resolution expected
          ON expected.legacy_id = profile.apparatus_id
        UNION ALL
        SELECT 1
        FROM mini_apparatus_downtimes downtime
        JOIN _owner_shadow_master_resolution expected
          ON expected.legacy_id = downtime.apparatus_id
        UNION ALL
        SELECT 1
        FROM mini_apparatus_schedule_reservations reservation
        JOIN _owner_shadow_master_resolution expected
          ON expected.legacy_id = reservation.apparatus_id
    ) THEN
        RAISE EXCEPTION 'shadow apparatus has a foreign-keyed dependent row';
    END IF;

    SELECT count(*)
    INTO row_count
    FROM mini_apparatus_material_rules
    WHERE (apparatus = '7 ta rangli pechat'
           AND item_groups = '["Kley", "rulon"]'::jsonb
           AND requires_material
           AND requirement_groups = '[{"name":"Kley","item_groups":["Kley"],"min_required_count":1},{"name":"rulon","item_groups":["rulon"],"min_required_count":1}]'::jsonb)
       OR (apparatus IN ('8 ta rangli pechat', '9 ta rangli pechat')
           AND item_groups = '["rulon"]'::jsonb
           AND requires_material
           AND requirement_groups = '[{"name":"rulon","item_groups":["rulon"],"min_required_count":1}]'::jsonb);
    IF row_count <> 3 THEN
        RAISE EXCEPTION
            'legacy pechat material-rule precondition mismatch: expected 3 exact rows, found %',
            row_count;
    END IF;

    SELECT count(*)
    INTO row_count
    FROM mini_apparatus_material_rules
    WHERE apparatus IN (
              '7 ta rangli bosma aparat',
              '8 ta rangli bosma aparat',
              '9 ta rangli bosma aparat'
          )
      AND item_groups = '["kraska", "rulon"]'::jsonb
      AND NOT requires_material;
    IF row_count <> 3 THEN
        RAISE EXCEPTION
            'authoritative bosma material-rule precondition mismatch: expected 3 rows, found %',
            row_count;
    END IF;

    SELECT count(*)
    INTO row_count
    FROM mini_worker_groups
    WHERE apparatus = 'worker-settings'
      AND group_code IN ('E', 'G', 'H')
      AND shift = 'kunduz'
      AND worker_ids = '[]'::jsonb
      AND NOT accounting_enabled
      AND payload_json->>'apparatus' = 'worker-settings';
    IF row_count <> 3
       OR (SELECT count(*) FROM mini_worker_groups WHERE apparatus = 'worker-settings') <> 3
    THEN
        RAISE EXCEPTION
            'worker-settings precondition mismatch: expected exactly 3 empty E/G/H rows';
    END IF;

    SELECT count(*)
    INTO row_count
    FROM _owner_training_input_resolution expected
    JOIN mini_training_progress_batches actual
      ON actual.batch_id = expected.batch_id
     AND actual.order_id = expected.order_id
     AND actual.apparatus = expected.expected_source_apparatus
     AND actual.payload_json->>'apparatus' = expected.expected_source_apparatus
     AND actual.payload_json->>'current_apparatus' = expected.target_display_name
     AND actual.payload_json->>'next_apparatus' = expected.target_display_name
     AND actual.payload_json->>'used_by_apparatus' = expected.target_display_name
     AND actual.payload_json #>> '{payload_json,source_apparatus}' =
         expected.expected_source_apparatus;
    IF row_count <> 2 THEN
        RAISE EXCEPTION
            'training input precondition mismatch: expected 2 exact virtual-source rows, found %',
            row_count;
    END IF;

    SELECT count(*)
    INTO row_count
    FROM _owner_production_node_resolution expected
    JOIN mini_production_maps map_row ON map_row.id = expected.map_id
    CROSS JOIN LATERAL jsonb_array_elements(map_row.map_json->'nodes') node
    WHERE node->>'id' = expected.node_id
      AND node->>'kind' = 'apparatus'
      AND node->>'title' = expected.expected_title
      AND btrim(COALESCE(node->>'apparatus_id', '')) = '';
    IF row_count <> 12 OR (
        SELECT count(*)
        FROM mini_production_maps map_row
        CROSS JOIN LATERAL jsonb_array_elements(map_row.map_json->'nodes') node
        WHERE node->>'kind' = 'apparatus'
          AND btrim(COALESCE(node->>'apparatus_id', '')) = ''
    ) <> 12 THEN
        RAISE EXCEPTION
            'production-map owner mapping precondition mismatch: expected exactly 12 blank nodes, found % exact',
            row_count;
    END IF;

    SELECT count(*)
    INTO row_count
    FROM _owner_production_node_resolution expected
    JOIN mini_production_map_nodes actual
      ON actual.map_id = expected.map_id
     AND actual.node_id = expected.node_id
     AND actual.kind = 'apparatus'
     AND actual.title = expected.expected_title
     AND btrim(COALESCE(actual.payload_json->>'apparatus_id', '')) = '';
    IF row_count <> 12 OR (
        SELECT count(*)
        FROM mini_production_map_nodes
        WHERE kind = 'apparatus'
          AND btrim(COALESCE(payload_json->>'apparatus_id', '')) = ''
    ) <> 12 THEN
        RAISE EXCEPTION
            'production-map mirror precondition mismatch: expected exactly 12 blank nodes, found % exact',
            row_count;
    END IF;

    SELECT count(*)
    INTO row_count
    FROM _owner_training_node_resolution expected
    JOIN mini_training_production_maps map_row ON map_row.id = expected.map_id
    CROSS JOIN LATERAL jsonb_array_elements(map_row.map_json->'nodes') node
    WHERE node->>'id' = expected.node_id
      AND node->>'kind' = 'apparatus'
      AND node->>'title' = expected.expected_title
      AND btrim(COALESCE(node->>'apparatus_id', '')) = '';
    IF row_count <> 8 OR (
        SELECT count(*)
        FROM mini_training_production_maps map_row
        CROSS JOIN LATERAL jsonb_array_elements(map_row.map_json->'nodes') node
        WHERE node->>'kind' = 'apparatus'
          AND btrim(COALESCE(node->>'apparatus_id', '')) = ''
    ) <> 8 THEN
        RAISE EXCEPTION
            'training-map owner mapping precondition mismatch: expected exactly 8 blank nodes, found % exact',
            row_count;
    END IF;
END
$$;

DELETE FROM mini_apparatus_material_rules
WHERE apparatus IN (
    '7 ta rangli pechat',
    '8 ta rangli pechat',
    '9 ta rangli pechat'
);

DELETE FROM mini_worker_groups
WHERE apparatus = 'worker-settings'
  AND group_code IN ('E', 'G', 'H');

DELETE FROM mini_apparatus master
USING _owner_shadow_master_resolution expected
WHERE master.id = expected.legacy_id;

UPDATE mini_training_progress_batches target
SET apparatus = expected.target_display_name,
    payload_json = jsonb_set(
        jsonb_set(
            jsonb_set(
                jsonb_set(
                    target.payload_json,
                    '{apparatus}',
                    to_jsonb(expected.canonical_id),
                    true
                ),
                '{current_apparatus}',
                to_jsonb(expected.canonical_id),
                true
            ),
            '{next_apparatus}',
            to_jsonb(expected.canonical_id),
            true
        ),
        '{used_by_apparatus}',
        to_jsonb(expected.canonical_id),
        true
    )
FROM _owner_training_input_resolution expected
WHERE target.batch_id = expected.batch_id;

UPDATE mini_production_maps map_row
SET map_json = jsonb_set(
    map_row.map_json,
    '{nodes}',
    (
        SELECT jsonb_agg(
            CASE
                WHEN expected.canonical_id IS NULL THEN nodes.node
                ELSE jsonb_set(
                    nodes.node,
                    '{apparatus_id}',
                    to_jsonb(expected.canonical_id),
                    true
                )
            END
            ORDER BY nodes.ordinality
        )
        FROM jsonb_array_elements(map_row.map_json->'nodes')
             WITH ORDINALITY AS nodes(node, ordinality)
        LEFT JOIN _owner_production_node_resolution expected
          ON expected.map_id = map_row.id
         AND expected.node_id = nodes.node->>'id'
    ),
    true
)
WHERE map_row.id IN (
    SELECT DISTINCT map_id FROM _owner_production_node_resolution
);

UPDATE mini_production_map_nodes target
SET payload_json = jsonb_set(
    target.payload_json,
    '{apparatus_id}',
    to_jsonb(expected.canonical_id),
    true
)
FROM _owner_production_node_resolution expected
WHERE target.map_id = expected.map_id
  AND target.node_id = expected.node_id;

UPDATE mini_training_production_maps map_row
SET map_json = jsonb_set(
    map_row.map_json,
    '{nodes}',
    (
        SELECT jsonb_agg(
            CASE
                WHEN expected.canonical_id IS NULL THEN nodes.node
                ELSE jsonb_set(
                    nodes.node,
                    '{apparatus_id}',
                    to_jsonb(expected.canonical_id),
                    true
                )
            END
            ORDER BY nodes.ordinality
        )
        FROM jsonb_array_elements(map_row.map_json->'nodes')
             WITH ORDINALITY AS nodes(node, ordinality)
        LEFT JOIN _owner_training_node_resolution expected
          ON expected.map_id = map_row.id
         AND expected.node_id = nodes.node->>'id'
    ),
    true
)
WHERE map_row.id IN (
    SELECT map_id FROM _owner_training_node_resolution
);

DO $$
DECLARE
    row_count BIGINT;
BEGIN
    IF (SELECT count(*) FROM mini_schema_migrations) <> 61
       OR (SELECT version
           FROM mini_schema_migrations
           ORDER BY substring(version FROM 1 FOR 4)::INTEGER DESC
           LIMIT 1) <> '0061_order_reset_append_only_override'
    THEN
        RAISE EXCEPTION 'owner resolution changed migration history';
    END IF;

    IF (SELECT count(*) FROM mini_apparatus) <> 10
       OR EXISTS (
           SELECT 1
           FROM mini_apparatus master
           JOIN _owner_shadow_master_resolution expected
             ON expected.legacy_id = master.id
       )
    THEN
        RAISE EXCEPTION 'shadow apparatus postcondition mismatch';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM mini_apparatus_material_rules
        WHERE apparatus IN (
            '7 ta rangli pechat',
            '8 ta rangli pechat',
            '9 ta rangli pechat'
        )
    ) OR (
        SELECT count(*)
        FROM mini_apparatus_material_rules
        WHERE apparatus IN (
            '7 ta rangli bosma aparat',
            '8 ta rangli bosma aparat',
            '9 ta rangli bosma aparat'
        )
          AND item_groups = '["kraska", "rulon"]'::jsonb
          AND NOT requires_material
    ) <> 3 THEN
        RAISE EXCEPTION 'material-rule postcondition mismatch';
    END IF;

    IF EXISTS (
        SELECT 1 FROM mini_worker_groups WHERE apparatus = 'worker-settings'
    ) THEN
        RAISE EXCEPTION 'worker-settings postcondition mismatch';
    END IF;

    SELECT count(*)
    INTO row_count
    FROM _owner_training_input_resolution expected
    JOIN mini_training_progress_batches actual
      ON actual.batch_id = expected.batch_id
     AND actual.apparatus = expected.target_display_name
     AND actual.payload_json->>'apparatus' = expected.canonical_id
     AND actual.payload_json->>'current_apparatus' = expected.canonical_id
     AND actual.payload_json->>'next_apparatus' = expected.canonical_id
     AND actual.payload_json->>'used_by_apparatus' = expected.canonical_id
     AND actual.payload_json #>> '{payload_json,source_apparatus}' =
         expected.expected_source_apparatus;
    IF row_count <> 2 THEN
        RAISE EXCEPTION 'training input postcondition mismatch';
    END IF;

    SELECT count(*)
    INTO row_count
    FROM _owner_production_node_resolution expected
    JOIN mini_production_maps map_row ON map_row.id = expected.map_id
    CROSS JOIN LATERAL jsonb_array_elements(map_row.map_json->'nodes') node
    WHERE node->>'id' = expected.node_id
      AND node->>'apparatus_id' = expected.canonical_id;
    IF row_count <> 12 THEN
        RAISE EXCEPTION 'production-map JSON postcondition mismatch';
    END IF;

    SELECT count(*)
    INTO row_count
    FROM _owner_production_node_resolution expected
    JOIN mini_production_map_nodes actual
      ON actual.map_id = expected.map_id
     AND actual.node_id = expected.node_id
     AND actual.payload_json->>'apparatus_id' = expected.canonical_id;
    IF row_count <> 12 THEN
        RAISE EXCEPTION 'production-map mirror postcondition mismatch';
    END IF;

    SELECT count(*)
    INTO row_count
    FROM _owner_training_node_resolution expected
    JOIN mini_training_production_maps map_row ON map_row.id = expected.map_id
    CROSS JOIN LATERAL jsonb_array_elements(map_row.map_json->'nodes') node
    WHERE node->>'id' = expected.node_id
      AND node->>'apparatus_id' = expected.canonical_id;
    IF row_count <> 8 THEN
        RAISE EXCEPTION 'training-map postcondition mismatch';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM mini_production_maps map_row
        CROSS JOIN LATERAL jsonb_array_elements(map_row.map_json->'nodes') node
        WHERE node->>'kind' = 'apparatus'
          AND btrim(COALESCE(node->>'apparatus_id', '')) = ''
        UNION ALL
        SELECT 1
        FROM mini_production_map_nodes
        WHERE kind = 'apparatus'
          AND btrim(COALESCE(payload_json->>'apparatus_id', '')) = ''
        UNION ALL
        SELECT 1
        FROM mini_training_production_maps map_row
        CROSS JOIN LATERAL jsonb_array_elements(map_row.map_json->'nodes') node
        WHERE node->>'kind' = 'apparatus'
          AND btrim(COALESCE(node->>'apparatus_id', '')) = ''
    ) THEN
        RAISE EXCEPTION 'blank apparatus node remains after owner resolution';
    END IF;
END
$$;

COMMIT;
