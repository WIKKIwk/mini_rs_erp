-- Complete the canonical-apparatus cutover without rewriting migrations 0062
-- through 0064.  Legacy apparatus columns remain display/audit snapshots;
-- every live identity constraint and runtime projection below uses the
-- canonical apparatus id instead.

CREATE TEMP TABLE _canonical_apparatus_aliases (
    legacy_key TEXT PRIMARY KEY,
    canonical_id TEXT NOT NULL
) ON COMMIT DROP;

INSERT INTO _canonical_apparatus_aliases (legacy_key, canonical_id)
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

CREATE TEMP TABLE _canonical_apparatus_candidates (
    legacy_key TEXT NOT NULL,
    canonical_id TEXT NOT NULL
) ON COMMIT DROP;

INSERT INTO _canonical_apparatus_candidates (legacy_key, canonical_id)
SELECT legacy_key, canonical_id
FROM _canonical_apparatus_aliases;

INSERT INTO _canonical_apparatus_candidates (legacy_key, canonical_id)
SELECT lower(btrim(master.id)),
       COALESCE(alias.canonical_id, master.id)
FROM mini_apparatus master
LEFT JOIN _canonical_apparatus_aliases alias
  ON alias.legacy_key = lower(btrim(master.id))
WHERE btrim(master.id) <> '';

INSERT INTO _canonical_apparatus_candidates (legacy_key, canonical_id)
SELECT lower(btrim(master.name)),
       COALESCE(alias.canonical_id, master.id)
FROM mini_apparatus master
LEFT JOIN _canonical_apparatus_aliases alias
  ON alias.legacy_key = lower(btrim(master.name))
WHERE btrim(master.name) <> '';

INSERT INTO _canonical_apparatus_candidates (legacy_key, canonical_id)
SELECT lower(btrim(master.base_name)),
       COALESCE(alias.canonical_id, master.id)
FROM mini_apparatus master
LEFT JOIN _canonical_apparatus_aliases alias
  ON alias.legacy_key = lower(btrim(master.base_name))
WHERE btrim(master.base_name) <> '';

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

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM _canonical_apparatus_aliases alias
        WHERE NOT EXISTS (
            SELECT 1
            FROM mini_apparatus master
            WHERE master.id = alias.canonical_id
        )
    ) THEN
        RAISE EXCEPTION '0065 canonical apparatus alias points to a missing master row';
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_training_production_maps map_row
        CROSS JOIN LATERAL jsonb_array_elements(map_row.map_json->'nodes') AS nodes(node)
        CROSS JOIN LATERAL (
            VALUES
                (nodes.node->>'apparatus_id'),
                (nodes.node->>'alternative_assigned_apparatus_id')
        ) AS identity(identity_value)
        LEFT JOIN _canonical_apparatus_legacy_map mapping
          ON mapping.legacy_key = lower(btrim(identity.identity_value))
        WHERE nodes.node->>'kind' = 'apparatus'
          AND btrim(COALESCE(identity.identity_value, '')) <> ''
          AND NOT EXISTS (
              SELECT 1
              FROM mini_apparatus master
              WHERE master.id = COALESCE(
                  mapping.canonical_id,
                  btrim(identity.identity_value)
              )
          )
    ) THEN
        RAISE EXCEPTION
            '0065 training production-map apparatus identity is not a canonical master row';
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_training_progress_batches batch
        CROSS JOIN LATERAL (
            VALUES
                (batch.payload_json->>'apparatus'),
                (batch.payload_json->>'current_apparatus'),
                (batch.payload_json->>'next_apparatus'),
                (batch.payload_json->>'used_by_apparatus'),
                (batch.payload_json->>'processed_by_apparatus')
        ) AS identity(identity_value)
        LEFT JOIN _canonical_apparatus_legacy_map mapping
          ON mapping.legacy_key = lower(btrim(identity.identity_value))
        WHERE btrim(COALESCE(identity.identity_value, '')) <> ''
          AND NOT EXISTS (
              SELECT 1
              FROM mini_apparatus master
              WHERE master.id = COALESCE(
                  mapping.canonical_id,
                  btrim(identity.identity_value)
              )
          )
    ) THEN
        RAISE EXCEPTION
            '0065 training progress JSON apparatus identity is not a canonical master row';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM mini_training_raw_material_assignments assignment
        LEFT JOIN _canonical_apparatus_legacy_map mapping
          ON mapping.legacy_key = lower(btrim(assignment.payload_json->>'apparatus'))
        WHERE btrim(COALESCE(assignment.payload_json->>'apparatus', '')) <> ''
          AND NOT EXISTS (
              SELECT 1
              FROM mini_apparatus master
              WHERE master.id = COALESCE(
                  mapping.canonical_id,
                  btrim(assignment.payload_json->>'apparatus')
              )
          )
    ) THEN
        RAISE EXCEPTION
            '0065 training material JSON apparatus identity is not a canonical master row';
    END IF;
