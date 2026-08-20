-- Close the post-cutover write gap left by the one-time payload validation in
-- 0065.  The canonical payload is a persisted projection of the master row:
-- its immutable identity must match id and its display projection must match
-- name.  No identity is inferred from the display title.

-- Validate the already-persisted rows before installing the live constraint.
-- The explicit IS TRUE guard is intentional: missing JSON paths and JSON nulls
-- must fail closed rather than becoming an UNKNOWN CHECK result.
DO $$
DECLARE
    invalid_id TEXT;
BEGIN
    SELECT master.id
    INTO invalid_id
    FROM mini_apparatus master
    CROSS JOIN LATERAL (
        SELECT master.payload_json->'canonical_apparatus' AS canonical
    ) payload
    WHERE NOT (
        (
            master.id = btrim(master.id)
            AND octet_length(master.id) <= 128
            AND master.id !~ '[[:space:][:cntrl:]]'
            AND master.id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
            AND btrim(master.name) <> ''
            AND char_length(master.name) <= 256
            AND master.name !~ '[[:cntrl:]]'
            AND jsonb_typeof(master.payload_json) = 'object'
            AND jsonb_typeof(payload.canonical) = 'object'
            AND payload.canonical ?& ARRAY[
                'identity', 'classification', 'capabilities',
                'capability_profiles', 'policies', 'capacity', 'training',
                'provenance', 'versioning', 'aas'
            ]
            AND jsonb_typeof(payload.canonical->'identity') = 'object'
            AND jsonb_typeof(payload.canonical #> '{identity,id}') = 'string'
            AND payload.canonical #>> '{identity,id}' = master.id
            AND jsonb_typeof(payload.canonical #> '{identity,display}') = 'object'
            AND jsonb_typeof(
                payload.canonical #> '{identity,display,display_name}'
            ) = 'string'
            AND payload.canonical #>> '{identity,display,display_name}' = master.name
            AND btrim(
                payload.canonical #>> '{identity,display,display_name}'
            ) <> ''
            AND char_length(
                payload.canonical #>> '{identity,display,display_name}'
            ) <= 256
            AND payload.canonical #>> '{identity,display,display_name}'
                !~ '[[:cntrl:]]'
            AND (
                NOT (
                    (payload.canonical #> '{identity,display}')
                    ? 'description'
                )
                OR jsonb_typeof(
                    payload.canonical #> '{identity,display,description}'
                ) = 'string'
            )
            AND (
                NOT (
                    (payload.canonical #> '{identity,display}')
                    ? 'catalog_order'
                )
                OR jsonb_typeof(
                    payload.canonical #> '{identity,display,catalog_order}'
                ) = 'number'
            )
            AND jsonb_typeof(payload.canonical->'classification') = 'object'
            AND jsonb_typeof(payload.canonical #> '{classification,family}') = 'string'
            AND jsonb_typeof(payload.canonical #> '{classification,kind}') = 'string'
            AND CASE
                WHEN jsonb_typeof(payload.canonical->'capabilities') = 'array'
                    THEN jsonb_array_length(payload.canonical->'capabilities') > 0
                ELSE FALSE
            END
            AND jsonb_typeof(payload.canonical->'capability_profiles') = 'array'
            AND jsonb_typeof(payload.canonical->'policies') = 'object'
            AND payload.canonical #>> '{policies,queue}' IN (
                'strict_sequence', 'free_pick'
            )
            AND jsonb_typeof(payload.canonical #> '{policies,material}') = 'object'
            AND jsonb_typeof(
                payload.canonical #> '{policies,material,requires_material}'
            ) = 'boolean'
            AND payload.canonical #>> '{policies,material,start_policy}' IN (
                'state_all', 'requirement_groups'
            )
            AND jsonb_typeof(
                payload.canonical #> '{policies,material,item_groups}'
            ) = 'array'
            AND jsonb_typeof(
                payload.canonical #> '{policies,material,requirement_groups}'
            ) = 'array'
            AND payload.canonical #>> '{policies,tooling}' IN (
                'qolip_scan_required', 'qolip_scan_not_required'
            )
            AND jsonb_typeof(payload.canonical->'capacity') = 'object'
            AND CASE
                WHEN (payload.canonical #>> '{capacity,capacity_slots}') ~ '^[0-9]+$'
                    THEN (payload.canonical #>> '{capacity,capacity_slots}')::NUMERIC
                         BETWEEN 1 AND 64
                ELSE FALSE
            END
            AND jsonb_typeof(payload.canonical #> '{capacity,setup_minutes}') = 'number'
            AND jsonb_typeof(payload.canonical #> '{capacity,cleanup_minutes}') = 'number'
            AND CASE
                WHEN (payload.canonical #>> '{capacity,efficiency_percent}') ~ '^[0-9]+$'
                    THEN (payload.canonical #>> '{capacity,efficiency_percent}')::NUMERIC
                         BETWEEN 1 AND 200
                ELSE FALSE
            END
            AND jsonb_typeof(payload.canonical #> '{capacity,finite_capacity}') = 'boolean'
            AND jsonb_typeof(payload.canonical #> '{capacity,working_windows}') = 'array'
            AND jsonb_typeof(payload.canonical->'training') = 'object'
            AND jsonb_typeof(payload.canonical #> '{training,enabled}') = 'boolean'
            AND jsonb_typeof(payload.canonical->'provenance') = 'object'
            AND payload.canonical #>> '{provenance,source}' IN ('default', 'custom')
            AND (
                NOT ((payload.canonical->'provenance') ? 'source_ref')
                OR jsonb_typeof(
                    payload.canonical #> '{provenance,source_ref}'
                ) IN ('string', 'null')
            )
            AND jsonb_typeof(payload.canonical->'versioning') = 'object'
            AND CASE
                WHEN (payload.canonical #>> '{versioning,revision}') ~ '^[0-9]+$'
                    THEN (payload.canonical #>> '{versioning,revision}')::NUMERIC > 0
                ELSE FALSE
            END
            AND jsonb_typeof(payload.canonical->'aas') = 'object'
            AND jsonb_typeof(payload.canonical #> '{aas,submodel_id}') = 'string'
            AND payload.canonical #>> '{aas,submodel_id}' =
                'urn:mini-rs-erp:submodel:apparatus:' ||
                substr(master.id, length('apparatus:') + 1)
            AND jsonb_typeof(payload.canonical #> '{aas,semantic_id}') = 'string'
            AND payload.canonical #>> '{aas,semantic_id}' =
                'urn:mini-rs-erp:semantic-id:submodel:apparatus:1'
            AND jsonb_typeof(payload.canonical #> '{aas,idta_release}') = 'string'
            AND payload.canonical #>> '{aas,idta_release}' = '26-01'
            AND jsonb_typeof(
                payload.canonical #> '{aas,aas_metamodel_version}'
            ) = 'string'
            AND payload.canonical #>> '{aas,aas_metamodel_version}' = '3.2.0'
            AND jsonb_typeof(payload.canonical #> '{aas,aasx_part_5_version}') = 'string'
            AND payload.canonical #>> '{aas,aasx_part_5_version}' = 'IDTA-01005 v3.2'
            AND jsonb_typeof(payload.canonical #> '{aas,package_format}') = 'string'
            AND payload.canonical #>> '{aas,package_format}' =
                'Open Packaging Conventions'
            AND jsonb_typeof(payload.canonical #> '{aas,media_type}') = 'string'
            AND payload.canonical #>> '{aas,media_type}' =
                'application/asset-administration-shell-package'
            AND (
                NOT (payload.canonical ? 'placement')
                OR jsonb_typeof(payload.canonical->'placement') = 'null'
                OR (
                    jsonb_typeof(payload.canonical->'placement') = 'object'
                    AND jsonb_typeof(
                        payload.canonical #> '{placement,factory_map_object_id}'
                    ) = 'string'
                    AND btrim(
                        payload.canonical #>> '{placement,factory_map_object_id}'
                    ) <> ''
                    AND char_length(
                        payload.canonical #>> '{placement,factory_map_object_id}'
                    ) <= 128
                    AND payload.canonical #>> '{placement,factory_map_object_id}'
                        !~ '[[:cntrl:]]'
                )
            )
        ) IS TRUE
    )
    LIMIT 1;

    IF invalid_id IS NOT NULL THEN
        RAISE EXCEPTION
            '0067 canonical apparatus payload invariant preflight failed for id %',
            invalid_id;
    END IF;
END
$$;

-- The non-UNKNOWN CHECK is deliberately repeated as a row-level invariant so
-- direct INSERT/UPDATE statements cannot create a new inconsistent projection.
ALTER TABLE mini_apparatus
    ADD CONSTRAINT mini_apparatus_canonical_payload_contract_check
    CHECK (
        (
            id = btrim(id)
            AND octet_length(id) <= 128
            AND id !~ '[[:space:][:cntrl:]]'
            AND id ~ '^apparatus:[a-z0-9._-]+:[a-z0-9._-]+$'
            AND btrim(name) <> ''
            AND char_length(name) <= 256
            AND name !~ '[[:cntrl:]]'
            AND jsonb_typeof(payload_json) = 'object'
            AND jsonb_typeof(payload_json->'canonical_apparatus') = 'object'
            AND payload_json->'canonical_apparatus' ?& ARRAY[
                'identity', 'classification', 'capabilities',
                'capability_profiles', 'policies', 'capacity', 'training',
                'provenance', 'versioning', 'aas'
            ]
            AND jsonb_typeof(payload_json #> '{canonical_apparatus,identity}') = 'object'
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,identity,id}'
            ) = 'string'
            AND payload_json #>> '{canonical_apparatus,identity,id}' = id
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,identity,display}'
            ) = 'object'
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,identity,display,display_name}'
            ) = 'string'
            AND payload_json #>>
                '{canonical_apparatus,identity,display,display_name}' = name
            AND btrim(
                payload_json #>>
                '{canonical_apparatus,identity,display,display_name}'
            ) <> ''
            AND char_length(
                payload_json #>>
                '{canonical_apparatus,identity,display,display_name}'
            ) <= 256
            AND payload_json #>>
                '{canonical_apparatus,identity,display,display_name}'
                !~ '[[:cntrl:]]'
            AND (
                NOT (
                    (payload_json #> '{canonical_apparatus,identity,display}')
                    ? 'description'
                )
                OR jsonb_typeof(
                    payload_json #>
                    '{canonical_apparatus,identity,display,description}'
                ) = 'string'
            )
            AND (
                NOT (
                    (payload_json #> '{canonical_apparatus,identity,display}')
                    ? 'catalog_order'
                )
                OR jsonb_typeof(
                    payload_json #>
                    '{canonical_apparatus,identity,display,catalog_order}'
                ) = 'number'
            )
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,classification}'
            ) = 'object'
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,classification,family}'
            ) = 'string'
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,classification,kind}'
            ) = 'string'
            AND CASE
                WHEN jsonb_typeof(
                    payload_json #> '{canonical_apparatus,capabilities}'
                ) = 'array'
                    THEN jsonb_array_length(
                        payload_json #> '{canonical_apparatus,capabilities}'
                    ) > 0
                ELSE FALSE
            END
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,capability_profiles}'
            ) = 'array'
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,policies}'
            ) = 'object'
            AND payload_json #>>
                '{canonical_apparatus,policies,queue}' IN (
                    'strict_sequence', 'free_pick'
                )
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,policies,material}'
            ) = 'object'
            AND jsonb_typeof(
                payload_json #>
                '{canonical_apparatus,policies,material,requires_material}'
            ) = 'boolean'
            AND payload_json #>>
                '{canonical_apparatus,policies,material,start_policy}' IN (
                    'state_all', 'requirement_groups'
                )
            AND jsonb_typeof(
                payload_json #>
                '{canonical_apparatus,policies,material,item_groups}'
            ) = 'array'
            AND jsonb_typeof(
                payload_json #>
                '{canonical_apparatus,policies,material,requirement_groups}'
            ) = 'array'
            AND payload_json #>>
                '{canonical_apparatus,policies,tooling}' IN (
                    'qolip_scan_required', 'qolip_scan_not_required'
                )
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,capacity}'
            ) = 'object'
            AND CASE
                WHEN payload_json #>>
                    '{canonical_apparatus,capacity,capacity_slots}' ~ '^[0-9]+$'
                    THEN (payload_json #>>
                        '{canonical_apparatus,capacity,capacity_slots}')::NUMERIC
                        BETWEEN 1 AND 64
                ELSE FALSE
            END
            AND jsonb_typeof(
                payload_json #>
                '{canonical_apparatus,capacity,setup_minutes}'
            ) = 'number'
            AND jsonb_typeof(
                payload_json #>
                '{canonical_apparatus,capacity,cleanup_minutes}'
            ) = 'number'
            AND CASE
                WHEN payload_json #>>
                    '{canonical_apparatus,capacity,efficiency_percent}' ~ '^[0-9]+$'
                    THEN (payload_json #>>
                        '{canonical_apparatus,capacity,efficiency_percent}')::NUMERIC
                        BETWEEN 1 AND 200
                ELSE FALSE
            END
            AND jsonb_typeof(
                payload_json #>
                '{canonical_apparatus,capacity,finite_capacity}'
            ) = 'boolean'
            AND jsonb_typeof(
                payload_json #>
                '{canonical_apparatus,capacity,working_windows}'
            ) = 'array'
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,training}'
            ) = 'object'
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,training,enabled}'
            ) = 'boolean'
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,provenance}'
            ) = 'object'
            AND payload_json #>>
                '{canonical_apparatus,provenance,source}' IN ('default', 'custom')
            AND (
                NOT (
                    (payload_json #> '{canonical_apparatus,provenance}')
                    ? 'source_ref'
                )
                OR jsonb_typeof(
                    payload_json #>
                    '{canonical_apparatus,provenance,source_ref}'
                ) IN ('string', 'null')
            )
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,versioning}'
            ) = 'object'
            AND CASE
                WHEN payload_json #>>
                    '{canonical_apparatus,versioning,revision}' ~ '^[0-9]+$'
                    THEN (payload_json #>>
                        '{canonical_apparatus,versioning,revision}')::NUMERIC > 0
                ELSE FALSE
            END
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,aas}'
            ) = 'object'
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,aas,submodel_id}'
            ) = 'string'
            AND payload_json #>> '{canonical_apparatus,aas,submodel_id}' =
                'urn:mini-rs-erp:submodel:apparatus:' ||
                substr(id, length('apparatus:') + 1)
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,aas,semantic_id}'
            ) = 'string'
            AND payload_json #>> '{canonical_apparatus,aas,semantic_id}' =
                'urn:mini-rs-erp:semantic-id:submodel:apparatus:1'
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,aas,idta_release}'
            ) = 'string'
            AND payload_json #>> '{canonical_apparatus,aas,idta_release}' = '26-01'
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,aas,aas_metamodel_version}'
            ) = 'string'
            AND payload_json #>>
                '{canonical_apparatus,aas,aas_metamodel_version}' = '3.2.0'
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,aas,aasx_part_5_version}'
            ) = 'string'
            AND payload_json #>>
                '{canonical_apparatus,aas,aasx_part_5_version}' = 'IDTA-01005 v3.2'
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,aas,package_format}'
            ) = 'string'
            AND payload_json #>>
                '{canonical_apparatus,aas,package_format}' =
                'Open Packaging Conventions'
            AND jsonb_typeof(
                payload_json #> '{canonical_apparatus,aas,media_type}'
            ) = 'string'
            AND payload_json #>>
                '{canonical_apparatus,aas,media_type}' =
                'application/asset-administration-shell-package'
            AND (
                NOT (
                    (payload_json #> '{canonical_apparatus}') ? 'placement'
                )
                OR jsonb_typeof(payload_json #> '{canonical_apparatus,placement}') = 'null'
                OR (
                    jsonb_typeof(
                        payload_json #> '{canonical_apparatus,placement}'
                    ) = 'object'
                    AND jsonb_typeof(
                        payload_json #>
                        '{canonical_apparatus,placement,factory_map_object_id}'
                    ) = 'string'
                    AND btrim(payload_json #>>
                        '{canonical_apparatus,placement,factory_map_object_id}') <> ''
                    AND char_length(payload_json #>>
                        '{canonical_apparatus,placement,factory_map_object_id}') <= 128
                    AND payload_json #>>
                        '{canonical_apparatus,placement,factory_map_object_id}'
                        !~ '[[:cntrl:]]'
                )
            )
        ) IS TRUE
    );
