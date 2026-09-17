-- Backfill persisted production maps after topology alternatives became
-- canonical at the validated save boundary.
--
-- Existing explicit groups and assignments are authoritative. This migration
-- only repairs unassigned laminate stages whose topology is unambiguous:
-- matching incoming/outgoing edges and work parameters, with the two active
-- laminate apparatuses having the same runtime profile. A one-sided stage is
-- expanded to the missing compatible apparatus so old orders and templates
-- receive the same two-machine alternative contract as newly saved maps.

CREATE TEMP TABLE mini_0122_changed_maps (
    map_id TEXT PRIMARY KEY,
    map_json JSONB NOT NULL
) ON COMMIT DROP;

CREATE TEMP TABLE mini_0122_apparatus_nodes (
    map_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    apparatus_id TEXT NOT NULL,
    group_id TEXT NOT NULL,
    assigned_apparatus_id TEXT NOT NULL,
    incoming JSONB NOT NULL,
    outgoing JSONB NOT NULL,
    work_parameters JSONB NOT NULL,
    equipment_class_id TEXT,
    execution_profile_json JSONB,
    capabilities_json JSONB,
    PRIMARY KEY (map_id, node_id)
) ON COMMIT DROP;

DO $$
DECLARE
    map_row RECORD;
    node_row RECORD;
    pair_row RECORD;
    map_nodes JSONB;
    map_edges JSONB;
    original_edges JSONB;
    v_node_value JSONB;
    clone_value JSONB;
    edge_value JSONB;
    new_edge_value JSONB;
    group_id TEXT;
    group_base TEXT;
    group_suffix INTEGER;
    target_apparatus_id TEXT;
    target_title TEXT;
    new_node_id TEXT;
    node_index INTEGER;
    other_node_count INTEGER;
    changed BOOLEAN;
    x_value DOUBLE PRECISION;
    lam1_class TEXT;
    lam2_class TEXT;
    lam1_profile JSONB;
    lam2_profile JSONB;
    lam1_capabilities JSONB;
    lam2_capabilities JSONB;