END
$$;

-- Freeze requests are identity-bearing even though their original target
-- column is retained as a display snapshot for historical chat cards.
ALTER TABLE mini_order_freeze_requests
    ADD COLUMN IF NOT EXISTS canonical_target_apparatus_id TEXT;

ALTER TABLE mini_order_freeze_requests
    DROP CONSTRAINT IF EXISTS mini_order_freeze_requests_canonical_target_apparatus_shape_check;
ALTER TABLE mini_order_freeze_requests
    ADD CONSTRAINT mini_order_freeze_requests_canonical_target_apparatus_shape_check
    CHECK (
        canonical_target_apparatus_id IS NULL OR (
            octet_length(canonical_target_apparatus_id) <= 128
            AND canonical_target_apparatus_id = btrim(canonical_target_apparatus_id)
            AND canonical_target_apparatus_id !~ '[[:space:][:cntrl:]]'
            AND canonical_target_apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
        )
    );

UPDATE mini_order_freeze_requests request
SET canonical_target_apparatus_id = COALESCE(
        NULLIF(btrim(request.canonical_target_apparatus_id), ''),
        mapping.canonical_id
    )
FROM _canonical_apparatus_legacy_map mapping
WHERE btrim(request.target_apparatus) <> ''
  AND lower(btrim(request.target_apparatus)) = mapping.legacy_key;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_order_freeze_requests request
        LEFT JOIN mini_apparatus master
          ON master.id = request.canonical_target_apparatus_id
        WHERE btrim(request.target_apparatus) <> ''
          AND (
              request.canonical_target_apparatus_id IS NULL
              OR master.id IS NULL
          )
    ) THEN
        RAISE EXCEPTION
            '0065 unresolved or orphan freeze target apparatus identity';
    END IF;
END
$$;

ALTER TABLE mini_order_freeze_requests
    DROP CONSTRAINT IF EXISTS mini_order_freeze_requests_canonical_target_apparatus_fk;
ALTER TABLE mini_order_freeze_requests
    ADD CONSTRAINT mini_order_freeze_requests_canonical_target_apparatus_fk
    FOREIGN KEY (canonical_target_apparatus_id) REFERENCES mini_apparatus(id)
    ON DELETE RESTRICT NOT VALID;
ALTER TABLE mini_order_freeze_requests
    VALIDATE CONSTRAINT mini_order_freeze_requests_canonical_target_apparatus_fk;

ALTER TABLE mini_order_freeze_requests
    DROP CONSTRAINT IF EXISTS mini_order_freeze_requests_canonical_target_apparatus_required;
ALTER TABLE mini_order_freeze_requests
    ADD CONSTRAINT mini_order_freeze_requests_canonical_target_apparatus_required
    CHECK (
        (btrim(target_apparatus) = '' AND canonical_target_apparatus_id IS NULL)
        OR (btrim(target_apparatus) <> '' AND canonical_target_apparatus_id IS NOT NULL)
    );

CREATE INDEX IF NOT EXISTS idx_mini_order_freeze_requests_canonical_target_apparatus
    ON mini_order_freeze_requests (canonical_target_apparatus_id, requested_at_unix DESC);

