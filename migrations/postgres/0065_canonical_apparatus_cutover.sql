-- Final canonical-apparatus cutover.
--
-- 0063 and 0064 intentionally staged nullable columns.  This migration is
-- the only point at which legacy apparatus text is resolved.  Resolution is
-- deterministic and exact (trimmed, case-insensitive equality only): an
-- explicit legacy mapping or exactly one mini_apparatus id/name match is
-- required.  Zero matches and ambiguous matches abort this transaction.

CREATE TEMP TABLE _canonical_apparatus_explicit_map (
    legacy_key TEXT PRIMARY KEY,
    canonical_id TEXT NOT NULL
) ON COMMIT DROP;

INSERT INTO _canonical_apparatus_explicit_map (legacy_key, canonical_id)
VALUES
    ('apparatus:default:extruder_laminatsiya', 'apparatus:default:asset-004'),
    ('extruder laminatsiya', 'apparatus:default:asset-004'),
    ('apparatus:default:flexo_pechat', 'apparatus:default:asset-005'),
    ('flexo pechat', 'apparatus:default:asset-005'),
    ('apparatus:default:laminatsiya_1', 'apparatus:default:asset-007'),
    ('laminatsiya 1', 'apparatus:default:asset-007'),
    ('apparatus:default:laminatsiya_2', 'apparatus:default:asset-008'),
    ('laminatsiya 2', 'apparatus:default:asset-008'),
    ('apparatus:default:rezka', 'apparatus:default:asset-010'),
    ('rezka', 'apparatus:default:asset-010'),
    ('apparatus:default:bosma_7', 'apparatus:default:bosma_7'),
    ('7 ta rangli bosma aparat', 'apparatus:default:bosma_7'),
    ('7 ta rangli bosma', 'apparatus:default:bosma_7'),
    ('7 ta rangli pechat', 'apparatus:default:bosma_7'),
    ('apparatus:default:bosma_8', 'apparatus:default:bosma_8'),
    ('8 ta rangli bosma aparat', 'apparatus:default:bosma_8'),
    ('8 ta rangli bosma', 'apparatus:default:bosma_8'),
    ('8 ta rangli pechat', 'apparatus:default:bosma_8'),
    ('apparatus:default:bosma_9', 'apparatus:default:bosma_9'),
    ('9 ta rangli bosma aparat', 'apparatus:default:bosma_9'),
    ('9 ta rangli bosma', 'apparatus:default:bosma_9'),
    ('9 ta rangli pechat', 'apparatus:default:bosma_9'),
    ('apparatus:default:holodniy_kley', 'apparatus:default:holodniy_kley'),
    ('holodniy kley aparat', 'apparatus:default:holodniy_kley'),
    ('holodniy kley', 'apparatus:default:holodniy_kley'),
    ('apparatus:default:paket', 'apparatus:default:paket'),
    ('paket aparat', 'apparatus:default:paket'),
    ('paket', 'apparatus:default:paket'),
    ('rezka apparat', 'apparatus:default:asset-010');

DO $$
DECLARE
    mapping RECORD;
    row_count BIGINT;
BEGIN
    FOR mapping IN
        SELECT *
        FROM (VALUES
            ('apparatus:default:extruder_laminatsiya', 'apparatus:default:asset-004', 'Extruder laminatsiya'),
            ('apparatus:default:flexo_pechat', 'apparatus:default:asset-005', 'Flexo pechat'),
            ('apparatus:default:laminatsiya_1', 'apparatus:default:asset-007', 'Laminatsiya 1'),
            ('apparatus:default:laminatsiya_2', 'apparatus:default:asset-008', 'Laminatsiya 2'),
            ('apparatus:default:rezka', 'apparatus:default:asset-010', 'Rezka')
        ) AS v(old_id, new_id, display_name)
    LOOP
        EXECUTE 'SELECT count(*)
                 FROM mini_apparatus
                 WHERE id = $1
                   AND lower(btrim(name)) = lower(btrim($2))'
            INTO row_count USING mapping.old_id, mapping.display_name;
        IF row_count <> 1 THEN
            RAISE EXCEPTION
                '0065 legacy apparatus mapping requires exactly one master row for id % with expected name %, found %',
                mapping.old_id, mapping.display_name, row_count;
        END IF;

        EXECUTE 'SELECT count(*) FROM mini_apparatus WHERE id = $1'
            INTO row_count USING mapping.new_id;
        IF row_count <> 0 THEN
            RAISE EXCEPTION
                '0065 opaque canonical target % already exists while legacy id % is present',
                mapping.new_id, mapping.old_id;
        END IF;
    END LOOP;
END
$$;

-- Every master row must receive a canonical two-segment ID before references
-- are backfilled.  Existing canonical IDs are retained unless they are
-- title-derived; arbitrary legacy IDs use a deterministic UTF-8 hex mapping,
-- which is stable across stores and cannot become a one-segment ID.  The
-- length and collision checks deliberately abort instead of guessing.
CREATE TEMP TABLE _canonical_apparatus_master_map (
    legacy_id TEXT PRIMARY KEY,
    canonical_id TEXT NOT NULL
) ON COMMIT DROP;

INSERT INTO _canonical_apparatus_master_map (legacy_id, canonical_id)
SELECT master.id,
       CASE
           WHEN explicit_map.canonical_id IS NOT NULL
               THEN explicit_map.canonical_id
           WHEN master.id = btrim(master.id)
                AND octet_length(master.id) <= 128
                AND master.id !~ '[[:space:][:cntrl:]]'
                AND master.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
                AND regexp_replace(
                        lower(split_part(master.id, ':', 3)),
                        '[^a-z0-9]', '', 'g'
                    ) <> regexp_replace(
                        lower(master.name),
                        '[^a-z0-9]', '', 'g'
                    )
               THEN master.id
           ELSE 'apparatus:legacy:' || encode(
               convert_to(
                   'mini-rs-erp/apparatus-legacy-id/v1:id:' || master.id,
                   'UTF8'
               ),
               'hex'
           )
       END
FROM mini_apparatus master
LEFT JOIN _canonical_apparatus_explicit_map explicit_map
  ON explicit_map.legacy_key = lower(btrim(master.id));

DO $$
DECLARE duplicate_id TEXT;
BEGIN
    SELECT canonical_id
    INTO duplicate_id
    FROM _canonical_apparatus_master_map
    GROUP BY canonical_id
    HAVING count(*) > 1
    ORDER BY canonical_id
    LIMIT 1;
    IF duplicate_id IS NOT NULL THEN
        RAISE EXCEPTION
            '0065 deterministic canonical apparatus mapping collides at %, add an explicit mapping',
            duplicate_id;
    END IF;

    IF EXISTS (
        SELECT 1
        FROM _canonical_apparatus_master_map
        WHERE octet_length(canonical_id) > 128
           OR canonical_id <> btrim(canonical_id)
           OR canonical_id ~ '[[:space:][:cntrl:]]'
           OR canonical_id !~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
    ) THEN
        RAISE EXCEPTION
            '0065 legacy apparatus ID needs an explicit safe migration mapping';
    END IF;
END
$$;

-- Build the complete exact legacy-id/name map.  A duplicate name or other
-- ambiguous legacy key is a migration error, never a first-row choice.
CREATE TEMP TABLE _canonical_apparatus_candidates (
    legacy_key TEXT NOT NULL,
    canonical_id TEXT NOT NULL
) ON COMMIT DROP;

INSERT INTO _canonical_apparatus_candidates (legacy_key, canonical_id)
SELECT legacy_key, canonical_id
FROM _canonical_apparatus_explicit_map;

INSERT INTO _canonical_apparatus_candidates (legacy_key, canonical_id)
SELECT lower(btrim(a.id)),
       COALESCE(e.canonical_id, master_map.canonical_id)
FROM mini_apparatus a
LEFT JOIN _canonical_apparatus_explicit_map e
  ON e.legacy_key = lower(btrim(a.id))
JOIN _canonical_apparatus_master_map master_map
  ON master_map.legacy_id = a.id
WHERE btrim(a.id) <> '';

INSERT INTO _canonical_apparatus_candidates (legacy_key, canonical_id)
SELECT lower(btrim(a.name)),
       COALESCE(e.canonical_id, master_map.canonical_id)
FROM mini_apparatus a
LEFT JOIN _canonical_apparatus_explicit_map e
  ON e.legacy_key = lower(btrim(a.name))
JOIN _canonical_apparatus_master_map master_map
  ON master_map.legacy_id = a.id
WHERE btrim(a.name) <> '';

INSERT INTO _canonical_apparatus_candidates (legacy_key, canonical_id)
SELECT lower(btrim(a.base_name)),
       COALESCE(e.canonical_id, master_map.canonical_id)
FROM mini_apparatus a
LEFT JOIN _canonical_apparatus_explicit_map e
  ON e.legacy_key = lower(btrim(a.base_name))
JOIN _canonical_apparatus_master_map master_map
  ON master_map.legacy_id = a.id
WHERE btrim(a.base_name) <> '';

-- A JSON payload may already contain the future opaque canonical id before
-- the master row is renamed. Self-map every future id before validating
-- persisted JSON so already-canonical identities remain valid and unknown
-- identities still fail closed.
INSERT INTO _canonical_apparatus_candidates (legacy_key, canonical_id)
SELECT lower(btrim(canonical_id)), canonical_id
FROM _canonical_apparatus_master_map;

DO $$
DECLARE ambiguous_key TEXT;
BEGIN
    SELECT legacy_key
    INTO ambiguous_key
    FROM _canonical_apparatus_candidates
    GROUP BY legacy_key
    HAVING count(DISTINCT canonical_id) <> 1
    ORDER BY legacy_key
    LIMIT 1;
    IF ambiguous_key IS NOT NULL THEN
        RAISE EXCEPTION
            '0065 ambiguous legacy apparatus identity %, mapping must match exactly one master',
            ambiguous_key;
    END IF;
END
$$;

CREATE TEMP TABLE _canonical_apparatus_legacy_map (
    legacy_key TEXT PRIMARY KEY,
    canonical_id TEXT NOT NULL
) ON COMMIT DROP;

INSERT INTO _canonical_apparatus_legacy_map (legacy_key, canonical_id)
SELECT legacy_key, min(canonical_id)
FROM _canonical_apparatus_candidates
GROUP BY legacy_key;