BEGIN
    SELECT equipment_class_id, execution_profile_json, capabilities_json
    INTO lam1_class, lam1_profile, lam1_capabilities
    FROM mini_apparatus
    WHERE id = 'apparatus:default:asset-007'
      AND lifecycle_state = 'active';

    SELECT equipment_class_id, execution_profile_json, capabilities_json
    INTO lam2_class, lam2_profile, lam2_capabilities
    FROM mini_apparatus
    WHERE id = 'apparatus:default:asset-008'
      AND lifecycle_state = 'active';

    IF lam1_class IS NULL OR lam2_class IS NULL THEN
        RAISE EXCEPTION '0122 requires active Laminatsiya 1 and Laminatsiya 2 apparatuses';
    END IF;

    IF lam1_class <> lam2_class
       OR lam1_profile <> lam2_profile
       OR lam1_capabilities <> lam2_capabilities THEN
        RAISE EXCEPTION '0122 laminate apparatus runtime profiles are not compatible';
    END IF;

    FOR map_row IN
        SELECT id, map_json
        FROM mini_production_maps
        WHERE jsonb_typeof(map_json->'nodes') = 'array'
        ORDER BY id
    LOOP
        DELETE FROM mini_0122_apparatus_nodes WHERE map_id = map_row.id;

        INSERT INTO mini_0122_apparatus_nodes (
            map_id, node_id, apparatus_id, group_id, assigned_apparatus_id,
            incoming, outgoing, work_parameters,
            equipment_class_id, execution_profile_json, capabilities_json
        )
        SELECT
            map_row.id,
            node_value->>'id',
            node_value->>'apparatus_id',
            COALESCE(node_value->>'alternative_group_id', ''),
            COALESCE(node_value->>'alternative_assigned_apparatus_id', ''),
            COALESCE((
                SELECT jsonb_agg(
                    jsonb_build_array(edge->>'from', edge->>'branch')
                    ORDER BY edge->>'from', edge->>'branch'
                )
                FROM jsonb_array_elements(COALESCE(map_row.map_json->'edges', '[]'::jsonb)) AS edges(edge)
                WHERE edge->>'to' = node_value->>'id'
            ), '[]'::jsonb),
            COALESCE((
                SELECT jsonb_agg(
                    jsonb_build_array(edge->>'to', edge->>'branch')
                    ORDER BY edge->>'to', edge->>'branch'
                )
                FROM jsonb_array_elements(COALESCE(map_row.map_json->'edges', '[]'::jsonb)) AS edges(edge)
                WHERE edge->>'from' = node_value->>'id'
            ), '[]'::jsonb),
            jsonb_build_array(
                node_value->'formula',
                node_value->'item_code',
                node_value->'qty_formula',
                node_value->'from_location',
                node_value->'to_location',
                node_value->'rezka_kadr_count',
                node_value->'rezka_frame_groups',
                node_value->'rezka_label_length'
            ),
            apparatus.equipment_class_id,
            apparatus.execution_profile_json,
            apparatus.capabilities_json
        FROM jsonb_array_elements(map_row.map_json->'nodes') AS nodes(node_value)
        LEFT JOIN mini_apparatus apparatus
            ON apparatus.id = node_value->>'apparatus_id'
        WHERE node_value->>'kind' = 'apparatus'
          AND btrim(node_value->>'id') <> ''
          AND btrim(node_value->>'apparatus_id') <> '';

        map_nodes := map_row.map_json->'nodes';
        map_edges := COALESCE(map_row.map_json->'edges', '[]'::jsonb);
        changed := FALSE;

        -- First repair maps that already contain both compatible laminate
        -- nodes at the same graph occurrence. Existing explicit metadata is
        -- deliberately excluded.
        FOR pair_row IN
            SELECT
                left_node.node_id AS left_node_id,
                right_node.node_id AS right_node_id
            FROM mini_0122_apparatus_nodes left_node
            JOIN mini_0122_apparatus_nodes right_node
              ON right_node.map_id = left_node.map_id
             AND right_node.node_id > left_node.node_id
             AND right_node.apparatus_id IN (
                    'apparatus:default:asset-007',
                    'apparatus:default:asset-008'
                 )
             AND right_node.apparatus_id <> left_node.apparatus_id
             AND right_node.group_id = ''
             AND right_node.assigned_apparatus_id = ''
             AND right_node.incoming = left_node.incoming
             AND right_node.outgoing = left_node.outgoing
             AND right_node.work_parameters = left_node.work_parameters
             AND right_node.equipment_class_id = left_node.equipment_class_id
             AND right_node.execution_profile_json = left_node.execution_profile_json
             AND right_node.capabilities_json = left_node.capabilities_json
            WHERE left_node.map_id = map_row.id
              AND left_node.apparatus_id IN (
                    'apparatus:default:asset-007',
                    'apparatus:default:asset-008'
                  )
              AND left_node.group_id = ''
              AND left_node.assigned_apparatus_id = ''
              AND left_node.incoming <> '[]'::jsonb
              AND left_node.outgoing <> '[]'::jsonb
            ORDER BY left_node.node_id, right_node.node_id
        LOOP
            group_base := 'topology_alt:' || LEAST(pair_row.left_node_id, pair_row.right_node_id);
            group_id := group_base;
            group_suffix := 1;
            WHILE EXISTS (
                SELECT 1
                FROM jsonb_array_elements(map_nodes) AS nodes(node_value)
                WHERE COALESCE(node_value->>'alternative_group_id', '') = group_id
            ) LOOP
                group_id := group_base || ':' || group_suffix::TEXT;
                group_suffix := group_suffix + 1;
            END LOOP;

            SELECT jsonb_agg(
                CASE
                    WHEN node_value->>'id' IN (pair_row.left_node_id, pair_row.right_node_id)
                    THEN jsonb_set(
                        jsonb_set(
                            node_value,
                            '{alternative_group_id}',
                            to_jsonb(group_id),
                            TRUE
                        ),
                        '{alternative_group_label}',
                        to_jsonb('Laminatsiya'::TEXT),
                        TRUE
                    )
                    ELSE node_value
                END
                ORDER BY ordinality
            )
            INTO map_nodes
            FROM jsonb_array_elements(map_nodes) WITH ORDINALITY AS nodes(node_value, ordinality);
            changed := TRUE;
        END LOOP;

        -- Then expand an unambiguous one-sided laminate occurrence to the
        -- other compatible machine. A stage with another apparatus at the
        -- same graph/work signature, or any explicit alternative metadata, is
        -- left untouched because its intent cannot be inferred safely.
        FOR node_row IN
            SELECT candidate.*
            FROM mini_0122_apparatus_nodes candidate
            WHERE candidate.map_id = map_row.id
              AND candidate.apparatus_id IN (
                    'apparatus:default:asset-007',
                    'apparatus:default:asset-008'
                  )
              AND candidate.group_id = ''
              AND candidate.assigned_apparatus_id = ''
              AND candidate.incoming <> '[]'::jsonb
              AND candidate.outgoing <> '[]'::jsonb
            ORDER BY candidate.node_id
        LOOP
            SELECT count(*)
            INTO other_node_count
            FROM mini_0122_apparatus_nodes sibling
            WHERE sibling.map_id = map_row.id
              AND sibling.incoming = node_row.incoming
              AND sibling.outgoing = node_row.outgoing
              AND sibling.work_parameters = node_row.work_parameters;

            IF other_node_count <> 1 THEN
                CONTINUE;
            END IF;

            IF node_row.apparatus_id = 'apparatus:default:asset-007' THEN
                target_apparatus_id := 'apparatus:default:asset-008';
                target_title := 'Laminatsiya 2';
            ELSE
                target_apparatus_id := 'apparatus:default:asset-007';
                target_title := 'Laminatsiya 1';
            END IF;

            group_base := 'topology_alt:' || node_row.node_id;
            group_id := group_base;
            group_suffix := 1;
            WHILE EXISTS (
                SELECT 1
                FROM jsonb_array_elements(map_nodes) AS nodes(node_value)
                WHERE COALESCE(node_value->>'alternative_group_id', '') = group_id
            ) LOOP
                group_id := group_base || ':' || group_suffix::TEXT;
                group_suffix := group_suffix + 1;
            END LOOP;

            new_node_id := node_row.node_id || '_alt_' ||
                CASE WHEN target_apparatus_id = 'apparatus:default:asset-007' THEN 'lam1' ELSE 'lam2' END;
            group_suffix := 1;
            WHILE EXISTS (
                SELECT 1
                FROM jsonb_array_elements(map_nodes) AS nodes(node_value)
                WHERE node_value->>'id' = new_node_id
            ) LOOP
                new_node_id := node_row.node_id || '_alt_' ||
                    CASE WHEN target_apparatus_id = 'apparatus:default:asset-007' THEN 'lam1' ELSE 'lam2' END ||
                    '_' || group_suffix::TEXT;
                group_suffix := group_suffix + 1;
            END LOOP;

            SELECT ordinality - 1, node_value
            INTO node_index, v_node_value
            FROM jsonb_array_elements(map_nodes) WITH ORDINALITY AS nodes(node_value, ordinality)
            WHERE node_value->>'id' = node_row.node_id;

            IF v_node_value IS NULL THEN
                RAISE EXCEPTION '0122 could not locate node % in map %', node_row.node_id, map_row.id;
            END IF;

            map_nodes := jsonb_set(
                map_nodes,
                ARRAY[node_index::TEXT],
                jsonb_set(
                    jsonb_set(
                        jsonb_set(
                            jsonb_set(
                                v_node_value,
                                '{alternative_group_id}',
                                to_jsonb(group_id),
                                TRUE
                            ),
                            '{alternative_group_label}',
                            to_jsonb('Laminatsiya'::TEXT),
                            TRUE
                        ),
                        '{alternative_assigned_title}',
                        to_jsonb(''::TEXT),
                        TRUE
                    ),
                    '{alternative_assigned_apparatus_id}',
                    to_jsonb(''::TEXT),
                    TRUE
                ),
                TRUE
            );

            clone_value := v_node_value;
            clone_value := jsonb_set(clone_value, '{id}', to_jsonb(new_node_id), TRUE);
            clone_value := jsonb_set(clone_value, '{title}', to_jsonb(target_title), TRUE);
            clone_value := jsonb_set(clone_value, '{apparatus_id}', to_jsonb(target_apparatus_id), TRUE);
            clone_value := jsonb_set(clone_value, '{alternative_group_id}', to_jsonb(group_id), TRUE);
            clone_value := jsonb_set(clone_value, '{alternative_group_label}', to_jsonb('Laminatsiya'::TEXT), TRUE);
            clone_value := jsonb_set(clone_value, '{alternative_assigned_title}', to_jsonb(''::TEXT), TRUE);
            clone_value := jsonb_set(clone_value, '{alternative_assigned_apparatus_id}', to_jsonb(''::TEXT), TRUE);

            x_value := COALESCE((v_node_value->>'x')::DOUBLE PRECISION, 0.0);
            IF target_apparatus_id = 'apparatus:default:asset-007' THEN
                x_value := x_value - 110.0;
            ELSE
                x_value := x_value + 110.0;
            END IF;
            clone_value := jsonb_set(clone_value, '{x}', to_jsonb(x_value), TRUE);
            map_nodes := map_nodes || jsonb_build_array(clone_value);

            original_edges := map_edges;
            FOR edge_value IN
                SELECT value
                FROM jsonb_array_elements(original_edges) AS edges(value)
                WHERE value->>'from' = node_row.node_id
                   OR value->>'to' = node_row.node_id
            LOOP
                new_edge_value := edge_value;
                IF new_edge_value->>'from' = node_row.node_id THEN
                    new_edge_value := jsonb_set(new_edge_value, '{from}', to_jsonb(new_node_id), TRUE);
                END IF;
                IF new_edge_value->>'to' = node_row.node_id THEN
                    new_edge_value := jsonb_set(new_edge_value, '{to}', to_jsonb(new_node_id), TRUE);
                END IF;
                map_edges := map_edges || jsonb_build_array(new_edge_value);
            END LOOP;

            changed := TRUE;
        END LOOP;

        IF changed THEN
            map_row.map_json := jsonb_set(map_row.map_json, '{nodes}', map_nodes, TRUE);
            map_row.map_json := jsonb_set(map_row.map_json, '{edges}', map_edges, TRUE);
            INSERT INTO mini_0122_changed_maps (map_id, map_json)
            VALUES (map_row.id, map_row.map_json)
            ON CONFLICT (map_id) DO UPDATE SET map_json = EXCLUDED.map_json;
        END IF;
    END LOOP;