-- Warehouse assignments are now explicitly typed.  The legacy warehouse
-- column is retained as a display/audit snapshot for runtime compatibility;
-- assignment_kind, warehouse_name, and apparatus_id are authoritative.
ALTER TABLE mini_warehouse_assignments
    ADD COLUMN IF NOT EXISTS assignment_kind TEXT,
    ADD COLUMN IF NOT EXISTS warehouse_name TEXT,
    ADD COLUMN IF NOT EXISTS apparatus_id TEXT;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_warehouse_assignments assignment
        WHERE assignment.assignment_kind IS NULL
          AND assignment.warehouse_name IS NULL
          AND assignment.apparatus_id IS NULL
          AND EXISTS (
              SELECT 1
              FROM mini_warehouses warehouse
              WHERE warehouse.name = assignment.warehouse
          )
          AND EXISTS (
              SELECT 1
              FROM mini_apparatus apparatus
              WHERE apparatus.id = assignment.warehouse
                AND assignment.warehouse = btrim(assignment.warehouse)
                AND octet_length(assignment.warehouse) <= 128
                AND assignment.warehouse !~ '[[:space:][:cntrl:]]'
                AND assignment.warehouse ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
          )
    ) THEN
        RAISE EXCEPTION
            '0065 warehouse assignment legacy value matches both warehouse and apparatus identities';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM mini_warehouse_assignments assignment
        WHERE assignment.assignment_kind IS NULL
          AND assignment.warehouse_name IS NULL
          AND assignment.apparatus_id IS NULL
          AND NOT EXISTS (
              SELECT 1
              FROM mini_warehouses warehouse
              WHERE warehouse.name = assignment.warehouse
          )
          AND NOT EXISTS (
              SELECT 1
              FROM mini_apparatus apparatus
              WHERE apparatus.id = assignment.warehouse
                AND assignment.warehouse = btrim(assignment.warehouse)
                AND octet_length(assignment.warehouse) <= 128
                AND assignment.warehouse !~ '[[:space:][:cntrl:]]'
                AND assignment.warehouse ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
          )
    ) THEN
        RAISE EXCEPTION
            '0065 warehouse assignment legacy value matches neither warehouse nor canonical apparatus identity';
    END IF;
END
$$;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_warehouse_assignments assignment
        WHERE assignment.assignment_kind IS NULL
          AND assignment.warehouse_name IS NOT NULL
          AND assignment.apparatus_id IS NOT NULL
    ) THEN
        RAISE EXCEPTION
            '0065 warehouse assignment has both canonical identity columns populated before backfill';
    END IF;
END
$$;

UPDATE mini_warehouse_assignments assignment
SET assignment_kind = CASE
        WHEN assignment.warehouse_name IS NOT NULL THEN 'warehouse'
        WHEN assignment.apparatus_id IS NOT NULL THEN 'apparatus'
        WHEN EXISTS (
            SELECT 1
            FROM mini_warehouses warehouse
            WHERE warehouse.name = assignment.warehouse
        ) THEN 'warehouse'
        ELSE 'apparatus'
    END,
    warehouse_name = CASE
        WHEN assignment.warehouse_name IS NOT NULL THEN assignment.warehouse_name
        WHEN assignment.apparatus_id IS NOT NULL THEN NULL
        WHEN EXISTS (
            SELECT 1
            FROM mini_warehouses warehouse
            WHERE warehouse.name = assignment.warehouse
        ) THEN assignment.warehouse
        ELSE NULL
    END,
    apparatus_id = CASE
        WHEN assignment.warehouse_name IS NOT NULL THEN NULL
        WHEN assignment.apparatus_id IS NOT NULL THEN assignment.apparatus_id
        WHEN EXISTS (
            SELECT 1
            FROM mini_warehouses warehouse
            WHERE warehouse.name = assignment.warehouse
        ) THEN NULL
        ELSE assignment.warehouse
    END
WHERE assignment.assignment_kind IS NULL;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_warehouse_assignments assignment
        WHERE assignment.assignment_kind IS NULL
           OR assignment.assignment_kind NOT IN ('warehouse', 'apparatus')
           OR (
               assignment.assignment_kind = 'warehouse'
               AND (
                   assignment.warehouse_name IS NULL
                   OR assignment.apparatus_id IS NOT NULL
               )
           )
           OR (
               assignment.assignment_kind = 'apparatus'
               AND (
                   assignment.apparatus_id IS NULL
                   OR assignment.warehouse_name IS NOT NULL
               )
           )
    ) THEN
        RAISE EXCEPTION
            '0065 warehouse assignment does not have exactly one typed canonical identity';
    END IF;
END
$$;