-- 0063/0064 staged foreign keys point at the pre-cutover master ids.  They
-- must not prevent this transaction from moving references and the master
-- primary keys together.  Recreate them below after the deterministic
-- backfill and rename have completed.  The old composite identity checks are
-- intentionally retired: `apparatus_id`/`apparatus` are display/audit
-- snapshots after cutover, never an authority for identity.
ALTER TABLE mini_worker_groups
    DROP CONSTRAINT IF EXISTS mini_worker_groups_canonical_apparatus_fk;
ALTER TABLE mini_queue_sequences
    DROP CONSTRAINT IF EXISTS mini_queue_sequences_canonical_apparatus_fk;
ALTER TABLE mini_queue_states
    DROP CONSTRAINT IF EXISTS mini_queue_states_canonical_apparatus_fk;
ALTER TABLE mini_apparatus_queue_policies
    DROP CONSTRAINT IF EXISTS mini_apparatus_queue_policies_canonical_apparatus_fk;
ALTER TABLE mini_queue_action_events
    DROP CONSTRAINT IF EXISTS mini_queue_action_events_canonical_apparatus_fk;
ALTER TABLE mini_order_run_sessions
    DROP CONSTRAINT IF EXISTS mini_order_run_sessions_canonical_apparatus_fk;
ALTER TABLE mini_order_progress_events
    DROP CONSTRAINT IF EXISTS mini_order_progress_events_canonical_apparatus_fk;
ALTER TABLE mini_training_queue_states
    DROP CONSTRAINT IF EXISTS mini_training_queue_states_canonical_apparatus_fk;
ALTER TABLE mini_training_progress_batches
    DROP CONSTRAINT IF EXISTS mini_training_progress_batches_canonical_apparatus_fk;
ALTER TABLE mini_apparatus_capacity_profiles
    DROP CONSTRAINT IF EXISTS mini_apparatus_capacity_profiles_canonical_apparatus_fk,
    DROP CONSTRAINT IF EXISTS mini_apparatus_capacity_profiles_identity_fk;
ALTER TABLE mini_apparatus_downtimes
    DROP CONSTRAINT IF EXISTS mini_apparatus_downtimes_canonical_apparatus_fk,
    DROP CONSTRAINT IF EXISTS mini_apparatus_downtimes_identity_fk;
ALTER TABLE mini_apparatus_schedule_reservations
    DROP CONSTRAINT IF EXISTS mini_apparatus_schedule_reservations_canonical_apparatus_fk,
    DROP CONSTRAINT IF EXISTS mini_apparatus_schedule_reservations_identity_fk;
ALTER TABLE mini_apparatus_order_transfers
    DROP CONSTRAINT IF EXISTS mini_apparatus_order_transfers_canonical_from_fk,
    DROP CONSTRAINT IF EXISTS mini_apparatus_order_transfers_canonical_to_fk;
ALTER TABLE mini_apparatus_material_rules
    DROP CONSTRAINT IF EXISTS mini_apparatus_material_rules_canonical_apparatus_fk;
ALTER TABLE mini_factory_location_apparatus_links
    DROP CONSTRAINT IF EXISTS mini_factory_location_apparatus_links_apparatus_id_fkey;

-- Add typed projections for the remaining operational tables.  Legacy text
-- columns remain display/audit snapshots and are not identity keys after this
-- migration.
ALTER TABLE mini_production_map_nodes
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT,
    ADD COLUMN IF NOT EXISTS canonical_alternative_apparatus_id TEXT;

ALTER TABLE mini_progress_batches
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT,
    ADD COLUMN IF NOT EXISTS canonical_current_apparatus_id TEXT,
    ADD COLUMN IF NOT EXISTS canonical_next_apparatus_id TEXT,
    ADD COLUMN IF NOT EXISTS canonical_used_by_apparatus_id TEXT,
    ADD COLUMN IF NOT EXISTS canonical_processed_by_apparatus_id TEXT;

ALTER TABLE mini_training_queue_events
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_training_raw_material_assignments
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_training_apparatus_modes
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_training_input_batches
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_raw_material_assignments
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_laminatsiya_astatka_reports
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_rezka_astatka_reports
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_returned_paint_requests
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_returned_paint_images
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_training_returned_paint_reports
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;
ALTER TABLE mini_raw_material_events
    ADD COLUMN IF NOT EXISTS canonical_apparatus_id TEXT;

-- Backfill every live text reference through the exact map. Queue rows must
-- carry real canonical apparatus ids; training-input:% is only an input-map
-- token and is never a stored queue identity. The fourth flag permits blank
-- WIP stage snapshots.
DO $$
DECLARE
    item TEXT;
    table_name TEXT;
    source_column TEXT;
    canonical_column TEXT;
    allow_blank BOOLEAN;
    unresolved BIGINT;