END
$$;

UPDATE mini_production_maps maps
SET map_json = changed.map_json,
    updated_at = now()
FROM mini_0122_changed_maps changed
WHERE maps.id = changed.map_id;

DELETE FROM mini_production_map_edges edges
USING mini_0122_changed_maps changed
WHERE edges.map_id = changed.map_id;

DELETE FROM mini_production_map_nodes nodes
USING mini_0122_changed_maps changed
WHERE nodes.map_id = changed.map_id;

INSERT INTO mini_production_map_nodes (
    map_id, node_id, kind, title, canonical_apparatus_id,
    canonical_alternative_apparatus_id, payload_json
)
SELECT
    changed.map_id,
    node_value->>'id',
    node_value->>'kind',
    COALESCE(node_value->>'title', ''),
    CASE
        WHEN node_value->>'kind' = 'apparatus'
        THEN NULLIF(node_value->>'apparatus_id', '')
        ELSE NULL
    END,
    NULLIF(node_value->>'alternative_assigned_apparatus_id', ''),
    node_value
FROM mini_0122_changed_maps changed
CROSS JOIN LATERAL jsonb_array_elements(changed.map_json->'nodes') AS nodes(node_value);

INSERT INTO mini_production_map_edges (
    map_id, edge_index, from_node_id, to_node_id, branch, payload_json
)
SELECT
    changed.map_id,
    edges.ordinality - 1,
    edge_value->>'from',
    edge_value->>'to',
    COALESCE(edge_value->>'branch', ''),
    edge_value