ALTER TABLE mini_warehouse_assignments
    ALTER COLUMN assignment_kind SET NOT NULL;

ALTER TABLE mini_warehouse_assignments
    DROP CONSTRAINT IF EXISTS mini_warehouse_assignments_assignment_kind_check;
ALTER TABLE mini_warehouse_assignments
    ADD CONSTRAINT mini_warehouse_assignments_assignment_kind_check
    CHECK (
        (assignment_kind = 'warehouse'
            AND warehouse_name IS NOT NULL
            AND apparatus_id IS NULL)
        OR (assignment_kind = 'apparatus'
            AND apparatus_id IS NOT NULL
            AND warehouse_name IS NULL
            AND apparatus_id = btrim(apparatus_id)
            AND octet_length(apparatus_id) <= 128
            AND apparatus_id !~ '[[:space:][:cntrl:]]'
            AND apparatus_id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$')
    );

ALTER TABLE mini_warehouse_assignments
    DROP CONSTRAINT IF EXISTS mini_warehouse_assignments_warehouse_fkey;
ALTER TABLE mini_warehouse_assignments
    DROP CONSTRAINT IF EXISTS mini_warehouse_assignments_warehouse_name_fk;
ALTER TABLE mini_warehouse_assignments
    ADD CONSTRAINT mini_warehouse_assignments_warehouse_name_fk
    FOREIGN KEY (warehouse_name) REFERENCES mini_warehouses(name)
    ON UPDATE CASCADE
    ON DELETE RESTRICT NOT VALID;
ALTER TABLE mini_warehouse_assignments
    VALIDATE CONSTRAINT mini_warehouse_assignments_warehouse_name_fk;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_warehouse_assignments assignment
        WHERE assignment.assignment_kind = 'apparatus'
          AND NOT EXISTS (
              SELECT 1
              FROM mini_apparatus apparatus
              WHERE apparatus.id = assignment.apparatus_id
          )
    ) THEN
        RAISE EXCEPTION
            '0065 warehouse assignment apparatus identity is unresolved';
    END IF;
END
$$;

ALTER TABLE mini_warehouse_assignments
    DROP CONSTRAINT IF EXISTS mini_warehouse_assignments_apparatus_id_fk;
ALTER TABLE mini_warehouse_assignments
    ADD CONSTRAINT mini_warehouse_assignments_apparatus_id_fk
    FOREIGN KEY (apparatus_id) REFERENCES mini_apparatus(id)
    ON UPDATE RESTRICT
    ON DELETE RESTRICT NOT VALID;
ALTER TABLE mini_warehouse_assignments
    VALIDATE CONSTRAINT mini_warehouse_assignments_apparatus_id_fk;

CREATE INDEX IF NOT EXISTS idx_mini_warehouse_assignments_warehouse_name
    ON mini_warehouse_assignments (warehouse_name)
    WHERE assignment_kind = 'warehouse';
CREATE INDEX IF NOT EXISTS idx_mini_warehouse_assignments_apparatus_id
    ON mini_warehouse_assignments (apparatus_id)
    WHERE assignment_kind = 'apparatus';

-- The legacy primary key is a compatibility key and does not prevent the
-- same canonical identity from being assigned through a different legacy
-- snapshot.  Principal-scoped canonical uniqueness is therefore enforced by
-- separate partial indexes for the two typed assignment shapes.
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_warehouse_assignments_warehouse_identity_unique
    ON mini_warehouse_assignments (warehouse_name, principal_role, principal_ref)
    WHERE assignment_kind = 'warehouse';
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_warehouse_assignments_apparatus_identity_unique
    ON mini_warehouse_assignments (apparatus_id, principal_role, principal_ref)
    WHERE assignment_kind = 'apparatus';