BEGIN
    FOREACH item IN ARRAY ARRAY[
        'mini_worker_groups|apparatus|canonical_apparatus_id|false',
        'mini_queue_sequences|apparatus|canonical_apparatus_id|false',
        'mini_queue_states|apparatus|canonical_apparatus_id|false',
        'mini_apparatus_queue_policies|apparatus|canonical_apparatus_id|false',
        'mini_queue_action_events|apparatus|canonical_apparatus_id|false',
        'mini_order_run_sessions|apparatus|canonical_apparatus_id|false',
        'mini_order_progress_events|apparatus|canonical_apparatus_id|false',
        'mini_training_queue_states|apparatus|canonical_apparatus_id|false',
        'mini_training_queue_events|apparatus|canonical_apparatus_id|false',
        'mini_training_progress_batches|apparatus|canonical_apparatus_id|false',
        'mini_training_raw_material_assignments|apparatus|canonical_apparatus_id|false',
        'mini_training_apparatus_modes|apparatus|canonical_apparatus_id|false',
        'mini_training_input_batches|apparatus|canonical_apparatus_id|false',
        'mini_raw_material_assignments|apparatus|canonical_apparatus_id|false',
        'mini_apparatus_capacity_profiles|apparatus_id|canonical_apparatus_id|false',
        'mini_apparatus_downtimes|apparatus_id|canonical_apparatus_id|false',
        'mini_apparatus_schedule_reservations|apparatus_id|canonical_apparatus_id|false',
        'mini_apparatus_material_rules|apparatus|canonical_apparatus_id|false',
        'mini_laminatsiya_astatka_reports|apparatus|canonical_apparatus_id|false',
        'mini_rezka_astatka_reports|apparatus|canonical_apparatus_id|false',
        'mini_returned_paint_requests|apparatus|canonical_apparatus_id|false',
        'mini_returned_paint_images|apparatus|canonical_apparatus_id|false',
        'mini_training_returned_paint_reports|apparatus|canonical_apparatus_id|false',
        'mini_raw_material_events|apparatus|canonical_apparatus_id|true',
        'mini_progress_batches|apparatus|canonical_apparatus_id|false',
        'mini_progress_batches|current_apparatus|canonical_current_apparatus_id|true',
        'mini_progress_batches|next_apparatus|canonical_next_apparatus_id|true',
        'mini_progress_batches|used_by_apparatus|canonical_used_by_apparatus_id|true',
        'mini_progress_batches|processed_by_apparatus|canonical_processed_by_apparatus_id|true'
    ]
    LOOP
        table_name := split_part(item, '|', 1);
        source_column := split_part(item, '|', 2);
        canonical_column := split_part(item, '|', 3);
        allow_blank := split_part(item, '|', 4)::BOOLEAN;

        EXECUTE format(
            'UPDATE %I AS target
             SET %I = mapping.canonical_id
             FROM pg_temp._canonical_apparatus_legacy_map AS mapping
             WHERE lower(btrim(COALESCE(target.%I, ''''))) = mapping.legacy_key
               AND (target.%I IS NULL OR btrim(target.%I) = '''')',
            table_name, canonical_column, source_column,
            canonical_column, canonical_column
        );

        EXECUTE format(
            'SELECT count(*)
             FROM %I AS target
             LEFT JOIN pg_temp._canonical_apparatus_legacy_map AS mapping
               ON lower(btrim(COALESCE(target.%I, ''''))) = mapping.legacy_key
             WHERE (target.%I IS NULL OR btrim(target.%I) = '''')
               AND NOT ($1 AND btrim(COALESCE(target.%I, '''')) = '''')',
            table_name, source_column, canonical_column, canonical_column,
            source_column
        ) INTO unresolved USING allow_blank;

        IF unresolved <> 0 THEN
            RAISE EXCEPTION
                '0065 unresolved legacy apparatus reference in %.%: % row(s)',
                table_name, source_column, unresolved;
        END IF;
    END LOOP;
END
$$;

-- Transfer endpoints have two separately keyed canonical references.
UPDATE mini_apparatus_order_transfers transfer
SET canonical_from_apparatus_id = COALESCE(
        NULLIF(btrim(transfer.canonical_from_apparatus_id), ''),
        mapping.canonical_id
    )
FROM _canonical_apparatus_legacy_map mapping
WHERE lower(btrim(transfer.from_apparatus)) = mapping.legacy_key;
UPDATE mini_apparatus_order_transfers transfer
SET canonical_to_apparatus_id = COALESCE(
        NULLIF(btrim(transfer.canonical_to_apparatus_id), ''),
        mapping.canonical_id
    )
FROM _canonical_apparatus_legacy_map mapping
WHERE lower(btrim(transfer.to_apparatus)) = mapping.legacy_key;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM mini_apparatus_order_transfers
        WHERE canonical_from_apparatus_id IS NULL
           OR canonical_to_apparatus_id IS NULL
    ) THEN
        RAISE EXCEPTION '0065 unresolved apparatus transfer endpoint';
    END IF;
END
$$;

-- The JSON map is the persisted production-map contract, not a second source
-- of apparatus identity.  Validate and rewrite its identity fields through
-- the same exact map before the runtime can read it.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_production_maps
        WHERE map_json ? 'nodes'
          AND jsonb_typeof(map_json->'nodes') <> 'array'
    ) THEN
        RAISE EXCEPTION
            '0065 malformed production-map nodes payload; expected array';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_production_maps map_row
        CROSS JOIN LATERAL jsonb_array_elements(map_row.map_json->'nodes') AS nodes(node)
        LEFT JOIN _canonical_apparatus_legacy_map main_mapping
          ON main_mapping.legacy_key = lower(btrim(nodes.node->>'apparatus_id'))
        LEFT JOIN _canonical_apparatus_legacy_map alternative_mapping
          ON alternative_mapping.legacy_key = lower(btrim(
              nodes.node->>'alternative_assigned_apparatus_id'
          ))
        WHERE nodes.node->>'kind' = 'apparatus'
          AND (
              btrim(COALESCE(nodes.node->>'apparatus_id', '')) = ''
              OR main_mapping.canonical_id IS NULL
              OR (
                  btrim(COALESCE(nodes.node->>'alternative_assigned_apparatus_id', '')) <> ''
                  AND alternative_mapping.canonical_id IS NULL
              )
          )
    ) THEN
        RAISE EXCEPTION
            '0065 unresolved production-map JSON apparatus identity';
    END IF;
END
$$;

UPDATE mini_production_maps map_row
SET map_json = jsonb_set(
    map_row.map_json,
    '{nodes}',
    (
        SELECT jsonb_agg(
            CASE
                WHEN COALESCE(nodes.node->>'kind', '') <> 'apparatus' THEN nodes.node
                WHEN btrim(COALESCE(nodes.node->>'alternative_assigned_apparatus_id', '')) = ''
                    THEN jsonb_set(
                        nodes.node,
                        '{apparatus_id}',
                        to_jsonb(main_mapping.canonical_id),
                        true
                    )
                ELSE jsonb_set(
                    jsonb_set(
                        nodes.node,
                        '{apparatus_id}',
                        to_jsonb(main_mapping.canonical_id),
                        true
                    ),
                    '{alternative_assigned_apparatus_id}',
                    to_jsonb(alternative_mapping.canonical_id),
                    true
                )
            END
            ORDER BY nodes.ordinality
        )
        FROM jsonb_array_elements(map_row.map_json->'nodes')
             WITH ORDINALITY AS nodes(node, ordinality)
        LEFT JOIN _canonical_apparatus_legacy_map main_mapping
          ON main_mapping.legacy_key = lower(btrim(nodes.node->>'apparatus_id'))
        LEFT JOIN _canonical_apparatus_legacy_map alternative_mapping
          ON alternative_mapping.legacy_key = lower(btrim(
              nodes.node->>'alternative_assigned_apparatus_id'
          ))
    ),
    true
)
WHERE jsonb_typeof(map_row.map_json->'nodes') = 'array';

-- Production-map node/alternative identity is typed in the relational mirror.
-- The JSON payload remains a display/history projection, but its apparatus
-- fields must be resolvable by the same exact map.
UPDATE mini_production_map_nodes node
SET canonical_apparatus_id = mapping.canonical_id
FROM _canonical_apparatus_legacy_map mapping
WHERE lower(btrim(node.payload_json->>'apparatus_id')) = mapping.legacy_key
  AND node.kind = 'apparatus';
UPDATE mini_production_map_nodes node
SET canonical_alternative_apparatus_id = mapping.canonical_id
FROM _canonical_apparatus_legacy_map mapping
WHERE lower(btrim(node.payload_json->>'alternative_assigned_apparatus_id')) = mapping.legacy_key
  AND btrim(COALESCE(node.payload_json->>'alternative_assigned_apparatus_id', '')) <> '';

UPDATE mini_production_map_nodes node
SET payload_json = jsonb_set(
    jsonb_set(
        node.payload_json,
        '{apparatus_id}',
        to_jsonb(node.canonical_apparatus_id),
        true
    ),
    '{alternative_assigned_apparatus_id}',
    to_jsonb(COALESCE(node.canonical_alternative_apparatus_id, '')),
    true
)
WHERE node.kind = 'apparatus';

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_apparatus_groups
        WHERE payload_json ? 'apparatus'
          AND jsonb_typeof(payload_json->'apparatus') <> 'array'
    ) THEN
        RAISE EXCEPTION
            '0065 malformed mini_apparatus_groups apparatus payload; expected array';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_apparatus_groups group_row
        CROSS JOIN LATERAL jsonb_array_elements_text(
            group_row.payload_json->'apparatus'
        ) AS values(value)
        LEFT JOIN _canonical_apparatus_legacy_map mapping
          ON mapping.legacy_key = lower(btrim(values.value))
        WHERE btrim(values.value) = '' OR mapping.canonical_id IS NULL
    ) THEN
        RAISE EXCEPTION
            '0065 unresolved or blank apparatus identity in mini_apparatus_groups payload';
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM mini_production_map_nodes
        WHERE kind = 'apparatus'
          AND (canonical_apparatus_id IS NULL OR btrim(canonical_apparatus_id) = '')
    ) THEN
        RAISE EXCEPTION '0065 unresolved production-map apparatus node identity';
    END IF;
    IF EXISTS (
        SELECT 1 FROM mini_production_map_nodes
        WHERE btrim(COALESCE(payload_json->>'alternative_assigned_apparatus_id', '')) <> ''
          AND (canonical_alternative_apparatus_id IS NULL
               OR btrim(canonical_alternative_apparatus_id) = '')
    ) THEN
        RAISE EXCEPTION '0065 unresolved production-map alternative apparatus identity';
    END IF;
END
$$;

-- Groups and the stored map JSON are identity-bearing projections too.  Apply
-- only exact known-id replacements; free-form titles are not parsed.
UPDATE mini_apparatus_groups group_row
SET payload_json = jsonb_set(
    group_row.payload_json,
    '{apparatus}',
    COALESCE((
        SELECT jsonb_agg(to_jsonb(COALESCE(mapping.canonical_id, value)) ORDER BY ordinality)
        FROM jsonb_array_elements_text(group_row.payload_json->'apparatus')
             WITH ORDINALITY AS values(value, ordinality)
        LEFT JOIN _canonical_apparatus_legacy_map mapping
          ON mapping.legacy_key = lower(btrim(values.value))
    ), '[]'::jsonb),
    true
)
WHERE jsonb_typeof(group_row.payload_json->'apparatus') = 'array';

-- Update foreign-keyed placement links before changing master primary keys.
UPDATE mini_factory_location_apparatus_links link
SET apparatus_id = mapping.canonical_id
FROM _canonical_apparatus_legacy_map mapping
WHERE lower(btrim(link.apparatus_id)) = mapping.legacy_key;

-- Make capacity explicit in every canonical master payload.  This is persisted
-- configuration, not a runtime fallback.
UPDATE mini_apparatus
SET payload_json = jsonb_set(
    CASE WHEN jsonb_typeof(payload_json) = 'object' THEN payload_json ELSE '{}'::jsonb END,
    '{capacity}',
    jsonb_build_object(
        'capacity_slots', 1,
        'setup_minutes', 0,
        'cleanup_minutes', 0,
        'efficiency_percent', 100,
        'finite_capacity', TRUE,
        'working_windows', '[]'::jsonb
    ),
    true
)
WHERE NOT (payload_json ? 'capacity') OR payload_json->'capacity' IS NULL;

-- Materialize the complete canonical runtime payload before renaming any
-- master ids.  The compatibility master stays at the top level, while every
-- row receives a validated-shape canonical configuration immediately.  A
-- pre-cutover nested canonical blob is retained field-for-field where it is
-- complete; its identity id is the only identity value that may change.
DO $$
DECLARE
    apparatus_row RECORD;
    source_payload JSONB;
    existing_canonical JSONB;
    generated JSONB;
    canonical JSONB;
    identity JSONB;
    display JSONB;
    policies JSONB;
    material JSONB;
    capabilities JSONB;
    profiles JSONB;
    capacity JSONB;
    placement JSONB;
    family TEXT;
    kind TEXT;
    canonical_id TEXT;
    color_stations INTEGER;
    training_enabled BOOLEAN;
    catalog_order BIGINT;
BEGIN
    FOR apparatus_row IN
        SELECT master.id AS legacy_id,
               master.name,
               master.payload_json,
               master_map.canonical_id
        FROM mini_apparatus master
        JOIN _canonical_apparatus_master_map master_map
          ON master_map.legacy_id = master.id
        ORDER BY master.id
    LOOP
        canonical_id := apparatus_row.canonical_id;
        source_payload := CASE
            WHEN jsonb_typeof(apparatus_row.payload_json) = 'object'
                THEN apparatus_row.payload_json
            ELSE '{}'::jsonb
        END;
        existing_canonical := source_payload->'canonical_apparatus';
        IF jsonb_typeof(existing_canonical) <> 'object' THEN
            existing_canonical := '{}'::jsonb;
        END IF;

        family := NULLIF(lower(btrim(source_payload->>'family')), '');
        IF family IS NULL THEN
            IF canonical_id IN (
                'apparatus:default:bosma_7', 'apparatus:default:bosma_8',
                'apparatus:default:bosma_9', 'apparatus:default:asset-005'
            ) THEN
                family := 'pechat';
            ELSIF canonical_id IN (
                'apparatus:default:asset-004',
                'apparatus:default:asset-007',
                'apparatus:default:asset-008'
            ) THEN
                family := 'laminatsiya';
            ELSIF canonical_id = 'apparatus:default:holodniy_kley' THEN
                family := 'kley';
            ELSIF canonical_id = 'apparatus:default:paket' THEN
                family := 'paket';
            ELSIF canonical_id = 'apparatus:default:asset-010' THEN
                family := 'rezka';
            ELSE
                family := 'other';
            END IF;
        END IF;

        kind := NULLIF(lower(btrim(source_payload->>'kind')), '');
        IF kind IS NULL THEN
            IF canonical_id IN (
                'apparatus:default:bosma_7', 'apparatus:default:bosma_8',
                'apparatus:default:bosma_9'
            ) THEN
                kind := 'color_pechat';
            ELSIF canonical_id = 'apparatus:default:asset-004' THEN
                kind := 'extruder_laminatsiya';
            ELSIF canonical_id = 'apparatus:default:asset-005' THEN
                kind := 'flexo';
            ELSIF canonical_id = 'apparatus:default:holodniy_kley' THEN
                kind := 'holodniy_kley';
            ELSIF canonical_id IN (
                'apparatus:default:asset-007', 'apparatus:default:asset-008'
            ) THEN
                kind := 'laminatsiya';
            ELSIF canonical_id = 'apparatus:default:paket' THEN
                kind := 'paket';
            ELSIF canonical_id = 'apparatus:default:asset-010' THEN
                kind := 'rezka';
            ELSE
                kind := 'other';
            END IF;
        END IF;

        IF NOT (
            (family = 'pechat' AND kind IN ('color_pechat', 'flexo'))
            OR (family = 'laminatsiya' AND kind IN ('laminatsiya', 'extruder_laminatsiya'))
            OR (family = 'rezka' AND kind = 'rezka')
            OR (family = 'paket' AND kind = 'paket')
            OR (family = 'kley' AND kind = 'holodniy_kley')
            OR (family = 'other' AND kind = 'other')
        ) THEN
            RAISE EXCEPTION
                '0065 apparatus % has invalid family/kind metadata %, %; add an explicit safe mapping',
                apparatus_row.legacy_id, family, kind;
        END IF;

        capabilities := source_payload->'capabilities';
        IF capabilities IS NULL OR jsonb_typeof(capabilities) <> 'array' THEN
            capabilities := '[]'::jsonb;
        ELSE
            SELECT COALESCE(
                       jsonb_agg(to_jsonb(lower(values.value)) ORDER BY values.ordinality),
                       '[]'::jsonb
                   )
            INTO capabilities
            FROM jsonb_array_elements_text(capabilities)
                 WITH ORDINALITY AS values(value, ordinality);
        END IF;
        IF jsonb_array_length(capabilities) = 0 THEN
            capabilities := CASE kind
                WHEN 'color_pechat' THEN '["print", "pechat"]'::jsonb
                WHEN 'flexo' THEN '["print", "pechat", "flexo"]'::jsonb
                WHEN 'laminatsiya' THEN '["laminate"]'::jsonb
                WHEN 'extruder_laminatsiya' THEN '["laminate"]'::jsonb
                WHEN 'rezka' THEN '["cut"]'::jsonb
                WHEN 'paket' THEN '["package"]'::jsonb
                WHEN 'holodniy_kley' THEN '["glue"]'::jsonb
                ELSE '["apparatus"]'::jsonb
            END;
        END IF;
        IF EXISTS (
            SELECT 1
            FROM jsonb_array_elements_text(capabilities) AS values(value)
            WHERE values.value NOT IN (
                'print', 'pechat', 'flexo', 'laminate',
                'cut', 'package', 'glue', 'apparatus'
            )
        ) OR (
            SELECT count(*) FROM jsonb_array_elements_text(capabilities)
        ) <> (
            SELECT count(DISTINCT value) FROM jsonb_array_elements_text(capabilities)
        ) THEN
            RAISE EXCEPTION
                '0065 apparatus % has invalid or duplicate capability metadata',
                apparatus_row.legacy_id;
        END IF;

        profiles := existing_canonical->'capability_profiles';
        IF profiles IS NULL OR jsonb_typeof(profiles) <> 'array'
            OR jsonb_array_length(profiles) = 0 THEN
            profiles := source_payload->'capability_profiles';
        END IF;
        IF profiles IS NULL OR jsonb_typeof(profiles) <> 'array'
            OR jsonb_array_length(profiles) = 0 THEN
            SELECT COALESCE(
                       jsonb_agg(
                           jsonb_build_object(
                               'code', values.value,
                               'level', 1,
                               'enabled', TRUE
                           ) ORDER BY values.ordinality
                       ),
                       '[]'::jsonb
                   )
            INTO profiles
            FROM jsonb_array_elements_text(capabilities)
                 WITH ORDINALITY AS values(value, ordinality);
        END IF;
        IF jsonb_typeof(profiles) <> 'array'
           OR EXISTS (
               SELECT 1
               FROM jsonb_array_elements(
                   CASE
                       WHEN jsonb_typeof(profiles) = 'array' THEN profiles
                       ELSE '[]'::jsonb
                   END
               ) AS profile
               WHERE jsonb_typeof(profile) <> 'object'
                  OR jsonb_typeof(profile->'code') <> 'string'
                  OR profile->>'code' NOT IN (
                      'print', 'pechat', 'flexo', 'laminate',
                      'cut', 'package', 'glue', 'apparatus'
                  )
                  OR profile->>'code' NOT IN (
                      SELECT jsonb_array_elements_text(capabilities)
                  )
                  OR (profile ? 'level' AND (
                      jsonb_typeof(profile->'level') <> 'number'
                      OR CASE
                          WHEN (profile->>'level') ~ '^[0-9]+$'
                              THEN (profile->>'level')::NUMERIC NOT BETWEEN 1 AND 100
                          ELSE TRUE
                      END
                  ))
                  OR (profile ? 'valid_from_unix' AND
                      profile->'valid_from_unix' IS NOT NULL AND
                      jsonb_typeof(profile->'valid_from_unix') <> 'number')
                  OR (profile ? 'valid_to_unix' AND
                      profile->'valid_to_unix' IS NOT NULL AND
                      jsonb_typeof(profile->'valid_to_unix') <> 'number')
                  OR (
                      CASE
                          WHEN profile->>'valid_from_unix' ~ '^-?[0-9]+$'
                               AND profile->>'valid_to_unix' ~ '^-?[0-9]+$'
                          THEN (profile->>'valid_to_unix')::BIGINT
                               <= (profile->>'valid_from_unix')::BIGINT
                          ELSE FALSE
                      END
                  )
           )
           OR EXISTS (
               SELECT 1
               FROM jsonb_array_elements(
                   CASE
                       WHEN jsonb_typeof(profiles) = 'array' THEN profiles
                       ELSE '[]'::jsonb
                   END
               ) AS profile
               GROUP BY profile->>'code', profile->>'valid_from_unix'
               HAVING count(*) > 1
           ) THEN
            SELECT COALESCE(
                       jsonb_agg(
                           jsonb_build_object(
                               'code', values.value,
                               'level', 1,
                               'enabled', TRUE
                           ) ORDER BY values.ordinality
                       ),
                       '[]'::jsonb
                   )
            INTO profiles
            FROM jsonb_array_elements_text(capabilities)
                 WITH ORDINALITY AS values(value, ordinality);
        END IF;

        capacity := jsonb_build_object(
            'capacity_slots', 1,
            'setup_minutes', 0,
            'cleanup_minutes', 0,
            'efficiency_percent', 100,
            'finite_capacity', TRUE,
            'working_windows', '[]'::jsonb
        );
        IF jsonb_typeof(source_payload->'capacity') = 'object' THEN
            capacity := capacity || source_payload->'capacity';
        END IF;
        IF jsonb_typeof(existing_canonical->'capacity') = 'object' THEN
            capacity := capacity || existing_canonical->'capacity';
        END IF;
        IF jsonb_typeof(capacity) <> 'object'
           OR (capacity->>'capacity_slots') !~ '^[0-9]+$'
           OR (
               CASE
                   WHEN (capacity->>'capacity_slots') ~ '^[0-9]+$'
                       THEN (capacity->>'capacity_slots')::NUMERIC NOT BETWEEN 1 AND 64
                   ELSE TRUE
               END
           )
           OR (capacity->>'setup_minutes') !~ '^[0-9]+$'
           OR (
               CASE
                   WHEN (capacity->>'setup_minutes') ~ '^[0-9]+$'
                       THEN (capacity->>'setup_minutes')::NUMERIC > 2147483647
                   ELSE TRUE
               END
           )
           OR (capacity->>'cleanup_minutes') !~ '^[0-9]+$'
           OR (
               CASE
                   WHEN (capacity->>'cleanup_minutes') ~ '^[0-9]+$'
                       THEN (capacity->>'cleanup_minutes')::NUMERIC > 2147483647
                   ELSE TRUE
               END
           )
           OR (capacity->>'efficiency_percent') !~ '^[0-9]+$'
           OR (
               CASE
                   WHEN (capacity->>'efficiency_percent') ~ '^[0-9]+$'
                       THEN (capacity->>'efficiency_percent')::NUMERIC NOT BETWEEN 1 AND 200
                   ELSE TRUE
               END
           )
           OR capacity->>'finite_capacity' NOT IN ('true', 'false')
           OR jsonb_typeof(capacity->'working_windows') <> 'array'
           OR EXISTS (
               SELECT 1
               FROM jsonb_array_elements(
                   CASE
                       WHEN jsonb_typeof(capacity->'working_windows') = 'array'
                           THEN capacity->'working_windows'
                       ELSE '[]'::jsonb
                   END
               ) AS working_window
               WHERE jsonb_typeof(working_window) <> 'object'
                  OR (working_window->>'weekday') !~ '^[0-9]+$'
                  OR CASE
                      WHEN (working_window->>'weekday') ~ '^[0-9]+$'
                          THEN (working_window->>'weekday')::NUMERIC NOT BETWEEN 1 AND 7
                      ELSE TRUE
                  END
                  OR (working_window->>'start_minute') !~ '^[0-9]+$'
                  OR (working_window->>'end_minute') !~ '^[0-9]+$'
                  OR CASE
                      WHEN (working_window->>'start_minute') ~ '^[0-9]+$'
                           AND (working_window->>'end_minute') ~ '^[0-9]+$'
                          THEN (working_window->>'start_minute')::NUMERIC
                               >= (working_window->>'end_minute')::NUMERIC
                               OR (working_window->>'end_minute')::NUMERIC > 1440
                      ELSE TRUE
                  END
           ) THEN
            capacity := jsonb_build_object(
                'capacity_slots', 1,
                'setup_minutes', 0,
                'cleanup_minutes', 0,
                'efficiency_percent', 100,
                'finite_capacity', TRUE,
                'working_windows', '[]'::jsonb
            );
        END IF;

        color_stations := NULL;
        IF kind = 'color_pechat' THEN
            color_stations := CASE
                WHEN source_payload->>'color_stations' ~ '^[0-9]+$'
                    THEN (source_payload->>'color_stations')::INTEGER
                ELSE NULL
            END;
            IF color_stations IS NULL THEN
                color_stations := CASE canonical_id
                    WHEN 'apparatus:default:bosma_7' THEN 7
                    WHEN 'apparatus:default:bosma_8' THEN 8
                    WHEN 'apparatus:default:bosma_9' THEN 9
                    ELSE NULL
                END;
            END IF;
            IF color_stations IS NULL OR color_stations NOT BETWEEN 7 AND 9 THEN
                RAISE EXCEPTION
                    '0065 color apparatus % has no valid color_stations metadata',
                    apparatus_row.legacy_id;
            END IF;
        END IF;

        training_enabled := CASE
            WHEN lower(COALESCE(source_payload->>'training_enabled', '')) = 'true' THEN TRUE
            WHEN lower(COALESCE(source_payload->>'training_enabled', '')) = 'false' THEN FALSE
            ELSE FALSE
        END;
        catalog_order := CASE
            WHEN source_payload->>'sort_order' ~ '^[0-9]+$'
                THEN LEAST((source_payload->>'sort_order')::NUMERIC, 4294967295)::BIGINT
            ELSE 0
        END;
        placement := NULL;
        IF btrim(COALESCE(source_payload->>'factory_map_object_id', '')) <> '' THEN
            placement := jsonb_build_object(
                'factory_map_object_id', btrim(source_payload->>'factory_map_object_id')
            );
        END IF;

        generated := jsonb_build_object(
            'identity', jsonb_build_object(
                'id', canonical_id,
                'display', jsonb_build_object(
                    'display_name', apparatus_row.name,
                    'description', '',
                    'catalog_order', catalog_order
                )
            ),
            'classification', jsonb_build_object(
                'family', family,
                'kind', kind,
                'color_stations', color_stations
            ),
            'capabilities', capabilities,
            'capability_profiles', profiles,
            'policies', jsonb_build_object(
                'queue', 'strict_sequence',
                'material', jsonb_build_object(
                    'requires_material', FALSE,
                    'start_policy', 'state_all',
                    'item_groups', '[]'::jsonb,
                    'requirement_groups', '[]'::jsonb
                ),
                'tooling', CASE
                    WHEN family = 'pechat' THEN 'qolip_scan_required'
                    ELSE 'qolip_scan_not_required'
                END
            ),
            'capacity', capacity,
            'placement', placement,
            'training', jsonb_build_object('enabled', training_enabled),
            'provenance', jsonb_build_object(
                'source', CASE
                    WHEN canonical_id LIKE 'apparatus:default:%' THEN 'default'
                    ELSE 'custom'
                END,
                'source_ref', NULL
            ),
            'versioning', jsonb_build_object('revision', 1),
            'aas', jsonb_build_object(
                'submodel_id', 'urn:mini-rs-erp:submodel:apparatus:' ||
                    substr(canonical_id, length('apparatus:') + 1),
                'semantic_id', 'urn:mini-rs-erp:semantic-id:submodel:apparatus:1',
                'idta_release', '26-01',
                'aas_metamodel_version', '3.2.0',
                'aasx_part_5_version', 'IDTA-01005 v3.2',
                'package_format', 'Open Packaging Conventions',
                'media_type', 'application/asset-administration-shell-package'
            )
        );

        identity := generated->'identity';
        display := generated->'identity'->'display';
        IF jsonb_typeof(existing_canonical #> '{identity,display}') = 'object' THEN
            -- The migrated master supplies the required display identity.  A
            -- partial nested canonical blob may retain optional metadata, but
            -- it must never replace display_name with an empty/stale value.
            IF jsonb_typeof(existing_canonical #> '{identity,display,description}') = 'string'
               AND char_length(existing_canonical #>> '{identity,display,description}') <= 2000
               AND (existing_canonical #>> '{identity,display,description}') !~ '[[:cntrl:]]'
            THEN
                display := jsonb_set(
                    display,
                    '{description}',
                    existing_canonical #> '{identity,display,description}',
                    TRUE
                );
            END IF;
            IF jsonb_typeof(existing_canonical #> '{identity,display,catalog_order}') = 'number'
               AND (existing_canonical #>> '{identity,display,catalog_order}') ~ '^[0-9]+$'
               AND (existing_canonical #>> '{identity,display,catalog_order}')::NUMERIC <= 4294967295
            THEN
                display := jsonb_set(
                    display,
                    '{catalog_order}',
                    existing_canonical #> '{identity,display,catalog_order}',
                    TRUE
                );
            END IF;
        END IF;
        identity := jsonb_set(identity, '{display}', display, TRUE);
        identity := jsonb_set(identity, '{id}', to_jsonb(canonical_id), TRUE);

        policies := generated->'policies';
        IF jsonb_typeof(existing_canonical->'policies') = 'object' THEN
            policies := policies || existing_canonical->'policies';
            material := (generated->'policies'->'material') || COALESCE(
                CASE
                    WHEN jsonb_typeof(existing_canonical #> '{policies,material}') = 'object'
                        THEN existing_canonical #> '{policies,material}'
                    ELSE '{}'::jsonb
                END,
                '{}'::jsonb
            );
            policies := jsonb_set(policies, '{material}', material, TRUE);
        END IF;

        canonical := generated || (existing_canonical - 'identity' - 'policies');
        canonical := jsonb_set(canonical, '{identity}', identity, TRUE);
        canonical := jsonb_set(canonical, '{policies}', policies, TRUE);
        IF jsonb_typeof(canonical->'classification') <> 'object'
           OR NOT (canonical->'classification' ?& ARRAY['family', 'kind'])
           OR jsonb_typeof(canonical->'capabilities') <> 'array'
           OR (
               CASE
                   WHEN jsonb_typeof(canonical->'capabilities') = 'array'
                       THEN jsonb_array_length(canonical->'capabilities') = 0
                   ELSE TRUE
               END
           )
           OR jsonb_typeof(canonical->'policies') <> 'object'
           OR NOT (canonical->'policies' ? 'queue')
           OR jsonb_typeof(canonical->'capacity') <> 'object'
           OR jsonb_typeof(canonical->'training') <> 'object'
           OR jsonb_typeof(canonical->'provenance') <> 'object'
           OR jsonb_typeof(canonical->'versioning') <> 'object'
           OR jsonb_typeof(canonical->'aas') <> 'object'
           OR canonical #>> '{policies,queue}' NOT IN ('strict_sequence', 'free_pick')
           OR canonical #>> '{policies,tooling}' NOT IN (
               'qolip_scan_required', 'qolip_scan_not_required'
           )
           OR jsonb_typeof(canonical #> '{policies,material}') <> 'object'
           OR canonical #>> '{policies,material,requires_material}' NOT IN ('true', 'false')
           OR canonical #>> '{policies,material,start_policy}' NOT IN (
               'state_all', 'requirement_groups'
           )
           OR jsonb_typeof(canonical #> '{policies,material,item_groups}') <> 'array'
           OR jsonb_typeof(canonical #> '{policies,material,requirement_groups}') <> 'array'
           OR (
               canonical #>> '{policies,material,requires_material}' = 'false'
               AND (
                   canonical #>> '{policies,material,start_policy}' <> 'state_all'
                   OR (
                       CASE
                           WHEN jsonb_typeof(canonical #> '{policies,material,item_groups}') = 'array'
                               THEN jsonb_array_length(canonical #> '{policies,material,item_groups}') <> 0
                           ELSE TRUE
                       END
                   )
                   OR (
                       CASE
                           WHEN jsonb_typeof(canonical #> '{policies,material,requirement_groups}') = 'array'
                               THEN jsonb_array_length(canonical #> '{policies,material,requirement_groups}') <> 0
                           ELSE TRUE
                       END
                   )
               )
           )
           OR (
               canonical #>> '{policies,material,requires_material}' = 'true'
               AND NOT (
                   canonical #>> '{policies,material,start_policy}' = 'state_all'
                   AND jsonb_array_length(canonical #> '{policies,material,item_groups}') > 0
                   AND jsonb_array_length(canonical #> '{policies,material,requirement_groups}') = 0
                   OR canonical #>> '{policies,material,start_policy}' = 'requirement_groups'
                   AND jsonb_array_length(canonical #> '{policies,material,item_groups}') = 0
                   AND jsonb_array_length(canonical #> '{policies,material,requirement_groups}') > 0
               )
           )
        THEN
            -- A partial nested blob is not allowed to replace a required
            -- generated section.  The complete generated payload remains the
            -- durable fallback; optional display metadata was merged above.
            canonical := generated;
            canonical := jsonb_set(canonical, '{identity}', identity, TRUE);
        END IF;
        UPDATE mini_apparatus
        SET payload_json = jsonb_set(source_payload, '{canonical_apparatus}', canonical, TRUE)
        WHERE id = apparatus_row.legacy_id;
    END LOOP;
END
$$;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_apparatus master
        JOIN _canonical_apparatus_master_map master_map
          ON master_map.legacy_id = master.id
        WHERE jsonb_typeof(master.payload_json->'canonical_apparatus') <> 'object'
           OR jsonb_typeof(master.payload_json #> '{canonical_apparatus,identity}') <> 'object'
           OR jsonb_typeof(master.payload_json #> '{canonical_apparatus,classification}') <> 'object'
           OR jsonb_typeof(master.payload_json #> '{canonical_apparatus,capabilities}') <> 'array'
           OR jsonb_typeof(master.payload_json #> '{canonical_apparatus,capability_profiles}') <> 'array'
           OR jsonb_typeof(master.payload_json #> '{canonical_apparatus,policies}') <> 'object'
           OR jsonb_typeof(master.payload_json #> '{canonical_apparatus,capacity}') <> 'object'
           OR jsonb_typeof(master.payload_json #> '{canonical_apparatus,training}') <> 'object'
           OR jsonb_typeof(master.payload_json #> '{canonical_apparatus,provenance}') <> 'object'
           OR jsonb_typeof(master.payload_json #> '{canonical_apparatus,versioning}') <> 'object'
           OR jsonb_typeof(master.payload_json #> '{canonical_apparatus,aas}') <> 'object'
           OR master.payload_json #>> '{canonical_apparatus,identity,id}' <> master_map.canonical_id
           OR btrim(COALESCE(
               master.payload_json #>> '{canonical_apparatus,identity,display,display_name}',
               ''
           )) = ''
    ) THEN
        RAISE EXCEPTION
            '0065 canonical apparatus payload materialization is incomplete';
    END IF;
END
$$;

-- Validate the materialized JSON contract before the primary-key cutover.
-- The relational checks below cannot prove the nested payload is complete;
-- this guard keeps malformed or partial legacy canonical blobs out of the
-- durable master even when their top-level JSON types happen to match.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_apparatus master
        JOIN _canonical_apparatus_master_map master_map
          ON master_map.legacy_id = master.id
        CROSS JOIN LATERAL (
            SELECT master.payload_json->'canonical_apparatus' AS canonical
        ) payload
        WHERE NOT (
            jsonb_typeof(payload.canonical) = 'object'
            AND jsonb_typeof(payload.canonical->'identity') = 'object'
            AND payload.canonical #>> '{identity,id}' = master_map.canonical_id
            AND master_map.canonical_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
            AND octet_length(master_map.canonical_id) <= 128
            AND jsonb_typeof(payload.canonical #> '{identity,display}') = 'object'
            AND jsonb_typeof(payload.canonical #> '{identity,display,display_name}') = 'string'
            AND btrim(payload.canonical #>> '{identity,display,display_name}') <> ''
            AND char_length(payload.canonical #>> '{identity,display,display_name}') <= 256
            AND jsonb_typeof(payload.canonical->'classification') = 'object'
            AND (
                (payload.canonical #>> '{classification,family}' = 'pechat'
                 AND payload.canonical #>> '{classification,kind}' IN ('color_pechat', 'flexo'))
                OR (payload.canonical #>> '{classification,family}' = 'laminatsiya'
                    AND payload.canonical #>> '{classification,kind}' IN ('laminatsiya', 'extruder_laminatsiya'))
                OR (payload.canonical #>> '{classification,family}' = 'rezka'
                    AND payload.canonical #>> '{classification,kind}' = 'rezka')
                OR (payload.canonical #>> '{classification,family}' = 'paket'
                    AND payload.canonical #>> '{classification,kind}' = 'paket')
                OR (payload.canonical #>> '{classification,family}' = 'kley'
                    AND payload.canonical #>> '{classification,kind}' = 'holodniy_kley')
                OR (payload.canonical #>> '{classification,family}' = 'other'
                    AND payload.canonical #>> '{classification,kind}' = 'other')
            )
            AND (
                payload.canonical #>> '{classification,kind}' <> 'color_pechat'
                OR (
                    CASE
                        WHEN jsonb_typeof(payload.canonical #> '{classification,color_stations}') = 'number'
                             AND (payload.canonical #>> '{classification,color_stations}') ~ '^[0-9]+$'
                           THEN (payload.canonical #>> '{classification,color_stations}')::NUMERIC BETWEEN 7 AND 9
                        ELSE FALSE
                    END
                )
            )
            AND jsonb_typeof(payload.canonical->'capabilities') = 'array'
            AND CASE
                WHEN jsonb_typeof(payload.canonical->'capabilities') = 'array'
                    THEN jsonb_array_length(payload.canonical->'capabilities') > 0
                ELSE FALSE
            END
            AND NOT EXISTS (
                SELECT 1
                FROM jsonb_array_elements_text(
                    CASE
                        WHEN jsonb_typeof(payload.canonical->'capabilities') = 'array'
                            THEN payload.canonical->'capabilities'
                        ELSE '[]'::jsonb
                    END
                ) AS capability(value)
                WHERE capability.value NOT IN (
                    'print', 'pechat', 'flexo', 'laminate',
                    'cut', 'package', 'glue', 'apparatus'
                )
            )
            AND NOT EXISTS (
                SELECT capability.value
                FROM jsonb_array_elements_text(
                    CASE
                        WHEN jsonb_typeof(payload.canonical->'capabilities') = 'array'
                            THEN payload.canonical->'capabilities'
                        ELSE '[]'::jsonb
                    END
                ) AS capability(value)
                GROUP BY capability.value
                HAVING count(*) > 1
            )
            AND jsonb_typeof(payload.canonical->'capability_profiles') = 'array'
            AND NOT EXISTS (
                SELECT 1
                FROM jsonb_array_elements(
                    CASE
                        WHEN jsonb_typeof(payload.canonical->'capability_profiles') = 'array'
                            THEN payload.canonical->'capability_profiles'
                        ELSE '[]'::jsonb
                    END
                ) AS profile(value)
                WHERE jsonb_typeof(profile.value) <> 'object'
                   OR jsonb_typeof(profile.value->'code') <> 'string'
                   OR profile.value->>'code' NOT IN (
                       SELECT jsonb_array_elements_text(
                           CASE
                               WHEN jsonb_typeof(payload.canonical->'capabilities') = 'array'
                                   THEN payload.canonical->'capabilities'
                               ELSE '[]'::jsonb
                           END
                       )
                   )
                   OR (profile.value ? 'level' AND (
                       jsonb_typeof(profile.value->'level') <> 'number'
                       OR CASE
                           WHEN profile.value->>'level' ~ '^[0-9]+$'
                               THEN (profile.value->>'level')::NUMERIC NOT BETWEEN 1 AND 100
                           ELSE TRUE
                       END
                   ))
            )
            AND jsonb_typeof(payload.canonical->'policies') = 'object'
            AND payload.canonical #>> '{policies,queue}' IN ('strict_sequence', 'free_pick')
            AND NOT (
                payload.canonical #>> '{classification,family}' = 'pechat'
                AND payload.canonical #>> '{policies,queue}' = 'free_pick'
            )
            AND payload.canonical #>> '{policies,tooling}' IN (
                'qolip_scan_required', 'qolip_scan_not_required'
            )
            AND NOT (
                payload.canonical #>> '{policies,tooling}' = 'qolip_scan_required'
                AND NOT (
                    payload.canonical #>> '{classification,family}' = 'pechat'
                    AND payload.canonical #>> '{classification,kind}' IN ('color_pechat', 'flexo')
                )
            )
            AND jsonb_typeof(payload.canonical #> '{policies,material}') = 'object'
            AND payload.canonical #>> '{policies,material,requires_material}' IN ('true', 'false')
            AND payload.canonical #>> '{policies,material,start_policy}' IN (
                'state_all', 'requirement_groups'
            )
            AND jsonb_typeof(payload.canonical #> '{policies,material,item_groups}') = 'array'
            AND jsonb_typeof(payload.canonical #> '{policies,material,requirement_groups}') = 'array'
            AND NOT (
                payload.canonical #>> '{policies,material,requires_material}' = 'false'
                AND (
                    payload.canonical #>> '{policies,material,start_policy}' <> 'state_all'
                    OR jsonb_array_length(payload.canonical #> '{policies,material,item_groups}') <> 0
                    OR jsonb_array_length(payload.canonical #> '{policies,material,requirement_groups}') <> 0
                )
            )
            AND NOT (
                payload.canonical #>> '{policies,material,requires_material}' = 'true'
                AND NOT (
                    payload.canonical #>> '{policies,material,start_policy}' = 'state_all'
                    AND jsonb_array_length(payload.canonical #> '{policies,material,item_groups}') > 0
                    AND jsonb_array_length(payload.canonical #> '{policies,material,requirement_groups}') = 0
                    OR payload.canonical #>> '{policies,material,start_policy}' = 'requirement_groups'
                    AND jsonb_array_length(payload.canonical #> '{policies,material,item_groups}') = 0
                    AND jsonb_array_length(payload.canonical #> '{policies,material,requirement_groups}') > 0
                )
            )
            AND jsonb_typeof(payload.canonical->'capacity') = 'object'
            AND CASE
                WHEN (payload.canonical #>> '{capacity,capacity_slots}') ~ '^[0-9]+$'
                    THEN (payload.canonical #>> '{capacity,capacity_slots}')::NUMERIC BETWEEN 1 AND 64
                ELSE FALSE
            END
            AND CASE
                WHEN (payload.canonical #>> '{capacity,efficiency_percent}') ~ '^[0-9]+$'
                    THEN (payload.canonical #>> '{capacity,efficiency_percent}')::NUMERIC BETWEEN 1 AND 200
                ELSE FALSE
            END
            AND jsonb_typeof(payload.canonical #> '{capacity,working_windows}') = 'array'
            AND jsonb_typeof(payload.canonical->'training') = 'object'
            AND jsonb_typeof(payload.canonical #> '{training,enabled}') = 'boolean'
            AND jsonb_typeof(payload.canonical->'provenance') = 'object'
            AND payload.canonical #>> '{provenance,source}' IN ('default', 'custom')
            AND jsonb_typeof(payload.canonical->'versioning') = 'object'
            AND CASE
                WHEN (payload.canonical #>> '{versioning,revision}') ~ '^[0-9]+$'
                    THEN (payload.canonical #>> '{versioning,revision}')::NUMERIC > 0
                ELSE FALSE
            END
            AND jsonb_typeof(payload.canonical->'aas') = 'object'
            AND payload.canonical #>> '{aas,submodel_id}' =
                'urn:mini-rs-erp:submodel:apparatus:' ||
                substr(master_map.canonical_id, length('apparatus:') + 1)
            AND payload.canonical #>> '{aas,semantic_id}' =
                'urn:mini-rs-erp:semantic-id:submodel:apparatus:1'
            AND payload.canonical #>> '{aas,idta_release}' = '26-01'
            AND payload.canonical #>> '{aas,aas_metamodel_version}' = '3.2.0'
            AND payload.canonical #>> '{aas,aasx_part_5_version}' = 'IDTA-01005 v3.2'
            AND payload.canonical #>> '{aas,package_format}' = 'Open Packaging Conventions'
            AND payload.canonical #>> '{aas,media_type}' =
                'application/asset-administration-shell-package'
        )
    ) THEN
        RAISE EXCEPTION
            '0065 canonical apparatus payload failed nested contract validation';
    END IF;
END
$$;

-- Rename only through the explicit/unique map.  The five changed default keys
-- are now opaque and cannot be rejected as title-derived IDs.
UPDATE mini_apparatus master
SET id = mapping.canonical_id
FROM _canonical_apparatus_legacy_map mapping
WHERE lower(btrim(master.id)) = mapping.legacy_key
  AND master.id <> mapping.canonical_id;

-- Keep legacy columns as truthful display snapshots and move the primary-key
-- column of capacity/scheduling rows to the same canonical id.  The
-- canonical columns remain the only runtime identity and are checked below.
UPDATE mini_apparatus_capacity_profiles profile
SET apparatus_id = profile.canonical_apparatus_id,
    apparatus = master.name
FROM mini_apparatus master
WHERE master.id = profile.canonical_apparatus_id
  AND profile.apparatus_id <> profile.canonical_apparatus_id;
UPDATE mini_apparatus_downtimes downtime
SET apparatus_id = downtime.canonical_apparatus_id,
    apparatus = master.name
FROM mini_apparatus master
WHERE master.id = downtime.canonical_apparatus_id
  AND downtime.apparatus_id <> downtime.canonical_apparatus_id;
UPDATE mini_apparatus_schedule_reservations reservation
SET apparatus_id = reservation.canonical_apparatus_id,
    apparatus = master.name
FROM mini_apparatus master
WHERE master.id = reservation.canonical_apparatus_id
  AND reservation.apparatus_id <> reservation.canonical_apparatus_id;

DO $$
DECLARE expected_defaults INTEGER;
BEGIN
    SELECT count(*) INTO expected_defaults
    FROM mini_apparatus
    WHERE id IN (
        'apparatus:default:bosma_7', 'apparatus:default:bosma_8',
        'apparatus:default:bosma_9', 'apparatus:default:asset-004',
        'apparatus:default:asset-005', 'apparatus:default:holodniy_kley',
        'apparatus:default:asset-007', 'apparatus:default:asset-008',
        'apparatus:default:paket', 'apparatus:default:asset-010'
    );
    IF expected_defaults <> 10 THEN
        RAISE EXCEPTION '0065 canonical default apparatus cutover expected 10 rows, found %', expected_defaults;
    END IF;
END
$$;

-- Seed exactly one persisted capacity profile for every master.  Existing
-- canonical profiles are retained; missing profiles are materialized from the
-- canonical master payload.
INSERT INTO mini_apparatus_capacity_profiles (
    apparatus_id, canonical_apparatus_id, apparatus, capacity_slots, setup_minutes, cleanup_minutes,
    efficiency_percent, finite_capacity, working_windows, capabilities,
    capability_levels, notes, updated_at
)
SELECT master.id,
       master.id,
       master.name,
       COALESCE((master.payload_json #>> '{canonical_apparatus,capacity,capacity_slots}')::INTEGER, 1),
       COALESCE((master.payload_json #>> '{canonical_apparatus,capacity,setup_minutes}')::INTEGER, 0),
       COALESCE((master.payload_json #>> '{canonical_apparatus,capacity,cleanup_minutes}')::INTEGER, 0),
       COALESCE((master.payload_json #>> '{canonical_apparatus,capacity,efficiency_percent}')::INTEGER, 100),
       COALESCE((master.payload_json #>> '{canonical_apparatus,capacity,finite_capacity}')::BOOLEAN, TRUE),
       COALESCE(master.payload_json #> '{canonical_apparatus,capacity,working_windows}', '[]'::jsonb),
       COALESCE(master.payload_json #> '{canonical_apparatus,capabilities}', '[]'::jsonb),
       COALESCE((
           SELECT jsonb_object_agg(profile->>'code', COALESCE((profile->>'level')::INTEGER, 1))
           FROM jsonb_array_elements(
               COALESCE(master.payload_json #> '{canonical_apparatus,capability_profiles}', '[]'::jsonb)
           ) profile
       ), '{}'::jsonb),
       '', now()
FROM mini_apparatus master
ON CONFLICT (canonical_apparatus_id) DO NOTHING;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_apparatus master
        LEFT JOIN mini_apparatus_capacity_profiles profile
          ON profile.canonical_apparatus_id = master.id
        WHERE profile.canonical_apparatus_id IS NULL
    ) THEN
        RAISE EXCEPTION '0065 every canonical apparatus must have one capacity profile';
    END IF;
    IF EXISTS (
        SELECT canonical_apparatus_id
        FROM mini_apparatus_capacity_profiles
        GROUP BY canonical_apparatus_id
        HAVING count(*) <> 1
    ) THEN
        RAISE EXCEPTION '0065 duplicate canonical capacity profile';
    END IF;
END
$$;

-- Final relational constraints: canonical IDs are now the authoritative
-- conflict/FK keys.  The legacy text columns deliberately remain for audit and
-- display snapshots only.
ALTER TABLE mini_apparatus
    VALIDATE CONSTRAINT mini_apparatus_id_canonical_shape_check;
ALTER TABLE mini_worker_groups
    ADD CONSTRAINT mini_worker_groups_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_queue_sequences
    ADD CONSTRAINT mini_queue_sequences_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_queue_states
    ADD CONSTRAINT mini_queue_states_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_apparatus_queue_policies
    ADD CONSTRAINT mini_apparatus_queue_policies_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_queue_action_events
    ADD CONSTRAINT mini_queue_action_events_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_order_run_sessions
    ADD CONSTRAINT mini_order_run_sessions_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_order_progress_events
    ADD CONSTRAINT mini_order_progress_events_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_training_queue_states
    ADD CONSTRAINT mini_training_queue_states_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_training_progress_batches
    ADD CONSTRAINT mini_training_progress_batches_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_apparatus_capacity_profiles
    ADD CONSTRAINT mini_apparatus_capacity_profiles_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_apparatus_downtimes
    ADD CONSTRAINT mini_apparatus_downtimes_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_apparatus_schedule_reservations
    ADD CONSTRAINT mini_apparatus_schedule_reservations_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_apparatus_order_transfers
    ADD CONSTRAINT mini_apparatus_order_transfers_canonical_from_fk
        FOREIGN KEY (canonical_from_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID,
    ADD CONSTRAINT mini_apparatus_order_transfers_canonical_to_fk
        FOREIGN KEY (canonical_to_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_apparatus_material_rules
    ADD CONSTRAINT mini_apparatus_material_rules_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_returned_paint_requests
    ADD CONSTRAINT mini_returned_paint_requests_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_returned_paint_images
    ADD CONSTRAINT mini_returned_paint_images_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_training_returned_paint_reports
    ADD CONSTRAINT mini_training_returned_paint_reports_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_raw_material_events
    ADD CONSTRAINT mini_raw_material_events_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;

ALTER TABLE mini_worker_groups
    VALIDATE CONSTRAINT mini_worker_groups_canonical_apparatus_fk;
ALTER TABLE mini_queue_sequences
    VALIDATE CONSTRAINT mini_queue_sequences_canonical_apparatus_fk;
ALTER TABLE mini_queue_states
    VALIDATE CONSTRAINT mini_queue_states_canonical_apparatus_fk;
ALTER TABLE mini_apparatus_queue_policies
    VALIDATE CONSTRAINT mini_apparatus_queue_policies_canonical_apparatus_fk;
ALTER TABLE mini_queue_action_events
    VALIDATE CONSTRAINT mini_queue_action_events_canonical_apparatus_fk;
ALTER TABLE mini_order_run_sessions
    VALIDATE CONSTRAINT mini_order_run_sessions_canonical_apparatus_fk;
ALTER TABLE mini_order_progress_events
    VALIDATE CONSTRAINT mini_order_progress_events_canonical_apparatus_fk;
ALTER TABLE mini_training_queue_states
    VALIDATE CONSTRAINT mini_training_queue_states_canonical_apparatus_fk;
ALTER TABLE mini_training_progress_batches
    VALIDATE CONSTRAINT mini_training_progress_batches_canonical_apparatus_fk;
ALTER TABLE mini_apparatus_capacity_profiles
    VALIDATE CONSTRAINT mini_apparatus_capacity_profiles_canonical_apparatus_fk;
ALTER TABLE mini_apparatus_downtimes
    VALIDATE CONSTRAINT mini_apparatus_downtimes_canonical_apparatus_fk;
ALTER TABLE mini_apparatus_schedule_reservations
    VALIDATE CONSTRAINT mini_apparatus_schedule_reservations_canonical_apparatus_fk;
ALTER TABLE mini_apparatus_order_transfers
    VALIDATE CONSTRAINT mini_apparatus_order_transfers_canonical_from_fk;
ALTER TABLE mini_apparatus_order_transfers
    VALIDATE CONSTRAINT mini_apparatus_order_transfers_canonical_to_fk;
ALTER TABLE mini_apparatus_material_rules
    VALIDATE CONSTRAINT mini_apparatus_material_rules_canonical_apparatus_fk;
ALTER TABLE mini_returned_paint_requests
    VALIDATE CONSTRAINT mini_returned_paint_requests_canonical_apparatus_fk;
ALTER TABLE mini_returned_paint_images
    VALIDATE CONSTRAINT mini_returned_paint_images_canonical_apparatus_fk;
ALTER TABLE mini_training_returned_paint_reports
    VALIDATE CONSTRAINT mini_training_returned_paint_reports_canonical_apparatus_fk;
ALTER TABLE mini_raw_material_events
    VALIDATE CONSTRAINT mini_raw_material_events_canonical_apparatus_fk;

ALTER TABLE mini_production_map_nodes
    ADD CONSTRAINT mini_production_map_nodes_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID,
    ADD CONSTRAINT mini_production_map_nodes_canonical_alternative_fk
        FOREIGN KEY (canonical_alternative_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_production_map_nodes
    VALIDATE CONSTRAINT mini_production_map_nodes_canonical_apparatus_fk;
ALTER TABLE mini_production_map_nodes
    VALIDATE CONSTRAINT mini_production_map_nodes_canonical_alternative_fk;
ALTER TABLE mini_production_map_nodes
    ADD CONSTRAINT mini_production_map_nodes_apparatus_id_required
    CHECK (kind <> 'apparatus' OR canonical_apparatus_id IS NOT NULL);

ALTER TABLE mini_factory_location_apparatus_links
    ADD CONSTRAINT mini_factory_location_apparatus_links_apparatus_id_fkey
        FOREIGN KEY (apparatus_id) REFERENCES mini_apparatus(id) ON DELETE RESTRICT;

ALTER TABLE mini_progress_batches
    ADD CONSTRAINT mini_progress_batches_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID,
    ADD CONSTRAINT mini_progress_batches_canonical_current_fk
        FOREIGN KEY (canonical_current_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID,
    ADD CONSTRAINT mini_progress_batches_canonical_next_fk
        FOREIGN KEY (canonical_next_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID,
    ADD CONSTRAINT mini_progress_batches_canonical_used_by_fk
        FOREIGN KEY (canonical_used_by_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID,
    ADD CONSTRAINT mini_progress_batches_canonical_processed_by_fk
        FOREIGN KEY (canonical_processed_by_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_progress_batches
    VALIDATE CONSTRAINT mini_progress_batches_canonical_apparatus_fk;
ALTER TABLE mini_progress_batches
    VALIDATE CONSTRAINT mini_progress_batches_canonical_current_fk;
ALTER TABLE mini_progress_batches
    VALIDATE CONSTRAINT mini_progress_batches_canonical_next_fk;
ALTER TABLE mini_progress_batches
    VALIDATE CONSTRAINT mini_progress_batches_canonical_used_by_fk;
ALTER TABLE mini_progress_batches
    VALIDATE CONSTRAINT mini_progress_batches_canonical_processed_by_fk;

ALTER TABLE mini_training_queue_events
    ADD CONSTRAINT mini_training_queue_events_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_training_raw_material_assignments
    ADD CONSTRAINT mini_training_raw_material_assignments_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_training_apparatus_modes
    ADD CONSTRAINT mini_training_apparatus_modes_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_training_input_batches
    ADD CONSTRAINT mini_training_input_batches_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_raw_material_assignments
    ADD CONSTRAINT mini_raw_material_assignments_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_laminatsiya_astatka_reports
    ADD CONSTRAINT mini_laminatsiya_astatka_reports_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_rezka_astatka_reports
    ADD CONSTRAINT mini_rezka_astatka_reports_canonical_apparatus_fk
        FOREIGN KEY (canonical_apparatus_id) REFERENCES mini_apparatus(id) NOT VALID;
ALTER TABLE mini_training_queue_events
    VALIDATE CONSTRAINT mini_training_queue_events_canonical_apparatus_fk;
ALTER TABLE mini_training_raw_material_assignments
    VALIDATE CONSTRAINT mini_training_raw_material_assignments_canonical_apparatus_fk;
ALTER TABLE mini_training_apparatus_modes
    VALIDATE CONSTRAINT mini_training_apparatus_modes_canonical_apparatus_fk;
ALTER TABLE mini_training_input_batches
    VALIDATE CONSTRAINT mini_training_input_batches_canonical_apparatus_fk;
ALTER TABLE mini_raw_material_assignments
    VALIDATE CONSTRAINT mini_raw_material_assignments_canonical_apparatus_fk;
ALTER TABLE mini_laminatsiya_astatka_reports
    VALIDATE CONSTRAINT mini_laminatsiya_astatka_reports_canonical_apparatus_fk;
ALTER TABLE mini_rezka_astatka_reports
    VALIDATE CONSTRAINT mini_rezka_astatka_reports_canonical_apparatus_fk;

ALTER TABLE mini_worker_groups ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_queue_sequences ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_queue_states ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_apparatus_queue_policies ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_queue_action_events ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_order_run_sessions ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_order_progress_events ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_training_queue_states ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_training_queue_events ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_training_progress_batches ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_training_raw_material_assignments ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_training_apparatus_modes ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_training_input_batches ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_raw_material_assignments ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_laminatsiya_astatka_reports ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_rezka_astatka_reports ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_returned_paint_requests ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_returned_paint_images ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_training_returned_paint_reports ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_apparatus_capacity_profiles ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_apparatus_downtimes ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_apparatus_schedule_reservations ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_apparatus_order_transfers ALTER COLUMN canonical_from_apparatus_id SET NOT NULL;
ALTER TABLE mini_apparatus_order_transfers ALTER COLUMN canonical_to_apparatus_id SET NOT NULL;
ALTER TABLE mini_apparatus_material_rules ALTER COLUMN canonical_apparatus_id SET NOT NULL;
ALTER TABLE mini_progress_batches ALTER COLUMN canonical_apparatus_id SET NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_worker_groups_canonical_unique
    ON mini_worker_groups (canonical_apparatus_id, group_code);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_queue_sequences_canonical_unique
    ON mini_queue_sequences (canonical_apparatus_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_queue_states_canonical_unique
    ON mini_queue_states (canonical_apparatus_id, order_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_training_modes_canonical_unique
    ON mini_training_apparatus_modes (canonical_apparatus_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_training_raw_assignments_canonical_unique
    ON mini_training_raw_material_assignments
        (order_id, canonical_apparatus_id, lower(barcode));
CREATE INDEX IF NOT EXISTS idx_mini_raw_material_assignments_canonical_apparatus
    ON mini_raw_material_assignments (canonical_apparatus_id, order_id);
CREATE INDEX IF NOT EXISTS idx_mini_returned_paint_requests_canonical_apparatus
    ON mini_returned_paint_requests (canonical_apparatus_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_returned_paint_images_canonical_apparatus
    ON mini_returned_paint_images (canonical_apparatus_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_training_returned_paint_canonical_apparatus
    ON mini_training_returned_paint_reports (canonical_apparatus_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mini_raw_material_events_canonical_apparatus
    ON mini_raw_material_events (canonical_apparatus_id, occurred_at DESC);

CREATE OR REPLACE VIEW mini_canonical_apparatus_cutover_diagnostics AS
SELECT 'mini_worker_groups' AS source_table,
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL) AS unresolved_rows,
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id)) AS orphan_rows
FROM mini_worker_groups
UNION ALL SELECT 'mini_queue_sequences',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_queue_sequences
UNION ALL SELECT 'mini_queue_states',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_queue_states
UNION ALL SELECT 'mini_progress_batches',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_progress_batches
UNION ALL SELECT 'mini_production_map_nodes',
       count(*) FILTER (WHERE kind = 'apparatus' AND canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_production_map_nodes
UNION ALL SELECT 'mini_returned_paint_requests',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_returned_paint_requests
UNION ALL SELECT 'mini_returned_paint_images',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_returned_paint_images
UNION ALL SELECT 'mini_training_returned_paint_reports',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_training_returned_paint_reports
UNION ALL SELECT 'mini_raw_material_events',
       count(*) FILTER (WHERE btrim(COALESCE(apparatus, '')) <> ''
                         AND canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_raw_material_events
UNION ALL SELECT 'mini_apparatus_queue_policies',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_apparatus_queue_policies
UNION ALL SELECT 'mini_queue_action_events',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_queue_action_events
UNION ALL SELECT 'mini_order_run_sessions',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_order_run_sessions
UNION ALL SELECT 'mini_order_progress_events',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_order_progress_events
UNION ALL SELECT 'mini_training_queue_states',
       count(*) FILTER (
           WHERE btrim(COALESCE(apparatus, '')) <> ''
             AND canonical_apparatus_id IS NULL
       ),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_training_queue_states
UNION ALL SELECT 'mini_training_queue_events',
       count(*) FILTER (
           WHERE btrim(COALESCE(apparatus, '')) <> ''
             AND canonical_apparatus_id IS NULL
       ),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_training_queue_events
UNION ALL SELECT 'mini_training_progress_batches',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_training_progress_batches
UNION ALL SELECT 'mini_training_raw_material_assignments',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_training_raw_material_assignments
UNION ALL SELECT 'mini_training_apparatus_modes',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_training_apparatus_modes
UNION ALL SELECT 'mini_training_input_batches',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_training_input_batches
UNION ALL SELECT 'mini_raw_material_assignments',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_raw_material_assignments
UNION ALL SELECT 'mini_apparatus_capacity_profiles',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_apparatus_capacity_profiles
UNION ALL SELECT 'mini_apparatus_downtimes',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_apparatus_downtimes
UNION ALL SELECT 'mini_apparatus_schedule_reservations',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_apparatus_schedule_reservations
UNION ALL SELECT 'mini_apparatus_order_transfers',
       count(*) FILTER (
           WHERE canonical_from_apparatus_id IS NULL
              OR canonical_to_apparatus_id IS NULL
       ),
       count(*) FILTER (
           WHERE (canonical_from_apparatus_id IS NOT NULL
                  AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                  WHERE a.id = canonical_from_apparatus_id))
              OR (canonical_to_apparatus_id IS NOT NULL
                  AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                  WHERE a.id = canonical_to_apparatus_id))
       )
FROM mini_apparatus_order_transfers
UNION ALL SELECT 'mini_apparatus_material_rules',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_apparatus_material_rules
UNION ALL SELECT 'mini_laminatsiya_astatka_reports',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_laminatsiya_astatka_reports
UNION ALL SELECT 'mini_rezka_astatka_reports',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_apparatus_id))
FROM mini_rezka_astatka_reports
UNION ALL SELECT 'mini_progress_batches_current',
       count(*) FILTER (WHERE btrim(COALESCE(current_apparatus, '')) <> ''
                         AND canonical_current_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_current_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_current_apparatus_id))
FROM mini_progress_batches
UNION ALL SELECT 'mini_progress_batches_next',
       count(*) FILTER (WHERE btrim(COALESCE(next_apparatus, '')) <> ''
                         AND canonical_next_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_next_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_next_apparatus_id))
FROM mini_progress_batches
UNION ALL SELECT 'mini_progress_batches_used_by',
       count(*) FILTER (WHERE btrim(COALESCE(used_by_apparatus, '')) <> ''
                         AND canonical_used_by_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_used_by_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_used_by_apparatus_id))
FROM mini_progress_batches
UNION ALL SELECT 'mini_progress_batches_processed_by',
       count(*) FILTER (WHERE btrim(COALESCE(processed_by_apparatus, '')) <> ''
                         AND canonical_processed_by_apparatus_id IS NULL),
       count(*) FILTER (WHERE canonical_processed_by_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_processed_by_apparatus_id))
FROM mini_progress_batches
UNION ALL SELECT 'mini_production_map_node_alternatives',
       count(*) FILTER (
           WHERE btrim(COALESCE(payload_json->>'alternative_assigned_apparatus_id', '')) <> ''
             AND canonical_alternative_apparatus_id IS NULL
       ),
       count(*) FILTER (WHERE canonical_alternative_apparatus_id IS NOT NULL
                         AND NOT EXISTS (SELECT 1 FROM mini_apparatus a
                                         WHERE a.id = canonical_alternative_apparatus_id))
FROM mini_production_map_nodes;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM mini_canonical_apparatus_cutover_diagnostics
        WHERE unresolved_rows <> 0 OR orphan_rows <> 0
    ) THEN
        RAISE EXCEPTION
            '0065 canonical apparatus cutover diagnostics are not zero';
    END IF;
END
$$;
