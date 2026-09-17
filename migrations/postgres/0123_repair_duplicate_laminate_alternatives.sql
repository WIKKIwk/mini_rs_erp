-- Repair the remaining legacy laminate groups whose two unassigned nodes
-- point to the same active apparatus. Such nodes describe one alternative
-- occurrence, not two sequential operations. Only untouched stages are
-- normalized; any execution, WIP, queue, material, or run-session evidence
-- keeps the historical map authoritative.

CREATE TEMP TABLE mini_0123_changed_maps (
    map_id TEXT PRIMARY KEY,
    map_json JSONB NOT NULL
) ON COMMIT DROP;

DO $$
DECLARE
    map_row RECORD;
    duplicate_row RECORD;
    map_nodes JSONB;
    map_edges JSONB;
    v_node_value JSONB;
    new_node_value JSONB;
    node_index INTEGER;
    target_apparatus_id TEXT;
    target_title TEXT;
    changed BOOLEAN;
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
        RAISE EXCEPTION '0123 requires active Laminatsiya 1 and Laminatsiya 2 apparatuses';
    END IF;

    IF lam1_class <> lam2_class
       OR lam1_profile <> lam2_profile
       OR lam1_capabilities <> lam2_capabilities THEN
        RAISE EXCEPTION '0123 laminate apparatus runtime profiles are not compatible';
    END IF;

    FOR map_row IN
        SELECT id, map_json
        FROM mini_production_maps
        WHERE jsonb_typeof(map_json->'nodes') = 'array'
        ORDER BY id
    LOOP
        map_nodes := map_row.map_json->'nodes';
        map_edges := COALESCE(map_row.map_json->'edges', '[]'::jsonb);
        changed := FALSE;

        FOR duplicate_row IN
            WITH apparatus_nodes AS (
                SELECT
                    node_value->>'id' AS node_id,
                    node_value->>'apparatus_id' AS apparatus_id,
                    COALESCE(node_value->>'alternative_group_id', '') AS group_id,
                    COALESCE(node_value->>'alternative_assigned_apparatus_id', '') AS assigned_id,
                    COALESCE((
                        SELECT jsonb_agg(
                            jsonb_build_array(edge->>'from', edge->>'branch')
                            ORDER BY edge->>'from', edge->>'branch'
                        )
                        FROM jsonb_array_elements(COALESCE(map_row.map_json->'edges', '[]'::jsonb)) AS edges(edge)
                        WHERE edge->>'to' = node_value->>'id'
                    ), '[]'::jsonb) AS incoming,
                    COALESCE((
                        SELECT jsonb_agg(
                            jsonb_build_array(edge->>'to', edge->>'branch')
                            ORDER BY edge->>'to', edge->>'branch'
                        )
                        FROM jsonb_array_elements(COALESCE(map_row.map_json->'edges', '[]'::jsonb)) AS edges(edge)
                        WHERE edge->>'from' = node_value->>'id'
                    ), '[]'::jsonb) AS outgoing,
                    jsonb_build_array(
                        node_value->'formula',
                        node_value->'item_code',
                        node_value->'qty_formula',
                        node_value->'from_location',
                        node_value->'to_location',
                        node_value->'rezka_kadr_count',
                        node_value->'rezka_frame_groups',
                        node_value->'rezka_label_length'
                    ) AS work_parameters
                FROM jsonb_array_elements(map_row.map_json->'nodes') AS nodes(node_value)
                WHERE node_value->>'kind' = 'apparatus'
                  AND btrim(node_value->>'id') <> ''
                  AND btrim(node_value->>'apparatus_id') <> ''
            ), duplicate_occurrences AS (
                SELECT
                    group_id,
                    apparatus_id,
                    incoming,
                    outgoing,
                    work_parameters,
                    max(node_id) AS replacement_node_id
                FROM apparatus_nodes
                WHERE apparatus_id IN (
                        'apparatus:default:asset-007',
                        'apparatus:default:asset-008'
                    )
                  AND group_id <> ''
                  AND assigned_id = ''
                  AND incoming <> '[]'::jsonb
                  AND outgoing <> '[]'::jsonb
                GROUP BY group_id, apparatus_id, incoming, outgoing, work_parameters
                HAVING count(*) = 2
            )
            SELECT duplicate_occurrences.*
            FROM duplicate_occurrences
            WHERE (
                SELECT count(*)
                FROM apparatus_nodes
                WHERE incoming = duplicate_occurrences.incoming
                  AND outgoing = duplicate_occurrences.outgoing
                  AND work_parameters = duplicate_occurrences.work_parameters
            ) = 2
            ORDER BY group_id, replacement_node_id
        LOOP
            -- Do not rewrite a group after any operational record has been
            -- attached to either laminate apparatus in this map.
            IF EXISTS (
                SELECT 1
                FROM mini_order_run_sessions sessions
                WHERE sessions.order_id = map_row.id
                  AND sessions.canonical_apparatus_id IN (
                        'apparatus:default:asset-007',
                        'apparatus:default:asset-008'
                    )
            ) OR EXISTS (
                SELECT 1
                FROM mini_progress_batches batches
                WHERE batches.order_id = map_row.id
                  AND (
                        batches.canonical_apparatus_id IN (
                            'apparatus:default:asset-007',
                            'apparatus:default:asset-008'
                        )
                     OR batches.canonical_current_apparatus_id IN (
                            'apparatus:default:asset-007',
                            'apparatus:default:asset-008'
                        )
                     OR batches.canonical_used_by_apparatus_id IN (
                            'apparatus:default:asset-007',
                            'apparatus:default:asset-008'
                        )
                     OR batches.canonical_processed_by_apparatus_id IN (
                            'apparatus:default:asset-007',
                            'apparatus:default:asset-008'
                        )
                  )
            ) OR EXISTS (
                SELECT 1
                FROM mini_queue_states states
                WHERE states.order_id = map_row.id
                  AND states.canonical_apparatus_id IN (
                        'apparatus:default:asset-007',
                        'apparatus:default:asset-008'
                    )
            ) OR EXISTS (
                SELECT 1
                FROM mini_raw_material_assignments assignments
                WHERE assignments.order_id = map_row.id
                  AND assignments.canonical_apparatus_id IN (
                        'apparatus:default:asset-007',
                        'apparatus:default:asset-008'
                    )
            ) OR EXISTS (
                SELECT 1
                FROM mini_opening_wip_batches wip
                WHERE wip.order_id = map_row.id
            ) OR EXISTS (
                SELECT 1
                FROM mini_opening_wip_intakes intakes
                WHERE intakes.order_id = map_row.id
            ) THEN
                CONTINUE;
            END IF;

            IF duplicate_row.apparatus_id = 'apparatus:default:asset-007' THEN
                target_apparatus_id := 'apparatus:default:asset-008';
                target_title := 'Laminatsiya 2';
            ELSE
                target_apparatus_id := 'apparatus:default:asset-007';
                target_title := 'Laminatsiya 1';
            END IF;

            SELECT ordinality - 1, node_value
            INTO node_index, v_node_value
            FROM jsonb_array_elements(map_nodes) WITH ORDINALITY AS nodes(node_value, ordinality)
            WHERE node_value->>'id' = duplicate_row.replacement_node_id;

            IF v_node_value IS NULL THEN
                RAISE EXCEPTION '0123 could not locate node % in map %', duplicate_row.replacement_node_id, map_row.id;
            END IF;

            new_node_value := jsonb_set(
                jsonb_set(
                    jsonb_set(
                        jsonb_set(v_node_value, '{apparatus_id}', to_jsonb(target_apparatus_id), TRUE),
                        '{title}', to_jsonb(target_title), TRUE
                    ),
                    '{alternative_group_label}', to_jsonb('Laminatsiya'::TEXT), TRUE
                ),
                '{alternative_assigned_apparatus_id}', to_jsonb(''::TEXT), TRUE
            );
            map_nodes := jsonb_set(map_nodes, ARRAY[node_index::TEXT], new_node_value, TRUE);
            changed := TRUE;
        END LOOP;

        IF changed THEN
            map_row.map_json := jsonb_set(map_row.map_json, '{nodes}', map_nodes, TRUE);
            map_row.map_json := jsonb_set(map_row.map_json, '{edges}', map_edges, TRUE);
            INSERT INTO mini_0123_changed_maps (map_id, map_json)
            VALUES (map_row.id, map_row.map_json)
            ON CONFLICT (map_id) DO UPDATE SET map_json = EXCLUDED.map_json;
        END IF;
    END LOOP;
END
$$;

UPDATE mini_production_maps maps
SET map_json = changed.map_json,
    updated_at = now()
FROM mini_0123_changed_maps changed
WHERE maps.id = changed.map_id;

DELETE FROM mini_production_map_edges edges
USING mini_0123_changed_maps changed
WHERE edges.map_id = changed.map_id;

DELETE FROM mini_production_map_nodes nodes
USING mini_0123_changed_maps changed
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
FROM mini_0123_changed_maps changed
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
FROM mini_0123_changed_maps changed
CROSS JOIN LATERAL jsonb_array_elements(changed.map_json->'edges')
    WITH ORDINALITY AS edges(edge_value, ordinality);

-- Include maps that became visible on the target apparatus while retaining
-- the existing relative order and removing only stale/duplicate IDs.
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