-- Existing training maps are JSON contracts.  Rewrite only exact legacy
-- aliases; already-canonical training-only IDs are retained after shape
-- validation.  A title, malformed value, or ambiguous alias never becomes an
-- identity by inference.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_training_production_maps map_row
        WHERE map_row.map_json ? 'nodes'
          AND jsonb_typeof(map_row.map_json->'nodes') <> 'array'
    ) THEN
        RAISE EXCEPTION
            '0065 malformed training map nodes payload; expected array';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM mini_training_production_maps map_row
        CROSS JOIN LATERAL jsonb_array_elements(map_row.map_json->'nodes') AS nodes(node)
        CROSS JOIN LATERAL (
            VALUES
                ('apparatus_id', nodes.node->>'apparatus_id'),
                ('alternative_assigned_apparatus_id',
                    nodes.node->>'alternative_assigned_apparatus_id')
        ) AS identity(field_name, identity_value)
        LEFT JOIN _canonical_apparatus_legacy_map mapping
          ON mapping.legacy_key = lower(btrim(identity.identity_value))
        WHERE btrim(COALESCE(identity.identity_value, '')) <> ''
          AND mapping.canonical_id IS NULL
          AND NOT (
              identity.identity_value = btrim(identity.identity_value)
              AND octet_length(identity.identity_value) <= 128
              AND identity.identity_value !~ '[[:space:][:cntrl:]]'
              AND identity.identity_value ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
          )
    ) THEN
        RAISE EXCEPTION
            '0065 unresolved or malformed training map JSON apparatus identity';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM mini_training_production_maps map_row
        CROSS JOIN LATERAL jsonb_array_elements(map_row.map_json->'nodes') AS nodes(node)
        WHERE nodes.node->>'kind' = 'apparatus'
          AND btrim(COALESCE(nodes.node->>'apparatus_id', '')) = ''
          AND NOT (
              nodes.node->>'role_code' = 'training_input'
              AND nodes.node->>'item_code' IN (
                  'training-input:bosma',
                  'training-input:laminatsiya'
              )
          )
    ) THEN
        RAISE EXCEPTION
            '0065 training apparatus node has no apparatus identity';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM mini_training_production_maps map_row
        CROSS JOIN LATERAL jsonb_array_elements(map_row.map_json->'nodes') AS nodes(node)
        WHERE nodes.node->>'kind' = 'apparatus'
          AND nodes.node->>'role_code' = 'training_input'
          AND (
              btrim(COALESCE(nodes.node->>'apparatus_id', '')) <> ''
              OR btrim(COALESCE(nodes.node->>'alternative_assigned_apparatus_id', '')) <> ''
          )
    ) THEN
        RAISE EXCEPTION
            '0065 virtual training input node cannot carry production apparatus identity';
    END IF;
END
$$;

UPDATE mini_training_production_maps map_row
SET map_json = jsonb_set(
    map_row.map_json,
    '{nodes}',
    COALESCE((
        SELECT jsonb_agg(
            CASE
                WHEN btrim(COALESCE(nodes.node->>'apparatus_id', '')) = ''
                    AND btrim(COALESCE(nodes.node->>'alternative_assigned_apparatus_id', '')) = ''
                    THEN nodes.node
                ELSE jsonb_set(
                    CASE
                        WHEN btrim(COALESCE(nodes.node->>'apparatus_id', '')) = ''
                            THEN nodes.node
                        ELSE jsonb_set(
                            nodes.node,
                            '{apparatus_id}',
                            to_jsonb(COALESCE(
                                main_mapping.canonical_id,
                                btrim(nodes.node->>'apparatus_id')
                            )),
                            true
                        )
                    END,
                    '{alternative_assigned_apparatus_id}',
                    to_jsonb(CASE
                        WHEN btrim(COALESCE(nodes.node->>'alternative_assigned_apparatus_id', '')) = ''
                            THEN ''
                        ELSE COALESCE(
                            alternative_mapping.canonical_id,
                            btrim(nodes.node->>'alternative_assigned_apparatus_id')
                        )
                    END),
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
    ), '[]'::jsonb),
    true
)
WHERE jsonb_typeof(map_row.map_json->'nodes') = 'array';

-- Training progress and material assignment payloads also contain historical
-- apparatus strings.  Their relational canonical columns are the source of
-- truth; the JSON is rewritten to the same canonical projections.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_training_progress_batches batch
        CROSS JOIN LATERAL (
            VALUES
                ('apparatus', batch.payload_json->>'apparatus'),
                ('current_apparatus', batch.payload_json->>'current_apparatus'),
                ('next_apparatus', batch.payload_json->>'next_apparatus'),
                ('used_by_apparatus', batch.payload_json->>'used_by_apparatus'),
                ('processed_by_apparatus', batch.payload_json->>'processed_by_apparatus')
        ) AS identity(field_name, identity_value)
        LEFT JOIN _canonical_apparatus_legacy_map mapping
          ON mapping.legacy_key = lower(btrim(identity.identity_value))
        WHERE jsonb_typeof(batch.payload_json) <> 'object'
           OR (
               btrim(COALESCE(identity.identity_value, '')) <> ''
               AND mapping.canonical_id IS NULL
               AND NOT (
                   identity.identity_value = btrim(identity.identity_value)
                   AND octet_length(identity.identity_value) <= 128
                   AND identity.identity_value !~ '[[:space:][:cntrl:]]'
                   AND identity.identity_value ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
               )
           )
    ) THEN
        RAISE EXCEPTION
            '0065 unresolved or malformed training progress JSON apparatus identity';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM mini_training_progress_batches batch
        WHERE batch.canonical_apparatus_id IS NULL
           OR btrim(COALESCE(batch.payload_json->>'apparatus', '')) = ''
    ) THEN
        RAISE EXCEPTION
            '0065 training progress batch has no canonical apparatus identity';
    END IF;

    IF EXISTS (
        SELECT 1
        FROM mini_training_raw_material_assignments assignment
        WHERE jsonb_typeof(assignment.payload_json) <> 'object'
           OR (
               btrim(COALESCE(assignment.payload_json->>'apparatus', '')) <> ''
               AND NOT EXISTS (
                   SELECT 1
                   FROM _canonical_apparatus_legacy_map mapping
                   WHERE mapping.legacy_key = lower(btrim(assignment.payload_json->>'apparatus'))
               )
               AND NOT (
                   assignment.payload_json->>'apparatus' = btrim(assignment.payload_json->>'apparatus')
                   AND octet_length(assignment.payload_json->>'apparatus') <= 128
                   AND assignment.payload_json->>'apparatus' !~ '[[:space:][:cntrl:]]'
                   AND assignment.payload_json->>'apparatus' ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
               )
           )
    ) THEN
        RAISE EXCEPTION
            '0065 unresolved or malformed training material JSON apparatus identity';
    END IF;
