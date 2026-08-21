-- Final clean cutover. Upgrade databases must already have completed the exact
-- P11 manifest transaction at migration head 0069. A database that is being
-- created from an empty migration history may discard immutable historical
-- seed rows because they have never been runtime authority.

SELECT set_config('mini_rs_erp.canonical_writer', 'on', true);

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM mini_apparatus WHERE source_revision IS NULL) THEN
        IF current_setting('mini_rs_erp.fresh_database_bootstrap', true)
            IS DISTINCT FROM 'on'
        THEN
            RAISE EXCEPTION
                '0070 clean cutover requires completed P11 canonical manifest transaction';
        END IF;
        PERFORM set_config('mini_rs_erp.canonical_writer', 'on', true);
        DELETE FROM mini_apparatus_queue_policies;
        DELETE FROM mini_apparatus_material_rules;
        DELETE FROM mini_apparatus_capacity_profiles;
        DELETE FROM mini_apparatus;
    END IF;

    IF EXISTS (
        SELECT 1 FROM mini_apparatus WHERE source_revision IS NULL
        UNION ALL SELECT 1 FROM mini_apparatus_queue_policies WHERE source_revision IS NULL
        UNION ALL SELECT 1 FROM mini_apparatus_material_rules WHERE source_revision IS NULL
        UNION ALL SELECT 1 FROM mini_apparatus_capacity_profiles WHERE source_revision IS NULL
    ) THEN
        RAISE EXCEPTION '0070 legacy apparatus projection authority remains';
    END IF;
    IF EXISTS (SELECT 1 FROM mini_canonical_apparatus_projection_drift) THEN
        RAISE EXCEPTION '0070 canonical apparatus projection drift is not zero';
    END IF;
END
$$;

DROP VIEW IF EXISTS mini_apparatus_legacy_id_shape_diagnostics;
DROP VIEW IF EXISTS mini_canonical_apparatus_queue_policy_duplicate_diagnostics;
DROP VIEW IF EXISTS mini_canonical_apparatus_upsert_duplicate_diagnostics;
DROP VIEW IF EXISTS mini_unresolved_apparatus_reference_counts;
DROP VIEW IF EXISTS mini_apparatus_material_rule_canonical_diagnostics;
DROP VIEW IF EXISTS mini_canonical_apparatus_cutover_diagnostics;

ALTER TABLE mini_apparatus_capacity_profiles
    ADD COLUMN payload_json JSONB;

UPDATE mini_apparatus_capacity_profiles profile
SET payload_json = jsonb_build_object(
    'apparatus_id', profile.canonical_apparatus_id,
    'source_revision', profile.source_revision,
    'source_aasx_sha256', profile.source_aasx_sha256,
    'capacity_slots', profile.capacity_slots,
    'setup_minutes', profile.setup_minutes,
    'cleanup_minutes', profile.cleanup_minutes,
    'efficiency_percent', profile.efficiency_percent,
    'finite_capacity', profile.finite_capacity,
    'always_available', COALESCE(
        apparatus.capacity_json #>> '{availability,mode}', 'always'
    ) = 'always',
    'working_windows', profile.working_windows
)
FROM mini_apparatus apparatus
WHERE apparatus.id = profile.canonical_apparatus_id;

ALTER TABLE mini_apparatus
    ALTER COLUMN source_revision SET NOT NULL,
    ALTER COLUMN source_aasx_sha256 SET NOT NULL,
    ALTER COLUMN schema_version SET NOT NULL,
    ALTER COLUMN physical_asset_id SET NOT NULL,
    ALTER COLUMN equipment_class_id SET NOT NULL,
    ALTER COLUMN hierarchy_json SET NOT NULL,
    ALTER COLUMN capabilities_json SET NOT NULL,
    ALTER COLUMN execution_profile_json SET NOT NULL,
    ALTER COLUMN policies_json SET NOT NULL,
    ALTER COLUMN capacity_json SET NOT NULL,
    ALTER COLUMN lifecycle_state SET NOT NULL;

ALTER TABLE mini_apparatus_queue_policies
    ALTER COLUMN source_revision SET NOT NULL,
    ALTER COLUMN source_aasx_sha256 SET NOT NULL;
ALTER TABLE mini_apparatus_material_rules
    ALTER COLUMN source_revision SET NOT NULL,
    ALTER COLUMN source_aasx_sha256 SET NOT NULL;
ALTER TABLE mini_apparatus_capacity_profiles
    ALTER COLUMN payload_json SET NOT NULL,
    ALTER COLUMN source_revision SET NOT NULL,
    ALTER COLUMN source_aasx_sha256 SET NOT NULL;

ALTER TABLE mini_apparatus_queue_policies
    ADD CONSTRAINT mini_apparatus_queue_payload_source_check CHECK ((
        payload_json #>> '{apparatus_id}' = canonical_apparatus_id
        AND (payload_json #>> '{source_revision}')::BIGINT = source_revision
        AND payload_json #>> '{source_aasx_sha256}' = source_aasx_sha256
    ) IS TRUE);
ALTER TABLE mini_apparatus_material_rules
    ADD CONSTRAINT mini_apparatus_material_payload_source_check CHECK ((
        payload_json #>> '{apparatus_id}' = canonical_apparatus_id
        AND (payload_json #>> '{source_revision}')::BIGINT = source_revision
        AND payload_json #>> '{source_aasx_sha256}' = source_aasx_sha256
    ) IS TRUE);
ALTER TABLE mini_apparatus_capacity_profiles
    ADD CONSTRAINT mini_apparatus_capacity_payload_source_check CHECK ((
        payload_json #>> '{apparatus_id}' = canonical_apparatus_id
        AND (payload_json #>> '{source_revision}')::BIGINT = source_revision
        AND payload_json #>> '{source_aasx_sha256}' = source_aasx_sha256
    ) IS TRUE);

CREATE OR REPLACE FUNCTION mini_guard_canonical_projection_write()
RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF current_setting('mini_rs_erp.canonical_writer', true) IS DISTINCT FROM 'on' THEN
        RAISE EXCEPTION 'canonical apparatus projections are derived and read-only'
            USING ERRCODE = '42501';
    END IF;
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;
    RETURN NEW;
END
$$;

ALTER TABLE mini_apparatus DROP COLUMN group_id;
DROP TABLE mini_apparatus_groups;
ALTER TABLE mini_apparatus
    DROP COLUMN base_name,
    DROP COLUMN kind;

ALTER TABLE mini_apparatus_queue_policies
    DROP COLUMN apparatus,
    DROP COLUMN policy,
    DROP COLUMN actor_role,
    DROP COLUMN actor_ref,
    DROP COLUMN actor_display_name;

ALTER TABLE mini_apparatus_material_rules
    DROP COLUMN apparatus,
    DROP COLUMN item_groups,
    DROP COLUMN requirement_groups,
    DROP COLUMN requires_material;

ALTER TABLE mini_apparatus_capacity_profiles
    DROP COLUMN apparatus_id,
    DROP COLUMN apparatus,
    DROP COLUMN capacity_slots,
    DROP COLUMN setup_minutes,
    DROP COLUMN cleanup_minutes,
    DROP COLUMN efficiency_percent,
    DROP COLUMN finite_capacity,
    DROP COLUMN working_windows,
    DROP COLUMN capabilities,
    DROP COLUMN capability_levels,
    DROP COLUMN notes;