FROM mini_0122_changed_maps changed
CROSS JOIN LATERAL jsonb_array_elements(changed.map_json->'edges')
    WITH ORDINALITY AS edges(edge_value, ordinality);

-- Keep the persisted Lam1/Lam2 sequences aligned with the repaired map
-- visibility. Existing relative order is retained; newly visible orders are
-- appended in the same oldest-first order used by the runtime effective queue.
DO $$
DECLARE
    apparatus_id TEXT;
    visible_order_ids TEXT[];
    existing_order_ids JSONB;
    effective_order_ids JSONB;
    order_id TEXT;
    index_value INTEGER;
BEGIN
    FOREACH apparatus_id IN ARRAY ARRAY[
        'apparatus:default:asset-007'::TEXT,
        'apparatus:default:asset-008'::TEXT
    ]
    LOOP
        SELECT COALESCE(array_agg(maps.id ORDER BY maps.updated_at DESC, maps.id ASC), ARRAY[]::TEXT[])
        INTO visible_order_ids
        FROM mini_production_maps maps
        WHERE maps.id NOT LIKE 'template-%'
          AND EXISTS (
              SELECT 1
              FROM jsonb_array_elements(COALESCE(maps.map_json->'nodes', '[]'::jsonb)) AS nodes(node_value)
              WHERE node_value->>'kind' = 'apparatus'
                AND node_value->>'apparatus_id' = apparatus_id
          );

        SELECT sequences.order_ids
        INTO existing_order_ids
        FROM mini_queue_sequences sequences
        WHERE sequences.canonical_apparatus_id = apparatus_id
        FOR UPDATE;

        effective_order_ids := '[]'::JSONB;
        FOR order_id IN
            SELECT value
            FROM jsonb_array_elements_text(COALESCE(existing_order_ids, '[]'::jsonb)) AS entries(value)
        LOOP
            IF order_id = ANY(visible_order_ids)
               AND NOT (effective_order_ids @> jsonb_build_array(order_id))
            THEN
                effective_order_ids := effective_order_ids || jsonb_build_array(order_id);
            END IF;
        END LOOP;

        IF cardinality(visible_order_ids) > 0 THEN
            FOR index_value IN REVERSE cardinality(visible_order_ids)..1
            LOOP
                order_id := visible_order_ids[index_value];
                IF NOT (effective_order_ids @> jsonb_build_array(order_id)) THEN
                    effective_order_ids := effective_order_ids || jsonb_build_array(order_id);
                END IF;
            END LOOP;
        END IF;

        INSERT INTO mini_queue_sequences (
            apparatus, canonical_apparatus_id, order_ids, updated_at
        )
        VALUES (
            COALESCE((SELECT name FROM mini_apparatus WHERE id = apparatus_id), apparatus_id),
            apparatus_id,
            effective_order_ids,
            now()
        )
        ON CONFLICT (canonical_apparatus_id) DO UPDATE SET
            apparatus = EXCLUDED.apparatus,
            order_ids = EXCLUDED.order_ids,
            updated_at = EXCLUDED.updated_at;
    END LOOP;
END
$$;