END
$$;

UPDATE mini_training_progress_batches batch
SET payload_json = jsonb_set(
    jsonb_set(
        jsonb_set(
            jsonb_set(
                jsonb_set(
                    batch.payload_json,
                    '{apparatus}',
                    to_jsonb(batch.canonical_apparatus_id),
                    true
                ),
                '{current_apparatus}',
                to_jsonb(COALESCE(
                    (SELECT mapping.canonical_id
                     FROM _canonical_apparatus_legacy_map mapping
                     WHERE mapping.legacy_key = lower(btrim(batch.payload_json->>'current_apparatus'))),
                    NULLIF(btrim(batch.payload_json->>'current_apparatus'), ''),
                    ''
                )),
                true
            ),
            '{next_apparatus}',
            to_jsonb(COALESCE(
                (SELECT mapping.canonical_id
                 FROM _canonical_apparatus_legacy_map mapping
                 WHERE mapping.legacy_key = lower(btrim(batch.payload_json->>'next_apparatus'))),
                NULLIF(btrim(batch.payload_json->>'next_apparatus'), ''),
                ''
            )),
            true
        ),
        '{used_by_apparatus}',
        to_jsonb(COALESCE(
            (SELECT mapping.canonical_id
             FROM _canonical_apparatus_legacy_map mapping
             WHERE mapping.legacy_key = lower(btrim(batch.payload_json->>'used_by_apparatus'))),
            NULLIF(btrim(batch.payload_json->>'used_by_apparatus'), ''),
            ''
        )),
        true
    ),
    '{processed_by_apparatus}',
    to_jsonb(COALESCE(
        (SELECT mapping.canonical_id
         FROM _canonical_apparatus_legacy_map mapping
         WHERE mapping.legacy_key = lower(btrim(batch.payload_json->>'processed_by_apparatus'))),
        NULLIF(btrim(batch.payload_json->>'processed_by_apparatus'), ''),
        ''
    )),
    true
)
WHERE jsonb_typeof(batch.payload_json) = 'object';

UPDATE mini_training_raw_material_assignments assignment
SET payload_json = jsonb_set(
    assignment.payload_json,
    '{apparatus}',
    to_jsonb(assignment.canonical_apparatus_id),
    true
)
WHERE jsonb_typeof(assignment.payload_json) = 'object';

-- A canonical unique key must replace every live apparatus identity key.  The
-- canonical indexes were staged by 0062 through 0064; the training queue and
-- open-session indexes are added here before their legacy counterparts are
-- retired.  Any duplicate canonical identity aborts the transaction.
DO $$
BEGIN
    IF EXISTS (
        SELECT canonical_apparatus_id, order_id
        FROM mini_training_queue_states
        WHERE canonical_apparatus_id IS NOT NULL
        GROUP BY canonical_apparatus_id, order_id
        HAVING count(*) > 1
    ) THEN
        RAISE EXCEPTION '0065 duplicate canonical training queue state identity';
    END IF;
    IF EXISTS (
        SELECT canonical_apparatus_id, order_id
        FROM mini_order_run_sessions
        WHERE status IN ('active', 'paused', 'frozen', 'roll_detached')
        GROUP BY canonical_apparatus_id, order_id
        HAVING count(*) > 1
    ) THEN
        RAISE EXCEPTION '0065 duplicate open canonical order-run session identity';
    END IF;
END
$$;

CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_training_queue_states_canonical_unique
    ON mini_training_queue_states (canonical_apparatus_id, order_id);

DROP INDEX IF EXISTS idx_mini_order_run_sessions_one_open;
CREATE UNIQUE INDEX IF NOT EXISTS idx_mini_order_run_sessions_one_open_canonical
    ON mini_order_run_sessions (canonical_apparatus_id, order_id)
    WHERE status IN ('active', 'paused', 'frozen', 'roll_detached');

ALTER TABLE mini_worker_groups
    DROP CONSTRAINT IF EXISTS mini_worker_groups_pkey;
ALTER TABLE mini_queue_sequences
    DROP CONSTRAINT IF EXISTS mini_queue_sequences_pkey;
ALTER TABLE mini_queue_states
    DROP CONSTRAINT IF EXISTS mini_queue_states_pkey;
ALTER TABLE mini_apparatus_queue_policies
    DROP CONSTRAINT IF EXISTS mini_apparatus_queue_policies_pkey;
ALTER TABLE mini_training_queue_states
    DROP CONSTRAINT IF EXISTS mini_training_queue_states_pkey;
ALTER TABLE mini_training_apparatus_modes
    DROP CONSTRAINT IF EXISTS mini_training_apparatus_modes_pkey;
ALTER TABLE mini_apparatus_material_rules
    DROP CONSTRAINT IF EXISTS mini_apparatus_material_rules_pkey;
ALTER TABLE mini_apparatus_capacity_profiles
    DROP CONSTRAINT IF EXISTS mini_apparatus_capacity_profiles_pkey;
ALTER TABLE mini_apparatus
    DROP CONSTRAINT IF EXISTS mini_apparatus_id_name_unique;
DROP INDEX IF EXISTS idx_mini_training_raw_assignments_identity;

DO $$
BEGIN
    IF EXISTS (
        SELECT canonical_apparatus_id, order_id
        FROM mini_training_queue_states
        WHERE canonical_apparatus_id IS NOT NULL
        GROUP BY canonical_apparatus_id, order_id
        HAVING count(*) > 1
    ) THEN
        RAISE EXCEPTION '0065 duplicate canonical training queue state identity';
    END IF;
END
$$;

CREATE OR REPLACE VIEW mini_canonical_authority_cutover_diagnostics AS
SELECT 'mini_order_freeze_requests' AS source_table,
       count(*) FILTER (
           WHERE btrim(target_apparatus) <> ''
             AND canonical_target_apparatus_id IS NULL
       ) AS unresolved_rows,
       count(*) FILTER (
           WHERE canonical_target_apparatus_id IS NOT NULL
             AND NOT EXISTS (
                 SELECT 1 FROM mini_apparatus master
                 WHERE master.id = canonical_target_apparatus_id
             )
       ) AS orphan_rows
FROM mini_order_freeze_requests
UNION ALL
SELECT 'mini_training_progress_batches',
       count(*) FILTER (WHERE canonical_apparatus_id IS NULL),
       count(*) FILTER (
           WHERE canonical_apparatus_id IS NOT NULL
             AND NOT EXISTS (
                 SELECT 1 FROM mini_apparatus master
                 WHERE master.id = canonical_apparatus_id
             )
       )
FROM mini_training_progress_batches;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_canonical_authority_cutover_diagnostics
        WHERE unresolved_rows <> 0 OR orphan_rows <> 0
    ) THEN
        RAISE EXCEPTION
            '0065 canonical authority cutover diagnostics are not zero';
    END IF;
END
$$;

-- Re-establish relational primary keys on the canonical identity columns after
-- the legacy apparatus keys are retired. The unique indexes were staged by
-- 0063/0064 and are consumed as constraints here so writes, conflict targets,
-- and foreign-key references all converge on ApparatusId.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM mini_worker_groups
        GROUP BY canonical_apparatus_id, group_code
        HAVING count(*) > 1
    ) THEN
        RAISE EXCEPTION '0065 duplicate canonical worker-group identity';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_queue_sequences
        GROUP BY canonical_apparatus_id
        HAVING count(*) > 1
    ) THEN
        RAISE EXCEPTION '0065 duplicate canonical queue-sequence identity';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_queue_states
        GROUP BY canonical_apparatus_id, order_id
        HAVING count(*) > 1
    ) THEN
        RAISE EXCEPTION '0065 duplicate canonical queue-state identity';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_apparatus_queue_policies
        GROUP BY canonical_apparatus_id
        HAVING count(*) > 1
    ) THEN
        RAISE EXCEPTION '0065 duplicate canonical queue-policy identity';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_training_apparatus_modes
        GROUP BY canonical_apparatus_id
        HAVING count(*) > 1
    ) THEN
        RAISE EXCEPTION '0065 duplicate canonical training-mode identity';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_apparatus_material_rules
        GROUP BY canonical_apparatus_id
        HAVING count(*) > 1
    ) THEN
        RAISE EXCEPTION '0065 duplicate canonical material-rule identity';
    END IF;
    IF EXISTS (
        SELECT 1
        FROM mini_apparatus_capacity_profiles
        GROUP BY canonical_apparatus_id
        HAVING count(*) > 1
    ) THEN
        RAISE EXCEPTION '0065 duplicate canonical capacity-profile identity';
    END IF;
END
$$;

ALTER TABLE mini_worker_groups
    ADD CONSTRAINT mini_worker_groups_pkey
    PRIMARY KEY USING INDEX idx_mini_worker_groups_canonical_unique;
ALTER TABLE mini_queue_sequences
    ADD CONSTRAINT mini_queue_sequences_pkey
    PRIMARY KEY USING INDEX idx_mini_queue_sequences_canonical_unique;
ALTER TABLE mini_queue_states
    ADD CONSTRAINT mini_queue_states_pkey
    PRIMARY KEY USING INDEX idx_mini_queue_states_canonical_unique;
ALTER TABLE mini_apparatus_queue_policies
    ADD CONSTRAINT mini_apparatus_queue_policies_pkey
    PRIMARY KEY USING INDEX idx_mini_apparatus_queue_policies_canonical_unique;
ALTER TABLE mini_training_apparatus_modes
    ADD CONSTRAINT mini_training_apparatus_modes_pkey
    PRIMARY KEY USING INDEX idx_mini_training_modes_canonical_unique;
ALTER TABLE mini_apparatus_material_rules
    ADD CONSTRAINT mini_apparatus_material_rules_pkey
    PRIMARY KEY USING INDEX idx_mini_apparatus_material_rules_canonical_unique;
ALTER TABLE mini_apparatus_capacity_profiles
    ADD CONSTRAINT mini_apparatus_capacity_profiles_pkey
    PRIMARY KEY USING INDEX idx_mini_apparatus_capacity_profiles_canonical_unique;

-- Training queue rows may represent an explicitly allow-listed virtual input
-- token and therefore cannot use canonical_apparatus_id as a primary key.
-- Keep a durable row identity while the canonical/virtual partial unique keys
-- above govern business identity.
ALTER TABLE mini_training_queue_states
    ADD COLUMN IF NOT EXISTS state_id BIGINT GENERATED BY DEFAULT AS IDENTITY;
ALTER TABLE mini_training_queue_states
    ADD CONSTRAINT mini_training_queue_states_pkey
    PRIMARY KEY (state_id);
